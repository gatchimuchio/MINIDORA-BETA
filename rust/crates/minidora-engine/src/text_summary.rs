//! Deterministic extractive sentence candidates with exact source spans.
//! This ranks supplied sentences; it does not verify summary coverage or meaning.
use crate::ports::{JsonKind, strict_json_document};
use minidora_domain::{Result, digest, failure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Range;

const MAX_REQUEST_BYTES: usize = 256_000;
const MAX_TEXT_SCALARS: usize = 100_000;
const MAX_SENTENCES: usize = 4_096;
const MAX_ARTIFACT_BYTES: usize = 2_000_000;
const OPERATION: &str = "TEXT_EXTRACTIVE_SUMMARY_CANDIDATES_V1";
const ALGORITHM: &str = "TOKEN_FREQUENCY_FIRST_SENTENCE_LONG_SENTENCE_PENALTY_V1";
const LABEL: &str =
    "抽出候補です。文の頻度と位置で選択しました。目的・意味・条件・否定の網羅は検証していません。";
const GUARANTEE: &str = "deterministic extractive sentence candidates ranked by token frequency, a first-sentence prior, and a long-sentence penalty; each retained sentence is copied verbatim with decoded-source byte offsets. Relevance, completeness, purpose, meaning, conditions, negation, factual truth, and writing quality are not verified";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SummaryRequest {
    text: String,
    max_sentences: usize,
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
    pub max_sentences: usize,
    pub sentence_count: usize,
    pub selected_sentence_indices: Vec<usize>,
    pub omitted_sentence_indices: Vec<usize>,
    pub sentences: Vec<ExtractedSentence>,
    pub coverage_verified: bool,
    pub guarantee: String,
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

fn fields(node: &crate::ports::JsonNode) -> Result<&BTreeMap<String, crate::ports::JsonNode>> {
    let JsonKind::Object(values) = &node.kind else {
        return Err(schema("summary request must be one JSON object"));
    };
    let expected = ["max_sentences", "text"];
    if values.len() != expected.len() || expected.iter().any(|key| !values.contains_key(*key)) {
        return Err(schema(
            "summary request fields must be exactly max_sentences and text",
        ));
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
    let fields = fields(&root)?;
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
    } = serde_json::from_str(document).map_err(|error| schema(error.to_string()))?;
    if !(1..=8).contains(&max_sentences) {
        return Err(schema("max_sentences must be an integer from 1 to 8"));
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
    score_sentences(&mut candidates);
    let mut ranking: Vec<usize> = (0..candidates.len()).collect();
    ranking.sort_by(|left, right| {
        compare_scores(candidates[*right].score, candidates[*left].score)
            .then_with(|| right.cmp(left))
    });
    let selected_ranked: Vec<usize> = ranking.into_iter().take(max_sentences).collect();
    let selection_ranks: BTreeMap<usize, usize> = selected_ranked
        .iter()
        .enumerate()
        .map(|(rank, index)| (*index, rank + 1))
        .collect();
    let mut selected: Vec<usize> = selected_ranked;
    selected.sort_unstable();
    let selected_set: std::collections::BTreeSet<_> = selected.iter().copied().collect();

    let mut output = String::from(LABEL);
    let mut spans = Vec::with_capacity(selected.len());
    for index in &selected {
        let candidate = &candidates[*index];
        output.push_str("\n- ");
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
        max_sentences,
        sentence_count: candidates.len(),
        selected_sentence_indices,
        omitted_sentence_indices,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranks_and_retains_exact_japanese_source_ranges() {
        let text = "猫は走る。猫は休む。雨なら猫は外へ出ない。";
        let document = serde_json::json!({"text":text,"max_sentences":2}).to_string();
        let report = extractive_summary_candidates(&document).unwrap();
        assert_eq!(report.operation, OPERATION);
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
}
