//! Explicit text import and atomic source-checked edits. Core supplies an
//! actor/case-scoped artifact reader; neither text nor stored JSON grants access.
use crate::document::compose_explicit_text;
use crate::ports::{JsonKind, JsonNode, strict_json_document};
use minidora_domain::{Result, canonical, digest, failure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::ops::Range;
use std::time::Instant;

const MAX_TEXT_BYTES: usize = 256_000;
const MAX_ARTIFACT_BYTES: usize = 2_000_000;
const MAX_REPLAY_BYTES: usize = 8_000_000;
const MAX_REVISIONS: usize = 16;
const IMPORT_OPERATION: &str = "TEXT_IMPORT_EXPLICIT_V1";
const EDIT_OPERATION: &str = "TEXT_EDIT_EXPLICIT_PATCHES_V1";
const GUARANTEE: &str = "explicit caller text and atomic expected-original edits with protected ranges and byte provenance; no inferred correction, factual truth or semantic-equivalence claim";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedTextRange {
    pub id: String,
    pub start: usize,
    pub end: usize,
    pub expected: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
pub enum TextOrigin {
    OriginalText {
        json_start: usize,
        json_end: usize,
        text_start: usize,
        text_end: usize,
    },
    ParentText {
        artifact_sha256: String,
        start: usize,
        end: usize,
    },
    ReplacementText {
        edit_id: String,
        json_start: usize,
        json_end: usize,
        text_start: usize,
        text_end: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextDocumentSpan {
    pub output_start: usize,
    pub output_end: usize,
    pub text: String,
    pub origin: TextOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedText {
    pub operation: String,
    pub source_document: String,
    pub source_sha256: String,
    pub text: String,
    pub output_sha256: String,
    pub max_chars: usize,
    pub protected: Vec<ProtectedTextRange>,
    pub spans: Vec<TextDocumentSpan>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextChange {
    pub id: String,
    pub old_start: usize,
    pub old_end: usize,
    pub new_start: usize,
    pub new_end: usize,
    pub expected: String,
    pub replacement: String,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditedText {
    pub operation: String,
    pub source_document: String,
    pub source_sha256: String,
    pub base_artifact: String,
    pub base_output_sha256: String,
    pub consumed_artifacts: Vec<String>,
    pub revision: usize,
    pub text: String,
    pub output_sha256: String,
    pub max_chars: usize,
    pub protected: Vec<ProtectedTextRange>,
    pub spans: Vec<TextDocumentSpan>,
    pub changes: Vec<TextChange>,
    pub guarantee: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ImportRequest {
    text: String,
    protected: Vec<ProtectedTextRange>,
    max_chars: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EditRequest {
    base_artifact: String,
    expected_text_sha256: String,
    edits: Vec<ExplicitEdit>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExplicitEdit {
    id: String,
    start: usize,
    end: usize,
    expected: String,
    replacement: String,
    reason: String,
}

struct ParsedEdit {
    request: EditRequest,
    replacement_json_spans: Vec<Range<usize>>,
}

struct VerifiedDocument {
    text: String,
    max_chars: usize,
    protected: Vec<ProtectedTextRange>,
    revision: usize,
    consumed: Vec<String>,
}

fn input_error(message: impl Into<String>) -> minidora_domain::Error {
    failure("TEXT_EDIT_INPUT", message)
}

fn admission(document: &str) -> Result<JsonNode> {
    if document.is_empty() || document.len() > MAX_TEXT_BYTES {
        return Err(input_error("expected 1..=256000 request body bytes"));
    }
    strict_json_document(document)
}

fn name(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(input_error(
            "IDs must be nonempty tokens of at most 128 UTF-8 bytes",
        ));
    }
    Ok(())
}

fn text_characters(value: &str) -> Result<()> {
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(input_error("unsupported control character in text"));
    }
    Ok(())
}

fn text_limits(value: &str, max_chars: usize) -> Result<()> {
    text_characters(value)?;
    if !(1..=100_000).contains(&max_chars) {
        return Err(input_error("max_chars must be in 1..=100000"));
    }
    if value.trim().is_empty() {
        return Err(input_error(
            "document text must contain non-whitespace content",
        ));
    }
    if value.len() > MAX_TEXT_BYTES || value.chars().count() > max_chars {
        return Err(failure(
            "TEXT_EDIT_OUTPUT_LIMIT",
            "text exceeds the inherited byte/character limit; no text is truncated",
        ));
    }
    Ok(())
}

fn protected_ranges(rows: &mut [ProtectedTextRange], text: &str) -> Result<()> {
    if rows.len() > 256 {
        return Err(input_error("at most 256 protected ranges are admitted"));
    }
    rows.sort_by_key(|row| (row.start, row.end));
    let mut ids = BTreeSet::new();
    let mut previous_end = 0usize;
    for row in rows {
        name(&row.id)?;
        if !ids.insert(row.id.clone())
            || row.start >= row.end
            || row.start < previous_end
            || text.get(row.start..row.end) != Some(row.expected.as_str())
        {
            return Err(input_error(
                "protected ranges require unique IDs, nonoverlapping UTF-8 byte ranges and matching expected text",
            ));
        }
        previous_end = row.end;
    }
    Ok(())
}

fn artifact_limit<T: Serialize>(value: &T) -> Result<()> {
    if canonical(value)?.len() > MAX_ARTIFACT_BYTES {
        return Err(failure(
            "TEXT_EDIT_ARTIFACT_LIMIT",
            "text artifact exceeds 2000000 serialized bytes",
        ));
    }
    Ok(())
}

fn field_span(root: &JsonNode, key: &str) -> Result<Range<usize>> {
    let JsonKind::Object(fields) = &root.kind else {
        return Err(input_error("expected one object"));
    };
    fields
        .get(key)
        .map(|node| node.span.clone())
        .ok_or_else(|| input_error(format!("missing field {key}")))
}

/// Preserve explicit original text and locks before later artifact-based edits.
pub fn import_explicit_text(document: &str) -> Result<ImportedText> {
    let root = admission(document)?;
    let mut request: ImportRequest =
        serde_json::from_str(document).map_err(|error| input_error(error.to_string()))?;
    text_limits(&request.text, request.max_chars)?;
    protected_ranges(&mut request.protected, &request.text)?;
    let json_span = field_span(&root, "text")?;
    let output_sha256 = digest(request.text.as_bytes());
    let span = TextDocumentSpan {
        output_start: 0,
        output_end: request.text.len(),
        text: request.text.clone(),
        origin: TextOrigin::OriginalText {
            json_start: json_span.start,
            json_end: json_span.end,
            text_start: 0,
            text_end: request.text.len(),
        },
    };
    let result = ImportedText {
        operation: IMPORT_OPERATION.into(),
        source_document: document.into(),
        source_sha256: digest(document.as_bytes()),
        text: request.text,
        output_sha256,
        max_chars: request.max_chars,
        protected: request.protected,
        spans: vec![span],
        guarantee: GUARANTEE.into(),
    };
    artifact_limit(&result)?;
    Ok(result)
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn parse_edit(document: &str) -> Result<ParsedEdit> {
    let root = admission(document)?;
    let request: EditRequest =
        serde_json::from_str(document).map_err(|error| input_error(error.to_string()))?;
    if !is_digest(&request.base_artifact) || !is_digest(&request.expected_text_sha256) {
        return Err(input_error(
            "base_artifact and expected_text_sha256 must be lowercase SHA-256 digests",
        ));
    }
    if !(1..=64).contains(&request.edits.len()) {
        return Err(input_error("edits must contain 1..=64 explicit changes"));
    }
    let mut ids = BTreeSet::new();
    for edit in &request.edits {
        name(&edit.id)?;
        text_characters(&edit.expected)?;
        text_characters(&edit.replacement)?;
        text_characters(&edit.reason)?;
        if !ids.insert(edit.id.clone())
            || edit.expected == edit.replacement
            || edit.reason.trim().is_empty()
            || edit.reason.chars().count() > 512
        {
            return Err(input_error(
                "edits require unique IDs, an actual change and a nonempty reason of at most 512 scalar values",
            ));
        }
    }
    let JsonKind::Object(fields) = &root.kind else {
        return Err(input_error("edit request must be an object"));
    };
    let Some(JsonNode {
        kind: JsonKind::Array(edits),
        ..
    }) = fields.get("edits")
    else {
        return Err(input_error("edits must be an array"));
    };
    let replacement_json_spans = edits
        .iter()
        .map(|node| field_span(node, "replacement"))
        .collect::<Result<Vec<_>>>()?;
    Ok(ParsedEdit {
        request,
        replacement_json_spans,
    })
}

fn deadline(started: Instant, deadline_ms: u64) -> Result<()> {
    if started.elapsed().as_millis() >= u128::from(deadline_ms) {
        return Err(failure(
            "TEXT_EDIT_DEADLINE",
            "text-edit request deadline exhausted",
        ));
    }
    Ok(())
}

fn append_span(
    text: &mut String,
    spans: &mut Vec<TextDocumentSpan>,
    piece: &str,
    origin: TextOrigin,
) {
    if !piece.is_empty() {
        let output_start = text.len();
        text.push_str(piece);
        spans.push(TextDocumentSpan {
            output_start,
            output_end: text.len(),
            text: piece.into(),
            origin,
        });
    }
}

fn apply_edits(
    document: &str,
    parsed: &ParsedEdit,
    base: &VerifiedDocument,
    started: Instant,
    deadline_ms: u64,
) -> Result<EditedText> {
    deadline(started, deadline_ms)?;
    let request = &parsed.request;
    if base.revision >= MAX_REVISIONS {
        return Err(failure(
            "TEXT_EDIT_HISTORY_LIMIT",
            "at most 16 editing revisions are admitted",
        ));
    }
    let base_output_sha256 = digest(base.text.as_bytes());
    if base_output_sha256 != request.expected_text_sha256 {
        return Err(failure(
            "TEXT_EDIT_BASE_MISMATCH",
            "expected_text_sha256 differs from the actual base artifact text",
        ));
    }
    let mut ordered: Vec<_> = request.edits.iter().enumerate().collect();
    ordered.sort_by_key(|(_, edit)| (edit.start, edit.end));
    let mut prior: Option<&ExplicitEdit> = None;
    let mut output_size = base.text.len();
    for (_, edit) in &ordered {
        deadline(started, deadline_ms)?;
        if edit.start > edit.end
            || base.text.get(edit.start..edit.end) != Some(edit.expected.as_str())
        {
            return Err(failure(
                "TEXT_EDIT_EXPECTED_TEXT",
                "an edit range is invalid or does not match the exact original text",
            ));
        }
        if prior.is_some_and(|previous| previous.end > edit.start || previous.start == edit.start) {
            return Err(failure(
                "TEXT_EDIT_OVERLAP",
                "edit ranges overlap or share an ambiguous starting position",
            ));
        }
        for lock in &base.protected {
            if (edit.start < lock.end && lock.start < edit.end)
                || (edit.start == edit.end && lock.start < edit.start && edit.start < lock.end)
            {
                return Err(failure(
                    "TEXT_EDIT_PROTECTED",
                    "an edit touches protected original text",
                ));
            }
        }
        output_size = output_size
            .checked_sub(edit.end - edit.start)
            .and_then(|size| size.checked_add(edit.replacement.len()))
            .ok_or_else(|| failure("TEXT_EDIT_OUTPUT_LIMIT", "output size overflow"))?;
        prior = Some(edit);
    }
    if output_size > MAX_TEXT_BYTES {
        return Err(failure(
            "TEXT_EDIT_OUTPUT_LIMIT",
            "edited text exceeds 256000 bytes",
        ));
    }
    let mut text = String::with_capacity(output_size);
    let mut spans = Vec::new();
    let mut changes = Vec::new();
    let mut cursor = 0usize;
    for (index, edit) in &ordered {
        deadline(started, deadline_ms)?;
        append_span(
            &mut text,
            &mut spans,
            &base.text[cursor..edit.start],
            TextOrigin::ParentText {
                artifact_sha256: request.base_artifact.clone(),
                start: cursor,
                end: edit.start,
            },
        );
        let new_start = text.len();
        let source_span = &parsed.replacement_json_spans[*index];
        append_span(
            &mut text,
            &mut spans,
            &edit.replacement,
            TextOrigin::ReplacementText {
                edit_id: edit.id.clone(),
                json_start: source_span.start,
                json_end: source_span.end,
                text_start: 0,
                text_end: edit.replacement.len(),
            },
        );
        changes.push(TextChange {
            id: edit.id.clone(),
            old_start: edit.start,
            old_end: edit.end,
            new_start,
            new_end: text.len(),
            expected: edit.expected.clone(),
            replacement: edit.replacement.clone(),
            reason: edit.reason.clone(),
        });
        cursor = edit.end;
    }
    append_span(
        &mut text,
        &mut spans,
        &base.text[cursor..],
        TextOrigin::ParentText {
            artifact_sha256: request.base_artifact.clone(),
            start: cursor,
            end: base.text.len(),
        },
    );
    text_limits(&text, base.max_chars)?;
    let mut protected = base.protected.clone();
    for lock in &mut protected {
        let shift = ordered
            .iter()
            .filter(|(_, edit)| edit.end <= lock.start)
            .map(|(_, edit)| edit.replacement.len() as isize - (edit.end - edit.start) as isize)
            .sum::<isize>();
        lock.start = lock
            .start
            .checked_add_signed(shift)
            .ok_or_else(|| input_error("protected range shift overflow"))?;
        lock.end = lock
            .end
            .checked_add_signed(shift)
            .ok_or_else(|| input_error("protected range shift overflow"))?;
    }
    protected_ranges(&mut protected, &text)?;
    let mut output_cursor = 0usize;
    for span in &spans {
        if span.output_start != output_cursor
            || text.get(span.output_start..span.output_end) != Some(span.text.as_str())
        {
            return Err(failure(
                "TEXT_EDIT_PROVENANCE",
                "output provenance must continuously cover exact text",
            ));
        }
        output_cursor = span.output_end;
    }
    if output_cursor != text.len() || spans.len() > 8192 {
        return Err(failure(
            "TEXT_EDIT_PROVENANCE",
            "unmapped output or provenance span overflow",
        ));
    }
    let result = EditedText {
        operation: EDIT_OPERATION.into(),
        source_document: document.into(),
        source_sha256: digest(document.as_bytes()),
        base_artifact: request.base_artifact.clone(),
        base_output_sha256,
        consumed_artifacts: base.consumed.clone(),
        revision: base.revision + 1,
        output_sha256: digest(text.as_bytes()),
        text,
        max_chars: base.max_chars,
        protected,
        spans,
        changes,
        guarantee: GUARANTEE.into(),
    };
    artifact_limit(&result)?;
    deadline(started, deadline_ms)?;
    Ok(result)
}

struct Replay<F> {
    load: F,
    seen: BTreeSet<String>,
    loaded_bytes: usize,
    started: Instant,
    deadline_ms: u64,
}

impl<F: FnMut(&str) -> Result<Vec<u8>>> Replay<F> {
    fn resolve(&mut self, sha: &str, depth: usize) -> Result<VerifiedDocument> {
        deadline(self.started, self.deadline_ms)?;
        if depth > MAX_REVISIONS || !self.seen.insert(sha.to_owned()) {
            return Err(failure(
                "TEXT_EDIT_HISTORY_LIMIT",
                "cyclic or over-limit text artifact ancestry",
            ));
        }
        let bytes = (self.load)(sha)?;
        self.loaded_bytes = self.loaded_bytes.saturating_add(bytes.len());
        if bytes.len() > MAX_ARTIFACT_BYTES || self.loaded_bytes > MAX_REPLAY_BYTES {
            return Err(failure(
                "TEXT_EDIT_REPLAY_LIMIT",
                "text ancestry exceeds its artifact or aggregate byte budget",
            ));
        }
        if digest(&bytes) != sha {
            return Err(failure(
                "TEXT_EDIT_BASE_INVALID",
                "base artifact bytes do not match their digest",
            ));
        }
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| failure("TEXT_EDIT_BASE_INVALID", "base artifact is not UTF-8"))?;
        strict_json_document(source)?;
        let value: serde_json::Value = serde_json::from_str(source).map_err(|_| {
            failure(
                "TEXT_EDIT_BASE_INVALID",
                "base artifact is not a valid document record",
            )
        })?;
        let document = value
            .get("source_document")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                failure(
                    "TEXT_EDIT_BASE_TYPE",
                    "base is not an explicit text document artifact",
                )
            })?;
        let result = match value.get("operation").and_then(serde_json::Value::as_str) {
            Some("TEXT_COMPOSE_EXPLICIT_FRAGMENTS") => {
                let expected = compose_explicit_text(document)?;
                verify_record(&bytes, &expected)?;
                let protected = expected
                    .units
                    .iter()
                    .filter(|unit| unit.protected)
                    .map(|unit| ProtectedTextRange {
                        id: unit.id.clone(),
                        start: unit.output_start,
                        end: unit.output_end,
                        expected: expected.text[unit.output_start..unit.output_end].into(),
                    })
                    .collect();
                VerifiedDocument {
                    text: expected.text,
                    max_chars: expected.max_chars,
                    protected,
                    revision: 0,
                    consumed: vec![sha.into()],
                }
            }
            Some(IMPORT_OPERATION) => {
                let expected = import_explicit_text(document)?;
                verify_record(&bytes, &expected)?;
                VerifiedDocument {
                    text: expected.text,
                    max_chars: expected.max_chars,
                    protected: expected.protected,
                    revision: 0,
                    consumed: vec![sha.into()],
                }
            }
            Some(EDIT_OPERATION) => {
                let parsed = parse_edit(document)?;
                let parent = self.resolve(&parsed.request.base_artifact, depth + 1)?;
                let expected =
                    apply_edits(document, &parsed, &parent, self.started, self.deadline_ms)?;
                verify_record(&bytes, &expected)?;
                let mut consumed = vec![sha.into()];
                consumed.extend(expected.consumed_artifacts);
                VerifiedDocument {
                    text: expected.text,
                    max_chars: expected.max_chars,
                    protected: expected.protected,
                    revision: expected.revision,
                    consumed,
                }
            }
            _ => {
                return Err(failure(
                    "TEXT_EDIT_BASE_TYPE",
                    "base artifact operation is not a supported text document type",
                ));
            }
        };
        deadline(self.started, self.deadline_ms)?;
        Ok(result)
    }
}

fn verify_record<T: Serialize>(bytes: &[u8], expected: &T) -> Result<()> {
    if canonical(expected)? != bytes {
        return Err(failure(
            "TEXT_EDIT_BASE_INVALID",
            "base artifact does not reproduce from its original source and editing ancestry",
        ));
    }
    Ok(())
}

/// Resolve actual authorized artifacts, replay their source/history, then apply
/// all edits against one original coordinate space. The loader belongs to Core.
pub fn edit_explicit_text<F: FnMut(&str) -> Result<Vec<u8>>>(
    document: &str,
    load: F,
    started: Instant,
    deadline_ms: u64,
) -> Result<EditedText> {
    deadline(started, deadline_ms)?;
    let parsed = parse_edit(document)?;
    let mut replay = Replay {
        load,
        seen: BTreeSet::new(),
        loaded_bytes: 0,
        started,
        deadline_ms,
    };
    let base = replay.resolve(&parsed.request.base_artifact, 0)?;
    apply_edits(document, &parsed, &base, started, deadline_ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};
    use std::collections::BTreeMap;

    fn save<T: Serialize>(store: &mut BTreeMap<String, Vec<u8>>, artifact: &T) -> String {
        let bytes = canonical(artifact).unwrap();
        let sha = digest(&bytes);
        store.insert(sha.clone(), bytes);
        sha
    }

    fn import(text: &str, protected: Value, max_chars: usize) -> ImportedText {
        import_explicit_text(
            &json!({"text":text,"protected":protected,"max_chars":max_chars}).to_string(),
        )
        .unwrap()
    }

    fn patch(id: &str, start: usize, end: usize, expected: &str, replacement: &str) -> Value {
        json!({"id":id,"start":start,"end":end,"expected":expected,"replacement":replacement,"reason":"explicit requested revision"})
    }

    fn request(base: &str, text: &str, edits: Value) -> String {
        json!({"base_artifact":base,"expected_text_sha256":digest(text.as_bytes()),"edits":edits})
            .to_string()
    }

    fn run(store: &BTreeMap<String, Vec<u8>>, source: &str) -> Result<EditedText> {
        edit_explicit_text(
            source,
            |sha| {
                store
                    .get(sha)
                    .cloned()
                    .ok_or_else(|| failure("MISSING_TEST_ARTIFACT", sha))
            },
            Instant::now(),
            30_000,
        )
    }

    fn code<T: std::fmt::Debug>(result: Result<T>, expected: &str) {
        match result.unwrap_err() {
            minidora_domain::Error::Contract { code, .. } => assert_eq!(code, expected),
            other => panic!("unexpected {other}"),
        }
    }

    #[test]
    fn imported_text_checks_exact_unicode_source_and_protected_ranges() {
        let text = "猫\r\n引用:「保持」\n末尾";
        let start = text.find("引用").unwrap();
        let end = start + "引用:「保持」".len();
        let source = json!({"text":text,"protected":[{"id":"quotation","start":start,"end":end,"expected":"引用:「保持」"}],"max_chars":64}).to_string();
        let result = import_explicit_text(&source).unwrap();
        assert_eq!(result.text, text);
        assert_eq!(result.output_sha256, digest(text.as_bytes()));
        assert_eq!(result.source_sha256, digest(source.as_bytes()));
        assert_eq!(result.spans.len(), 1);
        let TextOrigin::OriginalText {
            json_start,
            json_end,
            text_start,
            text_end,
        } = result.spans[0].origin
        else {
            panic!("wrong origin")
        };
        let decoded: String = serde_json::from_str(&source[json_start..json_end]).unwrap();
        assert_eq!(&decoded[text_start..text_end], text);
        let mut invalid: Value = serde_json::from_str(&source).unwrap();
        invalid["protected"][0]["start"] = json!(start + 1);
        code(
            import_explicit_text(&invalid.to_string()),
            "TEXT_EDIT_INPUT",
        );
        invalid = serde_json::from_str(&source).unwrap();
        invalid["protected"][0]["expected"] = json!("wrong");
        code(
            import_explicit_text(&invalid.to_string()),
            "TEXT_EDIT_INPUT",
        );
        invalid["protected"] = json!([]);
        invalid["max_chars"] = json!(2);
        code(
            import_explicit_text(&invalid.to_string()),
            "TEXT_EDIT_OUTPUT_LIMIT",
        );
        code(
            import_explicit_text(r#"{"text":"a","text":"b","protected":[],"max_chars":4}"#),
            "DUPLICATE_JSON_KEY",
        );
    }

    #[test]
    fn atomic_edits_use_original_coordinates_and_shift_protected_text() {
        let text = "aaa|引用:「保持」|末尾";
        let start = text.find("引用").unwrap();
        let end = start + "引用:「保持」".len();
        let base = import(
            text,
            json!([{"id":"lock","start":start,"end":end,"expected":"引用:「保持」"}]),
            128,
        );
        let mut store = BTreeMap::new();
        let base_sha = save(&mut store, &base);
        let source = request(
            &base_sha,
            text,
            json!([
                patch("remove-tail", end, text.len(), "|末尾", ""),
                patch("insert-before-lock", start, start, "", "前"),
                patch("replace", 0, 3, "aaa", "longaaa")
            ]),
        );
        let first = run(&store, &source).unwrap();
        assert_eq!(first.text, "longaaa|前引用:「保持」");
        assert_eq!(first.revision, 1);
        assert_eq!(
            first.consumed_artifacts.as_slice(),
            std::slice::from_ref(&base_sha)
        );
        assert_eq!(first.protected[0].start, start + 4 + "前".len());
        assert_eq!(first.protected[0].expected, "引用:「保持」");
        assert_eq!(
            first
                .changes
                .iter()
                .map(|row| row.id.as_str())
                .collect::<Vec<_>>(),
            ["replace", "insert-before-lock", "remove-tail"]
        );
        assert!(first.spans.iter().any(|span| matches!(&span.origin, TextOrigin::ParentText { artifact_sha256, .. } if artifact_sha256 == &base_sha)));
        assert!(first.spans.iter().any(|span| matches!(&span.origin, TextOrigin::ReplacementText { edit_id, .. } if edit_id == "replace") && span.text == "longaaa"));
        let first_sha = save(&mut store, &first);
        let next = request(
            &first_sha,
            &first.text,
            json!([patch("next", 0, 7, "longaaa", "bbb")]),
        );
        let second = run(&store, &next).unwrap();
        assert_eq!(second.text, "bbb|前引用:「保持」");
        assert_eq!(second.revision, 2);
        assert_eq!(second.consumed_artifacts, [first_sha, base_sha]);
        assert_eq!(second.protected[0].start, "bbb|前".len());
    }

    #[test]
    fn edits_reject_stale_overlapping_protected_and_invalid_unicode_requests() {
        let text = "猫abcLOCKend";
        let start = "猫abc".len();
        let end = start + 4;
        let base = import(
            text,
            json!([{"id":"lock","start":start,"end":end,"expected":"LOCK"}]),
            32,
        );
        let mut store = BTreeMap::new();
        let sha = save(&mut store, &base);
        let cases = [
            (
                json!([
                    patch("early", 3, 4, "a", "z"),
                    patch("late", end, text.len(), "wrong", "x")
                ]),
                "TEXT_EDIT_EXPECTED_TEXT",
            ),
            (
                json!([patch("one", 3, 5, "ab", "x"), patch("two", 4, 6, "bc", "y")]),
                "TEXT_EDIT_OVERLAP",
            ),
            (
                json!([patch("one", 3, 3, "", "x"), patch("two", 3, 4, "a", "y")]),
                "TEXT_EDIT_OVERLAP",
            ),
            (
                json!([patch("locked", start, end, "LOCK", "x")]),
                "TEXT_EDIT_PROTECTED",
            ),
            (
                json!([patch("locked", start + 1, start + 1, "", "x")]),
                "TEXT_EDIT_PROTECTED",
            ),
            (
                json!([patch("utf8", 1, 3, "猫", "x")]),
                "TEXT_EDIT_EXPECTED_TEXT",
            ),
            (json!([patch("noop", 3, 4, "a", "a")]), "TEXT_EDIT_INPUT"),
            (
                json!([patch("large", 3, 4, "a", &"x".repeat(64))]),
                "TEXT_EDIT_OUTPUT_LIMIT",
            ),
        ];
        for (edits, expected) in cases {
            code(run(&store, &request(&sha, text, edits)), expected);
            assert_eq!(store[&sha], canonical(&base).unwrap());
        }
        let mut stale: Value =
            serde_json::from_str(&request(&sha, text, json!([patch("one", 3, 4, "a", "z")])))
                .unwrap();
        stale["expected_text_sha256"] = json!("0".repeat(64));
        code(run(&store, &stale.to_string()), "TEXT_EDIT_BASE_MISMATCH");
    }

    #[test]
    fn composition_artifacts_are_replayed_and_keep_condition_protection() {
        let source = json!({
            "materials":[{"id":"m","text":"TitleKeep"}],
            "units":[
                {"id":"title","kind":"heading","fragments":[{"material":"m","start":0,"end":5}],"with":[],"required":false,"protected":false},
                {"id":"condition","kind":"condition","fragments":[{"material":"m","start":5,"end":9}],"with":[],"required":false,"protected":false}
            ],"order":["title"],"max_chars":128
        }).to_string();
        let base = compose_explicit_text(&source).unwrap();
        let mut store = BTreeMap::new();
        let sha = save(&mut store, &base);
        let result = run(
            &store,
            &request(
                &sha,
                &base.text,
                json!([patch("title", 2, 7, "Title", "Update")]),
            ),
        )
        .unwrap();
        assert_eq!(result.text, "# Update\n\n条件: Keep");
        assert_eq!(result.protected[0].expected, "条件: Keep");
        assert_eq!(result.protected[0].start, "# Update\n\n".len());
        let lock = &base.units[1];
        code(
            run(
                &store,
                &request(
                    &sha,
                    &base.text,
                    json!([patch(
                        "remove-condition",
                        lock.output_start,
                        lock.output_end,
                        "条件: Keep",
                        ""
                    )]),
                ),
            ),
            "TEXT_EDIT_PROTECTED",
        );
    }

    #[test]
    fn replay_rejects_forged_artifacts_and_enforces_history_and_deadline() {
        let mut store = BTreeMap::new();
        let base = import("a", json!([]), 32);
        let mut sha = save(&mut store, &base);
        let mut text = "a".to_owned();
        let mut last = None;
        for revision in 1..=16 {
            let replacement = if text == "a" { "b" } else { "a" };
            let source = request(
                &sha,
                &text,
                json!([patch("change", 0, 1, &text, replacement)]),
            );
            let result = run(&store, &source).unwrap();
            assert_eq!(result.revision, revision);
            text = result.text.clone();
            sha = save(&mut store, &result);
            last = Some(result);
        }
        code(
            run(
                &store,
                &request(&sha, &text, json!([patch("too-many", 0, 1, &text, "z")])),
            ),
            "TEXT_EDIT_HISTORY_LIMIT",
        );
        let mut forged = serde_json::to_value(last.unwrap()).unwrap();
        forged["text"] = json!("z");
        let forged_sha = save(&mut store, &forged);
        code(
            run(
                &store,
                &request(&forged_sha, "z", json!([patch("change", 0, 1, "z", "y")])),
            ),
            "TEXT_EDIT_BASE_INVALID",
        );
        let mut forged = serde_json::to_value(&base).unwrap();
        forged["extra"] = json!(true);
        let forged_sha = save(&mut store, &forged);
        code(
            run(
                &store,
                &request(&forged_sha, "a", json!([patch("change", 0, 1, "a", "b")])),
            ),
            "TEXT_EDIT_BASE_INVALID",
        );
        let unknown = save(
            &mut store,
            &json!({"operation":"OTHER","source_document":"{}"}),
        );
        code(
            run(
                &store,
                &request(&unknown, "a", json!([patch("change", 0, 1, "a", "b")])),
            ),
            "TEXT_EDIT_BASE_TYPE",
        );
        let source = request(&sha, &text, json!([patch("change", 0, 1, &text, "z")]));
        code(
            edit_explicit_text(
                &source,
                |_| panic!("expired request must not load artifacts"),
                Instant::now(),
                0,
            ),
            "TEXT_EDIT_DEADLINE",
        );
        let source = request(
            &"0".repeat(64),
            "a",
            json!([patch("change", 0, 1, "a", "b")]),
        );
        code(
            edit_explicit_text(
                &source,
                |_| Ok(canonical(&base).unwrap()),
                Instant::now(),
                30_000,
            ),
            "TEXT_EDIT_BASE_INVALID",
        );
    }
}
