//! Deterministic extractive sentence candidates with exact source spans.
//! This ranks supplied sentences; it does not verify summary coverage or meaning.
use crate::ports::{JsonKind, strict_json_document};
use minidora_domain::{EvidencePacket, Result, digest, failure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Range;

const MAX_REQUEST_BYTES: usize = 256_000;
const MAX_TEXT_SCALARS: usize = 100_000;
const MAX_SENTENCES: usize = 4_096;
const MAX_ARTIFACT_BYTES: usize = 2_000_000;
const OPERATION: &str = "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V4";
const ALGORITHM: &str =
    "TOKEN_FREQUENCY_FIRST_SENTENCE_LONG_SENTENCE_PENALTY_REQUIRED_CONTENT_OUTPUT_FORMAT_V1";
const LABEL: &str =
    "抽出候補です。文の頻度と位置で選択しました。目的・意味・条件・否定の網羅は検証していません。";
const GUARANTEE: &str = "deterministic extractive sentence candidates ranked by token frequency, a first-sentence prior, and a long-sentence penalty; caller-required source sentences are retained before ranked fill, and every retained sentence is copied verbatim with decoded-source byte offsets in the requested bullets or paragraph layout. Relevance, completeness, purpose, meaning, conditions, negation, factual truth, and writing quality are not verified";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SummaryRequest {
    text: String,
    max_sentences: usize,
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
    let required = ["max_sentences", "text"];
    if !(2..=5).contains(&values.len())
        || required.iter().any(|key| !values.contains_key(*key))
        || values.keys().any(|key| {
            !matches!(
                key.as_str(),
                "text" | "max_sentences" | "max_chars" | "format" | "required_sentence_indices"
            )
        })
    {
        return Err(schema(
            "summary request fields must include text and max_sentences; max_chars, format, and required_sentence_indices are optional",
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

fn sentence_candidates(text: &str) -> Result<Vec<Candidate>> {
    let mut sentences = Vec::new();
    let mut start = 0usize;
    let mut chars = text.char_indices().peekable();
    while let Some((at, character)) = chars.next() {
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
        } else if matches!(character, '\n' | '\r') {
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

/// Select a bounded set of source sentences by the inspected deterministic
/// extractive ranking family. This is a candidate operation, not a semantic
/// summary verifier; every selected sentence remains tied to exact source bytes.
pub fn extractive_summary_candidates(document: &str) -> Result<ExtractiveSummaryCandidates> {
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
    let max_node = &fields["max_sentences"];
    if !matches!(max_node.kind, JsonKind::Number) {
        return Err(schema("max_sentences must be an integer from 1 to 8"));
    }
    let SummaryRequest {
        text,
        max_sentences,
        max_chars,
        format,
        required_sentence_indices,
    } = serde_json::from_str(document).map_err(|error| schema(error.to_string()))?;
    if !(1..=8).contains(&max_sentences) {
        return Err(schema("max_sentences must be an integer from 1 to 8"));
    }
    let max_chars = max_chars.unwrap_or(MAX_TEXT_SCALARS);
    if !(1..=MAX_TEXT_SCALARS).contains(&max_chars) {
        return Err(schema("max_chars must be an integer from 1 to 100000"));
    }
    let format = SummaryFormat::from_request(format.as_deref())?;
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
    let mut candidates = sentence_candidates(&text)?;
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
    let mut selection_ranks: BTreeMap<usize, usize> = selected_ranked
        .iter()
        .enumerate()
        .map(|(rank, index)| (*index, rank + 1))
        .collect();
    let mut length_limited_sentence_indices = Vec::new();
    for index in ranking {
        if selected_ranked.len() == max_sentences {
            break;
        }
        if required_set.contains(&index) {
            continue;
        }
        let sentence_characters = candidates[index].text.chars().count();
        if selected_character_count.saturating_add(sentence_characters) <= max_chars {
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

    let mut output = String::from(LABEL);
    let mut spans = Vec::with_capacity(selected.len());
    for index in &selected {
        let candidate = &candidates[*index];
        match format {
            SummaryFormat::Bullets => output.push_str("\n- "),
            SummaryFormat::Paragraph if spans.is_empty() => output.push('\n'),
            SummaryFormat::Paragraph => output.push(' '),
        }
        let output_start = output.len();
        output.push_str(&candidate.text);
        let output_end = output.len();
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
        operation: OPERATION.into(),
        algorithm: ALGORITHM.into(),
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
    };
    if minidora_domain::canonical(&report)?.len() > MAX_ARTIFACT_BYTES {
        return Err(failure(
            "TEXT_SUMMARY_ARTIFACT_LIMIT",
            "extractive summary report exceeds 2000000 serialized bytes",
        ));
    }
    Ok(report)
}

/// Build a source-linked extractive candidate set from an already retained
/// fetched-body packet. This ranks copied sentences only; it does not verify
/// relevance, coverage, truth, or a source's license.
pub fn summarize_fetched_source(
    source_result_id: &str,
    packet: &EvidencePacket,
    source_packet_ref: &str,
) -> Result<SourceExtractiveSummaryCandidates> {
    let text = packet.extracted_text.as_deref().unwrap_or_default();
    if !sha256_id(source_result_id)
        || !sha256_id(&packet.id)
        || !sha256_id(source_packet_ref)
        || packet.packet_kind != "FETCHED_BODY"
        || packet.failure.is_some()
        || !packet.request_uri.starts_with("https://")
        || !packet.final_uri.starts_with("https://")
        || packet.raw_sha256 != digest(&packet.raw_body)
        || text.trim().is_empty()
    {
        return Err(failure(
            "SOURCE_EVIDENCE_INVALID",
            "a verified fetched-body packet with its exact source identities is required",
        ));
    }
    let document = serde_json::to_string(&serde_json::json!({
        "text": text,
        "max_sentences": 3,
        "format": "bullets"
    }))?;
    let summary = extractive_summary_candidates(&document)?;
    if summary.source_text_sha256 != digest(text.as_bytes()) {
        return Err(failure(
            "SOURCE_SUMMARY_SPAN_MISMATCH",
            "summary source text differs from the fetched packet's extracted text",
        ));
    }
    Ok(SourceExtractiveSummaryCandidates {
        operation: "SOURCE_EXTRACTIVE_SUMMARY_CANDIDATES_V1".into(),
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
        assert!(report.text.starts_with(LABEL));
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
        assert!(report.text.starts_with(&format!("{LABEL}\n")));
        assert!(!report.text.contains("\n- "));
        let copied = report
            .sentences
            .iter()
            .map(|sentence| sentence.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(report.text, format!("{LABEL}\n{copied}"));
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
