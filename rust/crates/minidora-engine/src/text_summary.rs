//! Deterministic extractive sentence candidates with exact source spans.
//! This ranks supplied sentences; it does not verify summary coverage or meaning.
use crate::ports::{JsonKind, strict_json_document};
use crate::store::{Store, VerifiedCaseHistory};
use minidora_domain::{
    ContentPlan, Error, EvidencePacket, Guarantee, InputBundle, Intent, Lifecycle, MeaningIR,
    Result, ResultRecord, SummaryLineLimit, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

const MAX_REQUEST_BYTES: usize = 256_000;
const MAX_TEXT_SCALARS: usize = 100_000;
const MAX_SENTENCES: usize = 4_096;
const MAX_ARTIFACT_BYTES: usize = 2_000_000;
const OPERATION: &str = "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4";
const LINE_OPERATION: &str = "TEXT_EXTRACTIVE_LINE_SUMMARY_CANDIDATES_V1";
const ALGORITHM: &str =
    "TOKEN_FREQUENCY_FIRST_SENTENCE_LONG_SENTENCE_PENALTY_REQUIRED_CONTENT_OUTPUT_FORMAT_V1";
const LINE_SUMMARY_ALGORITHM: &str =
    "TOKEN_FREQUENCY_FIRST_SENTENCE_LONG_SENTENCE_PENALTY_LINE_OUTPUT_V1";
pub(crate) const EXTRACTIVE_SUMMARY_LABEL: &str =
    "抽出候補です。文の頻度と位置で選択しました。目的・意味・条件・否定の網羅は検証していません。";
const GUARANTEE: &str = "deterministic extractive sentence candidates ranked by token frequency, a first-sentence prior, and a long-sentence penalty; caller-required source sentences are retained before ranked fill, and every retained sentence is copied verbatim with decoded-source byte offsets in the requested bullets or paragraph layout. Relevance, completeness, purpose, meaning, conditions, negation, factual truth, and writing quality are not verified";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SummaryRequest {
    text: String,
    max_sentences: Option<usize>,
    line_limit: Option<SummaryLineLimit>,
    max_chars: Option<usize>,
    format: Option<String>,
    required_sentence_indices: Option<Vec<usize>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractedSentence {
    pub sentence_index: usize,
    pub selection_rank: usize,
    pub source_start: usize,
    pub source_end: usize,
    pub output_start: usize,
    pub output_end: usize,
    pub text: String,
    pub score_numerator: u64,
    pub score_denominator: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtractiveSummaryCandidates {
    pub operation: String,
    pub algorithm: String,
    pub source_document: String,
    pub source_sha256: String,
    pub source_text_sha256: String,
    pub source_text: String,
    pub source_text_json_start: usize,
    pub source_text_json_end: usize,
    pub output_sha256: String,
    pub text: String,
    pub format: String,
    pub max_sentences: usize,
    pub max_chars: usize,
    pub required_sentence_indices: Vec<usize>,
    pub selected_character_count: usize,
    pub sentence_count: usize,
    pub selected_sentence_indices: Vec<usize>,
    pub omitted_sentence_indices: Vec<usize>,
    pub length_limited_sentence_indices: Vec<usize>,
    pub sentences: Vec<ExtractedSentence>,
    pub coverage_verified: bool,
    pub guarantee: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_limit: Option<SummaryLineLimit>,
}

pub(crate) const PREVIOUS_RESULT_SUMMARY_KIND: &str = "minidora.previous-result-summary.v1";
const PREVIOUS_RESULT_LINE_SUMMARY_KIND: &str = "minidora.previous-result-line-summary.v1";
pub(crate) const PREVIOUS_RESULT_SUMMARY_HEADER: &str = "前回結果からの抽出候補";
const LINE_SUMMARY_NOTE: &str = "（抽出候補・網羅性未確認）";

#[derive(Clone, Copy)]
pub(crate) enum PreviousResultSummaryLimits {
    Sentences {
        max_sentences: usize,
        max_chars: Option<usize>,
    },
    Lines(SummaryLineLimit),
}

impl PreviousResultSummaryLimits {
    pub(crate) fn from_intent(intent: &Intent) -> Option<Self> {
        match intent {
            Intent::SummarizePreviousResult {
                max_sentences,
                max_chars,
            } => Some(Self::Sentences {
                max_sentences: max_sentences.unwrap_or(3),
                max_chars: *max_chars,
            }),
            Intent::SummarizePreviousResultLines { limit } => Some(Self::Lines(*limit)),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreviousResultSummaryArtifact {
    pub kind: String,
    pub summary_result_id: String,
    pub source_result_id: String,
    pub source_content_plan_id: String,
    pub source_render_sha256: String,
    pub source_text_sha256: String,
    pub summary_unit_sha256: String,
    pub summary: ExtractiveSummaryCandidates,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_limit: Option<SummaryLineLimit>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceExtractiveSummaryCandidates {
    pub operation: String,
    pub source_result_id: String,
    pub source_packet_id: String,
    pub source_packet_ref: String,
    pub source_mode: String,
    pub source_request_uri: String,
    pub source_final_uri: String,
    pub observed_at_unix_ms: u64,
    pub raw_body_sha256: String,
    pub extracted_text_sha256: String,
    pub parser_version: String,
    pub license_status: String,
    pub summary: ExtractiveSummaryCandidates,
}

pub(crate) fn source_summary_only_wikipedia_closing_tags(
    report: &SourceExtractiveSummaryCandidates,
) -> bool {
    let Ok(uri) = url::Url::parse(&report.source_final_uri) else {
        return false;
    };
    uri.host_str() == Some(crate::search::SearchProvider::Wikipedia.host())
        && uri.path().starts_with("/wiki/")
        && uri
            .query_pairs()
            .any(|(key, value)| key == "action" && value == "raw")
        && !report.summary.sentences.is_empty()
        && report.summary.sentences.iter().all(|sentence| {
            let element = sentence
                .text
                .trim()
                .trim_end_matches(['。', '.', '！', '!', '？', '?'])
                .trim_end();
            element
                .strip_prefix("</")
                .and_then(|element| element.strip_suffix('>'))
                .is_some_and(|name| {
                    !name.is_empty() && name.bytes().all(|byte| byte.is_ascii_alphanumeric())
                })
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceQuestionCandidate {
    pub rank: usize,
    pub source_packet_id: String,
    pub source_packet_ref: String,
    pub source_mode: String,
    pub source_final_uri: String,
    pub raw_body_sha256: String,
    pub extracted_text_sha256: String,
    pub source_start: usize,
    pub source_end: usize,
    pub source_span_sha256: String,
    pub text: String,
    pub matched_query_terms: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceQuestionAnswer {
    pub operation: String,
    pub source_result_id: String,
    pub question_topic: String,
    pub query_terms: Vec<String>,
    pub source_packet_count: usize,
    pub matched_sentence_count: usize,
    pub candidates: Vec<SourceQuestionCandidate>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceQuestionPriorVersion {
    pub latest_observed_at_unix_ms: u64,
    pub raw_body_sha256: String,
    pub extracted_text_sha256: String,
    pub parser_version: String,
    pub packet_refs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceQuestionSourceVersion {
    pub source_final_uri: String,
    pub active_packet_id: String,
    pub active_packet_ref: String,
    pub active_observed_at_unix_ms: u64,
    pub active_raw_body_sha256: String,
    pub active_extracted_text_sha256: String,
    pub active_parser_version: String,
    pub active_version_packet_refs: Vec<String>,
    pub superseded_versions: Vec<SourceQuestionPriorVersion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SourceQuestionAnswerV2 {
    pub operation: String,
    pub source_result_id: String,
    pub question_topic: String,
    pub query_terms: Vec<String>,
    pub source_packet_count: usize,
    pub active_source_count: usize,
    pub superseded_version_count: usize,
    pub source_versions: Vec<SourceQuestionSourceVersion>,
    pub matched_sentence_count: usize,
    pub candidates: Vec<SourceQuestionCandidate>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceQuestionPacketVersion {
    raw_body_sha256: String,
    extracted_text_sha256: String,
    parser_version: String,
    latest_observed_at_unix_ms: u64,
    packet_indices: Vec<usize>,
}

pub(crate) const MAX_QUESTION_SOURCE_PACKETS: usize = 16;
const MAX_QUESTION_TERMS: usize = 32;
const MAX_QUESTION_CANDIDATES: usize = 5;

fn sha256_id(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SummaryFormat {
    Bullets,
    Paragraph,
}

impl SummaryFormat {
    fn from_request(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("bullets") => Ok(Self::Bullets),
            Some("paragraph") => Ok(Self::Paragraph),
            Some(_) => Err(schema("format must be either bullets or paragraph")),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Bullets => "bullets",
            Self::Paragraph => "paragraph",
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Score {
    numerator: u64,
    denominator: u64,
}

#[derive(Clone, Debug)]
struct Candidate {
    start: usize,
    end: usize,
    text: String,
    score: Score,
}

fn schema(message: impl Into<String>) -> minidora_domain::Error {
    failure("TEXT_SUMMARY_SCHEMA", message)
}

fn fields<'a>(
    document: &str,
    node: &'a crate::ports::JsonNode,
) -> Result<&'a BTreeMap<String, crate::ports::JsonNode>> {
    let JsonKind::Object(values) = &node.kind else {
        return Err(schema("summary request must be one JSON object"));
    };
    if !(2..=5).contains(&values.len())
        || !values.contains_key("text")
        || values.contains_key("max_sentences") == values.contains_key("line_limit")
        || values.keys().any(|key| {
            !matches!(
                key.as_str(),
                "text"
                    | "max_sentences"
                    | "line_limit"
                    | "max_chars"
                    | "format"
                    | "required_sentence_indices"
            )
        })
    {
        return Err(schema(
            "summary request requires text and exactly one of max_sentences or line_limit; max_chars, format, and required_sentence_indices are optional",
        ));
    }
    if values
        .get("line_limit")
        .is_some_and(|value| !matches!(value.kind, JsonKind::Object(_)))
    {
        return Err(schema(
            "line_limit must specify EXACT or AT_MOST with an integer from 1 to 8",
        ));
    }
    if values
        .get("max_chars")
        .is_some_and(|value| !matches!(value.kind, JsonKind::Number))
    {
        return Err(schema("max_chars must be an integer from 1 to 100000"));
    }
    if values.get("format").is_some_and(|value| {
        !matches!(value.kind, JsonKind::Scalar) || !document[value.span.clone()].starts_with('"')
    }) {
        return Err(schema("format must be either bullets or paragraph"));
    }
    if let Some(value) = values.get("required_sentence_indices") {
        let JsonKind::Array(indices) = &value.kind else {
            return Err(schema(
                "required_sentence_indices must be an array of source-order integers",
            ));
        };
        if indices.len() > 8 {
            return Err(schema(
                "required_sentence_indices may contain at most 8 indices",
            ));
        }
        for index in indices {
            if !matches!(index.kind, JsonKind::Number)
                || document[index.span.clone()].parse::<usize>().is_err()
            {
                return Err(schema(
                    "required_sentence_indices must contain nonnegative integer indices",
                ));
            }
        }
    }
    Ok(values)
}

fn trim_range(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let raw = text.get(start..end)?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let leading = raw.len() - raw.trim_start().len();
    Some(start + leading..start + leading + trimmed.len())
}

fn push_sentence(
    text: &str,
    start: usize,
    end: usize,
    sentences: &mut Vec<Candidate>,
) -> Result<()> {
    if let Some(range) = trim_range(text, start, end) {
        if sentences.len() >= MAX_SENTENCES {
            return Err(failure(
                "TEXT_SUMMARY_SENTENCE_LIMIT",
                "source contains more than 4096 sentence candidates",
            ));
        }
        sentences.push(Candidate {
            start: range.start,
            end: range.end,
            text: text[range].to_owned(),
            score: Score {
                numerator: 0,
                denominator: 1,
            },
        });
    }
    Ok(())
}

fn sentence_candidates(text: &str, line_layout: bool) -> Result<Vec<Candidate>> {
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((at, character)) = chars.next() {
        if line_layout
            && character == '.'
            && text[..at].chars().next_back().is_some_and(char::is_numeric)
            && chars.peek().is_some_and(|(_, next)| next.is_numeric())
        {
            continue;
        }
        if matches!(character, '。' | '！' | '？' | '!' | '?' | '.') {
            let mut end = at + character.len_utf8();
            while chars
                .peek()
                .is_some_and(|(_, next)| matches!(*next, '。' | '！' | '？' | '!' | '?' | '.'))
            {
                let (next_at, next) = chars.next().expect("peeked punctuation");
                end = next_at + next.len_utf8();
            }
            push_sentence(text, start, end, &mut sentences)?;
            start = end;
        } else if matches!(character, '\n' | '\r')
            || (line_layout && matches!(character, '\u{2028}' | '\u{2029}'))
        {
            push_sentence(text, start, at, &mut sentences)?;
            start = at + character.len_utf8();
            if character == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
                start += chars.next().expect("peeked LF").1.len_utf8();
            }
        }
    }
    push_sentence(text, start, text.len(), &mut sentences)?;
    Ok(sentences)
}

fn token_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || ('一'..='龥').contains(&character)
        || ('ぁ'..='ん').contains(&character)
        || ('ァ'..='ヶ').contains(&character)
}

fn tokens(text: &str) -> Vec<String> {
    let mut output = Vec::new();
    let mut start = None;
    for (at, character) in text.char_indices() {
        if token_character(character) {
            start.get_or_insert(at);
        } else if let Some(begin) = start.take() {
            let token = text[begin..at].to_lowercase();
            if token.chars().count() >= 2 {
                output.push(token);
            }
        }
    }
    if let Some(begin) = start {
        let token = text[begin..].to_lowercase();
        if token.chars().count() >= 2 {
            output.push(token);
        }
    }
    output
}

fn stop_word(word: &str) -> bool {
    matches!(
        word,
        "これ"
            | "それ"
            | "ため"
            | "こと"
            | "もの"
            | "よう"
            | "です"
            | "ます"
            | "した"
            | "する"
            | "いる"
            | "ある"
            | "から"
            | "まで"
            | "について"
            | "として"
    )
}

fn score_sentences(sentences: &mut [Candidate]) {
    let mut frequencies = BTreeMap::<String, u64>::new();
    let source_tokens = sentences
        .iter()
        .flat_map(|sentence| tokens(&sentence.text))
        .filter(|token| !stop_word(token));
    for token in source_tokens {
        *frequencies.entry(token).or_default() += 1;
    }
    for (index, sentence) in sentences.iter_mut().enumerate() {
        let words: Vec<_> = tokens(&sentence.text)
            .into_iter()
            .filter(|token| !stop_word(token))
            .collect();
        let numerator = words
            .iter()
            .map(|word| frequencies.get(word).copied().unwrap_or(0))
            .sum::<u64>();
        let mut score = Score {
            numerator,
            denominator: words.len().max(1) as u64,
        };
        if index == 0 {
            score.numerator *= 6;
            score.denominator *= 5;
        }
        if sentence.text.chars().count() > 180 {
            score.numerator *= 3;
            score.denominator *= 4;
        }
        sentence.score = score;
    }
}

fn compare_scores(left: Score, right: Score) -> std::cmp::Ordering {
    (u128::from(left.numerator) * u128::from(right.denominator))
        .cmp(&(u128::from(right.numerator) * u128::from(left.denominator)))
}

fn source_question_stop_word(word: &str) -> bool {
    stop_word(word)
        || matches!(
            word,
            "what"
                | "does"
                | "say"
                | "about"
                | "source"
                | "article"
                | "page"
                | "the"
                | "and"
                | "for"
                | "with"
                | "from"
                | "this"
                | "that"
                | "are"
                | "was"
                | "were"
                | "has"
                | "have"
                | "which"
        )
}

fn has_japanese_query_character(text: &str) -> bool {
    text.chars().any(|character| {
        ('一'..='龥').contains(&character)
            || ('ぁ'..='ん').contains(&character)
            || ('ァ'..='ヶ').contains(&character)
    })
}

fn query_term_matches_sentence(term: &str, sentence: &str, sentence_tokens: &[String]) -> bool {
    if has_japanese_query_character(term) {
        sentence.contains(term)
    } else {
        sentence_tokens.iter().any(|candidate| candidate == term)
    }
}

/// Return bounded exact-source sentence candidates selected by lexical overlap
/// with a caller-supplied topic. This does not claim semantic question
/// answering, source reliability, or coverage of the fetched document.
pub(crate) fn answer_fetched_source_question_v1(
    source_result_id: &str,
    question_topic: &str,
    packets: &[(usize, &EvidencePacket, String)],
) -> Result<SourceQuestionAnswer> {
    if !sha256_id(source_result_id)
        || question_topic.is_empty()
        || question_topic.len() > 512
        || packets.is_empty()
        || packets.len() > MAX_QUESTION_SOURCE_PACKETS
    {
        return Err(failure(
            "SOURCE_QUESTION_INPUT_LIMIT",
            "expected a committed source result, a topic up to 512 bytes and 1..16 fetched-body packets",
        ));
    }
    let mut query_terms = Vec::new();
    for term in tokens(question_topic)
        .into_iter()
        .filter(|term| !source_question_stop_word(term))
    {
        if !query_terms.contains(&term) {
            query_terms.push(term);
        }
    }
    if query_terms.is_empty() || query_terms.len() > MAX_QUESTION_TERMS {
        return Err(failure(
            "SOURCE_QUESTION_TOPIC_UNSUPPORTED",
            "the topic must contain 1..32 supported Japanese or English terms",
        ));
    }

    let mut ordered = Vec::<(usize, usize, usize, SourceQuestionCandidate)>::new();
    let mut matched_sentence_count = 0usize;
    for (packet_order, (evidence_index, packet, packet_ref)) in packets.iter().enumerate() {
        let extracted_text = packet.extracted_text.as_deref().unwrap_or_default();
        let packet_bytes = canonical(packet)?;
        if !sha256_id(&packet.id)
            || !sha256_id(packet_ref)
            || digest(&packet_bytes) != *packet_ref
            || packet.packet_kind != "FETCHED_BODY"
            || packet.failure.is_some()
            || packet.retry_after.is_some()
            || !packet.request_uri.starts_with("https://")
            || !packet.final_uri.starts_with("https://")
            || packet.raw_sha256 != digest(&packet.raw_body)
            || extracted_text.trim().is_empty()
            || extracted_text.chars().count() > MAX_TEXT_SCALARS
        {
            return Err(failure(
                "SOURCE_QUESTION_EVIDENCE_INVALID",
                "each question source must be an intact HTTPS fetched-body packet with bounded extracted text",
            ));
        }
        let extracted_text_sha256 = digest(extracted_text.as_bytes());
        for sentence in sentence_candidates(extracted_text, false)? {
            let sentence_tokens = tokens(&sentence.text);
            let matched_query_terms: Vec<_> = query_terms
                .iter()
                .filter(|term| query_term_matches_sentence(term, &sentence.text, &sentence_tokens))
                .cloned()
                .collect();
            if matched_query_terms.is_empty() {
                continue;
            }
            matched_sentence_count = matched_sentence_count.saturating_add(1);
            let candidate = SourceQuestionCandidate {
                rank: 0,
                source_packet_id: packet.id.clone(),
                source_packet_ref: packet_ref.clone(),
                source_mode: packet.mode.clone(),
                source_final_uri: packet.final_uri.clone(),
                raw_body_sha256: packet.raw_sha256.clone(),
                extracted_text_sha256: extracted_text_sha256.clone(),
                source_start: sentence.start,
                source_end: sentence.end,
                source_span_sha256: digest(sentence.text.as_bytes()),
                text: sentence.text,
                matched_query_terms,
            };
            ordered.push((
                packet_order,
                *evidence_index,
                candidate.source_start,
                candidate,
            ));
        }
    }
    if ordered.is_empty() {
        return Err(failure(
            "SOURCE_QUESTION_NO_MATCH",
            "no sentence in the actual fetched body contains a supported topic term",
        ));
    }
    ordered.sort_by(|left, right| {
        right
            .3
            .matched_query_terms
            .len()
            .cmp(&left.3.matched_query_terms.len())
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    let candidates = ordered
        .into_iter()
        .take(MAX_QUESTION_CANDIDATES)
        .enumerate()
        .map(|(index, (_, _, _, mut candidate))| {
            candidate.rank = index + 1;
            candidate
        })
        .collect();
    Ok(SourceQuestionAnswer {
        operation: "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1".into(),
        source_result_id: source_result_id.into(),
        question_topic: question_topic.into(),
        query_terms,
        source_packet_count: packets.len(),
        matched_sentence_count,
        candidates,
        guarantee: "extractive source passages ranked only by exact English token or Japanese substring overlap with the caller-supplied topic; semantic relevance, completeness, source reliability and factual truth are not verified".into(),
    })
}

pub(crate) fn source_question_answer_lines_v1(answer: &SourceQuestionAnswer) -> Vec<String> {
    let mut lines = vec![
        "前回取得した実本文から、質問語句と一致する原文候補を抽出しました。".into(),
        format!(
            "照合topic: {:?}。照合語: {}。候補文: {}件中{}件を表示。",
            answer.question_topic,
            answer.query_terms.join("、"),
            answer.matched_sentence_count,
            answer.candidates.len()
        ),
    ];
    for candidate in &answer.candidates {
        lines.push(format!(
            "候補{} / mode: {} / source: {} / packet: {} / raw-body SHA-256: {} / extracted-text SHA-256: {}",
            candidate.rank,
            candidate.source_mode,
            candidate.source_final_uri,
            candidate.source_packet_id,
            candidate.raw_body_sha256,
            candidate.extracted_text_sha256
        ));
        lines.push(format!(
            "  matched: {} / extracted-text byte[{}..{}) / span SHA-256: {}",
            candidate.matched_query_terms.join("、"),
            candidate.source_start,
            candidate.source_end,
            candidate.source_span_sha256
        ));
        lines.push(format!("  原文候補: {}", candidate.text));
    }
    lines.push("出典license情報: packetに記録されていないため未検証です。".into());
    lines.push(format!("保証範囲: {}。", answer.guarantee));
    lines
}

/// Select exact lexical matches from the newest observed body version for each
/// exact final URI. Older versions stay named in the answer artifact and remain
/// separately retained by the case; they are not presented as independent
/// sources. A same-time content conflict has no safe active version.
pub(crate) fn answer_fetched_source_question_v2(
    source_result_id: &str,
    question_topic: &str,
    packets: &[(usize, &EvidencePacket, String)],
) -> Result<SourceQuestionAnswerV2> {
    if !sha256_id(source_result_id)
        || question_topic.is_empty()
        || question_topic.len() > 512
        || packets.is_empty()
        || packets.len() > MAX_QUESTION_SOURCE_PACKETS
    {
        return Err(failure(
            "SOURCE_QUESTION_INPUT_LIMIT",
            "expected a committed source result, a topic up to 512 bytes and 1..16 fetched-body packets",
        ));
    }
    let mut query_terms = Vec::new();
    for term in tokens(question_topic)
        .into_iter()
        .filter(|term| !source_question_stop_word(term))
    {
        if !query_terms.contains(&term) {
            query_terms.push(term);
        }
    }
    if query_terms.is_empty() || query_terms.len() > MAX_QUESTION_TERMS {
        return Err(failure(
            "SOURCE_QUESTION_TOPIC_UNSUPPORTED",
            "the topic must contain 1..32 supported Japanese or English terms",
        ));
    }

    let mode = packets[0].1.mode.as_str();
    if mode.is_empty() {
        return Err(failure(
            "SOURCE_QUESTION_EVIDENCE_INVALID",
            "fetched-body evidence must identify its LIVE or fixed-packet mode",
        ));
    }
    for (_, packet, packet_ref) in packets {
        let extracted_text = packet.extracted_text.as_deref().unwrap_or_default();
        let packet_bytes = canonical(packet)?;
        if !sha256_id(&packet.id)
            || !sha256_id(packet_ref)
            || digest(&packet_bytes) != *packet_ref
            || packet.packet_kind != "FETCHED_BODY"
            || packet.failure.is_some()
            || packet.retry_after.is_some()
            || !packet.request_uri.starts_with("https://")
            || !packet.final_uri.starts_with("https://")
            || packet.raw_sha256 != digest(&packet.raw_body)
            || extracted_text.trim().is_empty()
            || extracted_text.chars().count() > MAX_TEXT_SCALARS
            || packet.mode.is_empty()
            || packet.parser_version.is_empty()
        {
            return Err(failure(
                "SOURCE_QUESTION_EVIDENCE_INVALID",
                "each question source must be an intact HTTPS fetched-body packet with bounded extracted text and parser identity",
            ));
        }
        if packet.mode != mode {
            return Err(failure(
                "SOURCE_QUESTION_MODE_MIXED",
                "LIVE and fixed-packet bodies cannot be combined in one source-question result",
            ));
        }
    }
    let SourceVersionSelection {
        active_packet_indices,
        source_versions,
        superseded_version_count,
    } = select_source_versions(packets, None)?;

    let mut ordered = Vec::<(usize, usize, usize, SourceQuestionCandidate)>::new();
    let mut matched_sentence_count = 0usize;
    for (source_order, packet_index) in active_packet_indices.iter().enumerate() {
        let (evidence_index, packet, packet_ref) = &packets[*packet_index];
        let extracted_text = packet
            .extracted_text
            .as_deref()
            .expect("validated active source body has extracted text");
        let extracted_text_sha256 = digest(extracted_text.as_bytes());
        for sentence in sentence_candidates(extracted_text, false)? {
            let sentence_tokens = tokens(&sentence.text);
            let matched_query_terms: Vec<_> = query_terms
                .iter()
                .filter(|term| query_term_matches_sentence(term, &sentence.text, &sentence_tokens))
                .cloned()
                .collect();
            if matched_query_terms.is_empty() {
                continue;
            }
            matched_sentence_count = matched_sentence_count.saturating_add(1);
            let candidate = SourceQuestionCandidate {
                rank: 0,
                source_packet_id: packet.id.clone(),
                source_packet_ref: packet_ref.clone(),
                source_mode: packet.mode.clone(),
                source_final_uri: packet.final_uri.clone(),
                raw_body_sha256: packet.raw_sha256.clone(),
                extracted_text_sha256: extracted_text_sha256.clone(),
                source_start: sentence.start,
                source_end: sentence.end,
                source_span_sha256: digest(sentence.text.as_bytes()),
                text: sentence.text,
                matched_query_terms,
            };
            ordered.push((
                source_order,
                *evidence_index,
                candidate.source_start,
                candidate,
            ));
        }
    }
    if ordered.is_empty() {
        return Err(failure(
            "SOURCE_QUESTION_NO_MATCH",
            "no sentence in the newest body version for any exact final URI contains a supported topic term",
        ));
    }
    ordered.sort_by(|left, right| {
        right
            .3
            .matched_query_terms
            .len()
            .cmp(&left.3.matched_query_terms.len())
            .then_with(|| left.0.cmp(&right.0))
            .then_with(|| left.1.cmp(&right.1))
            .then_with(|| left.2.cmp(&right.2))
    });
    let candidates = ordered
        .into_iter()
        .take(MAX_QUESTION_CANDIDATES)
        .enumerate()
        .map(|(index, (_, _, _, mut candidate))| {
            candidate.rank = index + 1;
            candidate
        })
        .collect();
    Ok(SourceQuestionAnswerV2 {
        operation: "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2".into(),
        source_result_id: source_result_id.into(),
        question_topic: question_topic.into(),
        query_terms,
        source_packet_count: packets.len(),
        active_source_count: source_versions.len(),
        superseded_version_count,
        source_versions,
        matched_sentence_count,
        candidates,
        guarantee: "exact English-token or Japanese-substring matches from the latest observed body/parser version for each exact final URI; older distinct versions and repeated observations remain linked, while source independence, semantic relevance, completeness, reliability and factual truth are not verified".into(),
    })
}

pub(crate) fn source_question_answer_lines_v2(answer: &SourceQuestionAnswerV2) -> Vec<String> {
    let mut lines = vec![
        "前回取得した実本文の最新版から、質問語句と一致する原文候補を抽出しました。".into(),
        format!(
            "照合topic: {:?}。照合語: {}。本文packet: {}件、exact final URI: {}件、旧本文版: {}件。",
            answer.question_topic,
            answer.query_terms.join("、"),
            answer.source_packet_count,
            answer.active_source_count,
            answer.superseded_version_count
        ),
    ];
    for version in &answer.source_versions {
        lines.push(format!(
            "source version: {} / active packet: {} / ref: {} / observed_at_unix_ms: {} / raw SHA-256: {} / extracted SHA-256: {} / parser: {}",
            version.source_final_uri,
            version.active_packet_id,
            version.active_packet_ref,
            version.active_observed_at_unix_ms,
            version.active_raw_body_sha256,
            version.active_extracted_text_sha256,
            version.active_parser_version
        ));
        lines.push(format!(
            "  同一本文版の取得packet refs: {}",
            version.active_version_packet_refs.join(", ")
        ));
        for prior in &version.superseded_versions {
            lines.push(format!(
                "  旧本文版は履歴に保持、今回の照合対象外 / observed_at_unix_ms: {} / raw SHA-256: {} / extracted SHA-256: {} / parser: {} / packet refs: {}",
                prior.latest_observed_at_unix_ms,
                prior.raw_body_sha256,
                prior.extracted_text_sha256,
                prior.parser_version,
                prior.packet_refs.join(", ")
            ));
        }
    }
    lines.push(format!(
        "照合結果: {}文中{}候補を表示。",
        answer.matched_sentence_count,
        answer.candidates.len()
    ));
    for candidate in &answer.candidates {
        lines.push(format!(
            "候補{} / mode: {} / source: {} / packet: {} / raw-body SHA-256: {} / extracted-text SHA-256: {}",
            candidate.rank,
            candidate.source_mode,
            candidate.source_final_uri,
            candidate.source_packet_id,
            candidate.raw_body_sha256,
            candidate.extracted_text_sha256
        ));
        lines.push(format!(
            "  matched: {} / extracted-text byte[{}..{}) / span SHA-256: {}",
            candidate.matched_query_terms.join("、"),
            candidate.source_start,
            candidate.source_end,
            candidate.source_span_sha256
        ));
        lines.push(format!("  原文候補: {}", candidate.text));
    }
    lines.push("出典license情報: packetに記録されていないため未検証です。".into());
    lines.push(format!("保証範囲: {}。", answer.guarantee));
    lines
}

/// Select a bounded set of source sentences by the inspected deterministic
/// extractive ranking family. This is a candidate operation, not a semantic
/// summary verifier; every selected sentence remains tied to exact source bytes.
pub fn extractive_summary_candidates(document: &str) -> Result<ExtractiveSummaryCandidates> {
    extractive_summary_candidates_internal(document, false)
}

pub(crate) fn direct_summary_candidates(
    bundle: &InputBundle,
) -> Result<ExtractiveSummaryCandidates> {
    let meaning = &bundle.meaning;
    if crate::input::structured_intent(&bundle.request.text)?.as_ref() == Some(&meaning.intent) {
        match &meaning.intent {
            Intent::TextSummaryCandidates { document }
                if meaning.parser_version == crate::input::PARSER_VERSION =>
            {
                return extractive_summary_candidates(document);
            }
            Intent::SummarizePastedText { source }
                if meaning.parser_version == crate::input::PASTED_TEXT_SUMMARY_PARSER_VERSION =>
            {
                let document = serde_json::to_string(&serde_json::json!({
                    "text": source.text,
                    "max_sentences": 3,
                }))?;
                return extractive_summary_candidates(&document);
            }
            _ => {}
        }
    }
    Err(failure(
        "TEXT_SUMMARY_CANDIDATE_MISMATCH",
        "the retained summary input version or source changed",
    ))
}

fn extractive_summary_candidates_internal(
    document: &str,
    line_layout: bool,
) -> Result<ExtractiveSummaryCandidates> {
    if document.is_empty() || document.len() > MAX_REQUEST_BYTES {
        return Err(failure(
            "TEXT_SUMMARY_INPUT_LIMIT",
            "summary request must contain 1..=256000 UTF-8 bytes",
        ));
    }
    let root = strict_json_document(document)?;
    let fields = fields(document, &root)?;
    let text_node = &fields["text"];
    if !matches!(text_node.kind, JsonKind::Scalar)
        || !document[text_node.span.clone()].starts_with('"')
    {
        return Err(schema("text must be a string"));
    }
    if fields
        .get("max_sentences")
        .is_some_and(|value| !matches!(value.kind, JsonKind::Number))
    {
        return Err(schema("max_sentences must be an integer from 1 to 8"));
    }
    let SummaryRequest {
        text,
        max_sentences,
        line_limit,
        max_chars,
        format,
        required_sentence_indices,
    } = serde_json::from_str(document).map_err(|error| schema(error.to_string()))?;
    let max_sentences = match (max_sentences, line_limit) {
        (Some(count), None) if (1..=8).contains(&count) => count,
        (None, Some(SummaryLineLimit::Exact(count) | SummaryLineLimit::AtMost(count)))
            if (1..=8).contains(&count) =>
        {
            count
        }
        (None, Some(_)) => {
            return Err(schema(
                "line_limit must specify EXACT or AT_MOST with an integer from 1 to 8",
            ));
        }
        _ => return Err(schema("max_sentences must be an integer from 1 to 8")),
    };
    let line_layout = line_layout || line_limit.is_some();
    let max_chars = max_chars.unwrap_or(MAX_TEXT_SCALARS);
    if !(1..=MAX_TEXT_SCALARS).contains(&max_chars) {
        return Err(schema("max_chars must be an integer from 1 to 100000"));
    }
    let format = SummaryFormat::from_request(format.as_deref())?;
    if line_limit.is_some() && matches!(format, SummaryFormat::Paragraph) {
        return Err(schema("line_limit requires bullets format"));
    }
    let mut required_sentence_indices = required_sentence_indices.unwrap_or_default();
    required_sentence_indices.sort_unstable();
    if required_sentence_indices
        .windows(2)
        .any(|pair| pair[0] == pair[1])
    {
        return Err(schema(
            "required_sentence_indices must not contain duplicates",
        ));
    }
    if text.trim().is_empty()
        || text.len() > MAX_REQUEST_BYTES
        || text.chars().count() > MAX_TEXT_SCALARS
        || text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\t' | '\n' | '\r'))
    {
        return Err(failure(
            "TEXT_SUMMARY_INPUT_LIMIT",
            "text must be nonempty, at most 100000 Unicode scalars/256000 bytes, with supported controls only",
        ));
    }
    let mut candidates = sentence_candidates(&text, line_layout)?;
    if candidates.is_empty() {
        return Err(schema("text contains no sentence candidates"));
    }
    if required_sentence_indices
        .iter()
        .any(|index| *index >= candidates.len())
    {
        return Err(failure(
            "TEXT_SUMMARY_REQUIRED_INDEX",
            "a required sentence index is outside the source sentence candidates",
        ));
    }
    if required_sentence_indices.len() > max_sentences {
        return Err(failure(
            "TEXT_SUMMARY_REQUIRED_LIMIT",
            "required source sentences exceed max_sentences",
        ));
    }
    let required_set: std::collections::BTreeSet<_> =
        required_sentence_indices.iter().copied().collect();
    let mut selected_ranked = required_sentence_indices.clone();
    let mut selected_character_count = selected_ranked
        .iter()
        .map(|index| candidates[*index].text.chars().count())
        .sum::<usize>();
    if selected_character_count > max_chars {
        return Err(failure(
            "TEXT_SUMMARY_REQUIRED_CHAR_LIMIT",
            "required source sentences exceed max_chars; no sentence was truncated",
        ));
    }
    score_sentences(&mut candidates);
    let mut ranking: Vec<usize> = (0..candidates.len()).collect();
    ranking.sort_by(|left, right| {
        compare_scores(candidates[*right].score, candidates[*left].score)
            .then_with(|| right.cmp(left))
    });
    let character_counts: Vec<_> = candidates
        .iter()
        .map(|candidate| candidate.text.chars().count())
        .collect();
    let exact_fit = if matches!(line_limit, Some(SummaryLineLimit::Exact(_))) {
        let needed = max_sentences - selected_ranked.len();
        if candidates.len() < max_sentences {
            return Err(failure(
                "RESULT_SUMMARY_LINE_COUNT_UNMET",
                format!(
                    "元の本文から抽出できる候補は{}行で、指定された{max_sentences}行に足りません。",
                    candidates.len()
                ),
            ));
        }
        let minimum = shortest_suffix_totals(&ranking, &character_counts, &required_set, needed);
        if minimum[0][needed] > max_chars - selected_character_count {
            return Err(failure(
                "RESULT_SUMMARY_LINE_COUNT_UNMET",
                format!("選択文の文字数上限内で指定された{max_sentences}行を確保できません。"),
            ));
        }
        Some(minimum)
    } else {
        None
    };
    let mut selection_ranks: BTreeMap<usize, usize> = selected_ranked
        .iter()
        .enumerate()
        .map(|(rank, index)| (*index, rank + 1))
        .collect();
    let mut length_limited_sentence_indices = Vec::new();
    for (position, index) in ranking.into_iter().enumerate() {
        if selected_ranked.len() == max_sentences {
            break;
        }
        if required_set.contains(&index) {
            continue;
        }
        let sentence_characters = character_counts[index];
        let needed_after = max_sentences - selected_ranked.len() - 1;
        let reserved = exact_fit
            .as_ref()
            .map_or(0, |minimum| minimum[position + 1][needed_after]);
        if sentence_characters.saturating_add(reserved) <= max_chars - selected_character_count {
            selected_character_count += sentence_characters;
            selection_ranks.insert(index, selected_ranked.len() + 1);
            selected_ranked.push(index);
        } else {
            length_limited_sentence_indices.push(index);
        }
    }
    if selected_ranked.is_empty() {
        return Err(failure(
            "TEXT_SUMMARY_CHAR_LIMIT",
            "no complete source sentence fits max_chars; no text was truncated",
        ));
    }
    length_limited_sentence_indices.sort_unstable();
    let mut selected: Vec<usize> = selected_ranked;
    selected.sort_unstable();
    let selected_set: std::collections::BTreeSet<_> = selected.iter().copied().collect();

    let mut output = if line_layout {
        String::new()
    } else {
        EXTRACTIVE_SUMMARY_LABEL.into()
    };
    let mut spans = Vec::with_capacity(selected.len());
    for index in &selected {
        let candidate = &candidates[*index];
        match format {
            SummaryFormat::Bullets if output.is_empty() => output.push_str("- "),
            SummaryFormat::Bullets => output.push_str("\n- "),
            SummaryFormat::Paragraph if spans.is_empty() => output.push('\n'),
            SummaryFormat::Paragraph => output.push(' '),
        }
        let output_start = output.len();
        output.push_str(&candidate.text);
        let output_end = output.len();
        if line_layout && spans.is_empty() {
            output.push_str(LINE_SUMMARY_NOTE);
        }
        spans.push(ExtractedSentence {
            sentence_index: *index,
            selection_rank: selection_ranks[index],
            source_start: candidate.start,
            source_end: candidate.end,
            output_start,
            output_end,
            text: candidate.text.clone(),
            score_numerator: candidate.score.numerator,
            score_denominator: candidate.score.denominator,
        });
    }
    let selected_sentence_indices: Vec<_> = spans.iter().map(|span| span.sentence_index).collect();
    let omitted_sentence_indices: Vec<_> = (0..candidates.len())
        .filter(|index| !selected_set.contains(index))
        .collect();
    let output_sha256 = digest(output.as_bytes());
    let report = ExtractiveSummaryCandidates {
        operation: if line_limit.is_some() {
            LINE_OPERATION
        } else {
            OPERATION
        }
        .into(),
        algorithm: if line_layout {
            LINE_SUMMARY_ALGORITHM
        } else {
            ALGORITHM
        }
        .into(),
        source_document: document.into(),
        source_sha256: digest(document.as_bytes()),
        source_text_sha256: digest(text.as_bytes()),
        source_text: text,
        source_text_json_start: text_node.span.start,
        source_text_json_end: text_node.span.end,
        output_sha256,
        text: output,
        format: format.as_str().into(),
        max_sentences,
        max_chars,
        required_sentence_indices,
        selected_character_count,
        sentence_count: candidates.len(),
        selected_sentence_indices,
        omitted_sentence_indices,
        length_limited_sentence_indices,
        sentences: spans,
        coverage_verified: false,
        guarantee: GUARANTEE.into(),
        line_limit,
    };
    if minidora_domain::canonical(&report)?.len() > MAX_ARTIFACT_BYTES {
        return Err(failure(
            "TEXT_SUMMARY_ARTIFACT_LIMIT",
            "extractive summary report exceeds 2000000 serialized bytes",
        ));
    }
    Ok(report)
}

// For an exact count, keep the highest-ranked choice that still permits a
// completion under the character cap. Each table cell is a minimum suffix cost.
fn shortest_suffix_totals(
    ranking: &[usize],
    character_counts: &[usize],
    required: &BTreeSet<usize>,
    needed: usize,
) -> Vec<[usize; 9]> {
    let mut minimum = vec![[usize::MAX; 9]; ranking.len() + 1];
    minimum[ranking.len()][0] = 0;
    for (position, &index) in ranking.iter().enumerate().rev() {
        let next = minimum[position + 1];
        minimum[position] = next;
        if !required.contains(&index) {
            for count in 1..=needed {
                let with_current = next[count - 1].saturating_add(character_counts[index]);
                minimum[position][count] = next[count].min(with_current);
            }
        }
    }
    minimum
}

pub(crate) fn summarize_previous_result_with_limits(
    summary_result_id: &str,
    source_result_id: &str,
    source_content_plan_id: &str,
    source_render_sha256: &str,
    source_text: &str,
    limits: PreviousResultSummaryLimits,
) -> Result<PreviousResultSummaryArtifact> {
    if !sha256_id(summary_result_id)
        || !sha256_id(source_result_id)
        || !sha256_id(source_content_plan_id)
        || !sha256_id(source_render_sha256)
    {
        return Err(failure(
            "RESULT_SUMMARY_SOURCE_INVALID",
            "summary inputs require exact result, content-plan and render identities",
        ));
    }
    let (max_sentences, max_chars, line_limit) = match limits {
        PreviousResultSummaryLimits::Sentences {
            max_sentences,
            max_chars,
        } => (max_sentences, max_chars, None),
        PreviousResultSummaryLimits::Lines(
            limit @ (SummaryLineLimit::Exact(count) | SummaryLineLimit::AtMost(count)),
        ) => (count, None, Some(limit)),
    };
    let document = match max_chars {
        Some(max_chars) => serde_json::to_string(&serde_json::json!({
            "text": source_text,
            "max_sentences": max_sentences,
            "max_chars": max_chars,
            "format": "bullets"
        }))?,
        None => serde_json::to_string(&serde_json::json!({
            "text": source_text,
            "max_sentences": max_sentences,
            "format": "bullets"
        }))?,
    };
    let summary = extractive_summary_candidates_internal(&document, line_limit.is_some())?;
    if summary.source_text != source_text
        || summary.source_text_sha256 != digest(source_text.as_bytes())
    {
        return Err(failure(
            "RESULT_SUMMARY_SOURCE_MISMATCH",
            "summary source differs from the exact prior result text",
        ));
    }
    if let Some(limit) = line_limit {
        let actual = summary.text.lines().count();
        let valid = match limit {
            SummaryLineLimit::Exact(count) => actual == count,
            SummaryLineLimit::AtMost(count) => (1..=count).contains(&actual),
        };
        if !valid {
            return Err(failure(
                "RESULT_SUMMARY_LINE_COUNT_UNMET",
                &format!(
                    "元の回答から抽出できた要約は{actual}行で、指定の行数条件を満たしません。"
                ),
            ));
        }
    }
    let mut artifact = PreviousResultSummaryArtifact {
        kind: if line_limit.is_some() {
            PREVIOUS_RESULT_LINE_SUMMARY_KIND
        } else {
            PREVIOUS_RESULT_SUMMARY_KIND
        }
        .into(),
        summary_result_id: summary_result_id.into(),
        source_result_id: source_result_id.into(),
        source_content_plan_id: source_content_plan_id.into(),
        source_render_sha256: source_render_sha256.into(),
        source_text_sha256: summary.source_text_sha256.clone(),
        summary_unit_sha256: String::new(),
        summary,
        line_limit,
    };
    artifact.summary_unit_sha256 = digest(artifact.output_unit().as_bytes());
    Ok(artifact)
}

impl PreviousResultSummaryArtifact {
    pub(crate) fn output_unit(&self) -> String {
        if self.line_limit.is_some() {
            return self.summary.text.clone();
        }
        format!(
            "{PREVIOUS_RESULT_SUMMARY_HEADER}（結果ID: {}）。\n{}",
            self.source_result_id, self.summary.text
        )
    }
}

pub(crate) struct RetainedPreviousResultSummary {
    pub(crate) content_ref: String,
    pub(crate) render_ref: String,
    pub(crate) summary_ref: String,
    artifact: PreviousResultSummaryArtifact,
    artifact_bytes: usize,
}

/// Recompute the saved report from the original summary request and the exact
/// result that was sealed as its source.
pub(crate) fn verify_retained_previous_result_summary(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<Option<RetainedPreviousResultSummary>> {
    let Some(limits) = PreviousResultSummaryLimits::from_intent(&bundle.meaning.intent) else {
        return Ok(None);
    };
    if matches!(limits, PreviousResultSummaryLimits::Lines(_))
        != (bundle.meaning.parser_version
            == crate::input::PREVIOUS_RESULT_LINE_SUMMARY_PARSER_VERSION)
    {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_MISMATCH",
            "the retained summary intent differs from its parser version",
        ));
    }
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let source_result_id = bundle
        .meaning
        .context_result_ref
        .as_deref()
        .ok_or_else(|| {
            failure(
                "RECOVERY_RESULT_SUMMARY_CONTEXT_MISMATCH",
                "summary has no sealed prior-result reference",
            )
        })?;
    let source = prior_results.get(source_result_id).ok_or_else(|| {
        failure(
            "RECOVERY_RESULT_SUMMARY_CONTEXT_MISMATCH",
            "exact source result is not retained",
        )
    })?;
    if source.id != source_result_id || source.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_CONTEXT_MISMATCH",
            "summary source is not the same committed prior result",
        ));
    }
    let (Some(source_plan), Some(source_render)) = (&source.content_plan, &source.render) else {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_SOURCE_UNAVAILABLE",
            "source result has no retained content plan or render",
        ));
    };
    let mut unsigned_plan = source_plan.clone();
    unsigned_plan.id.clear();
    if source_plan.id != identity(&unsigned_plan)?
        || source.text != source_plan.text
        || !crate::core::validate_render(source_plan, source_render)
    {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_SOURCE_INTEGRITY",
            "the retained source result does not match its content plan and render",
        ));
    }
    let content_bytes = canonical(source_plan)?;
    let content_ref = digest(&content_bytes);
    let render_bytes = canonical(source_render)?;
    let render_ref = digest(&render_bytes);
    for (reference, expected) in [(&content_ref, &content_bytes), (&render_ref, &render_bytes)] {
        let stored = store
            .get_bytes_if_present(actor, case, reference)?
            .ok_or_else(|| {
                failure(
                    "RECOVERY_RESULT_SUMMARY_SOURCE_UNAVAILABLE",
                    "the source output artifact is missing from this case",
                )
            })?;
        if &stored != expected {
            return Err(failure(
                "RECOVERY_RESULT_SUMMARY_SOURCE_INTEGRITY",
                "the source content or render artifact differs from the retained result",
            ));
        }
    }
    if bundle.context_sha256 != identity(&Some(source))? {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_CONTEXT_MISMATCH",
            "the summary input did not seal the exact source result",
        ));
    }
    let artifact = summarize_previous_result_with_limits(
        &result.id,
        &source.id,
        &source_plan.id,
        &source_render.actual_sha256,
        &source.text,
        limits,
    )?;
    if artifact.summary.sentence_count > request.budget.max_actions as usize {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_LIMIT",
            "the source sentence scan exceeds the original action budget",
        ));
    }
    let output_unit = artifact.output_unit();
    let artifact_bytes = canonical(&artifact)?;
    if artifact_bytes.len().saturating_add(output_unit.len()) > request.budget.max_evidence_bytes {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_LIMIT",
            "the retained summary exceeds the original evidence budget",
        ));
    }
    let summary_ref = digest(&artifact_bytes);
    let retained = store
        .get_bytes_if_present(actor, case, &summary_ref)?
        .ok_or_else(|| {
            failure(
                "RESULT_SUMMARY_ARTIFACT_UNAVAILABLE",
                "the recomputed summary has no retained report in this case",
            )
        })?;
    if retained != artifact_bytes {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_MISMATCH",
            "the retained summary artifact differs from the original result text",
        ));
    }
    let mut expected_dependencies = vec![
        summary_ref.clone(),
        source.id.clone(),
        content_ref.clone(),
        render_ref.clone(),
    ];
    expected_dependencies.extend(source.dependencies.iter().cloned());
    if result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.guarantee != Guarantee::Hypothesis
        || plan.guarantee != result.guarantee
        || plan.locale != request.locale
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || result.dependencies != expected_dependencies
        || result.text != output_unit
        || plan.text != output_unit
        || plan.required_units != [output_unit]
    {
        return Err(failure(
            "RECOVERY_RESULT_SUMMARY_MISMATCH",
            "the summary result, source dependencies, or displayed unit differs from the recomputed report",
        ));
    }
    Ok(Some(RetainedPreviousResultSummary {
        content_ref,
        render_ref,
        summary_ref,
        artifact,
        artifact_bytes: artifact_bytes.len(),
    }))
}

pub(crate) struct PreviousResultSummaryExplanation {
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) source_context_ref: Option<String>,
    sole_summary_packet_index: Option<usize>,
}

pub(crate) fn verify_source_explanation_result(
    store: &Store,
    input: &InputBundle,
    result: &ResultRecord,
    proof: &PreviousResultSummaryExplanation,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<()> {
    let request = &input.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let invalid = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            "the retained source explanation differs from its verified derivation",
        )
    };
    let plan = result.content_plan.as_ref().ok_or_else(invalid)?;
    let retained = match history {
        Some(history) => history.input_result(&result.input_id, result.input_version)?,
        None => store.get_input_result(actor, case, &result.input_id, result.input_version)?,
    };
    if result.lifecycle != Lifecycle::Commit
        || result.guarantee != Guarantee::Hypothesis
        || identity(request)? != result.id
        || retained.as_ref() != Some(result)
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || plan.locale != request.locale
        || plan.required_units != proof.units
        || result.text != proof.units.join("\n")
        || result.dependencies != proof.dependencies
        || !plan.dependencies.starts_with(&proof.dependencies)
    {
        return Err(invalid());
    }
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        result,
        None,
        "SOURCE_CONTEXT_INVALID",
        "retrieved source explanation",
    )?;
    Ok(())
}

pub(crate) fn previous_summary_explanation(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    prior: &ResultRecord,
) -> Result<Option<PreviousResultSummaryExplanation>> {
    if !matches!(bundle.meaning.intent, Intent::ExplainPrevious)
        || prior.guarantee != Guarantee::Hypothesis
    {
        return Ok(None);
    }
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let Some((_, summary_bundle)) =
        store.get_input_bundle(actor, case, &prior.input_id, prior.input_version)?
    else {
        if prior.text.starts_with(PREVIOUS_RESULT_SUMMARY_HEADER)
            || prior
                .text
                .lines()
                .next()
                .is_some_and(|line| line.starts_with("- ") && line.ends_with(LINE_SUMMARY_NOTE))
            || prior.text.contains("\n出典packet: ")
            || prior
                .text
                .contains("質問語句と一致する原文候補を抽出しました。")
        {
            return Err(failure(
                "MISSING_RESULT_SUMMARY_CONTEXT",
                "the prior summary or source answer has no retained sealed request",
            ));
        }
        return Ok(None);
    };
    let is_source_summary = matches!(
        summary_bundle.meaning.intent,
        Intent::SummarizePreviousSource
            | Intent::SummarizePreviousSourceConstrained { .. }
            | Intent::SummarizePreviousSourceLines { .. }
            | Intent::SummarizePreviousSourcePacketLines { .. }
    );
    let is_source_question = matches!(
        summary_bundle.meaning.intent,
        Intent::AskPreviousSourceQuestion { .. }
    );
    if !is_source_summary
        && !is_source_question
        && PreviousResultSummaryLimits::from_intent(&summary_bundle.meaning.intent).is_none()
    {
        return Ok(None);
    }
    if prior.lifecycle != Lifecycle::Commit
        || bundle.meaning.context_result_ref.as_deref() != Some(prior.id.as_str())
        || bundle.context_sha256 != identity(&Some(prior))?
    {
        return Err(failure(
            "MISSING_RESULT_SUMMARY_CONTEXT",
            "the explanation does not seal the expected committed summary or source answer",
        ));
    }
    let plan = prior.content_plan.as_ref().ok_or_else(|| {
        failure(
            "RESULT_SUMMARY_OUTPUT_UNAVAILABLE",
            "the summary has no retained content plan",
        )
    })?;
    if is_source_question {
        return previous_source_question_explanation(
            store,
            bundle,
            &summary_bundle,
            prior_results,
            prior,
            plan,
            None,
        );
    }
    if is_source_summary {
        return previous_source_summary_explanation(
            store,
            bundle,
            &summary_bundle,
            prior_results,
            prior,
            plan,
            None,
        );
    }
    let Some(proof) = verify_retained_previous_result_summary(
        store,
        &summary_bundle,
        prior_results,
        prior,
        plan,
    )?
    else {
        return Ok(None);
    };
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        prior,
        Some(&proof.summary_ref),
        "RESULT_SUMMARY_OUTPUT_UNAVAILABLE",
        "previous result summary",
    )?;

    let artifact = proof.artifact;
    let source = prior_results
        .get(&artifact.source_result_id)
        .ok_or_else(|| {
            failure(
                "RESULT_SUMMARY_SOURCE_UNAVAILABLE",
                "the source result is not retained",
            )
        })?;
    let (_, source_bundle) = store
        .get_input_bundle(actor, case, &source.input_id, source.input_version)?
        .ok_or_else(|| {
            failure(
                "RESULT_SUMMARY_SOURCE_UNAVAILABLE",
                "the source has no retained sealed request",
            )
        })?;
    if source.id != identity(&source_bundle.request)? {
        return Err(failure(
            "RESULT_SUMMARY_SOURCE_INTEGRITY",
            "the source result differs from its sealed request",
        ));
    }
    let summary = artifact.summary;
    let budget = &request.budget;
    if proof.artifact_bytes > budget.max_evidence_bytes
        || summary.sentence_count > budget.max_actions as usize
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the retained summary exceeds the explanation budget",
        ));
    }
    let mut units = vec![format!(
        "前回結果の要約説明（要約結果ID: {}; 元結果ID: {}）。元結果は当時の採用本文です。この説明は現在の事実性を再判定するものではありません。",
        prior.id, artifact.source_result_id,
    )];
    for sentence in &summary.sentences {
        units.push(format!(
            "選択文{}（選択順位{}、元結果byte {}..{}）: {}",
            sentence.sentence_index,
            sentence.selection_rank,
            sentence.source_start,
            sentence.source_end,
            sentence.text,
        ));
    }
    units.push(format!(
        "候補文{}件のうち{}件を抽出しました。残り{}件は今回の選択に含まれていません。要約の網羅性・意味・条件保持は未検証です。",
        summary.sentence_count, summary.sentences.len(), summary.omitted_sentence_indices.len(),
    ));
    if units.join("\n").len() > budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the explanation exceeds its evidence budget",
        ));
    }
    let mut dependencies = vec![prior.id.clone()];
    dependencies.extend(prior.dependencies.iter().cloned());
    Ok(Some(PreviousResultSummaryExplanation {
        units,
        dependencies,
        source_context_ref: None,
        sole_summary_packet_index: None,
    }))
}

/// Build a source-linked extractive candidate set from an already retained
/// fetched-body packet. This ranks copied sentences only; it does not verify
/// relevance, coverage, truth, or a source's license.
pub fn summarize_fetched_source(
    source_result_id: &str,
    packet: &EvidencePacket,
    source_packet_ref: &str,
) -> Result<SourceExtractiveSummaryCandidates> {
    summarize_fetched_source_with_limits(source_result_id, packet, source_packet_ref, 3, None)
}

struct SourceVersionSelection {
    active_packet_indices: Vec<usize>,
    source_versions: Vec<SourceQuestionSourceVersion>,
    superseded_version_count: usize,
}

fn select_source_versions(
    packets: &[(usize, &EvidencePacket, String)],
    final_uri_scope: Option<&str>,
) -> Result<SourceVersionSelection> {
    let mut source_groups = Vec::<(String, Vec<usize>)>::new();
    for (packet_index, (_, packet, _)) in packets.iter().enumerate() {
        if final_uri_scope.is_some_and(|scope| scope != packet.final_uri) {
            continue;
        }
        if let Some((_, indices)) = source_groups
            .iter_mut()
            .find(|(source_uri, _)| source_uri == &packet.final_uri)
        {
            if packets[indices[0]].1.mode != packet.mode {
                return Err(failure(
                    "SOURCE_VERSION_MODE_MIXED",
                    "observations of the same URI use different evidence modes",
                ));
            }
            indices.push(packet_index);
        } else {
            source_groups.push((packet.final_uri.clone(), vec![packet_index]));
        }
    }

    let mut active_packet_indices = Vec::with_capacity(source_groups.len());
    let mut source_versions = Vec::with_capacity(source_groups.len());
    let mut superseded_version_count = 0usize;
    for (source_final_uri, packet_indices) in &source_groups {
        let mut versions = Vec::<SourceQuestionPacketVersion>::new();
        for packet_index in packet_indices {
            let (_, packet, _) = &packets[*packet_index];
            let extracted_text = packet
                .extracted_text
                .as_deref()
                .expect("validated source body has extracted text");
            let extracted_text_sha256 = digest(extracted_text.as_bytes());
            if let Some(version) = versions.iter_mut().find(|version| {
                version.raw_body_sha256 == packet.raw_sha256
                    && version.extracted_text_sha256 == extracted_text_sha256
                    && version.parser_version == packet.parser_version
            }) {
                version.latest_observed_at_unix_ms = version
                    .latest_observed_at_unix_ms
                    .max(packet.observed_at_unix_ms);
                version.packet_indices.push(*packet_index);
            } else {
                versions.push(SourceQuestionPacketVersion {
                    raw_body_sha256: packet.raw_sha256.clone(),
                    extracted_text_sha256,
                    parser_version: packet.parser_version.clone(),
                    latest_observed_at_unix_ms: packet.observed_at_unix_ms,
                    packet_indices: vec![*packet_index],
                });
            }
        }
        let latest_observation = versions
            .iter()
            .map(|version| version.latest_observed_at_unix_ms)
            .max()
            .expect("every source group has a packet");
        let latest_versions: Vec<_> = versions
            .iter()
            .enumerate()
            .filter(|(_, version)| version.latest_observed_at_unix_ms == latest_observation)
            .map(|(index, _)| index)
            .collect();
        if latest_versions.len() != 1 {
            return Err(failure(
                "SOURCE_VERSION_AMBIGUOUS",
                "the same exact final URI has different body or parser versions at the latest observed time",
            ));
        }
        let active_version_index = latest_versions[0];
        let active_version = &versions[active_version_index];
        let active_packet_index = *active_version
            .packet_indices
            .iter()
            .max_by(|left, right| {
                let (_, left_packet, left_ref) = &packets[**left];
                let (_, right_packet, right_ref) = &packets[**right];
                left_packet
                    .observed_at_unix_ms
                    .cmp(&right_packet.observed_at_unix_ms)
                    .then_with(|| left_ref.cmp(right_ref))
            })
            .expect("active version has a packet");
        let (_, active_packet, active_packet_ref) = &packets[active_packet_index];

        let mut active_version_packet_refs: Vec<_> = active_version
            .packet_indices
            .iter()
            .map(|index| {
                let (_, packet, packet_ref) = &packets[*index];
                (packet.observed_at_unix_ms, packet_ref.clone())
            })
            .collect();
        active_version_packet_refs.sort();

        let mut superseded_versions = Vec::new();
        for (index, version) in versions.iter().enumerate() {
            if index == active_version_index {
                continue;
            }
            let mut packet_refs: Vec<_> = version
                .packet_indices
                .iter()
                .map(|packet_index| {
                    let (_, packet, packet_ref) = &packets[*packet_index];
                    (packet.observed_at_unix_ms, packet_ref.clone())
                })
                .collect();
            packet_refs.sort();
            superseded_versions.push((
                version.latest_observed_at_unix_ms,
                SourceQuestionPriorVersion {
                    latest_observed_at_unix_ms: version.latest_observed_at_unix_ms,
                    raw_body_sha256: version.raw_body_sha256.clone(),
                    extracted_text_sha256: version.extracted_text_sha256.clone(),
                    parser_version: version.parser_version.clone(),
                    packet_refs: packet_refs
                        .into_iter()
                        .map(|(_, reference)| reference)
                        .collect(),
                },
            ));
        }
        superseded_versions.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.raw_body_sha256.cmp(&right.1.raw_body_sha256))
        });
        superseded_version_count =
            superseded_version_count.saturating_add(superseded_versions.len());
        source_versions.push(SourceQuestionSourceVersion {
            source_final_uri: source_final_uri.clone(),
            active_packet_id: active_packet.id.clone(),
            active_packet_ref: active_packet_ref.clone(),
            active_observed_at_unix_ms: active_packet.observed_at_unix_ms,
            active_raw_body_sha256: active_packet.raw_sha256.clone(),
            active_extracted_text_sha256: active_version.extracted_text_sha256.clone(),
            active_parser_version: active_packet.parser_version.clone(),
            active_version_packet_refs: active_version_packet_refs
                .into_iter()
                .map(|(_, reference)| reference)
                .collect(),
            superseded_versions: superseded_versions
                .into_iter()
                .map(|(_, version)| version)
                .collect(),
        });
        active_packet_indices.push(active_packet_index);
    }

    Ok(SourceVersionSelection {
        active_packet_indices,
        source_versions,
        superseded_version_count,
    })
}

pub(crate) struct RetainedSourceSummary {
    pub(crate) source_packet_refs: Vec<(usize, String)>,
    pub(crate) source_context_ref: String,
    source_result_id: String,
    partial_notice: Option<&'static str>,
    reports: Vec<(usize, SourceExtractiveSummaryCandidates)>,
}

impl RetainedSourceSummary {
    fn sole_packet_index(&self) -> Option<usize> {
        match self.reports.as_slice() {
            [(index, _)] => Some(*index),
            _ => None,
        }
    }

    fn adopted_packet_index(&self, packet_id: &str) -> Result<usize> {
        let mut matches = self
            .reports
            .iter()
            .filter(|(_, report)| report.source_packet_id == packet_id);
        match (matches.next(), matches.next()) {
            (Some((index, _)), None) => Ok(*index),
            _ => Err(source_summary_packet_not_active()),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct SourceFollowup<'a> {
    context: &'a ResultRecord,
    pub(crate) base_context: &'a ResultRecord,
    pub(crate) source: &'a ResultRecord,
    selected_body_index: Option<usize>,
}

impl<'a> SourceFollowup<'a> {
    fn direct(context: &'a ResultRecord) -> Self {
        Self {
            context,
            base_context: context,
            source: context,
            selected_body_index: None,
        }
    }

    pub(crate) fn bodies(self) -> impl Iterator<Item = (usize, &'a EvidencePacket)> {
        source_summary_bodies(self.source).filter(move |(index, _)| {
            self.selected_body_index
                .is_none_or(|selected| *index == selected)
        })
    }

    pub(crate) fn dependencies(self) -> Vec<String> {
        let mut dependencies = vec![self.context.id.clone()];
        if self.source.id != self.context.id {
            dependencies.push(self.source.id.clone());
        }
        dependencies
    }
}

fn source_followup_for_replay<'a>(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    prior: &'a ResultRecord,
    resolved: Option<SourceFollowup<'a>>,
) -> Result<Option<SourceFollowup<'a>>> {
    if let Some(scope) = resolved {
        return Ok((scope.context == prior
            && bundle.meaning.context_result_ref.as_deref() == Some(prior.id.as_str())
            && identity(&Some(prior))? == bundle.context_sha256)
            .then_some(scope));
    }
    source_followup_context(store, bundle, prior_results, prior)
}

struct SourceContextStep<'a> {
    context: &'a ResultRecord,
    detail_input: Option<InputBundle>,
    derivation: &'a ResultRecord,
    derivation_input: InputBundle,
    reuse_sole_summary_packet: bool,
    requested_summary_packet: Option<String>,
}

fn derived_source_followup<'a>(
    store: &Store,
    history: &VerifiedCaseHistory<'_>,
    bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    context: &'a ResultRecord,
) -> Result<Option<SourceFollowup<'a>>> {
    let invalid = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            match bundle.meaning.parser_version.as_str() {
                crate::input::IMPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
                | crate::input::IMPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION => {
                    "the preceding source summary differs from the retained proof"
                }
                crate::input::EXPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
                | crate::input::EXPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION => {
                    "the explicitly referenced source summary differs from the retained proof"
                }
                _ => {
                    "the explicitly referenced source explanation or its derivation differs from the retained proof"
                }
            },
        )
    };
    let referent = |input: &InputBundle| {
        input
            .meaning
            .context_result_ref
            .as_ref()
            .and_then(|reference| prior_results.get(reference))
            .ok_or_else(invalid)
    };
    let registered = |result: &ResultRecord| {
        history
            .input_bundle(&result.input_id, result.input_version)?
            .map(|(_, input)| input)
            .ok_or_else(invalid)
    };
    let mut steps: Vec<SourceContextStep<'a>> = Vec::new();
    let mut visited = BTreeSet::new();
    // Resolve the original scope once, then verify each derived display from
    // oldest to newest. Related summaries and explanations can alternate.
    let mut scope = loop {
        let (input, target) = match steps.last() {
            Some(step) => (&step.derivation_input, referent(&step.derivation_input)?),
            None => (bundle, context),
        };
        let parser_version = input.meaning.parser_version.as_str();
        if !crate::input::uses_derived_source_context(parser_version) {
            break direct_source_followup(store, input, prior_results, target, Some(history))?
                .ok_or_else(invalid)?;
        }
        let expected_request_context =
            (!crate::input::uses_implicit_source_context(parser_version))
                .then_some(target.id.as_str());
        if input.request.context_ref.as_deref() != expected_request_context
            || input.meaning.context_result_ref.as_deref() != Some(target.id.as_str())
            || input.context_sha256 != identity(&Some(target))?
            || target.lifecycle != Lifecycle::Commit
            || target.guarantee != Guarantee::Hypothesis
            || !target.evidence.is_empty()
            || !visited.insert(target.id.as_str())
        {
            return Err(invalid());
        }
        let target_input = registered(target)?;
        if crate::input::uses_summary_source_context(parser_version) {
            if !crate::input::is_source_summary_intent(&target_input.meaning.intent) {
                return Err(invalid());
            }
            let requested_summary_packet =
                if crate::input::uses_adopted_summary_packet_context(parser_version) {
                    let Intent::SummarizePreviousSourcePacketLines { packet_id, .. } =
                        &input.meaning.intent
                    else {
                        return Err(invalid());
                    };
                    Some(packet_id.clone())
                } else {
                    None
                };
            steps.push(SourceContextStep {
                context: target,
                detail_input: None,
                derivation: target,
                derivation_input: target_input,
                reuse_sole_summary_packet: true,
                requested_summary_packet,
            });
            continue;
        }
        let detail_input = target_input;
        if !matches!(detail_input.meaning.intent, Intent::ExplainPrevious) {
            break SourceFollowup::direct(target);
        }
        let derivation = referent(&detail_input)?;
        let derivation_input = registered(derivation)?;
        if !matches!(
            derivation_input.meaning.intent,
            Intent::AskPreviousSourceQuestion { .. }
        ) && !crate::input::is_source_summary_intent(&derivation_input.meaning.intent)
        {
            break SourceFollowup::direct(target);
        }
        if detail_input.meaning.context_result_ref.as_deref() != Some(derivation.id.as_str())
            || detail_input.context_sha256 != identity(&Some(derivation))?
            || derivation.lifecycle != Lifecycle::Commit
        {
            return Err(invalid());
        }
        steps.push(SourceContextStep {
            context: target,
            detail_input: Some(detail_input),
            derivation,
            derivation_input,
            reuse_sole_summary_packet: crate::input::is_explanation_line_summary_version(
                parser_version,
            ),
            requested_summary_packet: None,
        });
    };

    for step in steps.iter().rev() {
        let derivation_plan = step.derivation.content_plan.as_ref().ok_or_else(invalid)?;
        let Some(detail_input) = &step.detail_input else {
            if history
                .input_result(&step.context.input_id, step.context.input_version)?
                .as_ref()
                != Some(step.context)
            {
                return Err(invalid());
            }
            let proof = verify_retained_source_summary(
                store,
                &step.derivation_input,
                prior_results,
                step.derivation,
                derivation_plan,
                Some(scope),
            )?
            .ok_or_else(invalid)?;
            crate::document::revalidate_previous_document_render(
                store,
                &bundle.request.actor_id,
                &bundle.request.case_id,
                step.context,
                None,
                "SOURCE_CONTEXT_INVALID",
                "retrieved source summary",
            )?;
            let index = if let Some(packet_id) = &step.requested_summary_packet {
                proof.adopted_packet_index(packet_id)?
            } else {
                proof.sole_packet_index().ok_or_else(|| {
                    failure(
                        "AMBIGUOUS_SOURCE_CONTEXT",
                        if bundle.meaning.parser_version
                            == crate::input::EXPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
                        {
                            "指定した資料要約に複数の出典があり、行数要約の対象を一つに決められません。"
                        } else {
                            "直前の資料要約に複数の出典があり、行数要約の対象を一つに決められません。"
                        },
                    )
                })?
            };
            scope.selected_body_index = Some(index);
            scope.context = step.context;
            continue;
        };
        let proof = if matches!(
            step.derivation_input.meaning.intent,
            Intent::AskPreviousSourceQuestion { .. }
        ) {
            previous_source_question_explanation(
                store,
                detail_input,
                &step.derivation_input,
                prior_results,
                step.derivation,
                derivation_plan,
                Some((scope, history)),
            )?
        } else {
            previous_source_summary_explanation(
                store,
                detail_input,
                &step.derivation_input,
                prior_results,
                step.derivation,
                derivation_plan,
                Some((scope, history)),
            )?
        };
        let Some(proof) = proof.filter(|proof| proof.source_context_ref.is_some()) else {
            scope = SourceFollowup::direct(step.context);
            continue;
        };
        verify_source_explanation_result(store, detail_input, step.context, &proof, Some(history))?;
        if step.reuse_sole_summary_packet
            && let Some(index) = proof.sole_summary_packet_index
        {
            // The verified report selected this original packet within the incoming scope.
            scope.selected_body_index = Some(index);
        }
        scope.context = step.context;
    }
    Ok(Some(scope))
}

/// Resolve the exact source context used by ordinary requests and their replay.
pub(crate) fn source_followup_context<'a>(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    context: &'a ResultRecord,
) -> Result<Option<SourceFollowup<'a>>> {
    if crate::input::uses_derived_source_context(&bundle.meaning.parser_version) {
        let request = &bundle.request;
        return store.with_verified_case_history(&request.actor_id, &request.case_id, |history| {
            derived_source_followup(store, history, bundle, prior_results, context)
        });
    }
    direct_source_followup(store, bundle, prior_results, context, None)
}

fn direct_source_followup<'a>(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    context: &'a ResultRecord,
    history: Option<&VerifiedCaseHistory<'_>>,
) -> Result<Option<SourceFollowup<'a>>> {
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    if bundle.meaning.context_result_ref.as_deref() != Some(context.id.as_str())
        || identity(&Some(context))? != bundle.context_sha256
        || !crate::recovery::source_context_available(store, actor, case, context, history)?
    {
        return Ok(None);
    }
    let direct = SourceFollowup::direct(context);
    if bundle.meaning.parser_version != crate::input::EXPLAINED_SEARCH_BODY_FOLLOWUP_PARSER_VERSION
        || context.guarantee != Guarantee::SourceReport
        || !context.evidence.is_empty()
    {
        return Ok(Some(direct));
    }

    match history {
        Some(history) => {
            selected_body_followup(store, history, prior_results, context, actor, case)
        }
        None => store.with_verified_case_history(actor, case, |history| {
            selected_body_followup(store, history, prior_results, context, actor, case)
        }),
    }
}

fn selected_body_followup<'a>(
    store: &Store,
    history: &VerifiedCaseHistory<'_>,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    context: &'a ResultRecord,
    actor: &str,
    case: &str,
) -> Result<Option<SourceFollowup<'a>>> {
    let invalid = || {
        failure(
            "SOURCE_CONTEXT_INVALID",
            "the saved explanation or its source differs from the sealed context",
        )
    };
    let (_, explanation) = history
        .input_bundle(&context.input_id, context.input_version)?
        .ok_or_else(invalid)?;
    if !matches!(explanation.meaning.intent, Intent::ExplainPrevious)
        || explanation.meaning.parser_version
            != crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    {
        return Ok(Some(SourceFollowup::direct(context)));
    }
    let source = explanation
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| prior_results.get(id))
        .ok_or_else(invalid)?;
    let Some(proof) = crate::search::previous_search_explanation_in_history(
        store,
        history,
        &explanation,
        prior_results,
        source,
    )?
    else {
        return Ok(Some(SourceFollowup::direct(context)));
    };
    let Some(selected_body_index) = proof.selected_body_index else {
        return Ok(Some(SourceFollowup::direct(context)));
    };
    let plan = context.content_plan.as_ref().ok_or_else(invalid)?;
    if identity(&explanation.request)? != context.id
        || history
            .input_result(&context.input_id, context.input_version)?
            .as_ref()
            != Some(context)
        || context.calculation.is_some()
        || plan.locale != explanation.request.locale
        || plan.required_units != proof.units
        || context.text != proof.units.join("\n")
        || context.dependencies != proof.dependencies
    {
        return Err(invalid());
    }
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        context,
        None,
        "SOURCE_CONTEXT_INVALID",
        "search candidate body explanation",
    )?;
    Ok(Some(SourceFollowup {
        context,
        base_context: context,
        source,
        selected_body_index: Some(selected_body_index),
    }))
}

pub(crate) fn source_summary_unit(report: &SourceExtractiveSummaryCandidates) -> String {
    if report.summary.line_limit.is_some() {
        return format!(
            "{}（出典: {}、出典packet: {}、取得本文SHA-256: {}。ライセンス情報: 未記録。）",
            report.summary.text,
            report.source_final_uri.escape_default(),
            report.source_packet_id,
            report.raw_body_sha256,
        );
    }
    format!(
        "出典: {}\n出典packet: {}\n取得本文SHA-256: {}\nライセンス情報: 未記録。\n{}",
        report.source_final_uri,
        report.source_packet_id,
        report.raw_body_sha256,
        report.summary.text,
    )
}

/// Verify the saved summary using the original source, limits and parser version.
pub(crate) fn verify_retained_source_summary<'a>(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    result: &ResultRecord,
    plan: &ContentPlan,
    resolved: Option<SourceFollowup<'a>>,
) -> Result<Option<RetainedSourceSummary>> {
    if !matches!(
        bundle.meaning.intent,
        Intent::SummarizePreviousSource
            | Intent::SummarizePreviousSourceConstrained { .. }
            | Intent::SummarizePreviousSourceLines { .. }
            | Intent::SummarizePreviousSourcePacketLines { .. }
    ) {
        return Ok(None);
    }
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let parser = crate::input::source_line_summary_parser(&bundle.meaning.intent);
    let line_summary = parser.is_some();
    let version = bundle.meaning.parser_version.as_str();
    let sealed_line_version = matches!(
        version,
        crate::input::SOURCE_LINE_SUMMARY_PARSER_VERSION
            | crate::input::SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
            | crate::input::SCOPED_SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
            | crate::input::EXPLICIT_EXPLANATION_PACKET_LINE_SUMMARY_PARSER_VERSION
            | crate::input::EXPLICIT_EXPLANATION_LINE_SUMMARY_PARSER_VERSION
            | crate::input::IMPLICIT_EXPLANATION_LINE_SUMMARY_PARSER_VERSION
            | crate::input::IMPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
            | crate::input::EXPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
            | crate::input::EXPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION
            | crate::input::IMPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION
    )
    .then_some(version);
    let compatible_line_version = match parser {
        Some(crate::input::SCOPED_SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION) => {
            matches!(
                version,
                crate::input::SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
                    | crate::input::EXPLICIT_EXPLANATION_PACKET_LINE_SUMMARY_PARSER_VERSION
            ) || crate::input::uses_adopted_summary_packet_context(version)
        }
        Some(crate::input::SOURCE_LINE_SUMMARY_PARSER_VERSION) => {
            crate::input::is_explanation_line_summary_version(version)
                || matches!(
                    version,
                    crate::input::IMPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
                        | crate::input::EXPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
                )
        }
        _ => false,
    };
    if parser != sealed_line_version && !compatible_line_version {
        return Err(failure(
            "RECOVERY_SOURCE_SUMMARY_MISMATCH",
            "the retained source line summary intent and parser version differ",
        ));
    }
    let constrained =
        line_summary || constrained_source_summary_limits(&bundle.meaning.intent).is_some();
    let prior_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "source summary has no sealed prior-result reference",
        )
    })?;
    let prior = prior_results.get(prior_id).ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "the exact prior source result is not retained",
        )
    })?;
    let context = source_followup_for_replay(store, bundle, prior_results, prior, resolved)?
        .ok_or_else(|| {
            failure(
                "RECOVERY_SOURCE_CONTEXT_MISMATCH",
                "the referenced source result does not match the sealed context",
            )
        })?;
    let prior = context.source;
    if line_summary && prior.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "the retained line summary source is not a completed retrieval",
        ));
    }
    if !result.evidence.is_empty() {
        return Err(failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "the derived result added undeclared evidence",
        ));
    }
    let mut source_packets = Vec::new();
    for (index, packet) in context.bodies() {
        let packet_bytes = canonical(packet)?;
        let packet_ref = digest(&packet_bytes);
        if store.get_bytes(actor, case, &packet_ref)? != packet_bytes {
            return Err(failure(
                "RECOVERY_SOURCE_PACKET_MISMATCH",
                "the original source packet blob differs from its retained record",
            ));
        }
        source_packets.push((index, packet, packet_ref));
    }
    if source_packets.is_empty() {
        return Err(failure(
            "RECOVERY_SOURCE_CONTEXT_MISMATCH",
            "the prior result lacks fetched bodies",
        ));
    }
    let active_positions = selected_source_summary_positions(&source_packets, &bundle.meaning)?;
    if constrained && active_positions.len() != 1 {
        return Err(failure(
            "RECOVERY_SOURCE_CONTEXT_AMBIGUOUS",
            "the constrained source summary requires exactly one selected source",
        ));
    }
    let source_packet_refs: Vec<_> = source_packets
        .iter()
        .map(|(index, _, reference)| (*index, reference.clone()))
        .collect();
    let mut dependencies = context.dependencies();
    dependencies.extend(
        source_packet_refs
            .iter()
            .map(|(_, reference)| reference.clone()),
    );
    let mut units = Vec::new();
    let mut report_bytes = 0usize;
    let mut reports = Vec::new();
    if let Some(stop) = partial_retrieval_stop(prior) {
        units.push(stop.notice().into());
    }
    for position in active_positions {
        let (index, packet, packet_ref) = &source_packets[position];
        let report = summarize_fetched_source_for_intent(
            &bundle.meaning.intent,
            &prior.id,
            packet,
            packet_ref,
        )?;
        let encoded = canonical(&report)?;
        let unit = source_summary_unit(&report);
        report_bytes = report_bytes.saturating_add(encoded.len());
        if line_summary {
            report_bytes = report_bytes.saturating_add(unit.len());
            if report.summary.sentence_count > request.budget.max_actions as usize {
                return Err(failure(
                    "RECOVERY_SOURCE_SUMMARY_LIMIT",
                    "the retained source sentence scan exceeds the original action budget",
                ));
            }
        }
        if report_bytes > request.budget.max_evidence_bytes {
            return Err(failure(
                "RECOVERY_SOURCE_SUMMARY_LIMIT",
                "the retained source summary exceeds the original evidence budget",
            ));
        }
        let report_ref = digest(&encoded);
        if store.get_bytes(actor, case, &report_ref)? != encoded {
            return Err(failure(
                "RECOVERY_SOURCE_SUMMARY_MISMATCH",
                "the retained source summary artifact differs from the exact source packet",
            ));
        }
        dependencies.push(report_ref);
        units.push(unit);
        reports.push((*index, report));
    }
    if result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.calculation.is_some()
        || dependencies != result.dependencies
        || units != plan.required_units
        || units.join("\n") != result.text
        || plan.text != result.text
        || result.guarantee != Guarantee::Hypothesis
        || plan.guarantee != result.guarantee
    {
        return Err(failure(
            "RECOVERY_SOURCE_SUMMARY_MISMATCH",
            "source dependencies or displayed candidate units differ from Core's retained derivation",
        ));
    }
    Ok(Some(RetainedSourceSummary {
        source_packet_refs,
        source_context_ref: context.base_context.id.clone(),
        source_result_id: prior.id.clone(),
        partial_notice: partial_retrieval_stop(prior).map(PartialRetrievalStop::notice),
        reports,
    }))
}

fn previous_source_summary_explanation<'a>(
    store: &Store,
    explanation: &InputBundle,
    summary_bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    summary_result: &ResultRecord,
    plan: &ContentPlan,
    resolved: Option<(SourceFollowup<'a>, &VerifiedCaseHistory<'_>)>,
) -> Result<Option<PreviousResultSummaryExplanation>> {
    let version = explanation.meaning.parser_version.as_str();
    let legacy = version == crate::input::TEXT_SUMMARY_CANDIDATE_EXPLANATION_PARSER_VERSION
        || crate::input::is_legacy_text_summary_explanation_version(version);
    if !legacy
        && !matches!(
            version,
            crate::input::SOURCE_SUMMARY_EXPLANATION_PARSER_VERSION
                | crate::input::SOURCE_QUESTION_EXPLANATION_PARSER_VERSION
                | crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
                | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
        )
    {
        return Ok(None);
    }
    let Some(proof) = verify_retained_source_summary(
        store,
        summary_bundle,
        prior_results,
        summary_result,
        plan,
        resolved.map(|(scope, _)| scope),
    )?
    else {
        return Ok(None);
    };
    let request = &explanation.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        summary_result,
        None,
        "SOURCE_SUMMARY_OUTPUT_UNAVAILABLE",
        "retrieved source summary",
    )?;
    let source = prior_results.get(&proof.source_result_id).ok_or_else(|| {
        failure(
            "SOURCE_SUMMARY_CONTEXT_INVALID",
            "the original retrieval result is missing",
        )
    })?;
    let (_, source_bundle) = match resolved {
        Some((_, history)) => history.input_bundle(&source.input_id, source.input_version),
        None => store.get_input_bundle(actor, case, &source.input_id, source.input_version),
    }?
    .ok_or_else(|| {
        failure(
            "SOURCE_SUMMARY_CONTEXT_INVALID",
            "the original retrieval request is missing",
        )
    })?;
    if source.id != identity(&source_bundle.request)? {
        return Err(failure(
            "SOURCE_SUMMARY_CONTEXT_INVALID",
            "the retrieval result does not match the original request",
        ));
    }
    let mut dependencies = vec![summary_result.id.clone()];
    if legacy {
        // The old explanation only displayed the already adopted summary.
        return Ok(Some(PreviousResultSummaryExplanation {
            units: vec![summary_result.text.clone()],
            dependencies,
            source_context_ref: None,
            sole_summary_packet_index: None,
        }));
    }

    let mut units = vec![format!(
        "前回の取得資料要約の根拠（要約結果ID: {}、取得結果ID: {}）。",
        summary_result.id, proof.source_result_id,
    )];
    if let Some(notice) = proof.partial_notice {
        units.push(notice.into());
    }
    for (index, (_, report)) in proof.reports.iter().enumerate() {
        let summary = &report.summary;
        let count_limit = match summary.line_limit {
            Some(SummaryLineLimit::Exact(count)) => format!(
                "指定行数: {count}。実際の行数: {}",
                summary.text.lines().count()
            ),
            Some(SummaryLineLimit::AtMost(count)) => format!(
                "行数上限: {count}。実際の行数: {}",
                summary.text.lines().count()
            ),
            None => format!("文数上限{}", summary.max_sentences),
        };
        units.extend([
            format!("出典{}: {}", index + 1, report.source_final_uri),
            format!("取得URL: {}", report.source_request_uri),
            format!(
                "取得モード: {} / 取得時刻（Unix ms）: {}",
                report.source_mode, report.observed_at_unix_ms
            ),
            format!(
                "出典packet ID: {} / SHA-256: {}",
                report.source_packet_id, report.source_packet_ref
            ),
            format!("原本文SHA-256: {}", report.raw_body_sha256),
            format!("抽出本文SHA-256: {}", report.extracted_text_sha256),
            format!(
                "抽出器: {}。ライセンス情報: 未記録。",
                report.parser_version
            ),
            format!(
                "候補文{}件、選択{}件、未選択{}件。{count_limit}、文字数上限{}。",
                summary.sentence_count,
                summary.sentences.len(),
                summary.omitted_sentence_indices.len(),
                summary.max_chars
            ),
        ]);
        for sentence in &summary.sentences {
            let mut chars = sentence.text.chars();
            let mut preview: String = chars.by_ref().take(128).collect();
            if chars.next().is_some() {
                preview.push('…');
            }
            units.push(format!(
                "選択文{}（順位{}、抽出本文byte {}..{}、要約本文byte {}..{}）: {:?}",
                sentence.sentence_index + 1,
                sentence.selection_rank,
                sentence.source_start,
                sentence.source_end,
                sentence.output_start,
                sentence.output_end,
                preview,
            ));
        }
    }
    units.push("要約の網羅性・意味・条件保持・事実性は未検証です。".into());
    if units.join("\n").len() > request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the source-summary explanation exceeds its evidence budget",
        ));
    }
    dependencies.extend(summary_result.dependencies.iter().cloned());
    let sole_summary_packet_index = proof.sole_packet_index();
    Ok(Some(PreviousResultSummaryExplanation {
        units,
        dependencies,
        source_context_ref: Some(proof.source_context_ref),
        sole_summary_packet_index,
    }))
}

pub(crate) fn recompute_source_question_artifact(
    operation: &str,
    source_result_id: &str,
    query: &str,
    packets: &[(usize, &EvidencePacket, String)],
) -> Result<(Vec<u8>, Vec<String>)> {
    match operation {
        "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1" => {
            let answer = answer_fetched_source_question_v1(source_result_id, query, packets)?;
            Ok((
                canonical(&answer)?,
                source_question_answer_lines_v1(&answer),
            ))
        }
        "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2" => {
            let answer = answer_fetched_source_question_v2(source_result_id, query, packets)?;
            Ok((
                canonical(&answer)?,
                source_question_answer_lines_v2(&answer),
            ))
        }
        _ => Err(failure(
            "RECOVERY_SOURCE_QUESTION_VERSION",
            "the retained source-question artifact uses an unsupported derivation version",
        )),
    }
}

pub(crate) fn verified_source_question_packets<'a>(
    store: &Store,
    actor: &str,
    case: &str,
    context: SourceFollowup<'a>,
) -> Result<Vec<(usize, &'a EvidencePacket, String)>> {
    let mut packets = Vec::new();
    for (index, packet) in context.bodies() {
        let encoded = canonical(packet)?;
        let packet_ref = digest(&encoded);
        if store.get_bytes(actor, case, &packet_ref)? != encoded {
            return Err(failure(
                "RECOVERY_SOURCE_QUESTION_PACKET_MISMATCH",
                "the original fetched-body packet differs from its retained artifact",
            ));
        }
        packets.push((index, packet, packet_ref));
    }
    Ok(packets)
}

pub(crate) struct RetainedSourceQuestion {
    pub(crate) source_packet_refs: Vec<(usize, String)>,
    pub(crate) source_context_ref: String,
    pub(crate) answer_ref: String,
    source_result_id: String,
    partial_notice: Option<&'static str>,
    operation: String,
    answer_record: serde_json::Value,
}

fn previous_source_question_explanation<'a>(
    store: &Store,
    explanation: &InputBundle,
    question_bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    question_result: &ResultRecord,
    plan: &ContentPlan,
    resolved: Option<(SourceFollowup<'a>, &VerifiedCaseHistory<'_>)>,
) -> Result<Option<PreviousResultSummaryExplanation>> {
    let version = explanation.meaning.parser_version.as_str();
    let legacy = matches!(
        version,
        crate::input::TEXT_SUMMARY_CANDIDATE_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_SUMMARY_EXPLANATION_PARSER_VERSION
    ) || crate::input::is_legacy_text_summary_explanation_version(version);
    if !legacy
        && !matches!(
            version,
            crate::input::SOURCE_QUESTION_EXPLANATION_PARSER_VERSION
                | crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
                | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
        )
    {
        return Ok(None);
    }
    let Some(proof) = verify_retained_source_question(
        store,
        question_bundle,
        prior_results,
        question_result,
        plan,
        resolved.map(|(scope, _)| scope),
    )?
    else {
        return Ok(None);
    };
    let request = &explanation.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        question_result,
        Some(&proof.answer_ref),
        "SOURCE_QUESTION_OUTPUT_UNAVAILABLE",
        "retrieved source answer",
    )?;
    let invalid = || {
        failure(
            "SOURCE_QUESTION_CONTEXT_INVALID",
            "the original question evidence or retrieval input is missing or changed",
        )
    };
    let source = prior_results
        .get(&proof.source_result_id)
        .ok_or_else(invalid)?;
    let (_, source_bundle) = match resolved {
        Some((_, history)) => history.input_bundle(&source.input_id, source.input_version),
        None => store.get_input_bundle(actor, case, &source.input_id, source.input_version),
    }?
    .ok_or_else(invalid)?;
    if source.id != identity(&source_bundle.request)? {
        return Err(invalid());
    }
    let mut dependencies = vec![question_result.id.clone()];
    if legacy {
        return Ok(Some(PreviousResultSummaryExplanation {
            units: vec![question_result.text.clone()],
            dependencies,
            source_context_ref: None,
            sole_summary_packet_index: None,
        }));
    }

    let (topic, terms, candidates, matched, scope) =
        if proof.operation == "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1" {
            let answer: SourceQuestionAnswer = serde_json::from_value(proof.answer_record)?;
            (
                answer.question_topic,
                answer.query_terms,
                answer.candidates,
                answer.matched_sentence_count,
                format!("保存済み本文packet {}件の全件", answer.source_packet_count),
            )
        } else {
            let answer: SourceQuestionAnswerV2 = serde_json::from_value(proof.answer_record)?;
            (
                answer.question_topic,
                answer.query_terms,
                answer.candidates,
                answer.matched_sentence_count,
                format!(
                    "保存済み本文packet {}件。最終URLごとの採用版 {}件、旧本文版 {}件。",
                    answer.source_packet_count,
                    answer.active_source_count,
                    answer.superseded_version_count
                ),
            )
        };
    let mut units = vec![format!(
        "前回の資料質問の根拠（質問結果ID: {}、取得結果ID: {}）。",
        question_result.id, proof.source_result_id,
    )];
    if let Some(notice) = proof.partial_notice {
        units.push(notice.into());
    }
    units.extend([
        format!("照合対象: {:?}", topic),
        format!("照合語: {}", terms.join("、")),
        format!("照合範囲: {scope}"),
        format!("回答レポート: {} / SHA-256: {}", proof.operation, proof.answer_ref),
        format!("一致した{matched}文から{}候補を採用。一致した語の種類が多い文を優先し、同数のときは資料と抽出位置の順で並べました。", candidates.len()),
    ]);
    for candidate in candidates {
        let packet = proof
            .source_packet_refs
            .iter()
            .find(|(_, reference)| reference == &candidate.source_packet_ref)
            .and_then(|(index, _)| source.evidence.get(*index))
            .ok_or_else(invalid)?;
        let mut chars = candidate.text.chars();
        let mut preview: String = chars.by_ref().take(128).collect();
        if chars.next().is_some() {
            preview.push('…');
        }
        units.extend([
            format!(
                "候補{} / 一致語: {} / 取得モード: {} / 出典: {}",
                candidate.rank,
                candidate.matched_query_terms.join("、"),
                candidate.source_mode,
                candidate.source_final_uri
            ),
            format!(
                "  packet ID: {} / SHA-256: {} / 取得時刻（Unix ms）: {} / 抽出器: {}",
                candidate.source_packet_id,
                candidate.source_packet_ref,
                packet.observed_at_unix_ms,
                packet.parser_version
            ),
            format!(
                "  原本文SHA-256: {} / 抽出本文SHA-256: {}",
                candidate.raw_body_sha256, candidate.extracted_text_sha256
            ),
            format!(
                "  抽出本文byte[{}..{}) / 範囲SHA-256: {} / 原文候補: {:?}",
                candidate.source_start, candidate.source_end, candidate.source_span_sha256, preview
            ),
        ]);
    }
    units.push(
        "語句の一致を確認しました。回答の意味・網羅性・資料の信頼性と事実性は未検証です。".into(),
    );
    if units.join("\n").len() > request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the source-question explanation exceeds its evidence budget",
        ));
    }
    dependencies.extend(question_result.dependencies.iter().cloned());
    Ok(Some(PreviousResultSummaryExplanation {
        units,
        dependencies,
        source_context_ref: Some(proof.source_context_ref),
        sole_summary_packet_index: None,
    }))
}

/// Replay the recorded lexical answer version against the original scoped packets.
pub(crate) fn verify_retained_source_question<'a>(
    store: &Store,
    bundle: &InputBundle,
    prior_results: &'a BTreeMap<String, ResultRecord>,
    result: &ResultRecord,
    plan: &ContentPlan,
    resolved: Option<SourceFollowup<'a>>,
) -> Result<Option<RetainedSourceQuestion>> {
    let Intent::AskPreviousSourceQuestion { query } = &bundle.meaning.intent else {
        return Ok(None);
    };
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let prior_id = bundle.meaning.context_result_ref.as_ref().ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "source question has no sealed prior-result reference",
        )
    })?;
    let prior = prior_results.get(prior_id).ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the exact prior source result is not retained",
        )
    })?;
    let context = source_followup_for_replay(store, bundle, prior_results, prior, resolved)?
        .ok_or_else(|| {
            failure(
                "RECOVERY_SOURCE_QUESTION_MISMATCH",
                "the referenced source result does not match the sealed context",
            )
        })?;
    let prior = context.source;
    let packets = verified_source_question_packets(store, actor, case, context)?;
    let source_packet_refs: Vec<_> = packets
        .iter()
        .map(|(index, _, reference)| (*index, reference.clone()))
        .collect();
    let mut dependencies = context.dependencies();
    dependencies.extend(
        source_packet_refs
            .iter()
            .map(|(_, reference)| reference.clone()),
    );
    let answer_ref = result.dependencies.last().ok_or_else(|| {
        failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the committed source question has no answer artifact reference",
        )
    })?;
    let stored_answer = store.get_bytes(actor, case, answer_ref)?;
    if digest(&stored_answer) != *answer_ref {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the retained source-question answer artifact digest is invalid",
        ));
    }
    let answer_record: serde_json::Value = serde_json::from_slice(&stored_answer)?;
    let operation = answer_record
        .get("operation")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            failure(
                "RECOVERY_SOURCE_QUESTION_VERSION",
                "the retained source-question artifact has no derivation version",
            )
        })?;
    let (encoded, mut expected_units) =
        recompute_source_question_artifact(operation, &prior.id, query, &packets)?;
    if let Some(stop) = partial_retrieval_stop(prior) {
        expected_units.insert(0, stop.notice().into());
    }
    if digest(&encoded) != *answer_ref || stored_answer != encoded {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the retained answer artifact differs from its versioned source-derived reconstruction",
        ));
    }
    let output_bytes = expected_units.join("\n").len();
    if encoded.len() > request.budget.max_evidence_bytes
        || output_bytes > request.budget.max_evidence_bytes
    {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "the retained answer artifact or output exceeds its exact source-derived identity",
        ));
    }
    dependencies.push(answer_ref.clone());
    if result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || result.calculation.is_some()
        || result.text != plan.text
        || result.text != expected_units.join("\n")
        || plan.guarantee != result.guarantee
        || plan.locale != request.locale
        || !result.evidence.is_empty()
        || dependencies != result.dependencies
        || expected_units != plan.required_units
        || result.guarantee != Guarantee::Hypothesis
    {
        return Err(failure(
            "RECOVERY_SOURCE_QUESTION_MISMATCH",
            "source packet dependencies, answer candidates or guarantee differ from the adopted result",
        ));
    }
    Ok(Some(RetainedSourceQuestion {
        source_packet_refs,
        source_context_ref: context.base_context.id.clone(),
        answer_ref: answer_ref.clone(),
        source_result_id: prior.id.clone(),
        partial_notice: partial_retrieval_stop(prior).map(PartialRetrievalStop::notice),
        operation: operation.into(),
        answer_record,
    }))
}

pub(crate) fn source_summary_bodies(
    prior: &ResultRecord,
) -> impl Iterator<Item = (usize, &EvidencePacket)> {
    prior.evidence.iter().enumerate().filter(|(_, packet)| {
        packet.packet_kind == "FETCHED_BODY"
            && packet.failure.is_none()
            && packet.retry_after.is_none()
            && packet.extracted_text.is_some()
    })
}

pub(crate) enum PartialRetrievalStop<'a> {
    AfterAllUrls,
    AtRedirect,
    BeforeFirstHop(FirstHopStop<'a>),
}

pub(crate) enum FirstHopStop<'a> {
    Wait(&'a str),
    Deadline,
    HttpStarts,
    EvidenceBytes,
}

impl PartialRetrievalStop<'_> {
    pub(crate) fn notice(self) -> &'static str {
        match self {
            Self::AfterAllUrls => {
                "元の取得は一部失敗しました。照合できた取得本文を使用しています。"
            }
            Self::AtRedirect => {
                "元の取得は転送時の待機条件で停止しました。照合できた取得本文を使用しています。"
            }
            Self::BeforeFirstHop(FirstHopStop::Wait(_)) => {
                "元の取得は次のURLの接続前に待機条件で停止しました。照合できた取得本文を使用しています。"
            }
            Self::BeforeFirstHop(FirstHopStop::Deadline) => {
                "元の取得は期限に達して停止しました。照合できた取得本文を使用しています。"
            }
            Self::BeforeFirstHop(FirstHopStop::HttpStarts) => {
                "元の取得は通信開始回数の上限に達して停止しました。照合できた取得本文を使用しています。"
            }
            Self::BeforeFirstHop(FirstHopStop::EvidenceBytes) => {
                "元の取得は証拠バイト数の上限に達して停止しました。照合できた取得本文を使用しています。"
            }
        }
    }
}

pub(crate) fn partial_retrieval_stop(result: &ResultRecord) -> Option<PartialRetrievalStop<'_>> {
    if result.lifecycle != Lifecycle::Suspend {
        return None;
    }
    let (code, message) = result.reason.as_deref()?.split_once(": ")?;
    let stop = match code {
        "FETCH_OR_PARSER_FAILURE"
            if result
                .evidence
                .iter()
                .any(|packet| packet.packet_kind == "FETCH_FAILURE") =>
        {
            PartialRetrievalStop::AfterAllUrls
        }
        crate::ports::REDIRECT_RETRY_DEFERRED | crate::ports::RETRY_DEFERRED => {
            let last = result.evidence.last()?;
            if last.packet_kind != "FETCH_FAILURE"
                || last.failure.as_deref() != Some(code)
                || !matches!(last.http_status, 301 | 302 | 303 | 307 | 308)
                || !last.raw_body.is_empty()
                || last.retry_after.is_some() != (code == crate::ports::REDIRECT_RETRY_DEFERRED)
            {
                return None;
            }
            PartialRetrievalStop::AtRedirect
        }
        "EVIDENCE_UNAVAILABLE" => {
            let (cause, detail) = message.split_once(": ")?;
            let stop = match (cause, detail) {
                (crate::ports::RETRY_DEFERRED, detail) => FirstHopStop::Wait(detail),
                (crate::ports::DEADLINE_EXCEEDED, crate::ports::RETRIEVAL_DEADLINE_ELAPSED) => {
                    FirstHopStop::Deadline
                }
                (
                    crate::ports::HTTP_START_BUDGET_EXHAUSTED,
                    crate::ports::NO_REMAINING_HTTP_STARTS,
                ) => FirstHopStop::HttpStarts,
                ("RESOURCE_LIMIT", crate::ports::NO_REMAINING_EVIDENCE_BYTES) => {
                    FirstHopStop::EvidenceBytes
                }
                _ => return None,
            };
            PartialRetrievalStop::BeforeFirstHop(stop)
        }
        _ => return None,
    };
    (result.evidence.iter().all(|packet| {
        matches!(
            packet.packet_kind.as_str(),
            "FETCHED_BODY" | "FETCH_FAILURE"
        )
    }) && source_summary_bodies(result).next().is_some())
    .then_some(stop)
}

fn source_summary_packet_positions(
    packets: &[(usize, &EvidencePacket, String)],
    parser_version: &str,
    requested_packet_id: Option<&str>,
) -> Result<Vec<usize>> {
    if !matches!(
        parser_version,
        crate::input::SOURCE_SUMMARY_PARSER_VERSION
            | crate::input::SOURCE_LINE_SUMMARY_PARSER_VERSION
            | crate::input::SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
            | crate::input::SCOPED_SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
            | crate::input::EXPLICIT_EXPLANATION_PACKET_LINE_SUMMARY_PARSER_VERSION
            | crate::input::EXPLICIT_EXPLANATION_LINE_SUMMARY_PARSER_VERSION
            | crate::input::IMPLICIT_EXPLANATION_LINE_SUMMARY_PARSER_VERSION
            | crate::input::IMPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
            | crate::input::EXPLICIT_SOURCE_SUMMARY_LINE_PARSER_VERSION
            | crate::input::EXPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION
            | crate::input::IMPLICIT_SOURCE_SUMMARY_PACKET_LINE_PARSER_VERSION
            | crate::input::EXPLAINED_SEARCH_BODY_FOLLOWUP_PARSER_VERSION
            | crate::input::EXPLICIT_SOURCE_EXPLANATION_FOLLOWUP_PARSER_VERSION
    ) {
        return Ok((0..packets.len()).collect());
    }
    for (_, packet, reference) in packets {
        validate_source_summary_body(packet, reference)?;
    }
    let final_uri_scope = if matches!(
        parser_version,
        crate::input::SCOPED_SOURCE_PACKET_LINE_SUMMARY_PARSER_VERSION
            | crate::input::EXPLICIT_EXPLANATION_PACKET_LINE_SUMMARY_PARSER_VERSION
    ) || crate::input::uses_adopted_summary_packet_context(parser_version)
    {
        let mut uris = packets.iter().filter_map(|(_, packet, _)| {
            (Some(packet.id.as_str()) == requested_packet_id).then_some(packet.final_uri.as_str())
        });
        let uri = uris.next().ok_or_else(source_summary_packet_not_active)?;
        if uris.any(|other| other != uri) {
            return Err(source_summary_packet_not_active());
        }
        Some(uri)
    } else {
        None
    };
    Ok(select_source_versions(packets, final_uri_scope)?.active_packet_indices)
}

pub(crate) fn selected_source_summary_positions(
    packets: &[(usize, &EvidencePacket, String)],
    meaning: &MeaningIR,
) -> Result<Vec<usize>> {
    let requested_packet_id = match &meaning.intent {
        Intent::SummarizePreviousSourcePacketLines { packet_id, .. } => Some(packet_id.as_str()),
        _ => None,
    };
    let positions =
        source_summary_packet_positions(packets, &meaning.parser_version, requested_packet_id)?;
    let Some(packet_id) = requested_packet_id else {
        return Ok(positions);
    };
    let mut matches = positions
        .into_iter()
        .filter(|position| packets[*position].1.id == packet_id);
    match (matches.next(), matches.next()) {
        (Some(position), None) => Ok(vec![position]),
        _ => Err(source_summary_packet_not_active()),
    }
}

fn source_summary_packet_not_active() -> Error {
    failure(
        "SOURCE_SUMMARY_PACKET_NOT_ACTIVE",
        "指定した出典packetを、参照した取得結果の現行本文から一意に選べません。",
    )
}

pub(crate) fn constrained_source_summary_limits(intent: &Intent) -> Option<(usize, Option<usize>)> {
    match intent {
        Intent::SummarizePreviousSourceConstrained {
            max_sentences,
            max_chars,
        } => Some((max_sentences.unwrap_or(3), *max_chars)),
        _ => None,
    }
}

fn validate_source_summary_body(packet: &EvidencePacket, reference: &str) -> Result<()> {
    if !sha256_id(&packet.id)
        || !sha256_id(reference)
        || packet.packet_kind != "FETCHED_BODY"
        || packet.failure.is_some()
        || packet.retry_after.is_some()
        || !packet.request_uri.starts_with("https://")
        || !packet.final_uri.starts_with("https://")
        || packet.raw_sha256 != digest(&packet.raw_body)
        || packet
            .extracted_text
            .as_deref()
            .is_none_or(|text| text.trim().is_empty())
    {
        return Err(failure(
            "SOURCE_EVIDENCE_INVALID",
            "a verified fetched-body packet with its exact source identities is required",
        ));
    }
    Ok(())
}

pub(crate) fn summarize_fetched_source_with_limits(
    source_result_id: &str,
    packet: &EvidencePacket,
    source_packet_ref: &str,
    max_sentences: usize,
    max_chars: Option<usize>,
) -> Result<SourceExtractiveSummaryCandidates> {
    summarize_fetched_source_internal(
        source_result_id,
        packet,
        source_packet_ref,
        PreviousResultSummaryLimits::Sentences {
            max_sentences,
            max_chars,
        },
    )
}

pub(crate) fn summarize_fetched_source_for_intent(
    intent: &Intent,
    source_result_id: &str,
    packet: &EvidencePacket,
    source_packet_ref: &str,
) -> Result<SourceExtractiveSummaryCandidates> {
    if let Intent::SummarizePreviousSourceLines { limit }
    | Intent::SummarizePreviousSourcePacketLines { limit, .. } = intent
    {
        return summarize_fetched_source_internal(
            source_result_id,
            packet,
            source_packet_ref,
            PreviousResultSummaryLimits::Lines(*limit),
        );
    }
    if let Some((max_sentences, max_chars)) = constrained_source_summary_limits(intent) {
        return summarize_fetched_source_with_limits(
            source_result_id,
            packet,
            source_packet_ref,
            max_sentences,
            max_chars,
        );
    }
    summarize_fetched_source(source_result_id, packet, source_packet_ref)
}

fn summarize_fetched_source_internal(
    source_result_id: &str,
    packet: &EvidencePacket,
    source_packet_ref: &str,
    limits: PreviousResultSummaryLimits,
) -> Result<SourceExtractiveSummaryCandidates> {
    if !sha256_id(source_result_id) {
        return Err(failure(
            "SOURCE_EVIDENCE_INVALID",
            "a verified fetched-body packet with its exact source identities is required",
        ));
    }
    validate_source_summary_body(packet, source_packet_ref)?;
    let text = packet
        .extracted_text
        .as_deref()
        .expect("validated fetched body has text");
    let document = match limits {
        PreviousResultSummaryLimits::Lines(limit) => serde_json::to_string(&serde_json::json!({
            "text": text,
            "line_limit": limit,
            "format": "bullets"
        }))?,
        PreviousResultSummaryLimits::Sentences {
            max_sentences,
            max_chars: Some(max_chars),
        } => serde_json::to_string(&serde_json::json!({
            "text": text,
            "max_sentences": max_sentences,
            "max_chars": max_chars,
            "format": "bullets"
        }))?,
        PreviousResultSummaryLimits::Sentences {
            max_sentences,
            max_chars: None,
        } => serde_json::to_string(&serde_json::json!({
            "text": text,
            "max_sentences": max_sentences,
            "format": "bullets"
        }))?,
    };
    let summary = extractive_summary_candidates(&document)?;
    if summary.source_text_sha256 != digest(text.as_bytes()) {
        return Err(failure(
            "SOURCE_SUMMARY_SPAN_MISMATCH",
            "summary source text differs from the fetched packet's extracted text",
        ));
    }
    Ok(SourceExtractiveSummaryCandidates {
        operation: if summary.line_limit.is_some() {
            "SOURCE_EXTRACTIVE_LINE_SUMMARY_CANDIDATES_V1"
        } else {
            "SOURCE_EXTRACTIVE_SUMMARY_CANDIDATES_V1"
        }
        .into(),
        source_result_id: source_result_id.into(),
        source_packet_id: packet.id.clone(),
        source_packet_ref: source_packet_ref.into(),
        source_mode: packet.mode.clone(),
        source_request_uri: packet.request_uri.clone(),
        source_final_uri: packet.final_uri.clone(),
        observed_at_unix_ms: packet.observed_at_unix_ms,
        raw_body_sha256: packet.raw_sha256.clone(),
        extracted_text_sha256: summary.source_text_sha256.clone(),
        parser_version: packet.parser_version.clone(),
        license_status: "NOT_RECORDED_IN_EVIDENCE_PACKET".into(),
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_question_packet(text: &str) -> EvidencePacket {
        let raw_body = text.as_bytes().to_vec();
        EvidencePacket {
            id: "b".repeat(64),
            mode: "LIVE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: "https://example.test/article".into(),
            final_uri: "https://example.test/article".into(),
            redirect_chain: Vec::new(),
            observed_at_unix_ms: 123,
            provider_id: "https-direct-url-v1".into(),
            http_status: 200,
            raw_sha256: digest(&raw_body),
            extracted_text: Some(text.into()),
            raw_body,
            parser_version: "strict-utf8-identity-v1".into(),
            independence_group: "https://example.test/article".into(),
            failure: None,
            retry_after: None,
        }
    }

    #[test]
    fn source_question_returns_exact_lexical_matches_with_source_spans() {
        let text = "Energy enters the ecosystem. Photosynthesis stores light energy. Plants use photosynthesis in their leaves.";
        let packet = source_question_packet(text);
        let packet_ref = digest(&canonical(&packet).unwrap());
        let answer = answer_fetched_source_question_v1(
            &"a".repeat(64),
            "photosynthesis",
            &[(2, &packet, packet_ref.clone())],
        )
        .unwrap();
        assert_eq!(answer.operation, "SOURCE_QUESTION_EXTRACTIVE_CANDIDATES_V1");
        assert_eq!(answer.query_terms, vec!["photosynthesis"]);
        assert_eq!(answer.matched_sentence_count, 2);
        assert_eq!(answer.candidates.len(), 2);
        assert_eq!(answer.candidates[0].source_packet_ref, packet_ref);
        assert_eq!(
            answer.candidates[0].text,
            "Photosynthesis stores light energy."
        );
        assert_eq!(
            answer.candidates[1].text,
            "Plants use photosynthesis in their leaves."
        );
        for candidate in &answer.candidates {
            assert_eq!(
                text.get(candidate.source_start..candidate.source_end),
                Some(candidate.text.as_str())
            );
            assert_eq!(
                candidate.source_span_sha256,
                digest(candidate.text.as_bytes())
            );
            assert!(
                candidate
                    .matched_query_terms
                    .contains(&"photosynthesis".into())
            );
        }
        assert!(answer.guarantee.contains("semantic relevance"));
    }

    #[test]
    fn source_question_supports_japanese_substrings_and_refuses_no_match() {
        let text = "植物の光合成は光を化学エネルギーへ変える。呼吸は別の過程である。";
        let packet = source_question_packet(text);
        let packet_ref = digest(&canonical(&packet).unwrap());
        let sources = [(0, &packet, packet_ref)];
        let answer =
            answer_fetched_source_question_v1(&"a".repeat(64), "光合成", &sources).unwrap();
        assert_eq!(
            answer.candidates[0].text,
            "植物の光合成は光を化学エネルギーへ変える。"
        );
        let error =
            answer_fetched_source_question_v1(&"a".repeat(64), "火山", &sources).unwrap_err();
        assert!(error.to_string().contains("SOURCE_QUESTION_NO_MATCH"));
    }

    fn observed_source_question_packet(text: &str, observed_at_unix_ms: u64) -> EvidencePacket {
        let mut packet = source_question_packet(text);
        packet.raw_body = text.as_bytes().to_vec();
        packet.raw_sha256 = digest(&packet.raw_body);
        packet.extracted_text = Some(text.into());
        packet.observed_at_unix_ms = observed_at_unix_ms;
        packet
    }

    #[test]
    fn source_question_v2_uses_the_latest_exact_uri_version_and_retains_old_identity() {
        let old = observed_source_question_packet(
            "Photosynthesis was thought to store light energy.",
            100,
        );
        let latest = observed_source_question_packet(
            "Photosynthesis stores chemical energy from light.",
            200,
        );
        let old_ref = digest(&canonical(&old).unwrap());
        let latest_ref = digest(&canonical(&latest).unwrap());
        let answer = answer_fetched_source_question_v2(
            &"a".repeat(64),
            "photosynthesis",
            &[(0, &old, old_ref.clone()), (1, &latest, latest_ref.clone())],
        )
        .unwrap();

        assert_eq!(
            answer.operation,
            "SOURCE_QUESTION_VERSIONED_EXTRACTIVE_CANDIDATES_V2"
        );
        assert_eq!(answer.source_packet_count, 2);
        assert_eq!(answer.active_source_count, 1);
        assert_eq!(answer.superseded_version_count, 1);
        assert_eq!(answer.matched_sentence_count, 1);
        assert_eq!(answer.candidates.len(), 1);
        assert_eq!(answer.candidates[0].source_packet_ref, latest_ref);
        assert_eq!(answer.candidates[0].text, latest.extracted_text.unwrap());
        let source_version = &answer.source_versions[0];
        assert_eq!(source_version.source_final_uri, old.final_uri);
        assert_eq!(source_version.active_packet_ref, latest_ref);
        assert_eq!(source_version.active_raw_body_sha256, latest.raw_sha256);
        assert_eq!(source_version.active_observed_at_unix_ms, 200);
        assert_eq!(source_version.active_version_packet_refs, vec![latest_ref]);
        assert_eq!(source_version.superseded_versions.len(), 1);
        assert_eq!(
            source_version.superseded_versions[0].raw_body_sha256,
            old.raw_sha256
        );
        assert_eq!(
            source_version.superseded_versions[0].packet_refs,
            vec![old_ref]
        );

        let rendered = source_question_answer_lines_v2(&answer).join("\n");
        assert!(rendered.contains("旧本文版は履歴に保持"));
        assert!(rendered.contains("Photosynthesis stores chemical energy from light."));
        assert!(!rendered.contains("Photosynthesis was thought to store light energy."));
    }

    #[test]
    fn source_question_v2_suspends_on_same_time_conflicts_and_mixed_modes() {
        let first = observed_source_question_packet("Photosynthesis stores light energy.", 100);
        let conflict =
            observed_source_question_packet("Photosynthesis stores chemical energy.", 100);
        let first_ref = digest(&canonical(&first).unwrap());
        let conflict_ref = digest(&canonical(&conflict).unwrap());
        let packets = [(0, &first, first_ref), (1, &conflict, conflict_ref)];
        let error = answer_fetched_source_question_v2(&"a".repeat(64), "photosynthesis", &packets)
            .unwrap_err();
        assert!(error.to_string().contains("SOURCE_VERSION_AMBIGUOUS"));
        let error = source_summary_packet_positions(
            &packets,
            crate::input::SOURCE_SUMMARY_PARSER_VERSION,
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("SOURCE_VERSION_AMBIGUOUS"));
        assert_eq!(
            source_summary_packet_positions(&packets, crate::input::PARSER_VERSION, None).unwrap(),
            vec![0, 1]
        );

        let mut fixture =
            observed_source_question_packet("Photosynthesis stores light energy.", 100);
        fixture.mode = "FIXTURE".into();
        let live_ref = digest(&canonical(&first).unwrap());
        let fixture_ref = digest(&canonical(&fixture).unwrap());
        let error = answer_fetched_source_question_v2(
            &"a".repeat(64),
            "photosynthesis",
            &[(0, &first, live_ref), (1, &fixture, fixture_ref)],
        )
        .unwrap_err();
        assert!(error.to_string().contains("SOURCE_QUESTION_MODE_MIXED"));
    }

    #[test]
    fn source_question_v2_deduplicates_same_body_observations_without_claiming_independence() {
        let first = observed_source_question_packet("Photosynthesis stores light energy.", 100);
        let second = observed_source_question_packet("Photosynthesis stores light energy.", 200);
        let first_ref = digest(&canonical(&first).unwrap());
        let second_ref = digest(&canonical(&second).unwrap());
        let answer = answer_fetched_source_question_v2(
            &"a".repeat(64),
            "photosynthesis",
            &[
                (0, &first, first_ref.clone()),
                (1, &second, second_ref.clone()),
            ],
        )
        .unwrap();
        assert_eq!(answer.active_source_count, 1);
        assert_eq!(answer.superseded_version_count, 0);
        assert_eq!(answer.source_versions[0].active_packet_ref, second_ref);
        assert_eq!(
            answer.source_versions[0].active_version_packet_refs,
            vec![first_ref, second_ref]
        );
        assert!(answer.guarantee.contains("source independence"));
    }

    #[test]
    fn fetched_source_followup_retains_packet_identity_and_unobserved_license() {
        let raw_body = "First source sentence. Second source sentence."
            .as_bytes()
            .to_vec();
        let packet = EvidencePacket {
            id: "b".repeat(64),
            mode: "LIVE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: "https://example.test/report".into(),
            final_uri: "https://example.test/report".into(),
            redirect_chain: Vec::new(),
            observed_at_unix_ms: 123,
            provider_id: "https-direct-url-v1".into(),
            http_status: 200,
            raw_sha256: digest(&raw_body),
            extracted_text: Some(String::from_utf8(raw_body.clone()).unwrap()),
            raw_body,
            parser_version: "strict-utf8-identity-v1".into(),
            independence_group: "https://example.test/report".into(),
            failure: None,
            retry_after: None,
        };
        let source_ref = "c".repeat(64);
        let report = summarize_fetched_source(&"a".repeat(64), &packet, &source_ref).unwrap();
        assert_eq!(report.source_packet_id, packet.id);
        assert_eq!(report.source_packet_ref, source_ref);
        assert_eq!(report.raw_body_sha256, packet.raw_sha256);
        assert_eq!(report.license_status, "NOT_RECORDED_IN_EVIDENCE_PACKET");
        assert!(!report.summary.coverage_verified);
        assert_eq!(report.summary.sentences.len(), 2);
        assert!(report.summary.sentences.iter().all(|sentence| {
            report
                .summary
                .source_text
                .get(sentence.source_start..sentence.source_end)
                == Some(sentence.text.as_str())
        }));

        let constrained = summarize_fetched_source_with_limits(
            &"a".repeat(64),
            &packet,
            &"c".repeat(64),
            1,
            Some(24),
        )
        .unwrap();
        assert_eq!(constrained.summary.max_sentences, 1);
        assert_eq!(constrained.summary.max_chars, 24);
        assert_eq!(constrained.summary.format, "bullets");
        assert_eq!(constrained.summary.sentences.len(), 1);
        assert!(constrained.summary.selected_character_count <= 24);
        assert_eq!(constrained.source_packet_id, packet.id);
        assert_eq!(constrained.raw_body_sha256, packet.raw_sha256);
    }

    #[test]
    fn ranks_and_retains_exact_japanese_source_ranges() {
        let text = "猫は走る。猫は休む。雨なら猫は外へ出ない。";
        let document = serde_json::json!({"text":text,"max_sentences":2}).to_string();
        let report = extractive_summary_candidates(&document).unwrap();
        assert_eq!(report.operation, OPERATION);
        assert_eq!(report.format, "bullets");
        assert!(report.required_sentence_indices.is_empty());
        assert_eq!(report.sentence_count, 3);
        assert_eq!(report.selected_sentence_indices, vec![0, 2]);
        assert_eq!(report.omitted_sentence_indices, vec![1]);
        assert!(!report.coverage_verified);
        for span in &report.sentences {
            assert_eq!(
                report.source_text.get(span.source_start..span.source_end),
                Some(span.text.as_str())
            );
            assert_eq!(
                report.text.get(span.output_start..span.output_end),
                Some(span.text.as_str())
            );
        }
        assert!(report.text.starts_with(EXTRACTIVE_SUMMARY_LABEL));
    }

    #[test]
    fn rejects_ambiguous_schema_empty_text_and_invalid_limits() {
        for document in [
            r#"{"text":"x。","max_sentences":1,"extra":true}"#,
            r#"{"text":"  ","max_sentences":1}"#,
            r#"{"text":"x。","max_sentences":9}"#,
            r#"{"text":"x。","max_sentences":1.5}"#,
            r#"{"text":"x。","max_sentences":1,"max_chars":0}"#,
            r#"{"text":"x。","max_sentences":1,"max_chars":100001}"#,
            r#"{"text":"x。","max_sentences":1,"max_chars":null}"#,
            r#"{"text":"x。","max_sentences":1,"format":"table"}"#,
            r#"{"text":"x。","max_sentences":1,"format":null}"#,
            r#"{"text":"x。","max_sentences":1,"format":1}"#,
            r#"{"text":"x。","max_sentences":1,"required_sentence_indices":null}"#,
            r#"{"text":"x。","max_sentences":1,"required_sentence_indices":"0"}"#,
            r#"{"text":"x。","max_sentences":1,"required_sentence_indices":[-1]}"#,
            r#"{"text":"x。","max_sentences":1,"required_sentence_indices":[1.5]}"#,
            r#"{"text":"x。","max_sentences":1,"required_sentence_indices":[0,0]}"#,
            r#"{"text":"x。","max_sentences":8,"required_sentence_indices":[0,1,2,3,4,5,6,7,8]}"#,
        ] {
            assert!(
                extractive_summary_candidates(document).is_err(),
                "{document}"
            );
        }
    }

    #[test]
    fn preserves_repeated_punctuation_and_line_boundaries_without_normalizing_text() {
        let text = "一行目！！\r\n二行目。  三行目";
        let document = serde_json::json!({"text":text,"max_sentences":8}).to_string();
        let report = extractive_summary_candidates(&document).unwrap();
        let actual: Vec<_> = report
            .sentences
            .iter()
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(actual, vec!["一行目！！", "二行目。", "三行目"]);
    }

    #[test]
    fn enforces_character_budget_without_truncating_or_discarding_later_fitting_sentences() {
        let text = "shared concept first. shared concept second. unique third.";
        let document = serde_json::json!({
            "text":text,
            "max_sentences":2,
            "max_chars":35
        })
        .to_string();
        let report = extractive_summary_candidates(&document).unwrap();
        assert_eq!(report.operation, "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4");
        assert_eq!(report.format, "bullets");
        assert!(report.required_sentence_indices.is_empty());
        assert_eq!(report.max_chars, 35);
        assert_eq!(report.selected_sentence_indices, vec![0, 2]);
        assert_eq!(report.omitted_sentence_indices, vec![1]);
        assert_eq!(report.length_limited_sentence_indices, vec![1]);
        assert_eq!(report.selected_character_count, 34);
        assert!(report.selected_character_count <= report.max_chars);
        assert!(report.text.contains("shared concept first."));
        assert!(report.text.contains("unique third."));
        assert!(!report.text.contains("shared concept second."));
        for span in &report.sentences {
            assert_eq!(
                report.source_text.get(span.source_start..span.source_end),
                Some(span.text.as_str())
            );
            assert_eq!(
                report.text.get(span.output_start..span.output_end),
                Some(span.text.as_str())
            );
        }
    }

    #[test]
    fn renders_paragraph_layout_with_exact_unicode_source_and_output_spans() {
        let text = "猫は走る。猫は休む。";
        let document = serde_json::json!({
            "text":text,
            "max_sentences":2,
            "format":"paragraph"
        })
        .to_string();
        let report = extractive_summary_candidates(&document).unwrap();
        assert_eq!(report.operation, OPERATION);
        assert_eq!(report.format, "paragraph");
        assert!(
            report
                .text
                .starts_with(&format!("{EXTRACTIVE_SUMMARY_LABEL}\n"))
        );
        assert!(!report.text.contains("\n- "));
        let copied = report
            .sentences
            .iter()
            .map(|sentence| sentence.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(report.text, format!("{EXTRACTIVE_SUMMARY_LABEL}\n{copied}"));
        for sentence in &report.sentences {
            assert_eq!(
                report
                    .source_text
                    .get(sentence.source_start..sentence.source_end),
                Some(sentence.text.as_str())
            );
            assert_eq!(
                report.text.get(sentence.output_start..sentence.output_end),
                Some(sentence.text.as_str())
            );
        }
        assert_eq!(report.output_sha256, digest(report.text.as_bytes()));
    }

    #[test]
    fn retains_required_source_sentences_before_ranked_fill_and_maps_them_exactly() {
        let text = "shared alpha. shared beta. unique gamma. shared delta.";
        let document = serde_json::json!({
            "text":text,
            "max_sentences":2,
            "max_chars":26,
            "required_sentence_indices":[2]
        })
        .to_string();
        let report = extractive_summary_candidates(&document).unwrap();
        assert_eq!(report.operation, OPERATION);
        assert_eq!(report.required_sentence_indices, vec![2]);
        assert_eq!(report.selected_sentence_indices, vec![0, 2]);
        assert_eq!(report.selected_character_count, 26);
        assert!(report.text.contains("shared alpha."));
        assert!(report.text.contains("unique gamma."));
        assert!(!report.text.contains("shared beta."));
        assert!(!report.text.contains("shared delta."));
        let required = report
            .sentences
            .iter()
            .find(|sentence| sentence.sentence_index == 2)
            .unwrap();
        let ranked = report
            .sentences
            .iter()
            .find(|sentence| sentence.sentence_index == 0)
            .unwrap();
        assert_eq!(required.selection_rank, 1);
        assert_eq!(ranked.selection_rank, 2);
        for sentence in &report.sentences {
            assert_eq!(
                report
                    .source_text
                    .get(sentence.source_start..sentence.source_end),
                Some(sentence.text.as_str())
            );
            assert_eq!(
                report.text.get(sentence.output_start..sentence.output_end),
                Some(sentence.text.as_str())
            );
        }
    }

    #[test]
    fn rejects_required_source_indices_that_are_unavailable_or_exceed_budgets() {
        for (document, code) in [
            (
                r#"{"text":"Only one.","max_sentences":1,"required_sentence_indices":[1]}"#,
                "TEXT_SUMMARY_REQUIRED_INDEX",
            ),
            (
                r#"{"text":"One. Two.","max_sentences":1,"required_sentence_indices":[0,1]}"#,
                "TEXT_SUMMARY_REQUIRED_LIMIT",
            ),
            (
                r#"{"text":"Required sentence.","max_sentences":1,"max_chars":4,"required_sentence_indices":[0]}"#,
                "TEXT_SUMMARY_REQUIRED_CHAR_LIMIT",
            ),
        ] {
            let error = extractive_summary_candidates(document).unwrap_err();
            assert!(error.to_string().contains(code), "{error}");
        }
    }
}
