//! One-time, source-preserving interpretation for an explicitly limited grammar.
//! Unrecognized language is retained as an unresolved intent, never guessed.
use aho_corasick::AhoCorasick;
use minidora_domain::*;
use regex::Regex;
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::{canonical_combining_class, compose};

pub const PARSER_VERSION: &str = "declarative-ja-en-v83-direct-english-assertion-source-ranges";
const SOURCE_EVIDENCE_VERSION: &str = "minidora.sealed-source-evidence.v1";

pub fn seal(request: &RequestEnvelope, previous: Option<&ResultRecord>) -> Result<InputBundle> {
    if [&request.case_id, &request.input_id, &request.actor_id]
        .iter()
        .any(|value| value.trim().is_empty())
    {
        return Err(failure(
            "INVALID_IDENTITY",
            "case, input and actor identities must be nonempty",
        ));
    }
    if request.text.is_empty() || request.text.len() > request.budget.max_input_bytes {
        return Err(failure(
            "INPUT_LIMIT",
            "input is empty or exceeds its byte budget",
        ));
    }
    if let Some(reference) = &request.context_ref
        && previous.is_none_or(|result| result.id != *reference)
    {
        return Err(failure(
            "CONTEXT_MISMATCH",
            "explicit context does not name the supplied actual prior result",
        ));
    }
    let intent = if matches!(request.locale.as_str(), "ja" | "en" | "ja-JP" | "en-US") {
        interpret(&request.text)?
    } else {
        Intent::Unknown {
            reason: "semantic verification supports only the declared Japanese/English grammar"
                .into(),
        }
    };
    let mut unresolved = match &intent {
        Intent::Unknown { reason } => vec![reason.clone()],
        _ => Vec::new(),
    };
    if matches!(intent, Intent::ExplainPrevious) && previous.is_none() {
        unresolved.push("no actual adopted previous result is available".into());
    }
    if matches!(intent, Intent::ReplacePreviousText { .. })
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved.push("no actual committed previous output is available to edit".into());
    }
    if matches!(intent, Intent::AskPreviousGroupMean)
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved
            .push("no actual committed previous grouped-statistics result is available".into());
    }
    if matches!(intent, Intent::AskPreviousRelation { .. })
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved.push(
            "no actual committed same-case result is available to resolve the relation subject"
                .into(),
        );
    }
    if matches!(intent, Intent::AssertPreviousRelation { .. })
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved.push(
            "no actual committed same-case relation result is available to resolve the assertion subject"
                .into(),
        );
    }
    if matches!(intent, Intent::AskPreviousSourceQuestion { .. })
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved.push("no actual committed previous fetched-source result is available".into());
    }
    if matches!(intent, Intent::VerifyPreviousCode { .. })
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved
            .push("no actual committed previous result is available for code verification".into());
    }
    if matches!(intent, Intent::FetchPreviousSearchCandidate { .. })
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved.push(
            "no actual committed previous search result is available for body retrieval".into(),
        );
    }
    if matches!(
        intent,
        Intent::SummarizePreviousSource | Intent::SummarizePreviousSourceConstrained { .. }
    ) && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved.push(
            "no actual committed previous result is available for source summarization".into(),
        );
    }
    if matches!(intent, Intent::SummarizePreviousResult { .. })
        && previous.is_none_or(|result| result.lifecycle != Lifecycle::Commit)
    {
        unresolved
            .push("no actual committed previous result is available for summarization".into());
    }
    if matches!(intent, Intent::CorrectCalculation { .. })
        && previous.is_none_or(|result| {
            result.lifecycle != Lifecycle::Commit || result.calculation.is_none()
        })
    {
        unresolved.push("no actual committed calculation is available to correct".into());
    }
    let context_sha256 = identity(&previous)?;
    let source_roles = match source_role_spans(&request.text, &intent) {
        Ok(source_roles) => source_roles,
        Err(Error::Contract { code, message }) if code == "SOURCE_SPAN_INDEX_LIMIT" => {
            unresolved.push(format!("source role indexing stopped: {message}"));
            BTreeMap::new()
        }
        Err(error) => return Err(error),
    };
    let meaning = MeaningIR {
        parser_version: PARSER_VERSION.into(),
        intent,
        // Offsets are UTF-8 byte offsets into the exact original, not normalized
        // character indices. Keeping the complete source avoids lossy mappings.
        spans: vec![SourceSpan {
            start: 0,
            end: request.text.len(),
            text: request.text.clone(),
        }],
        source_roles,
        unresolved,
        source_kind: "USER_REQUEST".into(),
        context_result_ref: previous.map(|result| result.id.clone()),
    };
    let raw_sha256 = digest(request.text.as_bytes());
    let fingerprint = identity(&(request, &raw_sha256, &context_sha256, PARSER_VERSION))?;
    let semantic_signature = identity(&meaning)?;
    Ok(InputBundle {
        request: request.clone(),
        raw_sha256,
        context_sha256,
        fingerprint,
        meaning,
        semantic_signature,
    })
}

type SourceRoleCandidates = BTreeMap<String, Vec<String>>;

fn add_source_candidate(candidates: &mut SourceRoleCandidates, role: &str, value: &str) {
    if value.is_empty() {
        return;
    }
    let values = candidates.entry(role.to_string()).or_default();
    if !values.iter().any(|candidate| candidate == value) {
        values.push(value.to_string());
    }
    if let Some(variable) = value.strip_prefix('$')
        && !variable.is_empty()
    {
        let source_form = variable.to_ascii_uppercase();
        if !values.iter().any(|candidate| candidate == &source_form) {
            values.push(source_form);
        }
    }
}

fn escape_role_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

fn serde_tag_role(tag: &str) -> String {
    let mut role = String::new();
    for part in tag.split('_').filter(|part| !part.is_empty()) {
        let mut characters = part.chars();
        if let Some(first) = characters.next() {
            role.push(first.to_ascii_uppercase());
            role.push_str(&characters.as_str().to_ascii_lowercase());
        }
    }
    role
}

fn collect_string_leaf_candidates(
    value: &serde_json::Value,
    role: &str,
    candidates: &mut SourceRoleCandidates,
) {
    match value {
        serde_json::Value::String(value) => add_source_candidate(candidates, role, value),
        serde_json::Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_string_leaf_candidates(value, &format!("{role}/{index}"), candidates);
            }
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if matches!(key.as_str(), "kind" | "reason") {
                    continue;
                }
                collect_string_leaf_candidates(
                    value,
                    &format!("{role}/{}", escape_role_segment(key)),
                    candidates,
                );
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn collect_semantic_aliases(
    value: &serde_json::Value,
    role: &str,
    candidates: &mut SourceRoleCandidates,
    japanese_predicate_roles: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_semantic_aliases(
                    value,
                    &format!("{role}/{index}"),
                    candidates,
                    japanese_predicate_roles,
                );
            }
        }
        serde_json::Value::Object(values) => {
            if values.get("predicate").and_then(serde_json::Value::as_str) == Some("is") {
                let predicate_role = format!("{role}/predicate");
                add_source_candidate(candidates, &predicate_role, "are");
                japanese_predicate_roles.push(predicate_role);
            }
            if values.get("negative").and_then(serde_json::Value::as_bool) == Some(true) {
                let role = format!("{role}/negation");
                for marker in [
                    "not",
                    "ではありません",
                    "ではない",
                    "じゃない",
                    "でありません",
                    "でない",
                ] {
                    add_source_candidate(candidates, &role, marker);
                }
            }
            for (key, value) in values {
                if matches!(key.as_str(), "kind" | "reason") {
                    continue;
                }
                collect_semantic_aliases(
                    value,
                    &format!("{role}/{}", escape_role_segment(key)),
                    candidates,
                    japanese_predicate_roles,
                );
            }
        }
        serde_json::Value::String(_)
        | serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_) => {}
    }
}

fn ascii_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn numeric_token_byte(byte: u8) -> bool {
    byte.is_ascii_digit() || matches!(byte, b'.' | b'/' | b'+' | b'-' | b'e' | b'E')
}

const MAX_SOURCE_SPAN_MATCH_EVENTS: usize = 1_000_000;

fn first_nfkd_character(character: char) -> char {
    character.to_string().nfkd().next().unwrap_or(character)
}

fn starts_new_nfkc_segment(previous_source: &str, next_character: char) -> bool {
    let next_decomposed = first_nfkd_character(next_character);
    if canonical_combining_class(next_decomposed) != 0 {
        return false;
    }
    let Some(previous_normalized) = previous_source.nfkc().last() else {
        return true;
    };
    canonical_combining_class(previous_normalized) != 0
        || compose(previous_normalized, next_decomposed).is_none()
}

fn normalized_source_with_ranges(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut normalized_source = String::new();
    let mut source_ranges = Vec::new();
    let mut source_segment = String::new();
    let mut segment_start = 0;
    let mut segment_end = 0;
    for (start, character) in text.char_indices() {
        let end = start + character.len_utf8();
        if !source_segment.is_empty() && starts_new_nfkc_segment(&source_segment, character) {
            let normalized_segment: String = source_segment.nfkc().collect();
            normalized_source.push_str(&normalized_segment);
            source_ranges.extend(std::iter::repeat_n(
                (segment_start, segment_end),
                normalized_segment.len(),
            ));
            source_segment.clear();
            segment_start = start;
        }
        if source_segment.is_empty() {
            segment_start = start;
        }
        source_segment.push(character);
        segment_end = end;
    }
    if !source_segment.is_empty() {
        let normalized_segment: String = source_segment.nfkc().collect();
        normalized_source.push_str(&normalized_segment);
        source_ranges.extend(std::iter::repeat_n(
            (segment_start, segment_end),
            normalized_segment.len(),
        ));
    }
    (normalized_source, source_ranges)
}

fn captured_source_span(
    text: &str,
    source_offset: usize,
    capture: regex::Match<'_>,
) -> Option<SourceSpan> {
    let captured = capture.as_str();
    let leading_bytes = captured.len() - captured.trim_start().len();
    let value = captured.trim();
    let start = source_offset + capture.start() + leading_bytes;
    let end = start + value.len();
    Some(SourceSpan {
        start,
        end,
        text: text.get(start..end)?.to_string(),
    })
}

fn japanese_assertion_source_roles(
    text: &str,
    proposition: &Proposition,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    japanese_assertion_source_roles_with_root(text, proposition, "/Assert/proposition")
}

fn japanese_assertion_source_roles_with_root(
    text: &str,
    proposition: &Proposition,
    role_root: &str,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    if proposition.predicate != "is" {
        return Ok(None);
    }

    let trimmed = text.trim();
    let source_offset = text.len() - text.trim_start().len();
    let statement = terminal_trim(trimmed);
    let (body, body_offset, scope_span) =
        if let Some(scoped) = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(statement) {
            let scope_capture = scoped.get(1).expect("scope capture group exists");
            let body_capture = scoped.get(2).expect("statement capture group exists");
            let Some(scope_span) = captured_source_span(text, source_offset, scope_capture) else {
                return Ok(None);
            };
            if scope_span.text != proposition.scope {
                return Ok(None);
            }
            (
                body_capture.as_str(),
                source_offset + body_capture.start(),
                Some(scope_span),
            )
        } else {
            if proposition.scope != "default" {
                return Ok(None);
            }
            (statement, source_offset, None)
        };

    let Some(relation) = regex(r"^([^はが]+?)(は|が)(.+)$")?.captures(body) else {
        return Ok(None);
    };
    let subject_capture = relation.get(1).expect("subject capture group exists");
    let predicate_capture = relation.get(2).expect("particle capture group exists");
    let object_capture = relation.get(3).expect("object capture group exists");
    let Some(subject_span) = captured_source_span(text, body_offset, subject_capture) else {
        return Ok(None);
    };
    let Some(predicate_span) = captured_source_span(text, body_offset, predicate_capture) else {
        return Ok(None);
    };
    if subject_span.text != proposition.subject {
        return Ok(None);
    }

    let raw_object = object_capture.as_str().trim();
    let raw_object_leading =
        object_capture.as_str().len() - object_capture.as_str().trim_start().len();
    let object_start = body_offset + object_capture.start() + raw_object_leading;
    let mut object_value = raw_object;
    let mut negative = false;
    let mut negation_span = None;
    for suffix in [
        "ではありません",
        "ではない",
        "じゃない",
        "でありません",
        "でない",
    ] {
        if let Some(prefix) = object_value.strip_suffix(suffix) {
            let marker_start = object_start + raw_object.len() - suffix.len();
            let marker_end = object_start + raw_object.len();
            let Some(marker_text) = text.get(marker_start..marker_end) else {
                return Ok(None);
            };
            negation_span = Some(SourceSpan {
                start: marker_start,
                end: marker_end,
                text: marker_text.to_string(),
            });
            object_value = prefix.trim_end();
            negative = true;
            break;
        }
    }
    if !negative {
        for suffix in ["である", "です", "だ"] {
            if let Some(prefix) = object_value.strip_suffix(suffix) {
                object_value = prefix.trim_end();
                break;
            }
        }
    }
    if object_value.is_empty()
        || object_value != proposition.object
        || negative != proposition.negative
    {
        return Ok(None);
    }
    let object_end = object_start + object_value.len();
    let Some(object_text) = text.get(object_start..object_end) else {
        return Ok(None);
    };
    let object_span = SourceSpan {
        start: object_start,
        end: object_end,
        text: object_text.to_string(),
    };

    let mut roles = BTreeMap::from([
        (format!("{role_root}/subject"), vec![subject_span]),
        (format!("{role_root}/predicate"), vec![predicate_span]),
        (format!("{role_root}/object"), vec![object_span]),
    ]);
    if let Some(scope_span) = scope_span {
        roles.insert(format!("{role_root}/scope"), vec![scope_span]);
    }
    if let Some(negation_span) = negation_span {
        roles.insert(format!("{role_root}/negation"), vec![negation_span]);
    }
    Ok(Some(roles))
}

fn english_assertion_source_roles(
    text: &str,
    proposition: &Proposition,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    english_assertion_source_roles_with_root(text, proposition, "/Assert/proposition")
}

fn english_assertion_source_roles_with_root(
    text: &str,
    proposition: &Proposition,
    role_root: &str,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    if proposition.predicate != "is"
        || parse_proposition(text, false)?.as_ref() != Some(proposition)
    {
        return Ok(None);
    }

    let trimmed = text.trim();
    let source_offset = text.len() - text.trim_start().len();
    let statement = terminal_trim(trimmed);
    let (body, body_offset, scope_span) =
        if let Some(scoped) = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(statement) {
            let scope_capture = scoped.get(1).expect("scope capture group exists");
            let body_capture = scoped.get(2).expect("statement capture group exists");
            let Some(scope_span) = captured_source_span(text, source_offset, scope_capture) else {
                return Ok(None);
            };
            if scope_span.text != proposition.scope {
                return Ok(None);
            }
            (
                body_capture.as_str(),
                source_offset + body_capture.start(),
                Some(scope_span),
            )
        } else {
            if proposition.scope != "default" {
                return Ok(None);
            }
            (statement, source_offset, None)
        };

    let Some(relation) = regex(r"(?i)^(.+?)\s+(is|are)\s+(not\s+)?(.+)$")?.captures(body) else {
        return Ok(None);
    };
    let subject_capture = relation.get(1).expect("subject capture group exists");
    let predicate_capture = relation.get(2).expect("predicate capture group exists");
    let object_capture = relation.get(4).expect("object capture group exists");
    let Some(subject_span) = captured_source_span(text, body_offset, subject_capture) else {
        return Ok(None);
    };
    let Some(predicate_span) = captured_source_span(text, body_offset, predicate_capture) else {
        return Ok(None);
    };
    let negation_span = if let Some(capture) = relation.get(3) {
        let Some(span) = captured_source_span(text, body_offset, capture) else {
            return Ok(None);
        };
        Some(span)
    } else {
        None
    };

    let raw_object = object_capture.as_str();
    let leading = raw_object.len() - raw_object.trim_start().len();
    let value = raw_object.trim();
    let value_start = body_offset + object_capture.start() + leading;
    let (article, object_start) = if let Some(article) = ["a", "an"].into_iter().find(|article| {
        value
            .strip_prefix(article)
            .is_some_and(|rest| rest.starts_with(' '))
    }) {
        let article_end = value_start + article.len();
        let Some(article_span) = source_span_at(text, value_start, article_end) else {
            return Ok(None);
        };
        (Some(article_span), article_end + 1)
    } else {
        (None, value_start)
    };
    let Some(object_text) = text.get(object_start..value_start + value.len()) else {
        return Ok(None);
    };
    if object_text.is_empty() || object_text.trim() != object_text {
        return Ok(None);
    }
    let Some(object_span) = source_span_at(text, object_start, object_start + object_text.len())
    else {
        return Ok(None);
    };

    let normalized = |value: &str| value.nfkc().collect::<String>();
    if normalized(&subject_span.text) != normalized(&proposition.subject)
        || !matches!(
            predicate_span.text.to_ascii_lowercase().as_str(),
            "is" | "are"
        )
        || normalized(object_text) != normalized(&proposition.object)
        || proposition.negative != negation_span.is_some()
        || negation_span
            .as_ref()
            .is_some_and(|span| !span.text.eq_ignore_ascii_case("not"))
    {
        return Ok(None);
    }

    let mut roles = BTreeMap::from([
        (format!("{role_root}/subject"), vec![subject_span]),
        (format!("{role_root}/predicate"), vec![predicate_span]),
        (format!("{role_root}/object"), vec![object_span]),
    ]);
    if let Some(article) = article {
        roles.insert(format!("{role_root}/article"), vec![article]);
    }
    if let Some(negation_span) = negation_span {
        roles.insert(format!("{role_root}/negation"), vec![negation_span]);
    }
    if let Some(scope_span) = scope_span {
        roles.insert(format!("{role_root}/scope"), vec![scope_span]);
    }
    Ok(Some(roles))
}

fn source_span_at(text: &str, start: usize, end: usize) -> Option<SourceSpan> {
    Some(SourceSpan {
        start,
        end,
        text: text.get(start..end)?.to_string(),
    })
}

fn japanese_transitive_assertion_source_roles(
    text: &str,
    proposition: &Proposition,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    japanese_transitive_assertion_source_roles_with_root(text, proposition, "/Assert/proposition")
}

fn japanese_transitive_assertion_source_roles_with_root(
    text: &str,
    proposition: &Proposition,
    role_root: &str,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    let trimmed = text.trim();
    let source_offset = text.len() - text.trim_start().len();
    let statement = terminal_trim(trimmed);
    let (body, body_offset, scope_span) =
        if let Some(scoped) = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(statement) {
            let scope_capture = scoped.get(1).expect("scope capture group exists");
            let body_capture = scoped.get(2).expect("statement capture group exists");
            let Some(scope_span) = captured_source_span(text, source_offset, scope_capture) else {
                return Ok(None);
            };
            if scope_span.text.nfkc().collect::<String>()
                != proposition.scope.nfkc().collect::<String>()
            {
                return Ok(None);
            }
            (
                body_capture.as_str(),
                source_offset + body_capture.start(),
                Some(scope_span),
            )
        } else {
            if proposition.scope != "default" {
                return Ok(None);
            }
            (statement, source_offset, None)
        };

    let Some(relation) = regex(r"^([^はが]+?)(は|が)(.+)$")?.captures(body) else {
        return Ok(None);
    };
    let subject_capture = relation.get(1).expect("subject capture group exists");
    let particle_capture = relation
        .get(2)
        .expect("subject-particle capture group exists");
    let rest_capture = relation
        .get(3)
        .expect("object and predicate capture group exists");
    let Some(subject_span) = captured_source_span(text, body_offset, subject_capture) else {
        return Ok(None);
    };
    let Some(subject_particle_span) = captured_source_span(text, body_offset, particle_capture)
    else {
        return Ok(None);
    };

    let rest_text = rest_capture.as_str();
    let rest_leading = rest_text.len() - rest_text.trim_start().len();
    let rest = rest_text.trim();
    let Some(object_particle_start_in_rest) = rest.rfind('を') else {
        return Ok(None);
    };
    let object_text = &rest[..object_particle_start_in_rest];
    let predicate_text = &rest[object_particle_start_in_rest + 'を'.len_utf8()..];
    let object_leading = object_text.len() - object_text.trim_start().len();
    let object = object_text.trim();
    let predicate_leading = predicate_text.len() - predicate_text.trim_start().len();
    let predicate = predicate_text.trim();
    if object.is_empty() || predicate.is_empty() {
        return Ok(None);
    }

    let rest_start = body_offset + rest_capture.start() + rest_leading;
    let object_start = rest_start + object_leading;
    let object_particle_start = rest_start + object_particle_start_in_rest;
    let predicate_start = object_particle_start + 'を'.len_utf8() + predicate_leading;
    let Some(object_span) = source_span_at(text, object_start, object_start + object.len()) else {
        return Ok(None);
    };
    let Some(object_particle_span) = source_span_at(
        text,
        object_particle_start,
        object_particle_start + 'を'.len_utf8(),
    ) else {
        return Ok(None);
    };
    let Some(predicate_span) =
        source_span_at(text, predicate_start, predicate_start + predicate.len())
    else {
        return Ok(None);
    };

    let normalized = |value: &str| value.nfkc().collect::<String>();
    let Some((parsed_subject, parsed_predicate, parsed_object, parsed_negative)) =
        japanese_transitive_proposition(body)?
    else {
        return Ok(None);
    };
    let Some((surface_predicate, surface_negative)) =
        japanese_transitive_verb(&predicate_span.text)
    else {
        return Ok(None);
    };
    if normalized(&subject_span.text) != normalized(&proposition.subject)
        || normalized(&object_span.text) != normalized(&proposition.object)
        || normalized(
            scope_span
                .as_ref()
                .map_or("default", |span| span.text.as_str()),
        ) != normalized(&proposition.scope)
        || !matches!(subject_particle_span.text.as_str(), "は" | "が")
        || object_particle_span.text != "を"
        || parsed_subject != proposition.subject
        || parsed_predicate != proposition.predicate
        || parsed_object != proposition.object
        || parsed_negative != proposition.negative
        || surface_predicate != proposition.predicate
        || surface_negative != proposition.negative
    {
        return Ok(None);
    }

    let mut roles = BTreeMap::from([
        (format!("{role_root}/subject"), vec![subject_span]),
        (
            format!("{role_root}/subject_particle"),
            vec![subject_particle_span],
        ),
        (format!("{role_root}/object"), vec![object_span]),
        (
            format!("{role_root}/object_particle"),
            vec![object_particle_span],
        ),
        (format!("{role_root}/predicate"), vec![predicate_span]),
    ]);
    if let Some(scope_span) = scope_span {
        roles.insert(format!("{role_root}/scope"), vec![scope_span]);
    }
    Ok(Some(roles))
}

struct PremiseCorrectionSourceContract<'a> {
    role_root: &'static str,
    subject: Option<&'a str>,
    predicate: &'a str,
    superseded_object: &'a str,
    replacement_object: &'a str,
    scope: &'a str,
}

fn premise_correction_source_contract(
    expected: &Intent,
) -> Option<PremiseCorrectionSourceContract<'_>> {
    match expected {
        Intent::CorrectPremise {
            superseded,
            replacement,
        } => Some(PremiseCorrectionSourceContract {
            role_root: "/CorrectPremise",
            subject: Some(&superseded.subject),
            predicate: &superseded.predicate,
            superseded_object: &superseded.object,
            replacement_object: &replacement.object,
            scope: &superseded.scope,
        }),
        Intent::CorrectPreviousPremise {
            predicate,
            superseded_object,
            replacement_object,
            scope,
        } => Some(PremiseCorrectionSourceContract {
            role_root: "/CorrectPreviousPremise",
            subject: None,
            predicate,
            superseded_object,
            replacement_object,
            scope: scope.as_deref().unwrap_or("default"),
        }),
        _ => None,
    }
}

fn correction_source_subject_matches(observed: &str, expected: Option<&str>) -> bool {
    let normalized = |value: &str| value.nfkc().collect::<String>();
    match expected {
        Some(expected) => normalized(observed) == normalized(expected),
        None => singular_previous_subject(observed),
    }
}

fn japanese_premise_correction_source_roles(
    text: &str,
    expected: &Intent,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    let Some(contract) = premise_correction_source_contract(expected) else {
        return Ok(None);
    };
    if contract.predicate != "is" || contract.superseded_object == contract.replacement_object {
        return Ok(None);
    }

    let trimmed = text.trim();
    let source_offset = text.len() - text.trim_start().len();
    let statement = terminal_trim(trimmed);
    let (body, body_offset, scope_span) =
        if let Some(scoped) = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(statement) {
            let scope_capture = scoped.get(1).expect("scope capture group exists");
            let body_capture = scoped.get(2).expect("statement capture group exists");
            let Some(scope_span) = captured_source_span(text, source_offset, scope_capture) else {
                return Ok(None);
            };
            if scope_span.text != contract.scope {
                return Ok(None);
            }
            (
                body_capture.as_str(),
                source_offset + body_capture.start(),
                Some(scope_span),
            )
        } else {
            if contract.scope != "default" {
                return Ok(None);
            }
            (statement, source_offset, None)
        };

    let Some(correction) =
        regex(r"^(?:(?:訂正|訂正します)[：:]\s*)?(.+?)(は|が)(.+?)(ではなく)(.+)$")?.captures(body)
    else {
        return Ok(None);
    };
    let subject_capture = correction.get(1).expect("subject capture group exists");
    let particle_capture = correction
        .get(2)
        .expect("subject-particle capture group exists");
    let superseded_capture = correction
        .get(3)
        .expect("superseded-object capture group exists");
    let marker_capture = correction
        .get(4)
        .expect("correction-marker capture group exists");
    let replacement_capture = correction
        .get(5)
        .expect("replacement-object capture group exists");
    let Some(subject_span) = captured_source_span(text, body_offset, subject_capture) else {
        return Ok(None);
    };
    let Some(particle_span) = captured_source_span(text, body_offset, particle_capture) else {
        return Ok(None);
    };
    let Some(superseded_span) = captured_source_span(text, body_offset, superseded_capture) else {
        return Ok(None);
    };
    let Some(marker_span) = captured_source_span(text, body_offset, marker_capture) else {
        return Ok(None);
    };
    let replacement_raw = replacement_capture.as_str();
    let replacement_leading = replacement_raw.len() - replacement_raw.trim_start().len();
    let replacement_value = replacement_raw.trim();
    let replacement_value = ["です", "である", "だ"]
        .iter()
        .find_map(|suffix| replacement_value.strip_suffix(suffix))
        .unwrap_or(replacement_value)
        .trim_end();
    if replacement_value.is_empty() {
        return Ok(None);
    }
    let replacement_start = body_offset + replacement_capture.start() + replacement_leading;
    let Some(replacement_span) = source_span_at(
        text,
        replacement_start,
        replacement_start + replacement_value.len(),
    ) else {
        return Ok(None);
    };

    if !matches!(particle_span.text.as_str(), "は" | "が")
        || !correction_source_subject_matches(&subject_span.text, contract.subject)
        || superseded_span.text.nfkc().collect::<String>()
            != contract.superseded_object.nfkc().collect::<String>()
        || replacement_span.text.nfkc().collect::<String>()
            != contract.replacement_object.nfkc().collect::<String>()
        || marker_span.text != "ではなく"
        || premise_correction_intent(text)?.as_ref() != Some(expected)
    {
        return Ok(None);
    }

    let mut roles = BTreeMap::from([
        (
            format!("{}/subject", contract.role_root),
            vec![subject_span],
        ),
        (
            format!("{}/subject_particle", contract.role_root),
            vec![particle_span],
        ),
        (
            format!("{}/superseded_object", contract.role_root),
            vec![superseded_span],
        ),
        (
            format!("{}/correction_marker", contract.role_root),
            vec![marker_span],
        ),
        (
            format!("{}/replacement_object", contract.role_root),
            vec![replacement_span],
        ),
    ]);
    if let Some(scope_span) = scope_span {
        roles.insert(format!("{}/scope", contract.role_root), vec![scope_span]);
    }
    Ok(Some(roles))
}

fn english_correction_object_source_spans(
    text: &str,
    source_offset: usize,
    capture: regex::Match<'_>,
    expected: &str,
) -> Option<(Option<SourceSpan>, SourceSpan)> {
    let captured = capture.as_str();
    let leading = captured.len() - captured.trim_start().len();
    let value = captured.trim();
    let start = source_offset + capture.start() + leading;
    let end = start + value.len();
    let (article, object_start) = ["a", "an", "the"]
        .iter()
        .find_map(|article| {
            let source_article = value.get(..article.len())?;
            let rest = value.get(article.len()..)?;
            if !source_article.eq_ignore_ascii_case(article) {
                return None;
            }
            let whitespace = rest.chars().next()?;
            if !whitespace.is_whitespace() {
                return None;
            }
            let whitespace_bytes = rest.len() - rest.trim_start().len();
            let article_span = source_span_at(text, start, start + article.len())?;
            Some((Some(article_span), start + article.len() + whitespace_bytes))
        })
        .unwrap_or((None, start));
    if object_start >= end {
        return None;
    }
    let object_span = source_span_at(text, object_start, end)?;
    let normalized = |value: &str| value.nfkc().collect::<String>();
    (normalized(&object_span.text) == normalized(expected)).then_some((article, object_span))
}

fn english_premise_correction_source_roles(
    text: &str,
    expected: &Intent,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    let Some(contract) = premise_correction_source_contract(expected) else {
        return Ok(None);
    };
    if contract.predicate != "is" || contract.superseded_object == contract.replacement_object {
        return Ok(None);
    }

    let trimmed = text.trim();
    let source_offset = text.len() - text.trim_start().len();
    let statement = terminal_trim(trimmed);
    let (body, body_offset, scope_span) =
        if let Some(scoped) = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(statement) {
            let scope_capture = scoped.get(1).expect("scope capture group exists");
            let body_capture = scoped.get(2).expect("statement capture group exists");
            let Some(scope_span) = captured_source_span(text, source_offset, scope_capture) else {
                return Ok(None);
            };
            if scope_span.text != contract.scope {
                return Ok(None);
            }
            (
                body_capture.as_str(),
                source_offset + body_capture.start(),
                Some(scope_span),
            )
        } else {
            if contract.scope != "default" {
                return Ok(None);
            }
            (statement, source_offset, None)
        };

    let Some(correction) =
        regex(r"(?i)^(?:correction:\s*)?(.+?)\s+(is|are)\s+(.+?),\s+(not)\s+(.+)$")?.captures(body)
    else {
        return Ok(None);
    };
    let subject_capture = correction.get(1).expect("subject capture group exists");
    let copula_capture = correction.get(2).expect("copula capture group exists");
    let replacement_capture = correction
        .get(3)
        .expect("replacement-object capture group exists");
    let marker_capture = correction
        .get(4)
        .expect("correction-marker capture group exists");
    let superseded_capture = correction
        .get(5)
        .expect("superseded-object capture group exists");
    let Some(subject_span) = captured_source_span(text, body_offset, subject_capture) else {
        return Ok(None);
    };
    let Some(copula_span) = captured_source_span(text, body_offset, copula_capture) else {
        return Ok(None);
    };
    let Some(marker_span) = captured_source_span(text, body_offset, marker_capture) else {
        return Ok(None);
    };
    let Some((superseded_article, superseded_object)) = english_correction_object_source_spans(
        text,
        body_offset,
        superseded_capture,
        contract.superseded_object,
    ) else {
        return Ok(None);
    };
    let Some((replacement_article, replacement_object)) = english_correction_object_source_spans(
        text,
        body_offset,
        replacement_capture,
        contract.replacement_object,
    ) else {
        return Ok(None);
    };
    if !matches!(copula_span.text.to_ascii_lowercase().as_str(), "is" | "are")
        || !marker_span.text.eq_ignore_ascii_case("not")
        || !correction_source_subject_matches(&subject_span.text, contract.subject)
        || premise_correction_intent(text)?.as_ref() != Some(expected)
    {
        return Ok(None);
    }

    let mut roles = BTreeMap::from([
        (
            format!("{}/subject", contract.role_root),
            vec![subject_span],
        ),
        (format!("{}/copula", contract.role_root), vec![copula_span]),
        (
            format!("{}/superseded_object", contract.role_root),
            vec![superseded_object],
        ),
        (
            format!("{}/correction_marker", contract.role_root),
            vec![marker_span],
        ),
        (
            format!("{}/replacement_object", contract.role_root),
            vec![replacement_object],
        ),
    ]);
    if let Some(article) = superseded_article {
        roles.insert(
            format!("{}/superseded_article", contract.role_root),
            vec![article],
        );
    }
    if let Some(article) = replacement_article {
        roles.insert(
            format!("{}/replacement_article", contract.role_root),
            vec![article],
        );
    }
    if let Some(scope_span) = scope_span {
        roles.insert(format!("{}/scope", contract.role_root), vec![scope_span]);
    }
    Ok(Some(roles))
}

fn premise_correction_source_roles(
    text: &str,
    expected: &Intent,
) -> Result<Option<BTreeMap<String, Vec<SourceSpan>>>> {
    if let Some(roles) = japanese_premise_correction_source_roles(text, expected)? {
        return Ok(Some(roles));
    }
    english_premise_correction_source_roles(text, expected)
}

/// Project complete source-role maps for supported assertions and corrections.
/// Contextual assertion/correction maps retain the literal pronoun; the sealed
/// bundle binds that request to the prior result used by Core.
pub(crate) fn assertion_source_evidence(
    bundle: &InputBundle,
) -> Result<Option<SealedSourceEvidence>> {
    if matches!(
        &bundle.meaning.intent,
        Intent::CorrectPremise { .. } | Intent::CorrectPreviousPremise { .. }
    ) {
        let contextual = matches!(
            &bundle.meaning.intent,
            Intent::CorrectPreviousPremise { .. }
        );
        if contextual && bundle.meaning.context_result_ref.is_none() {
            return Ok(None);
        }
        let Some(source_roles) =
            premise_correction_source_roles(&bundle.request.text, &bundle.meaning.intent)?
        else {
            return Ok(None);
        };
        if bundle.meaning.source_roles != source_roles {
            return Ok(None);
        }
        return Ok(Some(SealedSourceEvidence {
            evidence_version: SOURCE_EVIDENCE_VERSION.into(),
            input_fingerprint: identity(&bundle.request)?,
            bundle_fingerprint: bundle.fingerprint.clone(),
            raw_sha256: bundle.raw_sha256.clone(),
            parser_version: bundle.meaning.parser_version.clone(),
            semantic_signature: bundle.semantic_signature.clone(),
            source_roles,
        }));
    }

    let (proposition, role_root, contextual) = match &bundle.meaning.intent {
        Intent::Assert { proposition } => (proposition, "/Assert/proposition", false),
        Intent::AssertPreviousRelation { proposition, .. }
            if singular_previous_subject(&proposition.subject) =>
        {
            (proposition, "/AssertPreviousRelation/proposition", true)
        }
        _ => return Ok(None),
    };
    if contextual && bundle.meaning.context_result_ref.is_none() {
        return Ok(None);
    }

    if !contextual
        && let Some(source_roles) =
            english_assertion_source_roles_with_root(&bundle.request.text, proposition, role_root)?
    {
        if bundle.meaning.source_roles != source_roles {
            return Ok(None);
        }
        return Ok(Some(SealedSourceEvidence {
            evidence_version: SOURCE_EVIDENCE_VERSION.into(),
            input_fingerprint: identity(&bundle.request)?,
            bundle_fingerprint: bundle.fingerprint.clone(),
            raw_sha256: bundle.raw_sha256.clone(),
            parser_version: bundle.meaning.parser_version.clone(),
            semantic_signature: bundle.semantic_signature.clone(),
            source_roles,
        }));
    }

    if proposition.predicate != "is" {
        let Some(source_roles) = japanese_transitive_assertion_source_roles_with_root(
            &bundle.request.text,
            proposition,
            role_root,
        )?
        else {
            return Ok(None);
        };
        if bundle.meaning.source_roles != source_roles {
            return Ok(None);
        }
        return Ok(Some(SealedSourceEvidence {
            evidence_version: SOURCE_EVIDENCE_VERSION.into(),
            input_fingerprint: identity(&bundle.request)?,
            bundle_fingerprint: bundle.fingerprint.clone(),
            raw_sha256: bundle.raw_sha256.clone(),
            parser_version: bundle.meaning.parser_version.clone(),
            semantic_signature: bundle.semantic_signature.clone(),
            source_roles,
        }));
    }

    let roles = &bundle.meaning.source_roles;
    let subject_role = format!("{role_root}/subject");
    let predicate_role = format!("{role_root}/predicate");
    let object_role = format!("{role_root}/object");
    let scope_role = format!("{role_root}/scope");
    let negation_role = format!("{role_root}/negation");
    let allowed_roles = [
        subject_role.as_str(),
        predicate_role.as_str(),
        object_role.as_str(),
        scope_role.as_str(),
        negation_role.as_str(),
    ];
    if roles
        .keys()
        .any(|role| !allowed_roles.contains(&role.as_str()))
    {
        return Ok(None);
    }
    let single = |role: &str| -> Option<&SourceSpan> {
        let spans = roles.get(role)?;
        (spans.len() == 1).then(|| &spans[0])
    };
    let Some(subject) = single(&subject_role) else {
        return Ok(None);
    };
    let Some(predicate) = single(&predicate_role) else {
        return Ok(None);
    };
    let Some(object) = single(&object_role) else {
        return Ok(None);
    };
    let scope = if roles.contains_key(&scope_role) {
        let Some(scope) = single(&scope_role) else {
            return Ok(None);
        };
        Some(scope)
    } else {
        None
    };
    let negation = if roles.contains_key(&negation_role) {
        let Some(negation) = single(&negation_role) else {
            return Ok(None);
        };
        Some(negation)
    } else {
        None
    };
    let source_has_scope_tag = bundle.request.text.trim_start().starts_with("[scope=");
    if proposition.negative != negation.is_some() || source_has_scope_tag != scope.is_some() {
        return Ok(None);
    }

    let mut spans = vec![subject, predicate, object];
    spans.extend(scope);
    spans.extend(negation);
    if spans.iter().any(|span| {
        bundle.request.text.get(span.start..span.end) != Some(span.text.as_str())
            || span.start >= span.end
    }) {
        return Ok(None);
    }
    let mut ranges: Vec<_> = spans.iter().map(|span| (span.start, span.end)).collect();
    ranges.sort_unstable();
    if ranges.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Ok(None);
    }
    if scope.is_some_and(|span| span.end > subject.start)
        || subject.end > predicate.start
        || predicate.end > object.start
        || negation.is_some_and(|span| object.end > span.start)
    {
        return Ok(None);
    }

    let normalized = |value: &str| value.nfkc().collect::<String>();
    if normalized(&subject.text) != normalized(&proposition.subject)
        || !matches!(predicate.text.as_str(), "は" | "が")
        || object.text.nfkc().collect::<String>() != proposition.object.nfkc().collect::<String>()
        || scope.is_some_and(|span| {
            span.text.nfkc().collect::<String>() != proposition.scope.nfkc().collect::<String>()
        })
        || negation.is_some_and(|span| {
            ![
                "ではありません",
                "ではない",
                "じゃない",
                "でありません",
                "でない",
            ]
            .contains(&span.text.as_str())
        })
    {
        return Ok(None);
    }

    Ok(Some(SealedSourceEvidence {
        evidence_version: SOURCE_EVIDENCE_VERSION.into(),
        input_fingerprint: identity(&bundle.request)?,
        bundle_fingerprint: bundle.fingerprint.clone(),
        raw_sha256: bundle.raw_sha256.clone(),
        parser_version: bundle.meaning.parser_version.clone(),
        semantic_signature: bundle.semantic_signature.clone(),
        source_roles: roles.clone(),
    }))
}

fn has_token_boundaries(haystack: &str, start: usize, end: usize, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let needle = needle.as_bytes();
    let starts_word = needle.first().is_some_and(|byte| ascii_word_byte(*byte));
    let ends_word = needle.last().is_some_and(|byte| ascii_word_byte(*byte));
    if starts_word && start > 0 && ascii_word_byte(bytes[start - 1]) {
        return false;
    }
    if ends_word && end < bytes.len() && ascii_word_byte(bytes[end]) {
        return false;
    }
    let numeric = needle.iter().any(|byte| byte.is_ascii_digit())
        && needle.iter().all(|byte| numeric_token_byte(*byte));
    if numeric
        && ((start > 0 && numeric_token_byte(bytes[start - 1]))
            || (end < bytes.len() && numeric_token_byte(bytes[end])))
    {
        return false;
    }
    true
}

/// Build a versioned index from every string leaf in the sealed Intent back
/// to exact UTF-8 ranges in the original request. Intent's serde tag supplies
/// the stable role root; matching uses an NFKC view and returned text always
/// comes from the untouched original byte slice. Unknown's generated reason
/// is deliberately excluded from source evidence.
fn source_role_spans(text: &str, intent: &Intent) -> Result<BTreeMap<String, Vec<SourceSpan>>> {
    let intent_value = serde_json::to_value(intent)?;
    let Some(intent_object) = intent_value.as_object() else {
        return Ok(BTreeMap::new());
    };
    let Some(tag) = intent_object
        .get("kind")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(BTreeMap::new());
    };
    if tag == "UNKNOWN" {
        return Ok(BTreeMap::new());
    }
    let intent_role = format!("/{}", serde_tag_role(tag));
    if let Intent::Assert { proposition } = intent
        && let Some(source_roles) = japanese_assertion_source_roles(text, proposition)?
    {
        return Ok(source_roles);
    }
    if let Intent::Assert { proposition } = intent
        && let Some(source_roles) = english_assertion_source_roles(text, proposition)?
    {
        return Ok(source_roles);
    }
    if let Intent::Assert { proposition } = intent
        && let Some(source_roles) = japanese_transitive_assertion_source_roles(text, proposition)?
    {
        return Ok(source_roles);
    }
    if let Intent::AssertPreviousRelation { proposition, .. } = intent
        && singular_previous_subject(&proposition.subject)
        && let Some(source_roles) = japanese_assertion_source_roles_with_root(
            text,
            proposition,
            "/AssertPreviousRelation/proposition",
        )?
    {
        return Ok(source_roles);
    }
    if let Intent::AssertPreviousRelation { proposition, .. } = intent
        && singular_previous_subject(&proposition.subject)
        && let Some(source_roles) = japanese_transitive_assertion_source_roles_with_root(
            text,
            proposition,
            "/AssertPreviousRelation/proposition",
        )?
    {
        return Ok(source_roles);
    }
    if matches!(
        intent,
        Intent::CorrectPremise { .. } | Intent::CorrectPreviousPremise { .. }
    ) && let Some(source_roles) = premise_correction_source_roles(text, intent)?
    {
        return Ok(source_roles);
    }
    let mut candidates = SourceRoleCandidates::new();
    let mut japanese_predicate_roles = Vec::new();
    for (key, value) in intent_object {
        if key == "kind" {
            continue;
        }
        let role = format!("{intent_role}/{}", escape_role_segment(key));
        collect_string_leaf_candidates(value, &role, &mut candidates);
        collect_semantic_aliases(value, &role, &mut candidates, &mut japanese_predicate_roles);
    }
    if candidates.is_empty() {
        return Ok(BTreeMap::new());
    }

    let (normalized_source, source_ranges) = normalized_source_with_ranges(text);

    let mut pattern_roles = BTreeMap::<String, Vec<String>>::new();
    for (role, values) in candidates {
        for value in values {
            let normalized_value: String = value.nfkc().collect();
            if normalized_value.is_empty() || normalized_value.len() > normalized_source.len() {
                continue;
            }
            let roles = pattern_roles.entry(normalized_value).or_default();
            if !roles.iter().any(|existing| existing == &role) {
                roles.push(role.clone());
            }
        }
    }
    if pattern_roles.is_empty() {
        return Ok(BTreeMap::new());
    }
    let patterns = pattern_roles.keys().map(String::as_str).collect::<Vec<_>>();
    let automaton = AhoCorasick::new(&patterns).map_err(|error| {
        failure(
            "PARSER_CONFIGURATION",
            format!("source role index: {error}"),
        )
    })?;
    let mut pattern_spans = vec![Vec::<SourceSpan>::new(); patterns.len()];
    let mut match_events = 0;
    for matched in automaton.find_overlapping_iter(normalized_source.as_str()) {
        match_events += 1;
        if match_events > MAX_SOURCE_SPAN_MATCH_EVENTS {
            return Err(failure(
                "SOURCE_SPAN_INDEX_LIMIT",
                format!(
                    "source role matching exceeded {MAX_SOURCE_SPAN_MATCH_EVENTS} overlapping candidates"
                ),
            ));
        }
        let pattern_index = matched.pattern().as_usize();
        let pattern = patterns[pattern_index];
        if !has_token_boundaries(&normalized_source, matched.start(), matched.end(), pattern) {
            continue;
        }
        let source_start = source_ranges[matched.start()].0;
        let source_end = source_ranges[matched.end() - 1].1;
        let span = SourceSpan {
            start: source_start,
            end: source_end,
            text: text[source_start..source_end].to_string(),
        };
        let spans = &mut pattern_spans[pattern_index];
        if spans.len() < 32
            && !spans
                .iter()
                .any(|existing| existing.start == span.start && existing.end == span.end)
        {
            spans.push(span);
        }
    }
    for spans in &mut pattern_spans {
        spans.sort_by_key(|span| (span.start, span.end));
    }
    let mut result = BTreeMap::<String, Vec<SourceSpan>>::new();
    for (pattern_index, pattern) in patterns.iter().enumerate() {
        let Some(spans) = pattern_roles.get(*pattern) else {
            continue;
        };
        for role in spans {
            let role_spans = result.entry(role.clone()).or_default();
            role_spans.extend(pattern_spans[pattern_index].iter().cloned());
        }
    }
    for spans in result.values_mut() {
        spans.sort_by_key(|span| (span.start, span.end));
        spans.dedup_by(|left, right| left.start == right.start && left.end == right.end);
        spans.truncate(32);
    }
    for predicate_role in japanese_predicate_roles {
        if result
            .get(&predicate_role)
            .is_some_and(|spans| !spans.is_empty())
        {
            continue;
        }
        let Some(role_prefix) = predicate_role.strip_suffix("/predicate") else {
            continue;
        };
        let Some(subjects) = result.get(&format!("{role_prefix}/subject")) else {
            continue;
        };
        let Some(objects) = result.get(&format!("{role_prefix}/object")) else {
            continue;
        };
        let mut markers = Vec::new();
        for subject in subjects {
            for object in objects.iter().filter(|object| object.start >= subject.end) {
                let between = &text[subject.end..object.start];
                if let Some((index, marker)) = ["は", "が"]
                    .iter()
                    .filter_map(|marker| between.find(marker).map(|index| (index, *marker)))
                    .min_by_key(|(index, _)| *index)
                {
                    let start = subject.end + index;
                    let span = SourceSpan {
                        start,
                        end: start + marker.len(),
                        text: marker.to_string(),
                    };
                    if markers.len() < 32
                        && !markers.iter().any(|existing: &SourceSpan| {
                            existing.start == span.start && existing.end == span.end
                        })
                    {
                        markers.push(span);
                    }
                }
            }
        }
        markers.sort_by_key(|span| (span.start, span.end));
        if !markers.is_empty() {
            result.insert(predicate_role, markers);
        }
    }
    result.retain(|_, spans| !spans.is_empty());
    Ok(result)
}

fn regex(pattern: &str) -> Result<Regex> {
    Regex::new(pattern).map_err(|error| failure("PARSER_CONFIGURATION", error.to_string()))
}
fn terminal_trim(text: &str) -> &str {
    text.trim()
        .trim_end_matches(['。', '.', '!', '！', '?', '？'])
        .trim()
}

fn valid_calculation_step_id(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        && value.len() <= 32
}

fn calculation_step_references(expression: &str) -> std::result::Result<Vec<String>, String> {
    let mut references = Vec::new();
    let mut cursor = 0;
    while let Some(relative_start) = expression[cursor..].find("@{") {
        let start = cursor + relative_start;
        let id_start = start + 2;
        let Some(relative_end) = expression[id_start..].find('}') else {
            return Err("a calculation reference is missing its closing brace".into());
        };
        let end = id_start + relative_end;
        let id = &expression[id_start..end];
        if !valid_calculation_step_id(id) {
            return Err("calculation references require a valid step ID".into());
        }
        references.push(id.to_string());
        cursor = end + 1;
    }
    Ok(references)
}

fn calculation_sequence_intent(text: &str, header: &str) -> Result<Option<Intent>> {
    let japanese = matches!(header, "計算手順を実行して:" | "計算手順を実行して：");
    let english = header.eq_ignore_ascii_case("calculation-sequence:");
    if !japanese && !english {
        return Ok(None);
    }
    let Some(header_end) = text.find('\n') else {
        return Ok(Some(Intent::Unknown {
            reason: "calculation sequence requires a header newline and one or more step lines"
                .into(),
        }));
    };
    let mut line_start = header_end + 1;
    let mut seen = BTreeMap::<String, usize>::new();
    let mut steps = Vec::new();
    for raw_line in text[line_start..].split_inclusive('\n') {
        let line = raw_line
            .strip_suffix('\n')
            .unwrap_or(raw_line)
            .strip_suffix('\r')
            .unwrap_or(raw_line.strip_suffix('\n').unwrap_or(raw_line));
        if line.trim().is_empty() {
            line_start += raw_line.len();
            continue;
        }
        if steps.len() == 16 {
            return Ok(Some(Intent::Unknown {
                reason: "calculation sequence supports at most 16 declared steps".into(),
            }));
        }
        let Some(separator) = line.find(':') else {
            return Ok(Some(Intent::Unknown {
                reason: "each calculation sequence line must use step-id: expression".into(),
            }));
        };
        let id = line[..separator].trim();
        if !valid_calculation_step_id(id) || seen.contains_key(id) {
            return Ok(Some(Intent::Unknown {
                reason: "step IDs must be unique ASCII identifiers beginning with a letter".into(),
            }));
        }
        let raw_expression = &line[separator + 1..];
        let expression = raw_expression.trim();
        if expression.is_empty() || expression.len() > 8192 {
            return Ok(Some(Intent::Unknown {
                reason: "each calculation expression must contain 1..8192 UTF-8 bytes".into(),
            }));
        }
        let references = match calculation_step_references(expression) {
            Ok(references) => references,
            Err(reason) => return Ok(Some(Intent::Unknown { reason })),
        };
        let mut requires = Vec::new();
        for reference in references {
            if !seen.contains_key(&reference) {
                return Ok(Some(Intent::Unknown {
                    reason: format!(
                        "step {} may consume only an earlier declared result; reference {reference} is unavailable",
                        id
                    ),
                }));
            }
            if !requires.contains(&reference) {
                requires.push(reference);
            }
        }
        let leading_expression_bytes = raw_expression.len() - raw_expression.trim_start().len();
        let expression_start = line_start + separator + 1 + leading_expression_bytes;
        let step_index = steps.len();
        seen.insert(id.to_string(), step_index);
        steps.push(CalculationSequenceStep {
            id: id.to_string(),
            expression: SourceSpan {
                start: expression_start,
                end: expression_start + expression.len(),
                text: expression.to_string(),
            },
            requires,
        });
        line_start += raw_line.len();
    }
    if steps.is_empty() {
        return Ok(Some(Intent::Unknown {
            reason: "calculation sequence must contain at least one step".into(),
        }));
    }
    Ok(Some(Intent::CalculateSequence { steps }))
}

/// Explicit headers separate instructions from document/query data. The body is
/// sliced from the original request, without trim, newline conversion or NFKC.
fn structured_intent(text: &str) -> Result<Option<Intent>> {
    let first_line = text
        .split('\n')
        .next()
        .unwrap_or(text)
        .trim_end_matches('\r');
    let header = first_line.trim();
    if let Some(intent) = calculation_sequence_intent(text, header)? {
        return Ok(Some(intent));
    }
    if matches!(header, "コードを生成して:" | "コードを生成して：")
        || header.eq_ignore_ascii_case("code-generate:")
    {
        return Ok(Some(match text.split_once('\n') {
            Some((_, document)) => Intent::GenerateCode { document: document.into() },
            None => Intent::Unknown { reason: "code generation requires a header newline and explicit structured algorithm/constants document".into() },
        }));
    }
    if matches!(header, "文章を構成して:" | "文章を構成して：")
        || header.eq_ignore_ascii_case("text-compose:")
    {
        return Ok(Some(match text.split_once('\n') {
            Some((_, document)) => Intent::ComposeText { document: document.into() },
            None => Intent::Unknown { reason: "text composition requires a header newline and an explicit materials/units/order JSON document".into() },
        }));
    }
    if matches!(header, "文章を要約して:" | "文章を要約して：")
        || header.eq_ignore_ascii_case("text-summary:")
    {
        return Ok(Some(match text.split_once('\n') {
            Some((_, document)) => Intent::TextSummaryCandidates { document: document.into() },
            None => Intent::Unknown { reason: "text summary requires a header newline and an explicit text/max_sentences JSON document; output is an unverified extractive candidate".into() },
        }));
    }
    let text_operation = match header {
        "文章を取り込んで:" | "文章を取り込んで：" => Some(false),
        "文章を編集して:" | "文章を編集して：" => Some(true),
        _ if header.eq_ignore_ascii_case("text-import:") => Some(false),
        _ if header.eq_ignore_ascii_case("text-edit:") => Some(true),
        _ => None,
    };
    if let Some(edit) = text_operation {
        return Ok(Some(match text.split_once('\n') {
            Some((_, document)) if edit => Intent::EditText { document: document.into() },
            Some((_, document)) => Intent::ImportText { document: document.into() },
            None => Intent::Unknown { reason: "text import/edit requires a header newline and one complete explicit JSON document".into() },
        }));
    }
    if matches!(
        header,
        "文章の置換箇所を特定して:" | "文章の置換箇所を特定して："
    ) || header.eq_ignore_ascii_case("text-locate-replacement:")
    {
        return Ok(Some(match text.split_once('\n') {
            Some((_, document)) => Intent::LocateTextReplacement { document: document.into() },
            None => Intent::Unknown { reason: "text location requires a header newline and one complete explicit JSON document".into() },
        }));
    }
    let code_operation = match header {
        "コードを読んで:" | "コードを読んで：" => Some(0),
        "コードを評価して:" | "コードを評価して：" => Some(1),
        "コードを検証して:" | "コードを検証して：" => Some(2),
        _ if header.eq_ignore_ascii_case("code-read:") => Some(0),
        _ if header.eq_ignore_ascii_case("code-evaluate:") => Some(1),
        _ if header.eq_ignore_ascii_case("code-verify:") => Some(2),
        _ => None,
    };
    if let Some(operation) = code_operation {
        return Ok(Some(match text.split_once('\n') {
            Some((_, document)) => match operation {
                0 => Intent::ReadCode { document: document.into() },
                1 => Intent::EvaluateCode { document: document.into() },
                _ => Intent::VerifyCode { document: document.into() },
            },
            None => Intent::Unknown { reason: "code capability requires a header newline and one explicit source/settings JSON document".into() },
        }));
    }
    if header.starts_with("検索 ") || header.starts_with("search ") {
        if text.trim_end_matches(['\r', '\n']).contains(['\r', '\n']) {
            return Ok(Some(Intent::Unknown { reason: "metadata search accepts one explicitly bounded query line; following clauses are not silently discarded".into() }));
        }
        let Some(capture) =
            regex(r"^(?:検索|search)\s+(wikipedia|crossref|europepmc):[ \t]*(.+)$")?
                .captures(header)
        else {
            return Ok(Some(Intent::Unknown { reason: "search requires explicit wikipedia, crossref or europepmc provider and a colon-delimited query".into() }));
        };
        let raw_query = capture[2].trim();
        let query = if raw_query.starts_with('"') {
            serde_json::from_str::<String>(raw_query).map_err(|_| {
                failure(
                    "INVALID_SEARCH_QUERY",
                    "quoted query must be one complete JSON string",
                )
            })?
        } else {
            if regex(r"(?:保存|送信|作成|比較|計算|削除|実行)(?:してください|して|せよ)|(?i:\b(?:and|then)\s+(?:write|send|save|delete|calculate|compare|create)\b)")?.is_match(raw_query) {
                return Ok(Some(Intent::Unknown { reason: "additional requested effects are not represented by metadata-list search; quote the complete query explicitly when those words are literal search data".into() }));
            }
            raw_query.to_string()
        };
        if query.is_empty() {
            return Err(failure(
                "EMPTY_SEARCH_QUERY",
                "metadata query must not be empty",
            ));
        }
        return Ok(Some(Intent::Search {
            provider: capture[1].into(),
            query,
        }));
    }
    let looks_like_document = header.starts_with("CSVの列")
        || (header.starts_with("CSVの「")
            && header.contains("ごとに")
            && header.contains("統計量"))
        || header.starts_with("CSVを「")
        || header.starts_with("csv-select ")
        || header
            .get(..10)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("csv-filter"))
        || header
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("csv-sort"))
        || header
            .get(..12)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("csv-count-by"))
        || regex(r"(?i)^csv-group-statistics")?.is_match(header)
        || header.starts_with("CSVを列")
        || header
            .get(..14)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("csv-compare-by"))
        || regex(r"(?i)^csv-inner-join")?.is_match(header)
        || header.starts_with("csv-statistics ")
        || header.starts_with("CSVをJSONに変換して")
        || regex(r"(?i)^csv-to-json")?.is_match(header)
        || header.starts_with("JSONをCSVに変換して")
        || regex(r"(?i)^json-to-csv")?.is_match(header)
        || header == "JSONを比較して:"
        || header.starts_with("JSONオブジェクトからフィールド")
        || header
            .get(..19)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-object-project"))
        || header
            .get(..12)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-compare"))
        || header.starts_with("JSON表を列")
        || header.starts_with("JSON表を「")
        || header
            .get(..18)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-table-compare"))
        || regex(r"(?i)^json-table-inner-join")?.is_match(header)
        || header
            .get(..17)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-table-filter"))
        || header
            .get(..15)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-table-sort"))
        || header
            .get(..19)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-table-count-by"))
        || regex(r"(?i)^json-table-group-statistics")?.is_match(header)
        || (header.starts_with("JSON表の「")
            && header.contains("ごとに")
            && header.contains("統計量"))
        || header
            .get(..17)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-table-select"))
        || header
            .get(..18)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("json-table-project"))
        || header.starts_with("JSON表の列")
        || header.starts_with("JSON表から列")
        || header == "証拠を整理して:"
        || header
            .get(.."evidence-synthesize".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("evidence-synthesize"))
        || header.starts_with("JSONの配列")
        || header == "json-statistics"
        || header.starts_with("json-statistics ")
        || header.starts_with("置換「")
        || header.starts_with("replace ");
    if !looks_like_document {
        return Ok(None);
    }
    let Some((_, document)) = text.split_once('\n') else {
        return Ok(Some(Intent::Unknown {
            reason:
                "document operation requires a header newline and the exact original document body"
                    .into(),
        }));
    };
    let csv_to_json_header = match header {
        "CSVをJSONに変換して（先頭行をヘッダーとして）:" => Some(true),
        "CSVをJSONに変換して（ヘッダーなし）:" => Some(false),
        _ => regex(r"(?i)^csv-to-json[ \t]+(header|no-header):$")?
            .captures(header)
            .map(|capture| capture[1].eq_ignore_ascii_case("header")),
    };
    if let Some(has_header) = csv_to_json_header {
        return Ok(Some(Intent::CsvToJson {
            document: document.into(),
            has_header,
        }));
    }
    if let Some(capture) =
        regex(r"^CSVの列「([^」]+)」が値「([^」]*)」の行を選択して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvFilter {
            document: document.into(),
            column: capture[1].into(),
            equals: capture[2].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^csv-filter[ \t]+("(?:[^"\\]|\\.)*")[ \t]+equals[ \t]+("(?:[^"\\]|\\.)*"):$"#)?
            .captures(header)
    {
        return Ok(Some(Intent::CsvFilter {
            document: document.into(),
            column: serde_json::from_str::<String>(&capture[1])?,
            equals: serde_json::from_str::<String>(&capture[2])?,
        }));
    }
    if let Some(capture) =
        regex(r"^CSVの列「([^」]+)」を(昇順|降順)に並べ替えて[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvSort {
            document: document.into(),
            column: capture[1].into(),
            descending: &capture[2] == "降順",
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^csv-sort[ \t]+("(?:[^"\\]|\\.)*")[ \t]+(ascending|descending):$"#)?
            .captures(header)
    {
        return Ok(Some(Intent::CsvSort {
            document: document.into(),
            column: serde_json::from_str::<String>(&capture[1])?,
            descending: capture[2].eq_ignore_ascii_case("descending"),
        }));
    }
    if let Some(capture) = regex(r"^CSVを列「([^」]+)」で件数集計して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvGroupCount {
            document: document.into(),
            column: capture[1].into(),
        }));
    }
    if let Some(capture) =
        regex(r"^CSVの「([^」]+)」ごとに「([^」]+)」の統計量を計算して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvGroupStatistics {
            document: document.into(),
            group_column: capture[1].into(),
            value_column: capture[2].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^csv-group-statistics[ \t]+("(?:[^"\\]|\\.)*")[ \t]+("(?:[^"\\]|\\.)*"):$"#)?
            .captures(header)
    {
        return Ok(Some(Intent::CsvGroupStatistics {
            document: document.into(),
            group_column: serde_json::from_str::<String>(&capture[1])?,
            value_column: serde_json::from_str::<String>(&capture[2])?,
        }));
    }
    if let Some(capture) = regex(r"^CSVを列「([^」]+)」で2つの表を比較して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvCompare {
            document: document.into(),
            key_column: capture[1].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^csv-compare-by[ \t]+("(?:[^"\\]|\\.)*"):$"#)?.captures(header)
    {
        return Ok(Some(Intent::CsvCompare {
            document: document.into(),
            key_column: serde_json::from_str::<String>(&capture[1])?,
        }));
    }
    if let Some(capture) =
        regex(r"^CSVを「([^」]+)」と「([^」]+)」で内部結合して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvInnerJoin {
            document: document.into(),
            left_key_column: capture[1].into(),
            right_key_column: capture[2].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^csv-inner-join[ \t]+("(?:[^"\\]|\\.)*")[ \t]+("(?:[^"\\]|\\.)*"):$"#)?
            .captures(header)
    {
        return Ok(Some(Intent::CsvInnerJoin {
            document: document.into(),
            left_key_column: serde_json::from_str::<String>(&capture[1])?,
            right_key_column: serde_json::from_str::<String>(&capture[2])?,
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^csv-count-by[ \t]+("(?:[^"\\]|\\.)*"):$"#)?.captures(header)
    {
        return Ok(Some(Intent::CsvGroupCount {
            document: document.into(),
            column: serde_json::from_str::<String>(&capture[1])?,
        }));
    }
    if header == "JSONをCSVに変換して:" || regex(r"(?i)^json-to-csv:$")?.is_match(header) {
        return Ok(Some(Intent::JsonToCsv {
            document: document.into(),
        }));
    }
    if header == "JSONを比較して:" || regex(r"(?i)^json-compare:$")?.is_match(header) {
        return Ok(Some(Intent::JsonCompare {
            document: document.into(),
        }));
    }
    if let Some(capture) =
        regex(r"^JSONオブジェクトからフィールド(\[.*\])を抽出して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonObjectProject {
            document: document.into(),
            fields: serde_json::from_str::<Vec<String>>(&capture[1])?,
        }));
    }
    if let Some(capture) = regex(r#"(?i)^json-object-project[ \t]+(\[.*\]):$"#)?.captures(header) {
        return Ok(Some(Intent::JsonObjectProject {
            document: document.into(),
            fields: serde_json::from_str::<Vec<String>>(&capture[1])?,
        }));
    }
    if let Some(capture) = regex(r"^JSON表を列「([^」]+)」で比較して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableCompare {
            document: document.into(),
            key_column: capture[1].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^json-table-compare[ \t]+("(?:[^"\\]|\\.)*"):$"#)?.captures(header)
    {
        return Ok(Some(Intent::JsonTableCompare {
            document: document.into(),
            key_column: serde_json::from_str::<String>(&capture[1])?,
        }));
    }
    if let Some(capture) =
        regex(r"^JSON表を「([^」]+)」と「([^」]+)」で内部結合して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableInnerJoin {
            document: document.into(),
            left_key_column: capture[1].into(),
            right_key_column: capture[2].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^json-table-inner-join[ \t]+("(?:[^"\\]|\\.)*")[ \t]+("(?:[^"\\]|\\.)*"):$"#)?
            .captures(header)
    {
        return Ok(Some(Intent::JsonTableInnerJoin {
            document: document.into(),
            left_key_column: serde_json::from_str::<String>(&capture[1])?,
            right_key_column: serde_json::from_str::<String>(&capture[2])?,
        }));
    }
    if let Some(capture) =
        regex(r"^JSON表の列「([^」]+)」が値「([^」]*)」の行を選択して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableFilter {
            document: document.into(),
            column: capture[1].into(),
            equals: capture[2].into(),
        }));
    }
    if let Some(capture) = regex(
        r#"(?i)^json-table-filter[ \t]+("(?:[^"\\]|\\.)*")[ \t]+equals[ \t]+("(?:[^"\\]|\\.)*"):$"#,
    )?
    .captures(header)
    {
        return Ok(Some(Intent::JsonTableFilter {
            document: document.into(),
            column: serde_json::from_str::<String>(&capture[1])?,
            equals: serde_json::from_str::<String>(&capture[2])?,
        }));
    }
    if let Some(capture) =
        regex(r"^JSON表の列「([^」]+)」を(昇順|降順)に並べ替えて[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableSort {
            document: document.into(),
            column: capture[1].into(),
            descending: &capture[2] == "降順",
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^json-table-sort[ \t]+("(?:[^"\\]|\\.)*")[ \t]+(ascending|descending):$"#)?
            .captures(header)
    {
        return Ok(Some(Intent::JsonTableSort {
            document: document.into(),
            column: serde_json::from_str::<String>(&capture[1])?,
            descending: capture[2].eq_ignore_ascii_case("descending"),
        }));
    }
    if let Some(capture) = regex(r"^JSON表を列「([^」]+)」で件数集計して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableGroupCount {
            document: document.into(),
            column: capture[1].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^json-table-count-by[ \t]+("(?:[^"\\]|\\.)*"):$"#)?.captures(header)
    {
        return Ok(Some(Intent::JsonTableGroupCount {
            document: document.into(),
            column: serde_json::from_str::<String>(&capture[1])?,
        }));
    }
    if let Some(capture) =
        regex(r"^JSON表の「([^」]+)」ごとに「([^」]+)」の統計量を計算して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableGroupStatistics {
            document: document.into(),
            group_column: capture[1].into(),
            value_column: capture[2].into(),
        }));
    }
    if let Some(capture) = regex(
        r#"(?i)^json-table-group-statistics[ \t]+("(?:[^"\\]|\\.)*")[ \t]+("(?:[^"\\]|\\.)*"):$"#,
    )?
    .captures(header)
    {
        return Ok(Some(Intent::JsonTableGroupStatistics {
            document: document.into(),
            group_column: serde_json::from_str::<String>(&capture[1])?,
            value_column: serde_json::from_str::<String>(&capture[2])?,
        }));
    }
    if let Some(capture) = regex(r"^JSON表の列「([^」]+)」を選択して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableSelect {
            document: document.into(),
            column: capture[1].into(),
        }));
    }
    if let Some(capture) =
        regex(r#"(?i)^json-table-select[ \t]+("(?:[^"\\]|\\.)*"):$"#)?.captures(header)
    {
        return Ok(Some(Intent::JsonTableSelect {
            document: document.into(),
            column: serde_json::from_str::<String>(&capture[1])?,
        }));
    }
    if let Some(capture) = regex(r"^JSON表から列(\[.*\])を抽出して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonTableProject {
            document: document.into(),
            columns: serde_json::from_str::<Vec<String>>(&capture[1])?,
        }));
    }
    if let Some(capture) = regex(r#"(?i)^json-table-project[ \t]+(\[.*\]):$"#)?.captures(header) {
        return Ok(Some(Intent::JsonTableProject {
            document: document.into(),
            columns: serde_json::from_str::<Vec<String>>(&capture[1])?,
        }));
    }
    if header == "証拠を整理して:" || regex(r"(?i)^evidence-synthesize:$")?.is_match(header)
    {
        return Ok(Some(Intent::EvidenceSynthesize {
            document: document.into(),
        }));
    }
    if let Some(capture) = regex(r"^CSVの列「([^」]+)」の統計量を計算して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvStatistics {
            document: document.into(),
            column: capture[1].into(),
        }));
    }
    if let Some(raw) = header.strip_prefix("csv-statistics ") {
        let column = if raw.starts_with('"') {
            serde_json::from_str::<String>(raw).map_err(|_| {
                failure(
                    "INVALID_CSV_COLUMN",
                    "quoted column must be one complete JSON string",
                )
            })?
        } else if !raw.is_empty() && !raw.chars().any(char::is_whitespace) {
            raw.to_string()
        } else {
            return Ok(Some(Intent::Unknown {
                reason:
                    "CSV statistics column must be one literal token or one complete quoted string"
                        .into(),
            }));
        };
        return Ok(Some(Intent::CsvStatistics {
            document: document.into(),
            column,
        }));
    }
    if let Some(capture) =
        regex(r"^JSONの配列「([^」]*)」の統計量を計算して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::JsonStatistics {
            document: document.into(),
            pointer: capture[1].into(),
        }));
    }
    if header == "json-statistics" || header.starts_with("json-statistics ") {
        let raw = header.strip_prefix("json-statistics").unwrap_or("").trim();
        let pointer = if raw.starts_with('"') {
            serde_json::from_str::<String>(raw).map_err(|_| {
                failure(
                    "INVALID_JSON_POINTER",
                    "quoted pointer must be one complete JSON string",
                )
            })?
        } else if !raw.chars().any(char::is_whitespace) {
            raw.to_string()
        } else {
            return Ok(Some(Intent::Unknown {
                reason: "JSON statistics accepts one complete JSON Pointer only".into(),
            }));
        };
        return Ok(Some(Intent::JsonStatistics {
            document: document.into(),
            pointer,
        }));
    }
    if let Some(capture) = regex(r"^CSVの列「([^」]+)」を選択して[:：]$")?.captures(header)
    {
        return Ok(Some(Intent::CsvSelect {
            document: document.into(),
            column: capture[1].into(),
        }));
    }
    if let Some(raw) = header.strip_prefix("csv-select ") {
        let column = if raw.starts_with('"') {
            serde_json::from_str::<String>(raw).map_err(|_| {
                failure(
                    "INVALID_CSV_COLUMN",
                    "quoted column must be one complete JSON string",
                )
            })?
        } else if !raw.is_empty() && !raw.chars().any(char::is_whitespace) {
            raw.to_string()
        } else {
            return Ok(Some(Intent::Unknown {
                reason: "CSV column must be one literal token or one complete quoted string".into(),
            }));
        };
        return Ok(Some(Intent::CsvSelect {
            document: document.into(),
            column,
        }));
    }
    if let Some(capture) =
        regex(r"^置換「([^」]*)」[ \t]*→[ \t]*「([^」]*)」(?:[ \t]+件数[ \t]+([0-9]+))?[:：]$")?
            .captures(header)
    {
        let count = capture
            .get(3)
            .map(|value| {
                value.as_str().parse::<usize>().map_err(|_| {
                    failure("REPLACE_COUNT_LIMIT", "replacement count is out of range")
                })
            })
            .transpose()?;
        return Ok(Some(Intent::TextReplace {
            document: document.into(),
            old: capture[1].into(),
            new: capture[2].into(),
            count,
        }));
    }
    if let Some(capture) = regex(r#"^replace[ \t]+("(?:[^"\\]|\\.)*")[ \t]*->[ \t]*("(?:[^"\\]|\\.)*")(?:[ \t]+count[ \t]+([0-9]+))?:$"#)?.captures(header) {
        let count = capture.get(3).map(|value| value.as_str().parse::<usize>()
            .map_err(|_| failure("REPLACE_COUNT_LIMIT", "replacement count is out of range"))).transpose()?;
        return Ok(Some(Intent::TextReplace { document: document.into(), old: serde_json::from_str::<String>(&capture[1])?,
            new: serde_json::from_str::<String>(&capture[2])?, count }));
    }
    Ok(Some(Intent::Unknown { reason: "document operation header contains unsupported or ambiguous constraints; original body retained".into() }))
}

fn calculation_correction_intent(text: &str) -> Result<Option<Intent>> {
    let normalized: String = text.nfkc().collect();
    let text = normalized.trim();
    for (pattern, operand_index) in [
        (
            r"(?i)^(?:please\s+)?change\s+(?:the\s+)?(?:first|1st)(?:\s+operand)?\s+to\s+(.+)$",
            0,
        ),
        (
            r"(?i)^(?:please\s+)?change\s+(?:the\s+)?(?:second|2nd)(?:\s+operand)?\s+to\s+(.+)$",
            1,
        ),
        (r"(?i)^(?:please\s+)?change\s+operand\s*1\s+to\s+(.+)$", 0),
        (r"(?i)^(?:please\s+)?change\s+operand\s*2\s+to\s+(.+)$", 1),
        (
            r"^(?:計算の?)?第1項を\s*(.+?)\s*に(?:変更|修正)(?:してください|して|せよ)$",
            0,
        ),
        (
            r"^(?:計算の?)?第2項を\s*(.+?)\s*に(?:変更|修正)(?:してください|して|せよ)$",
            1,
        ),
        (
            r"^(?:計算の?)?第一項を\s*(.+?)\s*に(?:変更|修正)(?:してください|して|せよ)$",
            0,
        ),
        (
            r"^(?:計算の?)?第二項を\s*(.+?)\s*に(?:変更|修正)(?:してください|して|せよ)$",
            1,
        ),
    ] {
        if let Some(capture) = regex(pattern)?.captures(text) {
            return Ok(Some(Intent::CorrectCalculation {
                operand_index,
                replacement: capture[1].trim().to_string(),
            }));
        }
    }
    Ok(None)
}

fn premise_correction_intent(text: &str) -> Result<Option<Intent>> {
    if text.contains(['\n', '\r']) {
        return Ok(None);
    }
    let raw_statement = terminal_trim(text);
    let explicit_scope = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?
        .captures(raw_statement)
        .map(|capture| (capture[1].trim().to_string(), capture[2].trim().to_string()));
    let (scope, statement) = explicit_scope
        .clone()
        .unwrap_or_else(|| ("default".to_string(), raw_statement.to_string()));

    let japanese = regex(r"^(?:(?:訂正|訂正します)[：:]\s*)?(.+?)(?:は|が)(.+?)ではなく(.+)$")?;
    let english = regex(r"(?i)^(?:correction:\s*)?(.+?)\s+(?:is|are)\s+(.+?),\s+not\s+(.+)$")?;
    let parts = japanese
        .captures(&statement)
        .map(|capture| {
            (
                capture[1].trim().to_string(),
                capture[2].trim().to_string(),
                capture[3].trim().to_string(),
            )
        })
        .or_else(|| {
            english.captures(&statement).map(|capture| {
                (
                    capture[1].trim().to_string(),
                    capture[3].trim().to_string(),
                    capture[2].trim().to_string(),
                )
            })
        });
    let Some((subject, superseded_object, replacement_object)) = parts else {
        return Ok(None);
    };
    let replacement_object = match ["です", "である", "だ"]
        .iter()
        .find_map(|suffix| replacement_object.strip_suffix(suffix))
        .map(str::trim)
    {
        Some(value) if !value.is_empty() => value,
        Some(_) => {
            return Ok(Some(Intent::Unknown {
                reason: "premise correction needs a nonempty replacement relation".into(),
            }));
        }
        None => &replacement_object,
    };

    let prefix = format!("[scope={scope}]");
    let Some(superseded) =
        parse_proposition(&format!("{prefix}{subject} is {superseded_object}"), false)?
    else {
        return Ok(Some(Intent::Unknown {
            reason: "premise correction needs one supported prior relation".into(),
        }));
    };
    let Some(replacement) =
        parse_proposition(&format!("{prefix}{subject} is {replacement_object}"), false)?
    else {
        return Ok(Some(Intent::Unknown {
            reason: "premise correction needs one supported replacement relation".into(),
        }));
    };
    let lower_subject = superseded.subject.to_ascii_lowercase();
    let previous_subject = singular_previous_subject(&superseded.subject);
    let unresolved_pronoun = matches!(
        lower_subject.as_str(),
        "these" | "those" | "they" | "he" | "she"
    ) || matches!(superseded.subject.as_str(), "これら" | "それら");
    if previous_subject {
        return Ok(Some(Intent::CorrectPreviousPremise {
            predicate: superseded.predicate,
            superseded_object: superseded.object,
            replacement_object: replacement.object,
            scope: explicit_scope.map(|(scope, _)| scope),
        }));
    }
    if unresolved_pronoun {
        return Ok(Some(Intent::Unknown {
            reason: "premise correction needs one explicit named subject; pronouns are unresolved"
                .into(),
        }));
    }
    if superseded.negative
        || replacement.negative
        || superseded.subject != replacement.subject
        || superseded.predicate != replacement.predicate
        || superseded.scope != replacement.scope
        || superseded.object == replacement.object
    {
        return Ok(Some(Intent::Unknown {
            reason:
                "premise correction needs distinct positive relations for one subject and scope"
                    .into(),
        }));
    }
    Ok(Some(Intent::CorrectPremise {
        superseded,
        replacement,
    }))
}

fn inspect_premises_intent(text: &str) -> Result<Option<Intent>> {
    let recorded_patterns = [
        r"(?i)^what (?:facts|premises) (?:have been|are) (?:provided|recorded|stated) about (.+)$",
        r"^(.+?)(?:について|に関して)[、， ]*(?:(?:何|どんなこと)(?:が|を))?(?:分かって|わかって|知って)(?:いる|います)(?:こと)?(?:(?:を)?(?:教えて|説明して)(?:ください)?|ですか|でしょうか|か)?$",
    ];
    for pattern in recorded_patterns {
        if let Some(capture) = regex(pattern)?.captures(text) {
            let subject = capture[1].trim();
            if !subject.is_empty()
                && subject.len() <= 512
                && !subject.contains(['\n', '\r', '、', '，', ',', ';', '；'])
            {
                return Ok(Some(Intent::InspectPremises {
                    subject: subject.to_string(),
                }));
            }
            return Ok(Some(Intent::Unknown {
                reason: "premise inspection needs one nonempty subject without a clause list"
                    .into(),
            }));
        }
    }

    // These ordinary definitional forms query actor-scoped declared memory.
    // They do not authorize an open-world answer or a definition from a
    // retrieved source. Keep arithmetic expressions and unresolved pronouns
    // out of this route so a question is not silently reinterpreted as a
    // memory lookup for an unrelated subject.
    let natural_patterns = [
        r"(?i)^what (?:is|are) (.+)$",
        r"^(.+?)(?:とは|って)(?:何|なに)(?:ですか|でしょうか|です|なの|か)?$",
    ];
    for pattern in natural_patterns {
        if let Some(capture) = regex(pattern)?.captures(text) {
            let subject = capture[1].trim();
            let lower = subject.to_ascii_lowercase();
            let only_numeric_expression_chars = subject
                .chars()
                .all(|ch| ch.is_ascii_digit() || ". +-*/%^()=".contains(ch));
            let unresolved_pronoun =
                matches!(
                    lower.as_str(),
                    "it" | "this" | "that" | "these" | "those" | "they" | "he" | "she"
                ) || matches!(subject, "これ" | "それ" | "あれ" | "これら" | "それら");
            if subject.is_empty()
                || subject.len() > 512
                || subject.contains([
                    '\n', '\r', '、', '，', ',', ';', '；', '+', '*', '/', '^', '%', '=',
                ])
                || only_numeric_expression_chars
                || unresolved_pronoun
            {
                return Ok(None);
            }
            return Ok(Some(Intent::InspectPremises {
                subject: subject.to_string(),
            }));
        }
    }
    Ok(None)
}

fn relation_profile_intent(text: &str) -> Result<Option<Intent>> {
    let japanese = regex(
        r"^(.+?)(?:について|に関する)(?:保存されている|保存済みの|会話に保存された)?(?:情報|記録)(?:をもとに|から)(?:短い)?(?:紹介文|プロフィール)(?:を)?(?:書いて|作って|作成して)(?:ください)?$",
    )?;
    let english = regex(
        r"(?i)^(?:write|create) (?:a |an )?(?:short )?(?:profile|introduction) of (.+?) (?:from|using) (?:the )?(?:saved|recorded|provided) (?:facts|information)$",
    )?;
    let Some(captures) = japanese.captures(text).or_else(|| english.captures(text)) else {
        return Ok(None);
    };
    let subject = captures[1].trim();
    let lower = subject.to_ascii_lowercase();
    let only_numeric_expression_chars = subject
        .chars()
        .all(|ch| ch.is_ascii_digit() || ". +-*/%^()=".contains(ch));
    let unresolved_pronoun = matches!(
        lower.as_str(),
        "it" | "this" | "that" | "these" | "those" | "they" | "he" | "she"
    ) || matches!(subject, "これ" | "それ" | "あれ" | "これら" | "それら");
    if subject.is_empty()
        || subject.len() > 512
        || subject.contains([
            '\n', '\r', '、', '，', ',', ';', '；', '+', '*', '/', '^', '%', '=',
        ])
        || only_numeric_expression_chars
        || unresolved_pronoun
    {
        return Ok(Some(Intent::Unknown {
            reason: "relation profile drafting needs one explicit nonnumeric subject".into(),
        }));
    }
    Ok(Some(Intent::DraftRelationProfile {
        subject: subject.to_string(),
    }))
}

fn common_relations_question_intent(text: &str) -> Result<Option<Intent>> {
    let scoped = regex(r"^\[scope=([^\]]*)\]\s*(.+)$")?.captures(text);
    let scope = scoped
        .as_ref()
        .map(|capture| capture[1].trim().to_string())
        .unwrap_or_else(|| "default".into());
    let body = scoped.as_ref().map_or(text, |capture| {
        capture.get(2).expect("matched body").as_str()
    });
    let japanese = regex(r"^(.+?)と(.+?)の共通点は(?:何ですか|何でしょうか|何か|何)$")?;
    let english = regex(r"(?i)^what do (.+?) and (.+?) have in common$")?;
    let captures = japanese.captures(body).or_else(|| english.captures(body));
    let Some(captures) = captures else {
        return Ok(None);
    };
    let subject_a = captures[1].trim();
    let subject_b = captures[2].trim();
    let valid_subject = |subject: &str| {
        !subject.is_empty()
            && subject.len() <= 512
            && !subject.contains(['\n', '\r', '、', '，', ',', ';', '；', 'と'])
            && !matches!(
                subject.to_ascii_lowercase().as_str(),
                "it" | "this" | "that" | "these" | "those" | "they" | "he" | "she"
            )
            && !matches!(subject, "これ" | "それ" | "あれ" | "これら" | "それら")
    };
    if !valid_subject(subject_a)
        || !valid_subject(subject_b)
        || subject_a == subject_b
        || scope.is_empty()
    {
        return Ok(Some(Intent::Unknown {
            reason: "common-relation questions require two distinct explicit subjects and one nonempty scope".into(),
        }));
    }
    Ok(Some(Intent::AskCommonRelations {
        subject_a: subject_a.into(),
        subject_b: subject_b.into(),
        scope,
    }))
}

fn relation_inventory_comparison_intent(text: &str) -> Result<Option<Intent>> {
    let scoped = regex(r"^\[scope=([^\]]*)\]\s*(.+)$")?.captures(text);
    let scope = scoped
        .as_ref()
        .map(|capture| capture[1].trim().to_string())
        .unwrap_or_else(|| "default".into());
    let body = scoped.as_ref().map_or(text, |capture| {
        capture.get(2).expect("matched body").as_str()
    });
    let japanese = regex(
        r"^(.+?)と(.+?)の(?:保存済みの)?関係を(?:比較して|比較してください|比べて|比べてください)$",
    )?;
    let english = regex(r"(?i)^compare (?:the )?(?:saved )?relations for (.+?) and (.+?)$")?;
    let captures = japanese.captures(body).or_else(|| english.captures(body));
    let Some(captures) = captures else {
        return Ok(None);
    };
    let subject_a = captures[1].trim();
    let subject_b = captures[2].trim();
    if !valid_relation_subject(subject_a)
        || !valid_relation_subject(subject_b)
        || subject_a == subject_b
        || scope.is_empty()
    {
        return Ok(Some(Intent::Unknown {
            reason: "saved-relation comparisons require two distinct explicit subjects and one nonempty scope".into(),
        }));
    }
    Ok(Some(Intent::CompareRelationInventories {
        subject_a: subject_a.into(),
        subject_b: subject_b.into(),
        scope,
    }))
}

fn valid_relation_subject(subject: &str) -> bool {
    !subject.is_empty()
        && subject.len() <= 512
        && !subject.contains(['\n', '\r', '、', '，', ',', ';', '；', 'と'])
        && !matches!(
            subject.to_ascii_lowercase().as_str(),
            "it" | "this" | "that" | "these" | "those" | "they" | "he" | "she"
        )
        && !matches!(subject, "これ" | "それ" | "あれ" | "これら" | "それら")
}

fn previous_group_mean_question_intent(text: &str) -> Result<Option<Intent>> {
    let patterns = [
        r"(?i)^(?:which|what) group(?:'s)? (?:has )?(?:the )?highest (?:mean|average)(?: value)?$",
        r"(?i)^which group(?:'s)? (?:mean|average)(?: value)? (?:is )?(?:the )?highest$",
        r"^(?:(?:この|その)(?:集計|統計)?表(?:から|で)?[、， ]*)?(?:平均(?:値)?が(?:最も|一番|いちばん)(?:高い|大きい)|(?:最も|一番|いちばん)(?:高い|大きい)平均(?:値)?の)(?:グループ|分類|カテゴリ)(?:は|が)?(?:どれ|何|どの(?:グループ|分類|カテゴリ))(?:ですか|でしょうか|か)?$",
        r"^どの(?:グループ|分類|カテゴリ)の平均(?:値)?(?:が|は)(?:最も|一番|いちばん)(?:高い|大きい)(?:ですか|でしょうか|か)?$",
    ];
    for pattern in patterns {
        if regex(pattern)?.is_match(text) {
            return Ok(Some(Intent::AskPreviousGroupMean));
        }
    }
    Ok(None)
}

fn previous_text_replacement_intent(text: &str) -> Result<Option<Intent>> {
    let japanese = regex(
        r#"^(?:前回|直前)の(?:回答|文章)の[「『"](.+?)[」』"]を[「『"](.+?)[」』"]に(?:置き換えて|置換して)(?:ください)?$"#,
    )?;
    let english = regex(
        r#"(?i)^replace\s+["'](.+?)["']\s+with\s+["'](.+?)["']\s+in\s+(?:the\s+)?previous\s+(?:answer|response|text)$"#,
    )?;
    let captures = japanese.captures(text).or_else(|| english.captures(text));
    Ok(captures.map(|captures| Intent::ReplacePreviousText {
        old: captures[1].to_string(),
        new: captures[2].to_string(),
    }))
}

fn previous_source_question_intent(text: &str) -> Result<Option<Intent>> {
    let english = regex(r"(?i)^what does (?:the )?(?:source|article|page) say about\s+(.+)$")?;
    let japanese = regex(
        r#"^(?:資料|記事|本文)(?:では|に|から)?\s*[「『"']?(.+?)[」』"']?\s*について\s*(?:何|どう)(?:と)?(?:述べていますか|書かれていますか|記載されていますか|説明していますか|述べている|書かれている)$"#,
    )?;
    let captured = english
        .captures(text)
        .or_else(|| japanese.captures(text))
        .and_then(|captures| captures.get(1))
        .map(|value| {
            value
                .as_str()
                .trim()
                .trim_matches(['「', '」', '『', '』', '\"', '\'', ' ', '　'])
                .to_string()
        })
        .filter(|value| !value.is_empty());
    Ok(captured.map(|query| Intent::AskPreviousSourceQuestion { query }))
}

fn why_question_intent(text: &str) -> Result<Option<Intent>> {
    let statement = if let Some(value) = text.strip_prefix("なぜ") {
        Some(value)
    } else if let Some(value) = text.strip_prefix("どうして") {
        Some(value)
    } else if text.eq_ignore_ascii_case("why") {
        Some("")
    } else {
        regex(r"(?i)^why\s+(.+)$")?
            .captures(text)
            .map(|capture| capture.get(1).expect("matched group").as_str())
    };
    let Some(mut statement) = statement.map(str::trim) else {
        return Ok(None);
    };
    for suffix in [
        "なのですか",
        "なのか",
        "のですか",
        "ですか",
        "か",
        "なの",
        "です",
    ] {
        if let Some(value) = statement.strip_suffix(suffix) {
            statement = value.trim_end();
            break;
        }
    }
    if statement.is_empty() {
        return Ok(Some(Intent::ExplainPrevious));
    }
    if let Some(proposition) = parse_proposition(statement, false)? {
        if singular_previous_subject(&proposition.subject) {
            return Ok(Some(Intent::AskPreviousRelation {
                proposition,
                explain: true,
                explicit_scope: explicit_scope_prefix(statement)?,
            }));
        }
        Ok(Some(Intent::AskWhy { proposition }))
    } else {
        Ok(Some(Intent::Unknown {
            reason: "why-questions require one unambiguous supported relation and proof scope"
                .into(),
        }))
    }
}

pub(crate) fn singular_previous_subject(subject: &str) -> bool {
    matches!(
        subject.to_ascii_lowercase().as_str(),
        "it" | "this" | "that"
    ) || matches!(subject, "これ" | "それ" | "あれ")
}

fn explicit_scope_prefix(text: &str) -> Result<Option<String>> {
    Ok(regex(r"^\[scope=([^\]]+)\]")?
        .captures(text)
        .map(|capture| capture[1].trim().to_string()))
}

fn unsupported_conjunctive_relation_question() -> Intent {
    Intent::Unknown {
        reason: "conjunctive relation questions require two or three supported relation clauses with one explicit subject and scope".into(),
    }
}

fn conjunctive_relation_question_intent(text: &str) -> Result<Option<Intent>> {
    let scope_match = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(text);
    let scope = scope_match
        .as_ref()
        .map(|capture| capture[1].trim().to_string());
    let body = scope_match.as_ref().map_or(text, |capture| {
        capture.get(2).expect("matched body").as_str()
    });
    let english = regex(r"(?i)\s+and\s+")?;
    let english_connectors: Vec<_> = english.find_iter(body).collect();
    let japanese_katsu: Vec<_> = body.match_indices("かつ").collect();
    let japanese_kanji_katsu: Vec<_> = body.match_indices("且つ").collect();
    if (!english_connectors.is_empty()
        || !japanese_katsu.is_empty()
        || !japanese_kanji_katsu.is_empty())
        && (body.contains("または") || regex(r"(?i)\bor\b")?.is_match(body))
    {
        return Ok(Some(unsupported_conjunctive_relation_question()));
    }
    let mut connectors: Vec<(usize, usize, u8)> = english_connectors
        .iter()
        .map(|connector| (connector.start(), connector.end(), 0))
        .collect();
    connectors.extend(
        japanese_katsu
            .iter()
            .map(|(start, connector)| (*start, start + connector.len(), 1)),
    );
    connectors.extend(
        japanese_kanji_katsu
            .iter()
            .map(|(start, connector)| (*start, start + connector.len(), 2)),
    );
    connectors.sort_by_key(|(start, _, _)| *start);
    if connectors.is_empty() {
        return Ok(None);
    }
    if !(1..=2).contains(&connectors.len())
        || connectors
            .iter()
            .any(|(_, _, kind)| *kind != connectors[0].2)
    {
        return Ok(Some(unsupported_conjunctive_relation_question()));
    }
    let english_form = connectors[0].2 == 0;
    let mut clauses = Vec::with_capacity(connectors.len() + 1);
    let mut clause_start = 0;
    for (connector_start, connector_end, _) in &connectors {
        clauses.push(body[clause_start..*connector_start].trim());
        clause_start = *connector_end;
    }
    clauses.push(body[clause_start..].trim());
    if clauses.iter().any(|clause| clause.is_empty()) {
        return Ok(Some(unsupported_conjunctive_relation_question()));
    }
    let with_scope = |clause: &str| {
        scope.as_ref().map_or_else(
            || clause.to_string(),
            |scope| format!("[scope={scope}] {clause}"),
        )
    };
    let Some(first) = parse_proposition(&with_scope(clauses[0]), false)? else {
        return Ok(Some(unsupported_conjunctive_relation_question()));
    };
    let referential_subject = matches!(
        first.subject.to_ascii_lowercase().as_str(),
        "i" | "it" | "this" | "that" | "these" | "those" | "we" | "they" | "you" | "he" | "she"
    ) || matches!(
        first.subject.as_str(),
        "これ"
            | "それ"
            | "あれ"
            | "これら"
            | "それら"
            | "あれら"
            | "私"
            | "わたし"
            | "僕"
            | "ぼく"
            | "俺"
            | "あなた"
            | "君"
            | "彼"
            | "彼女"
    );
    if referential_subject {
        return Ok(Some(unsupported_conjunctive_relation_question()));
    }
    let mut propositions = vec![first.clone()];
    for clause in clauses.iter().skip(1) {
        let direct_clause = with_scope(clause);
        let proposition = if let Some(proposition) = parse_proposition(&direct_clause, false)? {
            proposition
        } else {
            let expanded = if english_form {
                format!("{} is {}", first.subject, clause)
            } else {
                format!("{}は{}", first.subject, clause)
            };
            let Some(proposition) = parse_proposition(&with_scope(&expanded), false)? else {
                return Ok(Some(unsupported_conjunctive_relation_question()));
            };
            proposition
        };
        if proposition.subject != first.subject || proposition.scope != first.scope {
            return Ok(Some(unsupported_conjunctive_relation_question()));
        }
        propositions.push(proposition);
    }
    Ok(Some(Intent::AskConjunction { propositions }))
}

/// Parse a rule antecedent containing exactly two conjunctively joined
/// copular propositions. The bounded grammar requires one subject and scope;
/// a shorthand second Japanese/English proposition inherits both from the
/// first proposition.
fn conjunctive_rule_premises(text: &str) -> Result<Option<Vec<Proposition>>> {
    let mut connectors: Vec<(usize, usize, bool)> = regex(r"(?i)\band\b")?
        .find_iter(text)
        .map(|found| (found.start(), found.end(), true))
        .collect();
    connectors.extend(
        text.match_indices("かつ")
            .map(|(start, token)| (start, start + token.len(), false)),
    );
    connectors.extend(
        text.match_indices("且つ")
            .map(|(start, token)| (start, start + token.len(), false)),
    );
    if connectors.is_empty() {
        return Ok(None);
    }
    if connectors.len() != 1 {
        return Ok(Some(Vec::new()));
    }
    let (start, end, english) = connectors[0];
    let left = text[..start].trim();
    let right = text[end..].trim();
    let Some(first) = parse_proposition(left, true)? else {
        return Ok(Some(Vec::new()));
    };
    let right_scope = explicit_scope_prefix(right)?;
    let right_body = regex(r"^\[scope=[^\]]+\]\s*(.*)$")?
        .captures(right)
        .map_or(right, |capture| {
            capture.get(1).expect("matched body").as_str()
        })
        .trim();
    let mut second = if let Some(proposition) = parse_proposition(right, true)? {
        proposition
    } else {
        let expanded = if english {
            format!("{} is {}", first.subject, right_body)
        } else {
            format!("{}は{}", first.subject, right_body)
        };
        let expanded = if let Some(scope) = right_scope.as_deref() {
            format!("[scope={scope}] {expanded}")
        } else {
            expanded
        };
        let Some(proposition) = parse_proposition(&expanded, true)? else {
            return Ok(Some(Vec::new()));
        };
        proposition
    };
    if right_scope.is_none() {
        second.scope = first.scope.clone();
    }
    if first.subject != second.subject || first.scope != second.scope {
        return Ok(Some(Vec::new()));
    }
    Ok(Some(vec![first, second]))
}

fn interpret(text: &str) -> Result<Intent> {
    let header = text
        .split_once('\n')
        .map_or_else(|| terminal_trim(text), |(header, _)| terminal_trim(header));
    if matches!(header, "生成コードを検証して:" | "生成コードを検証して：")
        || header.eq_ignore_ascii_case("verify-generated-code:")
    {
        return Ok(match text.split_once('\n') {
            Some((_, document)) => Intent::VerifyPreviousCode {
                document: document.into(),
            },
            None => Intent::Unknown {
                reason: "generated-code verification requires one finite case document".into(),
            },
        });
    }
    if let Some(intent) = structured_intent(text)? {
        return Ok(intent);
    }
    if let Some(intent) = meaning_comparison_intent(text)? {
        return Ok(intent);
    }
    let body = terminal_trim(text);
    if let Some(capture) = regex(
        r"(?i)^(?:検索結果の)?([1-5])件目の本文を(?:取得して|取得してください|読んで|読んでください)$",
    )?
    .captures(body)
    {
        return Ok(Intent::FetchPreviousSearchCandidate {
            rank: capture[1].parse().map_err(|_| {
                failure("INVALID_SEARCH_RANK", "candidate rank is outside its valid range")
            })?,
        });
    }
    if let Some(capture) =
        regex(r"(?i)^(?:fetch|read) (?:the )?body of (?:search )?result ([1-5])$")?.captures(body)
    {
        return Ok(Intent::FetchPreviousSearchCandidate {
            rank: capture[1].parse().map_err(|_| {
                failure(
                    "INVALID_SEARCH_RANK",
                    "candidate rank is outside its valid range",
                )
            })?,
        });
    }
    if let Some(correction) = calculation_correction_intent(body)? {
        return Ok(correction);
    }
    if let Some(correction) = premise_correction_intent(body)? {
        return Ok(correction);
    }
    if matches!(
        body.to_lowercase().as_str(),
        "cancel" | "stop" | "中止" | "停止" | "キャンセル" | "やめて"
    ) {
        return Ok(Intent::Cancel);
    }
    if let Some(intent) = constrained_previous_source_summary_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = previous_result_summary_intent(body)? {
        return Ok(intent);
    }
    if regex(r"(?i)^(?:(?:(?:前回|前)(?:の)?(?:取得した)?|取得した)(?:資料|本文|出典)|その(?:資料|本文|出典))(?:を)?要約(?:してください|して|せよ)$|^(?:summarize|summarise) (?:the )?(?:previous|retrieved) (?:source|body|document)$")?.is_match(body) {
        return Ok(Intent::SummarizePreviousSource);
    }
    if regex(r"(?i)^(?:explain (?:that|it|the (?:previous )?(?:result|answer))|(?:それ|その答え|その結果|前の結果|直前の結果|さっきの答え)(?:を)?(?:詳しく)?説明(?:してください|して|せよ)|詳しく説明して)$")?.is_match(body) {
        return Ok(Intent::ExplainPrevious);
    }
    if let Some(capture) = regex(r"(?i)^(?:invalidate|retract)\s+([^\s]+)$")?.captures(body) {
        return Ok(Intent::Invalidate {
            id: capture[1].into(),
        });
    }
    if let Some(capture) =
        regex(r"^(?:規則|事実|記録)?\s*([^\s]+?)\s*を(?:無効化|撤回)(?:してください|して|する)?$")?
            .captures(body)
    {
        return Ok(Intent::Invalidate {
            id: capture[1].into(),
        });
    }
    if let Some(capture) = regex(r"(?is)^json-pointer[ \t]+(\S*)[ \t]*\r?\n(.*)$")?.captures(text) {
        let pointer = capture[1].to_string();
        let document = capture[2].to_string();
        if !pointer.is_empty() && !pointer.starts_with('/') {
            return Err(failure(
                "INVALID_JSON_POINTER",
                "pointer must be empty or start with /",
            ));
        }
        return Ok(Intent::JsonPointer { document, pointer });
    }
    // Quoted documents are data. The supported declarative language does not
    // grant imperative authority to text inside a quote or a retrieved body.
    if body.starts_with(['「', '『', '"', '\'']) {
        return Ok(Intent::Unknown {
            reason:
                "quoted content requires an explicit supported operation; its instructions are data"
                    .into(),
        });
    }
    if let Some(intent) = why_question_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = relation_inventory_comparison_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = common_relations_question_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = relation_profile_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = inspect_premises_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = previous_text_replacement_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = previous_group_mean_question_intent(body)? {
        return Ok(intent);
    }
    if let Some(intent) = previous_source_question_intent(body)? {
        return Ok(intent);
    }
    if let Some(capture) =
        regex(r"(?is)^(?:もし\s*)?(.+?)(?:ならば|なら|であれば)[、,\s]*(.+)$")?.captures(body)
    {
        if text.trim_end().ends_with(['?', '？'])
            || body.ends_with("ですか")
            || body.ends_with("なのか")
        {
            return Ok(Intent::Unknown { reason: "a conditional question requires a supported proof-query interpretation; it is not a rule assertion".into() });
        }

        let left = capture[1].trim();
        let right = capture[2].trim();
        if let Some(premises) = conjunctive_rule_premises(left)? {
            if premises.len() == 2
                && let Some(conclusion) = parse_proposition(right, true)?
            {
                return Ok(Intent::AddConjunctiveRule {
                    premises,
                    conclusion,
                });
            }
            return Ok(Intent::Unknown {
                reason: "conjunctive rule requires exactly two same-subject, same-scope copular premises".into(),
            });
        }
        if let (Some(premise), Some(conclusion)) = (
            parse_proposition(left, true)?,
            parse_proposition(right, true)?,
        ) {
            return Ok(Intent::AddRule {
                premise,
                conclusion,
            });
        }
        return Ok(Intent::Unknown {
            reason: "conditional relation is outside the supported copular grammar".into(),
        });
    }
    if let Some(capture) = regex(r"(?is)^if\s+(.+?)\s+then\s+(.+)$")?.captures(body) {
        if text.trim_end().ends_with(['?', '？'])
            || body.ends_with("ですか")
            || body.ends_with("なのか")
        {
            return Ok(Intent::Unknown { reason: "a conditional question requires a supported proof-query interpretation; it is not a rule assertion".into() });
        }

        if let Some(premises) = conjunctive_rule_premises(&capture[1])? {
            if premises.len() == 2
                && let Some(conclusion) = parse_proposition(&capture[2], true)?
            {
                return Ok(Intent::AddConjunctiveRule {
                    premises,
                    conclusion,
                });
            }
            return Ok(Intent::Unknown {
                reason: "conjunctive rule requires exactly two same-subject, same-scope copular premises".into(),
            });
        }
        if let (Some(premise), Some(conclusion)) = (
            parse_proposition(&capture[1], true)?,
            parse_proposition(&capture[2], true)?,
        ) {
            return Ok(Intent::AddRule {
                premise,
                conclusion,
            });
        }
        return Ok(Intent::Unknown {
            reason: "conditional relation is outside the supported copular grammar".into(),
        });
    }
    let url_pattern = regex(r#"https?://[^\s<>「」『』"]+"#)?;
    let mut urls = Vec::new();
    for found in url_pattern.find_iter(text) {
        let value = found
            .as_str()
            .trim_end_matches(['。', '、', ',', '.', ')', '）', ']', '】']);
        let parsed =
            url::Url::parse(value).map_err(|_| failure("INVALID_URL", "malformed source URL"))?;
        if parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(failure(
                "INVALID_URL",
                "source URL needs a hostname and must not contain credentials",
            ));
        }
        if !urls.iter().any(|old| old == value) {
            urls.push(value.to_string());
        }
    }
    if !urls.is_empty() {
        let mut residual = text.to_string();
        for uri in &urls {
            residual = residual.replace(uri, " ");
        }
        let residual = residual
            .trim()
            .trim_matches([',', '、', '。', '.', '!', '！', ' ', '\n', '\r', '\t'])
            .trim()
            .to_lowercase();
        if matches!(
            residual.as_str(),
            "" | "fetch"
                | "read"
                | "retrieve"
                | "取得"
                | "本文取得"
                | "本文を取得"
                | "取得して"
                | "本文を取得して"
                | "を取得して"
                | "の本文を取得して"
                | "読んで"
                | "を読んで"
                | "の本文を読んで"
        ) {
            return Ok(Intent::Retrieve {
                urls,
                question: text.into(),
            });
        }
        return Ok(Intent::Unknown {
            reason: format!(
                "source URLs found, but additional requirements are not represented by the source-body retrieval operation: {residual}"
            ),
        });
    }
    if let Some(intent) = mathematics_intent(body)? {
        return Ok(intent);
    }
    if let Some(expression) = arithmetic_expression(body)? {
        return Ok(Intent::Calculate { expression });
    }
    if text.trim_end().ends_with(['?', '？'])
        && let Some(expression) = trailing_topic_arithmetic_question(body)?
    {
        return Ok(Intent::Calculate { expression });
    }
    if regex(r"(?i)^(?:which|what|who|where|when|how)\b")?.is_match(body)
        || regex(r"^(?:どの|どれ|何|なに|誰|だれ|どこ|いつ|どう|いくら|いくつ)")?.is_match(body)
    {
        return Ok(Intent::Unknown {
            reason: "the wh-question is outside the supported typed question grammar".into(),
        });
    }
    let question = text.trim_end().ends_with(['?', '？'])
        || body.ends_with("ですか")
        || body.ends_with("なのか")
        || body.ends_with('か')
        || regex(r"(?i)^is\s+")?.is_match(body);
    let query_body = body
        .strip_suffix("ですか")
        .or_else(|| body.strip_suffix("なのか"))
        .or_else(|| body.strip_suffix('か'))
        .unwrap_or(body);
    if let Some(intent) =
        transitive_object_question_intent(body, text.trim_end().ends_with(['?', '？']))?
    {
        return Ok(intent);
    }
    if question && let Some(intent) = conjunctive_relation_question_intent(query_body)? {
        return Ok(intent);
    }
    if let Some(proposition) = parse_proposition(query_body, false)? {
        return Ok(if singular_previous_subject(&proposition.subject) {
            if question {
                Intent::AskPreviousRelation {
                    proposition,
                    explain: false,
                    explicit_scope: explicit_scope_prefix(query_body)?,
                }
            } else {
                Intent::AssertPreviousRelation {
                    proposition,
                    explicit_scope: explicit_scope_prefix(query_body)?,
                }
            }
        } else if question {
            Intent::Ask { proposition }
        } else {
            Intent::Assert { proposition }
        });
    }
    Ok(Intent::Unknown { reason: "no unambiguous interpretation in the supported arithmetic, copular relation, context, source or JSON grammar".into() })
}

fn constrained_previous_source_summary_intent(text: &str) -> Result<Option<Intent>> {
    let japanese = regex(
        r"^(?:(?:(?:前回|前)(?:の)?(?:取得した)?|取得した)(?:資料|本文|出典)|その(?:資料|本文|出典))を(?:([1-8])点以内(?:[、,]([1-9][0-9]{0,5})(?:字|文字)以内)?|([1-9][0-9]{0,5})(?:字|文字)以内(?:[、,]([1-8])点以内)?)(?:で)?要約(?:してください|して|せよ)$",
    )?;
    let english = regex(
        r"(?i)^(?:summarize|summarise) (?:the )?(?:previous|retrieved) (?:source|body|document) in (?:up to ([1-8]) bullets?(?:,? and up to ([1-9][0-9]{0,5}) characters?)?|up to ([1-9][0-9]{0,5}) characters?(?:,? and up to ([1-8]) bullets?)?)$",
    )?;
    let (max_sentences, max_chars) = if let Some(captures) = japanese.captures(text) {
        let parse = |index: usize| {
            captures
                .get(index)
                .and_then(|value| value.as_str().parse().ok())
        };
        (parse(1).or_else(|| parse(4)), parse(2).or_else(|| parse(3)))
    } else if let Some(captures) = english.captures(text) {
        let parse = |index: usize| {
            captures
                .get(index)
                .and_then(|value| value.as_str().parse().ok())
        };
        (parse(1).or_else(|| parse(4)), parse(2).or_else(|| parse(3)))
    } else {
        return Ok(None);
    };
    if (max_sentences.is_none() && max_chars.is_none())
        || max_sentences.is_some_and(|count| !(1..=8).contains(&count))
        || max_chars.is_some_and(|count| !(1..=100_000).contains(&count))
    {
        return Ok(None);
    }
    Ok(Some(Intent::SummarizePreviousSourceConstrained {
        max_sentences,
        max_chars,
    }))
}

fn previous_result_summary_intent(text: &str) -> Result<Option<Intent>> {
    let japanese = regex(
        r"^(?:(?:前回|前|直前)(?:の)?(?:結果|回答|比較|説明)|その(?:結果|回答|比較|説明))(?:を)?(?:(?:([1-8])(?:文|点)以内)(?:[、,]([1-9][0-9]{0,5})(?:字|文字)以内)?|([1-9][0-9]{0,5})(?:字|文字)以内(?:[、,]([1-8])(?:文|点)以内)?)?(?:で)?要約(?:してください|して|せよ)$",
    )?;
    let english = regex(
        r"(?i)^summari[sz]e (?:the )?(?:previous|prior|last) (?:result|answer|comparison)(?: in up to ([1-8]) (?:sentences?|bullets?)(?:,? and up to ([1-9][0-9]{0,5}) characters?)?| up to ([1-9][0-9]{0,5}) characters?(?:,? and up to ([1-8]) (?:sentences?|bullets?))?)?$",
    )?;
    let (max_sentences, max_chars) = if let Some(captures) = japanese.captures(text) {
        let parse = |index: usize| {
            captures
                .get(index)
                .and_then(|value| value.as_str().parse().ok())
        };
        (parse(1).or_else(|| parse(4)), parse(2).or_else(|| parse(3)))
    } else if let Some(captures) = english.captures(text) {
        let parse = |index: usize| {
            captures
                .get(index)
                .and_then(|value| value.as_str().parse().ok())
        };
        (parse(1).or_else(|| parse(4)), parse(2).or_else(|| parse(3)))
    } else {
        return Ok(None);
    };
    if max_sentences.is_some_and(|count| !(1..=8).contains(&count))
        || max_chars.is_some_and(|count| !(1..=100_000).contains(&count))
    {
        return Ok(None);
    }
    Ok(Some(Intent::SummarizePreviousResult {
        max_sentences,
        max_chars,
    }))
}

fn meaning_comparison_intent(text: &str) -> Result<Option<Intent>> {
    let Some((instruction, labelled)) =
        text.split_once("\nA:").or_else(|| text.split_once("\nA："))
    else {
        return Ok(None);
    };
    if !meaning_comparison_instruction(instruction.trim())? {
        return Ok(None);
    }
    if comparison_has_unrepresented_effect(instruction.trim())? {
        return Ok(Some(Intent::Unknown {
            reason: "comparison request includes an additional effect not represented by the bounded comparison route".into(),
        }));
    }
    let Some((a, b)) = labelled
        .split_once("\nB:")
        .or_else(|| labelled.split_once("\nB："))
    else {
        return Ok(Some(Intent::Unknown {
            reason: "meaning comparison requires exactly labelled A and B statements".into(),
        }));
    };
    let a = a.trim();
    let b = b.trim();
    if a.is_empty() || b.is_empty() || b.contains("\nA:") || b.contains("\nB:") {
        return Ok(Some(Intent::Unknown {
            reason: "meaning comparison has empty or repeated labelled statements".into(),
        }));
    }
    Ok(Some(Intent::MeaningCompare {
        a: a.into(),
        b: b.into(),
    }))
}

fn meaning_comparison_instruction(instruction: &str) -> Result<bool> {
    [
        r"(?s)^(?:以下の)?(?:文)?Aと(?:文)?B.*(?:違い|比較|差)",
        r"(?s)^(?:(?:次の|以下の)?)(?:二|2)(?:つの)?文(?:を|の).*(?:違い|比較|差)",
        r"(?is)^compare\b.*",
        r"(?is)^(?:explain|describe)\s+(?:the\s+)?(?:difference|contrast)\s+between\s+A\s+and\s+B\b.*",
    ]
    .into_iter()
    .map(regex)
    .collect::<Result<Vec<_>>>()
    .map(|patterns| patterns.iter().any(|pattern| pattern.is_match(instruction)))
}

fn comparison_has_unrepresented_effect(instruction: &str) -> Result<bool> {
    Ok(regex(r"(?:送信|保存|削除|実行|計算|翻訳|公開|投稿)(?:してください|して|しろ|せよ)|(?i:\b(?:and|then)\s+(?:send|write|save|delete|calculate|execute|translate|publish)\b)")?
        .is_match(instruction))
}

#[derive(Debug)]
enum MathArray {
    Scalar(String),
    Array(Vec<MathArray>),
}
fn math_space(text: &str, at: &mut usize) {
    while text
        .as_bytes()
        .get(*at)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *at += 1;
    }
}
/// A bounded JSON numeric-array reader retains each raw numeric lexeme. It does
/// not round a decimal through serde_json's default floating-point Number view.
fn math_array(text: &str, at: &mut usize, depth: usize) -> Result<MathArray> {
    if depth > 3 {
        return Err(failure(
            "MATH_INPUT_SHAPE",
            "numeric arrays are nested too deeply",
        ));
    }
    math_space(text, at);
    if text.as_bytes().get(*at) == Some(&b'[') {
        *at += 1;
        let mut items = Vec::new();
        math_space(text, at);
        if text.as_bytes().get(*at) == Some(&b']') {
            *at += 1;
            return Ok(MathArray::Array(items));
        }
        loop {
            items.push(math_array(text, at, depth + 1)?);
            if items.len() > 4096 {
                return Err(failure(
                    "MATH_INPUT_LIMIT",
                    "too many numeric array elements",
                ));
            }
            math_space(text, at);
            match text.as_bytes().get(*at) {
                Some(b',') => {
                    *at += 1;
                }
                Some(b']') => {
                    *at += 1;
                    break;
                }
                _ => {
                    return Err(failure(
                        "MATH_INPUT_SHAPE",
                        "numeric array needs a comma or closing bracket",
                    ));
                }
            }
        }
        return Ok(MathArray::Array(items));
    }
    let start = *at;
    if text.as_bytes().get(*at) == Some(&b'"') {
        *at += 1;
        let mut escaped = false;
        while let Some(byte) = text.as_bytes().get(*at) {
            *at += 1;
            if !escaped && *byte == b'"' {
                let scalar: String = serde_json::from_str(&text[start..*at])?;
                return Ok(MathArray::Scalar(scalar));
            }
            escaped = !escaped && *byte == b'\\';
        }
        return Err(failure("MATH_INPUT_SHAPE", "unterminated numeric string"));
    }
    while text
        .as_bytes()
        .get(*at)
        .is_some_and(|byte| !byte.is_ascii_whitespace() && !matches!(byte, b',' | b']'))
    {
        *at += 1;
    }
    let scalar = &text[start..*at];
    if !regex(r"^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][-+]?[0-9]+)?$")?.is_match(scalar) {
        return Err(failure(
            "MATH_INPUT_SHAPE",
            "expected a numeric JSON lexeme or a quoted exact expression",
        ));
    }
    Ok(MathArray::Scalar(scalar.into()))
}
fn scalar_array(array: MathArray) -> Result<Vec<String>> {
    match array {
        MathArray::Array(values) => values
            .into_iter()
            .map(|value| match value {
                MathArray::Scalar(value) => Ok(value),
                MathArray::Array(_) => Err(failure(
                    "MATH_INPUT_SHAPE",
                    "expected a one-dimensional numeric array",
                )),
            })
            .collect(),
        MathArray::Scalar(_) => Err(failure("MATH_INPUT_SHAPE", "expected a numeric array")),
    }
}

fn labeled_math_array_payload<'a>(
    tail: &'a str,
    japanese_label: &str,
    english_label: &str,
    japanese: bool,
) -> Option<&'a str> {
    let rest = if japanese {
        tail.strip_prefix(japanese_label)?
    } else {
        let label = tail.get(..english_label.len())?;
        if !label.eq_ignore_ascii_case(english_label) {
            return None;
        }
        let rest = tail.get(english_label.len()..)?;
        let separator = rest.chars().next()?;
        if !separator.is_whitespace() && separator != ':' {
            return None;
        }
        rest
    };
    let rest = rest.trim_start();
    Some(rest.strip_prefix(':').unwrap_or(rest).trim_start())
}

fn quantity_statistics_intent(text: &str) -> Result<Option<Intent>> {
    let Some(payload) = regex(r"(?is)^(?:統計|statistics)\s*[:：]?\s*\[(.*)\]$")?
        .captures(text)
        .map(|capture| capture[1].to_string())
    else {
        return Ok(None);
    };
    let mut values = Vec::new();
    let mut units = Vec::new();
    for item in payload.split(',') {
        let Some((number, unit)) = crate::language::decimal_quantity_parts(item) else {
            return Ok(None);
        };
        values.push(number.to_string());
        units.push(unit.to_string());
        if values.len() > 4096 {
            return Ok(Some(Intent::Unknown {
                reason: "statistical input exceeds the 4096-observation limit".into(),
            }));
        }
    }
    if values.is_empty() {
        return Ok(None);
    }
    Ok(Some(Intent::Statistics {
        values,
        units: Some(units),
    }))
}

fn mathematics_intent(text: &str) -> Result<Option<Intent>> {
    if let Some(intent) = quantity_statistics_intent(text)? {
        return Ok(Some(intent));
    }
    if let Some(capture) =
        regex(r"(?is)^(評価|evaluate|evaluate-polynomial)\s+(.+?)\s+(?:代入|at)\s+(.+)$")?
            .captures(text)
    {
        let japanese = &capture[1] == "評価";
        let expression = capture[2].trim().to_string();
        let mut assignment_text = capture[3].trim().to_string();
        if japanese {
            for suffix in ["を計算してください", "を計算して"] {
                if let Some(stripped) = assignment_text.strip_suffix(suffix) {
                    assignment_text = stripped.trim().to_string();
                    break;
                }
            }
        }
        let assignment_pattern = regex(r"^([\p{L}_][\p{L}\p{N}_]*)\s*=\s*([0-9eE+\-*/^().\s]+)$")?;
        let mut bindings = BTreeMap::new();
        for item in assignment_text.split(',') {
            let Some(binding) = assignment_pattern.captures(item.trim()) else {
                return Ok(Some(Intent::Unknown {
                    reason: "polynomial evaluation requires comma-separated variable=exact-scalar bindings".into(),
                }));
            };
            let variable = binding[1].to_string();
            if bindings
                .insert(variable, binding[2].trim().to_string())
                .is_some()
            {
                return Ok(Some(Intent::Unknown {
                    reason: "polynomial evaluation does not allow duplicate variable bindings"
                        .into(),
                }));
            }
            if bindings.len() > 8 {
                return Ok(Some(Intent::Unknown {
                    reason: "polynomial evaluation accepts at most eight variable bindings".into(),
                }));
            }
        }
        if bindings.is_empty() {
            return Ok(Some(Intent::Unknown {
                reason: "polynomial evaluation requires at least one variable binding".into(),
            }));
        }
        return Ok(Some(Intent::EvaluatePolynomial {
            expression,
            bindings,
        }));
    }
    if let Some(capture) =
        regex(r"(?is)^(二次方程式|solve-quadratic)\s*[:：]?\s*(.+)$")?.captures(text)
    {
        let japanese = &capture[1] == "二次方程式";
        let mut payload = capture[2].trim();
        if japanese {
            for suffix in [
                "を解いてください",
                "を求めてください",
                "を解いて",
                "を求めて",
                "を解け",
            ] {
                if let Some(stripped) = payload.strip_suffix(suffix) {
                    payload = stripped.trim_end();
                    break;
                }
            }
        }
        let Some((left, right)) = payload.split_once('=') else {
            return Ok(Some(Intent::Unknown {
                reason: "quadratic equation syntax requires exactly one left=right equality".into(),
            }));
        };
        if right.contains('=') || left.trim().is_empty() || right.trim().is_empty() {
            return Ok(Some(Intent::Unknown {
                reason: "quadratic equation syntax requires exactly one nonempty equality".into(),
            }));
        }
        return Ok(Some(Intent::QuadraticEquation {
            left: left.trim().into(),
            right: right.trim().into(),
        }));
    }
    if let Some(capture) =
        regex(r"(?is)^(一次方程式|solve-linear)\s*[:：]?\s*(.+)$")?.captures(text)
    {
        let japanese = &capture[1] == "一次方程式";
        let mut payload = capture[2].trim();
        if japanese {
            for suffix in [
                "を解いてください",
                "を求めてください",
                "を解いて",
                "を求めて",
                "を解け",
            ] {
                if let Some(stripped) = payload.strip_suffix(suffix) {
                    payload = stripped.trim_end();
                    break;
                }
            }
        }
        let Some((left, right)) = payload.split_once('=') else {
            return Ok(Some(Intent::Unknown {
                reason: "linear equation syntax requires exactly one left=right equality".into(),
            }));
        };
        if right.contains('=') || left.trim().is_empty() || right.trim().is_empty() {
            return Ok(Some(Intent::Unknown {
                reason: "linear equation syntax requires exactly one nonempty equality".into(),
            }));
        }
        return Ok(Some(Intent::LinearEquation {
            left: left.trim().into(),
            right: right.trim().into(),
        }));
    }
    if let Some(capture) =
        regex(r"(?is)^(期待値|expected-value)\s*[:：]?\s*(\[.+)$")?.captures(text)
    {
        let payload = capture[2].trim();
        let mut outcomes_at = 0;
        let outcomes = scalar_array(math_array(payload, &mut outcomes_at, 0)?)?;
        let tail = payload[outcomes_at..].trim_start();
        let japanese = &capture[1] == "期待値";
        let Some(weight_payload) = labeled_math_array_payload(tail, "重み", "weights", japanese)
        else {
            return Ok(Some(Intent::Unknown {
                reason: "expected-value input requires one explicit weight array".into(),
            }));
        };
        let mut weights_at = 0;
        let weights = scalar_array(math_array(weight_payload, &mut weights_at, 0)?)?;
        let suffix = weight_payload[weights_at..].trim();
        let suffix_valid = if japanese {
            matches!(suffix, "を計算して" | "を計算してください")
        } else {
            suffix.is_empty()
        };
        if !suffix_valid {
            return Ok(Some(Intent::Unknown {
                reason: "expected-value input accepts only outcome and weight arrays with no extra constraints".into(),
            }));
        }
        return Ok(Some(Intent::FiniteProbabilityStatistics {
            outcomes,
            weights,
        }));
    }
    if let Some(capture) =
        regex(r"(?is)^(事象確率|event-probability)\s*[:：]?\s*(\[.+)$")?.captures(text)
    {
        let payload = capture[2].trim();
        let mut outcomes_at = 0;
        let outcomes = scalar_array(math_array(payload, &mut outcomes_at, 0)?)?;
        let japanese = &capture[1] == "事象確率";
        let tail = payload[outcomes_at..].trim_start();
        let Some(weight_payload) = labeled_math_array_payload(tail, "重み", "weights", japanese)
        else {
            return Ok(Some(Intent::Unknown {
                reason: "event probability requires one explicit weight array".into(),
            }));
        };
        let mut weights_at = 0;
        let weights = scalar_array(math_array(weight_payload, &mut weights_at, 0)?)?;
        let tail = weight_payload[weights_at..].trim_start();
        let Some(event_payload) = labeled_math_array_payload(tail, "事象値", "values", japanese)
        else {
            return Ok(Some(Intent::Unknown {
                reason: "event probability requires one explicit event-value array".into(),
            }));
        };
        let mut event_values_at = 0;
        let event_values = scalar_array(math_array(event_payload, &mut event_values_at, 0)?)?;
        let suffix = event_payload[event_values_at..].trim();
        let suffix_valid = if japanese {
            matches!(suffix, "を計算して" | "を計算してください")
        } else {
            suffix.is_empty()
        };
        if !suffix_valid {
            return Ok(Some(Intent::Unknown {
                reason: "event probability accepts only outcome, weight and event-value arrays with no extra constraints".into(),
            }));
        }
        return Ok(Some(Intent::FiniteEventProbability {
            outcomes,
            weights,
            event_values,
        }));
    }
    if let Some(capture) =
        regex(r"(?is)^(確率分布|normalize-probabilities)\s*[:：]?\s*(\[.+)$")?.captures(text)
    {
        let payload = capture[2].trim();
        let mut at = 0;
        let weights = scalar_array(math_array(payload, &mut at, 0)?)?;
        let suffix = payload[at..].trim();
        let suffix_valid = match &capture[1] {
            "確率分布" => matches!(suffix, "を正規化して" | "を正規化してください"),
            "normalize-probabilities" => suffix.is_empty(),
            _ => false,
        };
        if !suffix_valid {
            return Ok(Some(Intent::Unknown {
                reason: "probability normalization requires one explicit weight array and no extra constraints".into(),
            }));
        }
        return Ok(Some(Intent::NormalizeProbabilityWeights { weights }));
    }
    if let Some(capture) = regex(r"(?is)^(微分|積分|簡約|整理|derivative|differentiate|integral|integrate|simplify)\s+(.+?)(?:\s+(?:変数|variable)\s+([^\s]+))?$")?.captures(text) {
        let operation = match capture[1].to_lowercase().as_str() {
            "微分" | "derivative" | "differentiate" => "derive",
            "積分" | "integral" | "integrate" => "integrate",
            _ => "simplify",
        }.to_string();
        let variable = capture.get(3).map_or("", |value| value.as_str()).to_string();
        if operation != "simplify" && variable.is_empty() {
            return Ok(Some(Intent::Unknown { reason: "symbolic differentiation/integration requires an explicit variable".into() }));
        }
        if !variable.is_empty() && !regex(r"^[\p{L}_][\p{L}\p{N}_]*$")?.is_match(&variable) {
            return Ok(Some(Intent::Unknown { reason: "the symbolic target must be one identifier; trailing constraints cannot be discarded".into() }));
        }
        return Ok(Some(Intent::Symbolic { expression: capture[2].trim().to_string(), operation, variable }));
    }
    if let Some(capture) = regex(
        r"^(.+?)\s*を\s*([\p{L}_][\p{L}\p{N}_]*?)\s*で(微分|積分)(?:してください|して|せよ)?$",
    )?
    .captures(text)
    {
        return Ok(Some(Intent::Symbolic {
            expression: capture[1].trim().to_string(),
            operation: if &capture[3] == "微分" {
                "derive".into()
            } else {
                "integrate".into()
            },
            variable: capture[2].trim().to_string(),
        }));
    }
    if let Some(capture) =
        regex(r"(?is)^(?:連立方程式|linear system)\s*[:：]?\s*(.+)$")?.captures(text)
    {
        let payload = capture[1].trim();
        if !payload.starts_with('[') {
            return Ok(Some(Intent::Unknown { reason: "linear equations currently require coefficient matrix = right-hand-side notation; symbolic equations are not silently approximated".into() }));
        }
        let mut at = 0;
        let matrix = math_array(payload, &mut at, 0)?;
        let coefficients = match matrix {
            MathArray::Array(rows) => rows
                .into_iter()
                .map(scalar_array)
                .collect::<Result<Vec<_>>>()?,
            MathArray::Scalar(_) => {
                return Err(failure(
                    "MATH_INPUT_SHAPE",
                    "coefficient matrix is required",
                ));
            }
        };
        math_space(payload, &mut at);
        if payload.as_bytes().get(at) != Some(&b'=') {
            return Err(failure(
                "MATH_INPUT_SHAPE",
                "expected matrix = right-hand-side",
            ));
        }
        at += 1;
        let rhs = scalar_array(math_array(payload, &mut at, 0)?)?;
        let suffix = payload[at..].trim();
        if !matches!(
            suffix,
            "" | "を解いて" | "を解いてください" | "を求めて" | "を解け"
        ) {
            return Ok(Some(Intent::Unknown {
                reason: "unparsed constraints follow the linear system".into(),
            }));
        }
        return Ok(Some(Intent::LinearSystem { coefficients, rhs }));
    }
    if let Some(capture) = regex(r"(?is)^(?:統計|statistics)\s*[:：]?\s*(\[.+)$")?.captures(text)
    {
        let payload = capture[1].trim();
        let mut at = 0;
        let values = scalar_array(math_array(payload, &mut at, 0)?)?;
        let suffix = payload[at..].trim();
        let units = if suffix.is_empty() {
            None
        } else if let Some(unit) = suffix
            .strip_prefix("単位 ")
            .or_else(|| suffix.strip_prefix("unit "))
        {
            if !regex(r"^[A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63}$")?.is_match(unit.trim())
            {
                return Ok(Some(Intent::Unknown { reason: "unit must be one explicit supported unit token; remaining instructions cannot be treated as a unit".into() }));
            }
            Some(vec![unit.trim().to_string(); values.len()])
        } else {
            return Ok(Some(Intent::Unknown {
                reason: "unparsed statistical constraints or units".into(),
            }));
        };
        return Ok(Some(Intent::Statistics { values, units }));
    }
    if let Some(capture) = regex(r"^(.+?)\s*の(?:平均と分散|平均|分散|統計量)(?:を)?(?:計算して|計算してください|求めて|教えて)?$")?.captures(text) {
        let raw = capture[1].trim();
        let values = if raw.starts_with('[') {
            let mut at = 0;
            let values = scalar_array(math_array(raw, &mut at, 0)?)?;
            if !raw[at..].trim().is_empty() { return Ok(Some(Intent::Unknown { reason: "unparsed statistical input suffix".into() })); }
            values
        } else {
            let mut values = Vec::new();
            for value in raw.split([',', '、']) {
                let value = value.trim();
                if !regex(r"^[-+0-9eE.*/()\s]+$")?.is_match(value) || value.is_empty() {
                    return Ok(Some(Intent::Unknown { reason: "statistical observations must be explicit complete exact scalar expressions".into() }));
                }
                values.push(value.to_string());
            }
            values
        };
        return Ok(Some(Intent::Statistics { values, units: None }));
    }
    Ok(None)
}

fn arithmetic_expression(text: &str) -> Result<Option<String>> {
    // NFKC is confined to the arithmetic view: quoted JSON values, relation
    // constants and URI bytes must not be rewritten by arithmetic normalization.
    let normalized: String = text.nfkc().collect();
    let text = normalized.as_str();
    let mut candidate = text.trim().to_string();
    if let Some(capture) = regex(r"(?i)^(?:please\s+)?(?:calculate|compute|evaluate)\s+(.+)$")?.captures(text) {
        candidate = capture[1].trim().to_string();
    } else if let Some(capture) = regex(r"^(.+?)(?:を)?(?:計算|計算して|計算してください|計算しろ|計算せよ|はいくつ|はいくつですか|は何|は何ですか)$")?.captures(text) {
        candidate = capture[1].trim().trim_end_matches('を').trim().to_string();
    } else if let Some(capture) = regex(r"^(?:計算して|計算してください|計算)[:：\s]+(.+)$")?.captures(text) {
        candidate = capture[1].trim().to_string();
    }
    // A complete arithmetic surface is required; never accept a numeric prefix
    // while dropping a second request or an unparsed condition.
    if regex(r"^[-+]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?\s*[A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63}\s*[+-]\s*[-+]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][-+]?[0-9]+)?\s*[A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63}$")?
        .is_match(&candidate)
    {
        return Ok(Some(candidate));
    }
    if regex(r"^[0-9eE+*/%^().\s×÷−-]+$")?.is_match(&candidate)
        && candidate.chars().any(|ch| ch.is_ascii_digit())
    {
        return Ok(Some(candidate));
    }
    // Japanese binary arithmetic is a grammatical lowering to the same parser.
    for (pattern, op) in [
        (
            r"^([-+]?\d+(?:\.\d+)?)\s*(?:と|に)\s*([-+]?\d+(?:\.\d+)?)\s*を?\s*(?:足して|足す|加えて)(?:ください)?$",
            "+",
        ),
        (
            r"^([-+]?\d+(?:\.\d+)?)\s*から\s*([-+]?\d+(?:\.\d+)?)\s*を?\s*(?:引いて|引く)(?:ください)?$",
            "-",
        ),
        (
            r"^([-+]?\d+(?:\.\d+)?)\s*(?:に|を)\s*([-+]?\d+(?:\.\d+)?)\s*を?\s*(?:掛けて|かけて|掛ける|かける)(?:ください)?$",
            "*",
        ),
        (
            r"^([-+]?\d+(?:\.\d+)?)\s*を\s*([-+]?\d+(?:\.\d+)?)\s*で\s*(?:割って|割る)(?:ください)?$",
            "/",
        ),
    ] {
        if let Some(capture) = regex(pattern)?.captures(text) {
            return Ok(Some(format!("{}{}{}", &capture[1], op, &capture[2])));
        }
    }
    Ok(None)
}

/// Accept the concise Japanese topic-question form only when the preceding
/// text is a complete expression in the existing arithmetic grammar. An
/// actual question mark is required at the call site, so an incomplete
/// assertion such as `6+7は` remains unresolved.
fn trailing_topic_arithmetic_question(text: &str) -> Result<Option<String>> {
    let Some(expression) = text.trim().strip_suffix('は') else {
        return Ok(None);
    };
    arithmetic_expression(expression.trim())
}

fn japanese_transitive_verb(surface: &str) -> Option<(String, bool)> {
    let legacy = match surface {
        "食べる" | "食べます" => Some(("eat", false)),
        "食べない" | "食べません" => Some(("eat", true)),
        "使う" | "使います" => Some(("use", false)),
        "使わない" | "使いません" => Some(("use", true)),
        _ => None,
    };
    if let Some((predicate, negative)) = legacy {
        return Some((predicate.into(), negative));
    }

    // Do not mistake a polite conjugation (飲みます) for a dictionary-form
    // verb ending in す. Only normalize the plain forms whose conjugation
    // class is encoded unambiguously by the suffix.
    if surface.ends_with("ます") || surface.ends_with("ません") {
        return None;
    }
    let negative_endings = [
        ("わない", "う"),
        ("かない", "く"),
        ("がない", "ぐ"),
        ("さない", "す"),
        ("たない", "つ"),
        ("ばない", "ぶ"),
        ("まない", "む"),
        ("らない", "る"),
    ];
    for (negative_ending, dictionary_ending) in negative_endings {
        if let Some(stem) = surface.strip_suffix(negative_ending)
            && valid_japanese_verb_stem(stem)
        {
            return Some((format!("{stem}{dictionary_ending}"), true));
        }
    }
    for ending in ["う", "く", "ぐ", "す", "つ", "ぶ", "む"] {
        if surface.ends_with(ending)
            && surface.chars().count() >= 2
            && valid_japanese_verb_stem(surface)
        {
            return Some((surface.into(), false));
        }
    }
    None
}

fn valid_japanese_verb_stem(surface: &str) -> bool {
    !surface.is_empty()
        && surface.len() <= 512
        && regex(r"^[\p{Han}\p{Hiragana}\p{Katakana}ー]+$")
            .is_ok_and(|pattern| pattern.is_match(surface))
}

fn transitive_object_question_intent(text: &str, question_mark: bool) -> Result<Option<Intent>> {
    let scope_match = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(text);
    let scope = scope_match
        .as_ref()
        .map(|capture| capture[1].trim().to_string())
        .unwrap_or_else(|| "default".into());
    let body = scope_match.as_ref().map_or(text, |capture| {
        capture.get(2).expect("matched body").as_str()
    });
    let Some(captures) = regex(r"^([^はが]+?)(?:は|が)何を(.+)$")?.captures(body) else {
        return Ok(None);
    };
    let mut verb = captures[2].trim();
    let explicit_question = if let Some(without_question_particle) = verb.strip_suffix('か') {
        verb = without_question_particle;
        true
    } else if question_mark {
        if let Some(without_nominalizer) = verb.strip_suffix('の') {
            verb = without_nominalizer;
        }
        true
    } else {
        false
    };
    if !explicit_question || scope.is_empty() {
        return Ok(None);
    }
    let Some((predicate, negative)) = japanese_transitive_verb(verb) else {
        return Ok(None);
    };
    let subject = captures[1].trim();
    let lower = subject.to_ascii_lowercase();
    if subject.is_empty()
        || subject.len() > 512
        || subject.contains(['\n', '\r', '、', '，', ',', ';', '；', 'と'])
        || matches!(
            lower.as_str(),
            "it" | "this" | "that" | "these" | "those" | "they" | "he" | "she"
        )
        || matches!(subject, "これ" | "それ" | "あれ" | "これら" | "それら")
    {
        return Ok(None);
    }
    Ok(Some(Intent::AskRelationObjects {
        subject: subject.into(),
        predicate,
        negative,
        scope,
    }))
}

fn japanese_transitive_proposition(text: &str) -> Result<Option<(String, String, String, bool)>> {
    let Some(captures) = regex(r"^([^はが]+?)(?:は|が)(.+)$")?.captures(text) else {
        return Ok(None);
    };
    let subject = captures[1].trim();
    let rest = captures[2].trim();
    let Some((object, verb)) = rest.rsplit_once('を') else {
        return Ok(None);
    };
    let object = object.trim();
    let Some((predicate, negative)) = japanese_transitive_verb(verb.trim()) else {
        return Ok(None);
    };
    if subject.is_empty()
        || object.is_empty()
        || matches!(object, "何" | "なに" | "どれ" | "誰" | "だれ")
        || subject.len() > 512
        || object.len() > 512
        || subject.contains(['と', '、', ',', ';', '；', '\n', '\r'])
        || object.contains(['を', 'と', 'で', ';', '；', '\n', '\r'])
    {
        return Ok(None);
    }
    Ok(Some((subject.into(), predicate, object.into(), negative)))
}

/// Bounded copular and Japanese transitive relations, not a claim of general
/// Japanese parsing. Constants are preserved; only declared rule variables
/// gain variable syntax.
pub fn parse_proposition(text: &str, variables: bool) -> Result<Option<Proposition>> {
    let mut body = terminal_trim(text).trim().to_string();
    let mut scope = "default".to_string();
    if let Some(capture) = regex(r"^\[scope=([^\]]+)\]\s*(.+)$")?.captures(&body) {
        scope = capture[1].trim().to_string();
        body = capture[2].trim().to_string();
    }
    // These qualifiers cannot be represented by the single-premise copular
    // contract. Preserve them as unresolved input instead of opaque atoms.
    if regex(r"(?:すべて|全て|任意|一部|少なくとも|たいてい|通常|常に|必ず|かもしれ|とは限ら|じゃなく|ではなく|でなく|だけ|のみ|以外|かつ|且つ|および|及び|または|あるいは|もしくは|ただし|しかし|もし|なら|場合|時点|以前|以後|以降|現在|過去|将来|昨日|今日|明日|まで|から|[0-9]+[年月日])|(?i:\b(?:all|every|any|some|might|may|and|or|but|unless|if|then|when|before|after|currently|yesterday|today|tomorrow|never|always|only|except|please|also)\b)")?.is_match(&body) {
        return Ok(None);
    }
    let parsed = if let Some(parsed) = japanese_transitive_proposition(&body)? {
        Some(parsed)
    } else if let Some(capture) =
        regex(r"(?i)^(?:is\s+)?(.+?)\s+(?:is|are)\s+(not\s+)?(.+)$")?.captures(&body)
    {
        Some((
            capture[1].trim().to_string(),
            "is".into(),
            capture[3].trim().to_string(),
            capture.get(2).is_some(),
        ))
    } else if let Some(capture) = regex(r"(?i)^is\s+(.+?)\s+(not\s+)?(.+)$")?.captures(&body) {
        Some((
            capture[1].trim().to_string(),
            "is".into(),
            capture[3].trim().to_string(),
            capture.get(2).is_some(),
        ))
    } else if let Some(capture) = regex(r"^([^はが]+?)(?:は|が)(.+)$")?.captures(&body) {
        let subject = capture[1].trim().to_string();
        let mut object = capture[2].trim().to_string();
        let mut negative = false;
        for suffix in [
            "ではありません",
            "ではない",
            "じゃない",
            "でありません",
            "でない",
        ] {
            if let Some(value) = object.strip_suffix(suffix) {
                object = value.trim().to_string();
                negative = true;
                break;
            }
        }
        if !negative {
            for suffix in ["である", "です", "だ"] {
                if let Some(value) = object.strip_suffix(suffix) {
                    object = value.trim().to_string();
                    break;
                }
            }
        }
        if object.contains(['を', 'は', 'が', 'に', '。', '、', '!', '！', '?', '？']) {
            return Ok(None);
        }
        Some((subject, "is".into(), object, negative))
    } else {
        None
    };
    let Some((mut subject, predicate, mut object, negative)) = parsed else {
        return Ok(None);
    };
    if let Some(value) = object
        .strip_prefix("a ")
        .or_else(|| object.strip_prefix("an "))
    {
        object = value.into();
    }
    if subject.contains(['と', '、', ',', ';', '；', '\n', '\r'])
        || object.contains(['と', 'で', ';', '；', '\n', '\r'])
    {
        return Ok(None);
    }
    if subject.is_empty()
        || object.is_empty()
        || subject.len() > 512
        || object.len() > 512
        || scope.is_empty()
    {
        return Ok(None);
    }
    if variables {
        for term in [&mut subject, &mut object] {
            if matches!(term.as_str(), "X" | "Y" | "Z") {
                *term = format!("${}", term.to_lowercase());
            }
        }
    }
    Ok(Some(Proposition {
        subject,
        predicate,
        object,
        negative,
        scope,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn why_questions_parse_only_one_supported_relation() {
        for text in [
            "どうしてココは動物なのですか",
            "なぜココは動物ですか",
            "Why is Coco an animal?",
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::AskWhy { proposition }
                        if proposition.subject == "Coco" || proposition.subject == "ココ"
                ),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("Why is Coco an animal?").unwrap(),
            Intent::AskWhy { proposition }
                if proposition.subject == "Coco"
                    && proposition.object == "animal"
                    && proposition.predicate == "is"
                    && !proposition.negative
                    && proposition.scope == "default"
        ));
        assert!(matches!(
            interpret("If X is a bird then X is an animal").unwrap(),
            Intent::AddRule { premise, conclusion }
                if premise.subject == "$x"
                    && premise.object == "bird"
                    && conclusion.subject == "$x"
                    && conclusion.object == "animal"
        ));
        assert!(matches!(
            interpret("Coco is a bird").unwrap(),
            Intent::Assert { proposition }
                if proposition.subject == "Coco" && proposition.object == "bird"
        ));
        assert!(matches!(
            interpret("どうしてココは動物なのですか、そして送信して").unwrap(),
            Intent::Unknown { .. }
        ));
    }

    #[test]
    fn elliptical_why_questions_consume_the_actual_previous_result() {
        for text in ["なぜですか", "どうして？", "Why?"] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::ExplainPrevious),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("なぜそれはノートを使いますか").unwrap(),
            Intent::AskPreviousRelation {
                proposition,
                explain: true,
                explicit_scope: None,
            } if proposition.subject == "それ"
                && proposition.object == "ノート"
                && proposition.predicate == "use"
        ));
    }

    #[test]
    fn common_relation_questions_preserve_two_subjects_and_scope() {
        for (text, subject_a, subject_b, scope) in [
            ("ココとミケの共通点は何ですか", "ココ", "ミケ", "default"),
            (
                "[scope=biology]ココとミケの共通点は何?",
                "ココ",
                "ミケ",
                "biology",
            ),
            (
                "What do Coco and Mike have in common?",
                "Coco",
                "Mike",
                "default",
            ),
        ] {
            match interpret(text).unwrap() {
                Intent::AskCommonRelations {
                    subject_a: parsed_a,
                    subject_b: parsed_b,
                    scope: parsed_scope,
                } => {
                    assert_eq!(parsed_a, subject_a, "{text}");
                    assert_eq!(parsed_b, subject_b, "{text}");
                    assert_eq!(parsed_scope, scope, "{text}");
                }
                other => panic!("{text}: {other:?}"),
            }
        }
        for text in [
            "ココとココの共通点は何ですか",
            "ココとミケとサキの共通点は何ですか",
            "それとミケの共通点は何ですか",
        ] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::Unknown { .. }),
                "{text}"
            );
        }
    }

    #[test]
    fn saved_relation_comparisons_preserve_two_subjects_and_scope() {
        for (text, subject_a, subject_b, scope) in [
            (
                "ココとミケの保存済みの関係を比較して",
                "ココ",
                "ミケ",
                "default",
            ),
            (
                "[scope=biology]ココとミケの関係を比べて",
                "ココ",
                "ミケ",
                "biology",
            ),
            (
                "Compare saved relations for Coco and Mike",
                "Coco",
                "Mike",
                "default",
            ),
        ] {
            match interpret(text).unwrap() {
                Intent::CompareRelationInventories {
                    subject_a: parsed_a,
                    subject_b: parsed_b,
                    scope: parsed_scope,
                } => {
                    assert_eq!(parsed_a, subject_a, "{text}");
                    assert_eq!(parsed_b, subject_b, "{text}");
                    assert_eq!(parsed_scope, scope, "{text}");
                }
                other => panic!("{text}: {other:?}"),
            }
        }
        for text in [
            "ココとココの関係を比較して",
            "それとミケの関係を比較して",
            "ココとミケの関係を比較して送信して",
        ] {
            assert!(
                !matches!(
                    interpret(text).unwrap(),
                    Intent::CompareRelationInventories { .. }
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn conjunctive_relation_rules_keep_explicit_and_shorthand_premises() {
        for (text, first_object, second_object, conclusion_object) in [
            (
                "もし[scope=biology]Xは鳥かつXは羽毛持ちなら[scope=biology]Xは温血動物",
                "鳥",
                "羽毛持ち",
                "温血動物",
            ),
            (
                "もし[scope=biology]Xは鳥かつ[scope=biology]哺乳類なら[scope=biology]Xは温血動物",
                "鳥",
                "哺乳類",
                "温血動物",
            ),
            ("もしXは鳥かつ青ならXは動物", "鳥", "青", "動物"),
            (
                "If [scope=biology]X is a bird and X is feathered then [scope=biology]X is warm-blooded",
                "bird",
                "feathered",
                "warm-blooded",
            ),
            (
                "If X is a bird and blue then X is an animal",
                "bird",
                "blue",
                "animal",
            ),
            (
                "If X is a bird and a mammal then X is warm-blooded",
                "bird",
                "mammal",
                "warm-blooded",
            ),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::AddConjunctiveRule { premises, conclusion }
                        if premises.len() == 2
                            && premises[0].subject == "$x"
                            && premises[1].subject == "$x"
                            && premises[0].object == first_object
                            && premises[1].object == second_object
                            && premises[0].scope == premises[1].scope
                            && conclusion.subject == "$x"
                            && conclusion.object == conclusion_object
                ),
                "{text}"
            );
        }

        assert!(matches!(
            interpret("If X is a bird and X is Y then Y is an animal").unwrap(),
            Intent::AddConjunctiveRule { premises, conclusion }
                if premises.len() == 2
                    && premises[0].subject == "$x"
                    && premises[1].subject == "$x"
                    && premises[1].object == "$y"
                    && conclusion.subject == "$y"
                    && conclusion.object == "animal"
        ));

        for text in [
            "If X is a bird and Y is a mammal then X is warm-blooded",
            "If X is a bird and X is a mammal and X is a reptile then X is warm-blooded",
            "もしXは鳥または哺乳類ならXは温血動物",
            "もし[scope=biology]Xは鳥かつ[scope=ecology]Xは哺乳類なら[scope=biology]Xは温血動物",
        ] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::Unknown { .. }),
                "{text}"
            );
        }
    }

    #[test]
    fn singular_pronoun_relation_questions_retain_context_and_scope_requirements() {
        assert!(matches!(
            interpret("それは動物ですか").unwrap(),
            Intent::AskPreviousRelation {
                proposition,
                explain: false,
                explicit_scope: None,
            } if proposition.subject == "それ" && proposition.object == "動物"
        ));
        assert!(matches!(
            interpret("[scope=biology]it is an animal?").unwrap(),
            Intent::AskPreviousRelation {
                proposition,
                explain: false,
                explicit_scope: Some(scope),
            } if proposition.subject.eq_ignore_ascii_case("it")
                && proposition.object == "animal"
                && scope == "biology"
        ));
        assert!(matches!(
            interpret("なぜ[scope=biology]それは動物ですか").unwrap(),
            Intent::AskPreviousRelation {
                proposition,
                explain: true,
                explicit_scope: Some(scope),
            } if proposition.subject == "それ"
                && proposition.object == "動物"
                && scope == "biology"
        ));
        assert!(matches!(
            interpret("Why is it an animal?").unwrap(),
            Intent::AskPreviousRelation {
                proposition,
                explain: true,
                explicit_scope: None,
            } if proposition.subject.eq_ignore_ascii_case("it")
                && proposition.object == "animal"
        ));
        assert!(matches!(
            interpret("それらは動物ですか").unwrap(),
            Intent::Ask { proposition } if proposition.subject == "それら"
        ));
    }

    #[test]
    fn singular_pronoun_relation_assertions_keep_subject_unresolved_until_core() {
        assert!(matches!(
            interpret("それは温血動物です").unwrap(),
            Intent::AssertPreviousRelation {
                proposition,
                explicit_scope: None,
            } if proposition.subject == "それ"
                && proposition.object == "温血動物"
                && !proposition.negative
        ));
        assert!(matches!(
            interpret("[scope=biology]that is not a mammal").unwrap(),
            Intent::AssertPreviousRelation {
                proposition,
                explicit_scope: Some(scope),
            } if proposition.subject.eq_ignore_ascii_case("that")
                && proposition.object == "mammal"
                && proposition.negative
                && scope == "biology"
        ));
        assert!(matches!(
            interpret("それは哺乳類ではない").unwrap(),
            Intent::AssertPreviousRelation {
                proposition,
                explicit_scope: None,
            } if proposition.subject == "それ" && proposition.negative
        ));
        assert!(matches!(
            interpret("それらは温血動物です").unwrap(),
            Intent::Assert { proposition } if proposition.subject == "それら"
        ));
    }

    #[test]
    fn conjunctive_relation_questions_keep_both_same_subject_clauses() {
        assert!(matches!(
            interpret("[scope=biology]ココは鳥かつ動物ですか").unwrap(),
            Intent::AskConjunction { propositions }
                if propositions.len() == 2
                    && propositions[0].subject == "ココ"
                    && propositions[0].object == "鳥"
                    && propositions[0].scope == "biology"
                    && propositions[1].subject == "ココ"
                    && propositions[1].object == "動物"
                    && propositions[1].scope == "biology"
        ));
        assert!(matches!(
            interpret("ココは鳥且つ動物ですか").unwrap(),
            Intent::AskConjunction { propositions } if propositions.len() == 2
        ));
        assert!(matches!(
            interpret("Is Coco a bird and an animal?").unwrap(),
            Intent::AskConjunction { propositions }
                if propositions.len() == 2
                    && propositions[0].subject == "Coco"
                    && propositions[0].object == "bird"
                    && propositions[1].subject == "Coco"
                    && propositions[1].object == "animal"
        ));
        assert!(matches!(
            interpret("Coco is a bird and an animal?").unwrap(),
            Intent::AskConjunction { propositions } if propositions.len() == 2
        ));
        assert!(matches!(
            interpret("Is Coco not a bird and an animal?").unwrap(),
            Intent::AskConjunction { propositions }
                if propositions.len() == 2
                    && propositions[0].object == "bird"
                    && propositions[0].negative
                    && propositions[1].object == "animal"
                    && !propositions[1].negative
        ));
        assert!(matches!(
            interpret("[scope=biology]ココは鳥かつ動物かつ哺乳類ですか").unwrap(),
            Intent::AskConjunction { propositions }
                if propositions.len() == 3
                    && propositions[0].subject == "ココ"
                    && propositions[0].object == "鳥"
                    && propositions[1].subject == "ココ"
                    && propositions[1].object == "動物"
                    && propositions[2].subject == "ココ"
                    && propositions[2].object == "哺乳類"
                    && propositions.iter().all(|proposition| proposition.scope == "biology")
        ));
        assert!(matches!(
            interpret("Is Coco a bird and an animal and a mammal?").unwrap(),
            Intent::AskConjunction { propositions }
                if propositions.len() == 3
                    && propositions[0].subject == "Coco"
                    && propositions[0].object == "bird"
                    && propositions[1].subject == "Coco"
                    && propositions[1].object == "animal"
                    && propositions[2].subject == "Coco"
                    && propositions[2].object == "mammal"
        ));
        assert!(matches!(
            interpret("Is Coco not a bird and an animal and a mammal?").unwrap(),
            Intent::AskConjunction { propositions }
                if propositions.len() == 3
                    && propositions[0].negative
                    && !propositions[1].negative
                    && !propositions[2].negative
        ));
        for text in [
            "ココは鳥かつ動物かつ哺乳類かつ生物ですか",
            "ココは鳥かつ動物且つ哺乳類ですか",
            "ココは鳥かつ動物または哺乳類ですか",
            "ココは鳥かつミケは動物ですか",
            "それは鳥かつ動物ですか",
            "We are birds and animals?",
            "Is Coco a bird and an animal or a mammal?",
            "ココは鳥または動物ですか",
            "ココは鳥かつ動物です",
        ] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::Unknown { .. }),
                "{text}"
            );
        }
    }

    #[test]
    fn premise_inspection_questions_keep_the_named_subject() {
        for (text, expected) in [
            ("ココについて、何が分かっていますか", "ココ"),
            ("ココについてわかっていることを教えてください", "ココ"),
            ("ココに関して知っていることを説明して", "ココ"),
            ("ココとは何ですか", "ココ"),
            ("ココって何?", "ココ"),
            ("What facts have been recorded about Coco?", "Coco"),
            ("What is Coco?", "Coco"),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::InspectPremises { subject } if subject == expected
                ),
                "{text}"
            );
        }
        for text in ["What is 2+3?", "What is it?", "2+3とは何ですか"] {
            assert!(
                !matches!(interpret(text).unwrap(), Intent::InspectPremises { .. }),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("ココについて何が分かっているか、ミケについても").unwrap(),
            Intent::Unknown { .. }
        ));
    }

    #[test]
    fn natural_relation_profile_requests_keep_one_explicit_subject() {
        for (text, expected) in [
            (
                "ココについて保存されている情報をもとに短い紹介文を書いて",
                "ココ",
            ),
            ("Write a short profile of Coco from saved facts", "Coco"),
            (
                "Create an introduction of Mina using the recorded information",
                "Mina",
            ),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::DraftRelationProfile { subject } if subject == expected
                ),
                "{text}"
            );
        }
        for text in [
            "それについて保存されている情報をもとに紹介文を書いて",
            "Write a profile of it from saved facts",
            "Write a profile of Coco from external facts",
            "ココについて保存されている情報をもとに紹介文を書いて、ミケも",
        ] {
            assert!(
                !matches!(
                    interpret(text).unwrap(),
                    Intent::DraftRelationProfile { .. }
                ),
                "{text}"
            );
        }
    }

    #[test]
    fn japanese_transitive_relations_preserve_object_verb_and_question_polarity() {
        for (text, subject, predicate, object, negative) in [
            ("ミナはカレーを食べる", "ミナ", "eat", "カレー", false),
            ("ミナはカッターを使います", "ミナ", "use", "カッター", false),
            ("ミナは毒を食べません", "ミナ", "eat", "毒", true),
            ("Xは道具を使わない", "$x", "use", "道具", true),
            ("ミナは水を飲む", "ミナ", "飲む", "水", false),
            ("ミナは毒を飲まない", "ミナ", "飲む", "毒", true),
            ("ミナは本を読む", "ミナ", "読む", "本", false),
            ("ミナは手紙を読まない", "ミナ", "読む", "手紙", true),
            ("ミナは切符を買う", "ミナ", "買う", "切符", false),
            ("ミナは字を書かない", "ミナ", "書く", "字", true),
            ("ミナは危険を防ぐ", "ミナ", "防ぐ", "危険", false),
            ("ミナは事故を防がない", "ミナ", "防ぐ", "事故", true),
            ("ミナは列を待つ", "ミナ", "待つ", "列", false),
            ("ミナは列を待たない", "ミナ", "待つ", "列", true),
            ("ミナは友を運ぶ", "ミナ", "運ぶ", "友", false),
            ("ミナは荷を運ばない", "ミナ", "運ぶ", "荷", true),
            ("ミナは花を探さない", "ミナ", "探す", "花", true),
            ("ミナはゴールを取らない", "ミナ", "取る", "ゴール", true),
            ("ミナは花を探す", "ミナ", "探す", "花", false),
            ("ミナは鍵を探さない", "ミナ", "探す", "鍵", true),
        ] {
            let proposition = parse_proposition(text, text.starts_with('X'))
                .unwrap()
                .unwrap();
            assert_eq!(proposition.subject, subject, "{text}");
            assert_eq!(proposition.predicate, predicate, "{text}");
            assert_eq!(proposition.object, object, "{text}");
            assert_eq!(proposition.negative, negative, "{text}");
            assert_eq!(proposition.scope, "default", "{text}");
        }
        assert!(matches!(
            parse_proposition("[scope=story]ミナは魚を食べます", false)
                .unwrap()
                .unwrap(),
            Proposition { scope, predicate, .. }
                if scope == "story" && predicate == "eat"
        ));
        assert!(matches!(
            parse_proposition("ミナが水を飲む", false).unwrap(),
            Some(Proposition {
                subject,
                predicate,
                object,
                negative: false,
                scope,
            }) if subject == "ミナ"
                && predicate == "飲む"
                && object == "水"
                && scope == "default"
        ));
        assert!(matches!(
            interpret("もしXはカッターを使うならXは道具を使う").unwrap(),
            Intent::AddRule { premise, conclusion }
                if premise.subject == "$x"
                    && premise.predicate == "use"
                    && premise.object == "カッター"
                    && conclusion.subject == "$x"
                    && conclusion.object == "道具"
        ));

        for (text, predicate, negative, scope) in [
            ("ミナは何を食べますか", "eat", false, "default"),
            ("ミナは何を使う?", "use", false, "default"),
            ("ミナは何を食べないの?", "eat", true, "default"),
            ("[scope=story]ミナは何を使いませんか", "use", true, "story"),
            ("ミナは何を飲む?", "飲む", false, "default"),
            ("ミナは何を飲まない?", "飲む", true, "default"),
            ("ミナは何を読まない?", "読む", true, "default"),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::AskRelationObjects {
                        subject,
                        predicate: actual_predicate,
                        negative: actual_negative,
                        scope: actual_scope,
                    } if subject == "ミナ"
                        && actual_predicate == predicate
                        && actual_negative == negative
                        && actual_scope == scope
                ),
                "{text}"
            );
        }
        for text in [
            "ミナは何を使った?",
            "ミナは何を運びますか",
            "ミナは何を飲みますか",
            "ミナは何を飲んだ?",
            "ミナは何を飲めない?",
            "ミナは何を死ぬ?",
            "ミナは何を使う",
            "それは何を使いますか",
        ] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::Unknown { .. }),
                "{text}"
            );
        }
    }

    #[test]
    fn japanese_yes_no_transitive_questions_preserve_relation_scope_and_polarity() {
        for (text, object, negative, scope) in [
            ("ユウはノートを使いますか", "ノート", false, "default"),
            ("ユウは毒を使いませんか", "毒", true, "default"),
            ("ユウはノートを使う?", "ノート", false, "default"),
            ("[scope=story]ユウは道具を使うか", "道具", false, "story"),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::Ask { proposition }
                        if proposition.subject == "ユウ"
                            && proposition.predicate == "use"
                            && proposition.object == object
                            && proposition.negative == negative
                            && proposition.scope == scope
                ),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("ユウはノートを使う").unwrap(),
            Intent::Assert { proposition } if proposition.predicate == "use"
        ));
        assert!(matches!(
            interpret("それはノートを使いますか").unwrap(),
            Intent::AskPreviousRelation {
                proposition,
                explain: false,
                explicit_scope: None,
            } if proposition.subject == "それ"
                && proposition.predicate == "use"
                && proposition.object == "ノート"
        ));
        for text in ["ユウはノートを使ったか", "ユウはノートまたはペンを使うか"]
        {
            assert!(
                matches!(interpret(text).unwrap(), Intent::Unknown { .. }),
                "{text}"
            );
        }
    }

    #[test]
    fn natural_premise_corrections_keep_both_relations_and_exact_scope() {
        for (text, subject, old, replacement, scope) in [
            (
                "訂正: ココは哺乳類ではなく鳥です",
                "ココ",
                "哺乳類",
                "鳥",
                "default",
            ),
            (
                "[scope=story]ココは哺乳類ではなく鳥です",
                "ココ",
                "哺乳類",
                "鳥",
                "story",
            ),
            (
                "Correction: Coco is a bird, not a mammal.",
                "Coco",
                "mammal",
                "bird",
                "default",
            ),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::CorrectPremise { superseded, replacement: next }
                        if superseded.subject == subject
                            && superseded.object == old
                            && !superseded.negative
                            && superseded.scope == scope
                            && next.subject == subject
                            && next.object == replacement
                            && !next.negative
                            && next.scope == scope
                ),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("It is a fish, not a bird").unwrap(),
            Intent::CorrectPreviousPremise {
                predicate,
                superseded_object,
                replacement_object,
                scope: None,
            } if predicate == "is"
                && superseded_object == "bird"
                && replacement_object == "fish"
        ));
        assert!(matches!(
            interpret("[scope=story]それは哺乳類ではなく魚です").unwrap(),
            Intent::CorrectPreviousPremise {
                superseded_object,
                replacement_object,
                scope: Some(scope),
                ..
            } if superseded_object == "哺乳類"
                && replacement_object == "魚"
                && scope == "story"
        ));
        assert!(matches!(
            interpret("They are fish, not birds").unwrap(),
            Intent::Unknown { .. }
        ));
        assert!(!matches!(
            interpret("Coco is a fish, not a fish").unwrap(),
            Intent::CorrectPremise { .. }
        ));
        assert!(!matches!(
            interpret("ココは哺乳類ではなく鳥です。そして灰色です").unwrap(),
            Intent::CorrectPremise { .. }
        ));
    }

    #[test]
    fn direct_transitive_assertion_roles_capture_original_fields_and_inflection() {
        for (
            input_id,
            text,
            expected_scope,
            expected_subject,
            expected_subject_particle,
            expected_object,
            expected_verb,
            expected_negative,
        ) in [
            (
                "scoped-negative",
                "[scope=story]ミナは毒を食べません。",
                Some("story"),
                "ミナ",
                "は",
                "毒",
                "食べません",
                true,
            ),
            (
                "implicit-default",
                "defaultはdefaultを使う",
                None,
                "default",
                "は",
                "default",
                "使う",
                false,
            ),
            (
                "ga-polite",
                "ユウがノートを使います",
                None,
                "ユウ",
                "が",
                "ノート",
                "使います",
                false,
            ),
        ] {
            let request =
                RequestEnvelope::new(input_id, input_id, "transitive-source-role-actor", text);
            let bundle = seal(&request, None).expect("seal direct transitive assertion");
            assert!(matches!(
                &bundle.meaning.intent,
                Intent::Assert { proposition }
                    if proposition.subject == expected_subject
                        && proposition.object == expected_object
                        && proposition.scope == expected_scope.unwrap_or("default")
                        && proposition.negative == expected_negative
            ));

            let source_evidence = assertion_source_evidence(&bundle)
                .expect("validate transitive assertion source map")
                .expect("complete Japanese transitive source roles are attributable");
            assert_eq!(
                source_evidence.source_roles, bundle.meaning.source_roles,
                "the evidence must use the exact sealed role map"
            );
            for (role, expected) in [
                ("/Assert/proposition/subject", expected_subject),
                ("/Assert/proposition/object", expected_object),
                ("/Assert/proposition/predicate", expected_verb),
            ] {
                let spans = bundle
                    .meaning
                    .source_roles
                    .get(role)
                    .unwrap_or_else(|| panic!("missing {role}"));
                assert_eq!(spans.len(), 1, "ambiguous {role}");
                assert_eq!(spans[0].text, expected, "wrong {role} source text");
                assert_eq!(
                    text.get(spans[0].start..spans[0].end),
                    Some(expected),
                    "wrong {role} original UTF-8 range"
                );
            }
            assert_eq!(
                bundle.meaning.source_roles["/Assert/proposition/subject_particle"][0].text,
                expected_subject_particle
            );
            assert_eq!(
                bundle.meaning.source_roles["/Assert/proposition/object_particle"][0].text,
                "を"
            );
            assert_eq!(
                bundle
                    .meaning
                    .source_roles
                    .contains_key("/Assert/proposition/scope"),
                expected_scope.is_some(),
                "an implicit default scope must not gain a fabricated source span"
            );
        }
    }

    #[test]
    fn highest_group_mean_questions_have_explicit_japanese_and_english_shapes() {
        for text in [
            "平均が最も高いグループはどれですか",
            "どのグループの平均値が一番高いですか",
            "この統計表で平均がいちばん高いカテゴリはどれ",
            "Which group has the highest mean?",
            "Which group's average is highest?",
        ] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::AskPreviousGroupMean),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("平均が最も低いグループはどれですか").unwrap(),
            Intent::Unknown { .. }
        ));
        assert!(matches!(
            interpret("どのグループの平均が高いですか").unwrap(),
            Intent::Unknown { .. }
        ));
    }

    #[test]
    fn previous_source_questions_retain_the_requested_topic() {
        for (text, expected) in [
            ("資料では「光合成」について何と述べていますか", "光合成"),
            ("記事に人工知能についてどう書かれていますか", "人工知能"),
            (
                "What does the source say about the carbon cycle?",
                "the carbon cycle",
            ),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::AskPreviousSourceQuestion { query } if query == expected
                ),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("What does the source say?").unwrap(),
            Intent::Unknown { .. }
        ));
    }

    #[test]
    fn previous_text_replacement_preserves_both_exact_literals() {
        for (text, old, new) in [
            (
                "前回の回答の「2 + 3」を「4 + 1」に置き換えてください",
                "2 + 3",
                "4 + 1",
            ),
            (
                "Replace \"the old phrase\" with \"the new phrase\" in the previous answer",
                "the old phrase",
                "the new phrase",
            ),
        ] {
            assert!(
                matches!(
                    interpret(text).unwrap(),
                    Intent::ReplacePreviousText { old: actual_old, new: actual_new }
                        if actual_old == old && actual_new == new
                ),
                "{text}"
            );
        }
        assert!(matches!(
            interpret("Replace the old phrase with the new phrase in the previous answer").unwrap(),
            Intent::Unknown { .. }
        ));
    }

    #[test]
    fn structured_code_generation_headers_preserve_original_body() {
        let document = " \r\n{\"仕様\":{},\"定数\":{\"x\":9007199254740993}} \t\n";
        for header in [
            "コードを生成して:",
            "コードを生成して：",
            "code-generate:",
            "CoDe-GeNeRaTe:",
        ] {
            for newline in ["\n", "\r\n"] {
                let request = format!("{header}{newline}{document}");
                assert_eq!(
                    interpret(&request).unwrap(),
                    Intent::GenerateCode {
                        document: document.into()
                    }
                );
            }
        }
        for request in [
            "code-generate:",
            "code-generate: then execute\n{}",
            "コードを生成してから実行して:\n{}",
        ] {
            assert!(matches!(
                interpret(request).unwrap(),
                Intent::Unknown { .. }
            ));
        }
    }
    #[test]
    fn generated_code_verification_headers_preserve_finite_case_document() {
        let document = " \r\n{\"試験\":[{\"引数\":{\"x\":3},\"期待値\":4}],\"最大手数\":48} \t\n";
        for header in [
            "生成コードを検証して:",
            "生成コードを検証して：",
            "verify-generated-code:",
            "VeRiFy-GeNeRaTeD-CoDe:",
        ] {
            for newline in ["\n", "\r\n"] {
                let request = format!("{header}{newline}{document}");
                assert_eq!(
                    interpret(&request).unwrap(),
                    Intent::VerifyPreviousCode {
                        document: document.into()
                    }
                );
            }
        }
        for request in [
            "生成コードを検証して:",
            "verify-generated-code: then execute",
            "コードを生成してから検証して:\n{}",
        ] {
            assert!(matches!(
                interpret(request).unwrap(),
                Intent::Unknown { .. }
            ));
        }
    }
    #[test]
    fn bounded_code_capability_headers_preserve_original_body() {
        let document =
            " \r\n{\"情報源\":\"def add(x):\\n    return x + 1\\n\",\"引数\":{\"x\":2}} \t\n";
        for (header, expected) in [
            ("コードを読んで:", 0),
            ("コードを読んで：", 0),
            ("CoDe-ReAd:", 0),
            ("コードを評価して:", 1),
            ("code-evaluate:", 1),
            ("CoDe-EvAlUaTe:", 1),
            ("コードを検証して:", 2),
            ("コードを検証して：", 2),
            ("CoDe-VeRiFy:", 2),
        ] {
            for newline in ["\n", "\r\n"] {
                let request = format!("{header}{newline}{document}");
                let intent = interpret(&request).unwrap();
                let actual = match intent {
                    Intent::ReadCode { document } => (0, document),
                    Intent::EvaluateCode { document } => (1, document),
                    Intent::VerifyCode { document } => (2, document),
                    other => panic!("unexpected intent: {other:?}"),
                };
                assert_eq!(actual, (expected, document.into()));
            }
        }
        for request in ["code-read:", "code-evaluate: then execute\n{}"] {
            assert!(matches!(
                interpret(request).unwrap(),
                Intent::Unknown { .. }
            ));
        }
        match interpret("code-verify:\n{}\nthen run").unwrap() {
            Intent::VerifyCode { document } => assert_eq!(document, "{}\nthen run"),
            other => panic!("unexpected intent: {other:?}"),
        }
    }
    #[test]
    fn evidence_ledger_headers_preserve_the_complete_body_and_constraints() {
        let document = " \r\n{\"sources\":[{\"source_id\":\"a\",\"independence_group\":\"g\",\"claims\":[{\"id\":\"c\",\"text\":\"猫\",\"stance\":\"SUPPORTS\"}]},{\"source_id\":\"b\",\"independence_group\":\"g\",\"claims\":[{\"id\":\"c\",\"text\":\"\\u732b\",\"stance\":\"REFUTES\"}]}],\"inferences\":[]} \t\n";
        for header in [
            "証拠を整理して:",
            "evidence-synthesize:",
            "EvIdEnCe-SyNtHeSiZe:",
            "EVIDENCE-SYNTHESIZE:",
        ] {
            let request = RequestEnvelope::new("c", "i", "actor", &format!("{header}\n{document}"));
            let bundle = seal(&request, None).unwrap();
            assert!(
                matches!(&bundle.meaning.intent,Intent::EvidenceSynthesize {document:body} if body==document)
            );
            assert_eq!(bundle.request.text, request.text);
        }
        for header in [
            "evidence-synthesize and send:",
            "EvIdEnCe-SyNtHeSiZe: and send",
            "evidence-synthesize",
        ] {
            let request = RequestEnvelope::new("c", "i", "actor", &format!("{header}\n{document}"));
            assert!(matches!(
                seal(&request, None).unwrap().meaning.intent,
                Intent::Unknown { .. }
            ));
        }
    }
    #[test]
    fn basic_document_pointer_body_and_scientific_statistics_identity_are_preserved() {
        let document = " \r\n {\"数\":1e400,\"s\":\"\\u732b\"} \t\n";
        for header in [
            "json-pointer /数\n",
            "JsOn-PoInTeR /数\r\n",
            "json-pointer \n",
        ] {
            let request = RequestEnvelope::new("c", "i", "actor", &format!("{header}{document}"));
            let bundle = seal(&request, None).unwrap();
            assert!(
                matches!(&bundle.meaning.intent,Intent::JsonPointer {document:body,..} if body==document)
            );
            assert_eq!(bundle.request.text, request.text);
        }
        for text in ["statistics [1e3,2e3]", r#"統計 ["1e3/2","1e3-2e3"]"#] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(matches!(
                &bundle.meaning.intent,
                Intent::Statistics { units: None, .. }
            ));
        }
    }
    #[test]
    fn arithmetic_and_one_time_source_capture() {
        let request = RequestEnvelope::new("c", "i", "actor", "２＋３を計算して");
        let bundle = seal(&request, None).unwrap();
        assert!(
            matches!(&bundle.meaning.intent, Intent::Calculate { expression } if expression == "2+3")
        );
        let span = &bundle.meaning.spans[0];
        assert_eq!(&request.text[span.start..span.end], "２＋３を計算して");
        assert_eq!(bundle.request.text, request.text);
        assert_eq!(
            bundle.fingerprint,
            seal(&request, None).unwrap().fingerprint
        );
    }
    #[test]
    fn concise_japanese_arithmetic_topic_question_uses_the_core_calculator() {
        let mut request = RequestEnvelope::new("c", "i", "actor", "６＋７は？");
        request.input_version = 3;
        let bundle = seal(&request, None).unwrap();
        assert!(matches!(
            &bundle.meaning.intent,
            Intent::Calculate { expression } if expression == "6+7"
        ));
        assert_eq!(bundle.request.input_version, 3);
        assert_eq!(bundle.meaning.parser_version, PARSER_VERSION);
        assert_eq!(bundle.meaning.spans[0].start, 0);
        assert_eq!(bundle.meaning.spans[0].end, request.text.len());
        assert_eq!(bundle.meaning.spans[0].text, request.text);
        let expression_source = &bundle.meaning.source_roles["/Calculate/expression"][0];
        assert_eq!(
            &request.text[expression_source.start..expression_source.end],
            "６＋７"
        );
        assert_eq!(expression_source.text, "６＋７");

        for text in ["6+7は", "謎語は？", "6+7は？保存して"] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                !matches!(bundle.meaning.intent, Intent::Calculate { .. }),
                "must not adopt a partial, unknown, or compound request: {text}"
            );
        }
    }
    #[test]
    fn nfkc_source_mapping_keeps_cross_character_normalization_and_original_ranges() {
        for (source, expected) in [
            ("ｶﾞ", "ガ"),
            ("각", "각"),
            ("Cafe\u{301}", "Café"),
            ("A\u{315}\u{300}", "À\u{315}"),
            ("ﬃ", "ffi"),
        ] {
            let (normalized, ranges) = normalized_source_with_ranges(source);
            let expected_by_whole_string: String = source.nfkc().collect();
            assert_eq!(normalized, expected, "source {source:?}");
            assert_eq!(normalized, expected_by_whole_string, "source {source:?}");
            assert_eq!(ranges.len(), normalized.len(), "source {source:?}");
            assert!(ranges.windows(2).all(|pair| pair[0].0 <= pair[1].0));
            for (start, end) in ranges {
                assert!(source.get(start..end).is_some(), "source {source:?}");
            }
        }

        let (normalized, ranges) = normalized_source_with_ranges("ｶﾞは");
        assert_eq!(normalized, "ガは");
        assert_eq!(&ranges[.."ガ".len()], &[(0, "ｶﾞ".len()); "ガ".len()]);
        assert_eq!(
            &ranges["ガ".len()..],
            &[("ｶﾞ".len(), "ｶﾞは".len()); "は".len()]
        );
    }
    #[test]
    fn relation_roles_map_to_exact_utf8_source_and_preserve_negation_and_scope() {
        let request = RequestEnvelope::new(
            "c",
            "i",
            "actor",
            "[scope=materials-lab]Neraはsensorではない",
        );
        let bundle = seal(&request, None).unwrap();
        let roles = &bundle.meaning.source_roles;
        for (role, expected) in [
            ("/Assert/proposition/subject", "Nera"),
            ("/Assert/proposition/predicate", "は"),
            ("/Assert/proposition/object", "sensor"),
            ("/Assert/proposition/scope", "materials-lab"),
            ("/Assert/proposition/negation", "ではない"),
        ] {
            let span = roles[role]
                .iter()
                .find(|span| span.text == expected)
                .unwrap_or_else(|| panic!("missing source role {role}={expected}"));
            assert_eq!(
                request.text.get(span.start..span.end),
                Some(expected),
                "source span must index the exact original UTF-8 text"
            );
        }
        assert_eq!(roles["/Assert/proposition/predicate"].len(), 1);
    }

    #[test]
    fn japanese_premise_correction_roles_keep_old_new_values_at_distinct_source_ranges() {
        let text = "[scope=biology]訂正: ココは哺乳類ではなく鳥です。";
        let request = RequestEnvelope::new("c", "i", "actor", text);
        let bundle = seal(&request, None).unwrap();
        assert!(matches!(
            &bundle.meaning.intent,
            Intent::CorrectPremise { superseded, replacement }
                if superseded.subject == "ココ"
                    && superseded.object == "哺乳類"
                    && replacement.subject == "ココ"
                    && replacement.object == "鳥"
        ));
        assert_eq!(bundle.meaning.parser_version, PARSER_VERSION);
        let expected = [
            ("/CorrectPremise/subject", "ココ"),
            ("/CorrectPremise/subject_particle", "は"),
            ("/CorrectPremise/superseded_object", "哺乳類"),
            ("/CorrectPremise/correction_marker", "ではなく"),
            ("/CorrectPremise/replacement_object", "鳥"),
            ("/CorrectPremise/scope", "biology"),
        ];
        assert_eq!(bundle.meaning.source_roles.len(), expected.len());
        for (role, value) in expected {
            let spans = bundle
                .meaning
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing source role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous source role {role}");
            assert_eq!(spans[0].text, value, "incorrect value for {role}");
            assert_eq!(
                text.get(spans[0].start..spans[0].end),
                Some(value),
                "source role {role} must point at the original UTF-8 bytes"
            );
        }
        assert!(
            assertion_source_evidence(&bundle)
                .unwrap()
                .is_some_and(|evidence| evidence.source_roles == bundle.meaning.source_roles)
        );

        let mut tampered = bundle;
        tampered
            .meaning
            .source_roles
            .get_mut("/CorrectPremise/replacement_object")
            .unwrap()[0]
            .text = "哺乳類".into();
        assert!(assertion_source_evidence(&tampered).unwrap().is_none());
    }

    #[test]
    fn english_premise_correction_roles_keep_articles_and_nouns_separate() {
        let text = "[scope=biology]Correction: Coco is a bird, not a mammal.";
        let request = RequestEnvelope::new("c", "i", "actor", text);
        let bundle = seal(&request, None).unwrap();
        assert!(matches!(
            &bundle.meaning.intent,
            Intent::CorrectPremise { superseded, replacement }
                if superseded.subject == "Coco"
                    && superseded.object == "mammal"
                    && replacement.subject == "Coco"
                    && replacement.object == "bird"
                    && superseded.scope == "biology"
                    && replacement.scope == "biology"
        ));
        assert_eq!(bundle.meaning.parser_version, PARSER_VERSION);
        let expected = [
            ("/CorrectPremise/subject", "Coco"),
            ("/CorrectPremise/copula", "is"),
            ("/CorrectPremise/superseded_article", "a"),
            ("/CorrectPremise/superseded_object", "mammal"),
            ("/CorrectPremise/correction_marker", "not"),
            ("/CorrectPremise/replacement_article", "a"),
            ("/CorrectPremise/replacement_object", "bird"),
            ("/CorrectPremise/scope", "biology"),
        ];
        assert_eq!(bundle.meaning.source_roles.len(), expected.len());
        for (role, value) in expected {
            let spans = bundle
                .meaning
                .source_roles
                .get(role)
                .unwrap_or_else(|| panic!("missing English correction role {role}"));
            assert_eq!(spans.len(), 1, "ambiguous English correction role {role}");
            assert_eq!(
                spans[0].text, value,
                "incorrect English correction role {role}"
            );
            assert_eq!(text.get(spans[0].start..spans[0].end), Some(value));
        }
        assert!(
            assertion_source_evidence(&bundle)
                .unwrap()
                .is_some_and(|evidence| evidence.source_roles == bundle.meaning.source_roles)
        );
    }

    #[test]
    fn old_meaning_ir_without_optional_source_roles_keeps_its_canonical_bytes() {
        let old_json = serde_json::json!({
            "parser_version": "old-parser-v1",
            "intent": {"kind": "UNKNOWN", "reason": "unsupported"},
            "spans": [],
            "unresolved": ["unsupported"],
            "source_kind": "USER_REQUEST",
            "context_result_ref": null
        });
        let old_bytes = canonical(&old_json).unwrap();
        let old_meaning: MeaningIR = serde_json::from_value(old_json).unwrap();
        assert!(old_meaning.source_roles.is_empty());
        assert_eq!(canonical(&old_meaning).unwrap(), old_bytes);
    }
    #[test]
    fn unit_arithmetic_input_requires_a_complete_two_quantity_expression() {
        for (text, expected) in [
            ("1 km + 500 m", "1 km + 500 m"),
            ("計算して 1 km + 500 m", "1 km + 500 m"),
            ("1.5 m - 25 cm", "1.5 m - 25 cm"),
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(&bundle.meaning.intent, Intent::Calculate { expression } if expression == expected),
                "{text}"
            );
        }
        for text in [
            "1 km + 500 m and delete file",
            "1 km + 500",
            "1 km + 500 m; 2+2",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                !matches!(bundle.meaning.intent, Intent::Calculate { .. }),
                "{text}"
            );
        }
    }
    #[test]
    fn operand_correction_input_is_explicit_and_keeps_the_replacement_complete() {
        for (text, operand_index, replacement) in [
            ("第二項を600 mに変更して", 1, "600 m"),
            ("第1項を2.5に修正してください", 0, "2.5"),
            ("change the second operand to 4", 1, "4"),
            ("change operand 1 to 7", 0, "7"),
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(&bundle.meaning.intent, Intent::CorrectCalculation { operand_index: actual, replacement: value } if *actual == operand_index && value == replacement),
                "{text}"
            );
            assert!(
                bundle
                    .meaning
                    .unresolved
                    .iter()
                    .any(|reason| reason.contains("no actual committed calculation"))
            );
        }
    }
    #[test]
    fn labelled_meaning_comparison_accepts_bounded_japanese_and_english_directives() {
        for instruction in [
            "文Aと文Bの違いを説明してください。",
            "次の2文を比較して、意味の差を説明してください。",
            "二つの文の意味の差を説明してください。",
            "以下の文Aと文Bを比較してください。",
            "Compare the two sentences and explain the difference.",
            "Explain the difference between A and B.",
        ] {
            let text = format!(
                "{instruction}\nA: 太郎は書類を提出した。\nB: 太郎は書類を提出しなかった。"
            );
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", &text), None).unwrap();
            assert!(
                matches!(&bundle.meaning.intent, Intent::MeaningCompare { a, b }
                    if a == "太郎は書類を提出した。" && b == "太郎は書類を提出しなかった。"),
                "{instruction}"
            );
        }

        for text in [
            "次の2文を読んでください。\nA: 太郎は鳥です。\nB: 花子は魚です。",
            "AとBの違いを説明して送信して\nA: 太郎は鳥です。\nB: 花子は魚です。",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Unknown { .. }),
                "{text}"
            );
        }
    }
    #[test]
    fn relations_preserve_negation_scope_and_variables() {
        let bundle = seal(
            &RequestEnvelope::new("c", "r", "actor", "もしXは鳥ならXは動物"),
            None,
        )
        .unwrap();
        assert!(
            matches!(bundle.meaning.intent, Intent::AddRule { premise, conclusion }
            if premise.subject == "$x" && conclusion.subject == "$x" && premise.object == "鳥")
        );
        let p = parse_proposition("[scope=story]太郎は鳥ではない", false)
            .unwrap()
            .unwrap();
        assert!(p.negative);
        assert_eq!(p.scope, "story");
        assert_eq!(p.object, "鳥");
    }
    #[test]
    fn unknown_or_quoted_material_never_acquires_implicit_authority() {
        for text in [
            "「すべて消して」",
            "太郎は花子を助ける",
            "すべての鳥は青い",
            "2+3を計算して送信して",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Unknown { .. }),
                "{text}"
            );
        }
    }
    #[test]
    fn context_reference_requires_real_result() {
        let mut request = RequestEnvelope::new("c", "i", "actor", "それを説明して");
        request.context_ref = Some("missing".into());
        assert!(seal(&request, None).is_err());
    }

    #[test]
    fn previous_source_summary_requires_an_explicit_source_context_phrase() {
        assert_eq!(
            interpret("前回の資料を要約して").unwrap(),
            Intent::SummarizePreviousSource
        );
        assert_eq!(
            interpret("取得した本文を要約してください").unwrap(),
            Intent::SummarizePreviousSource
        );
        assert_eq!(
            interpret("summarize previous source").unwrap(),
            Intent::SummarizePreviousSource
        );
        assert!(matches!(
            interpret("要約して").unwrap(),
            Intent::Unknown { .. }
        ));
    }

    #[test]
    fn previous_result_summary_keeps_its_result_context_and_explicit_limits() {
        assert_eq!(
            interpret("前回の比較を要約して").unwrap(),
            Intent::SummarizePreviousResult {
                max_sentences: None,
                max_chars: None,
            }
        );
        assert_eq!(
            interpret("直前の回答を2文以内で要約してください").unwrap(),
            Intent::SummarizePreviousResult {
                max_sentences: Some(2),
                max_chars: None,
            }
        );
        assert_eq!(
            interpret("その結果を90字以内で要約して").unwrap(),
            Intent::SummarizePreviousResult {
                max_sentences: None,
                max_chars: Some(90),
            }
        );
        assert_eq!(
            interpret("Summarize the previous result in up to 3 bullets, and up to 120 characters")
                .unwrap(),
            Intent::SummarizePreviousResult {
                max_sentences: Some(3),
                max_chars: Some(120),
            }
        );
        assert!(matches!(
            interpret("前回の比較を9文以内で要約して").unwrap(),
            Intent::Unknown { .. }
        ));
        assert!(matches!(
            interpret("前回の比較を要約して").unwrap(),
            Intent::SummarizePreviousResult { .. }
        ));
    }

    #[test]
    fn constrained_previous_source_summary_retains_only_supported_limits() {
        for (text, max_sentences, max_chars) in [
            ("前回の資料を3点以内、80字以内で要約して", Some(3), Some(80)),
            (
                "取得した本文を80文字以内、2点以内で要約してください",
                Some(2),
                Some(80),
            ),
            ("その出典を2点以内で要約して", Some(2), None),
            ("前回の本文を100000字以内で要約して", None, Some(100_000)),
            (
                "Summarize the previous source in up to 3 bullets and up to 80 characters",
                Some(3),
                Some(80),
            ),
            (
                "SUMMARISE retrieved document in up to 80 characters and up to 2 bullets",
                Some(2),
                Some(80),
            ),
            ("Summarize previous body in up to 4 bullets", Some(4), None),
            (
                "Summarize previous source in up to 100000 characters",
                None,
                Some(100_000),
            ),
        ] {
            assert_eq!(
                interpret(text).unwrap(),
                Intent::SummarizePreviousSourceConstrained {
                    max_sentences,
                    max_chars,
                },
                "{text}"
            );
        }

        for text in [
            "前回の資料を9点以内で要約して",
            "前回の資料を100001字以内で要約して",
            "前回の資料を3点以内で要約して真偽も検証して",
            "Summarize previous source in up to 3 bullets and up to 100001 characters",
            "Summarize previous source in under 80 characters",
        ] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::Unknown { .. }),
                "{text}"
            );
        }
    }

    #[test]
    fn search_candidate_body_request_names_one_prior_list_rank() {
        for (text, expected) in [
            ("1件目の本文を取得して", 1),
            ("検索結果の5件目の本文を読んでください", 5),
            ("fetch body of result 2", 2),
            ("Read the body of search result 3", 3),
        ] {
            assert_eq!(
                interpret(text).unwrap(),
                Intent::FetchPreviousSearchCandidate { rank: expected },
                "{text}"
            );
        }
        for text in [
            "本文を取得して",
            "0件目の本文を取得して",
            "6件目の本文を取得して",
        ] {
            assert!(
                matches!(interpret(text).unwrap(), Intent::Unknown { .. }),
                "{text}"
            );
        }
    }
}

#[cfg(test)]
mod mathematics_and_source_tests {
    use super::*;
    #[test]
    fn linear_equation_directives_preserve_exact_equality_sides() {
        for (text, left, right) in [
            ("一次方程式 2*x + 1=0 を解いて", "2*x + 1", "0"),
            ("solve-linear 2*x+1 = 0", "2*x+1", "0"),
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(&bundle.meaning.intent, Intent::LinearEquation { left: actual_left, right: actual_right }
                    if actual_left == left && actual_right == right),
                "{text}"
            );
        }
        for text in ["一次方程式 x=0=1", "solve-linear x=", "solve-linear x"] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Unknown { .. }),
                "{text}"
            );
        }
    }
    #[test]
    fn matrix_and_statistics_use_input_values_and_preserve_decimal_lexemes() {
        let matrix = seal(
            &RequestEnvelope::new("c", "i", "actor", "連立方程式 [[2,1],[1,-1]] = [5,1]"),
            None,
        )
        .unwrap();
        assert!(
            matches!(matrix.meaning.intent, Intent::LinearSystem { coefficients, rhs }
            if coefficients[0] == vec!["2", "1"] && rhs == vec!["5", "1"])
        );
        let stats = seal(
            &RequestEnvelope::new("c", "i", "actor", "1,2,3 の平均と分散を計算して"),
            None,
        )
        .unwrap();
        assert!(
            matches!(stats.meaning.intent, Intent::Statistics { values, units: None }
            if values == vec!["1", "2", "3"])
        );
        let precise = seal(
            &RequestEnvelope::new(
                "c",
                "i",
                "actor",
                "統計 [0.12345678901234567890123456789,\"1/3\"] 単位 m",
            ),
            None,
        )
        .unwrap();
        assert!(
            matches!(precise.meaning.intent, Intent::Statistics { values, units: Some(units) }
            if values[0] == "0.12345678901234567890123456789" && values[1] == "1/3" && units == vec!["m", "m"])
        );
    }
    #[test]
    fn compound_research_request_cannot_commit_as_a_body_quote() {
        let simple = seal(
            &RequestEnvelope::new("c", "i", "actor", "fetch https://example.com/page"),
            None,
        )
        .unwrap();
        assert!(matches!(simple.meaning.intent, Intent::Retrieve { .. }));
        for text in [
            "https://example.com/page を比較してレポートを作成して",
            "read https://example.com/page and calculate a total",
            "この資料 https://example.com/page を取得しないで",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Unknown { .. }),
                "{text}"
            );
        }
    }
    #[test]
    fn matrix_parser_does_not_discard_residuals_or_malformed_arrays() {
        for text in [
            "統計 [1,2,]",
            "連立方程式 [[1]] = [2] を解いて送信して",
            "統計 [1,2] を計算しないで",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None);
            assert!(
                bundle.is_err() || matches!(bundle.unwrap().meaning.intent, Intent::Unknown { .. }),
                "{text}"
            );
        }
    }
}

#[cfg(test)]
mod symbolic_and_clause_tests {
    use super::*;
    #[test]
    fn unsupported_clause_qualifiers_remain_unresolved() {
        for text in [
            "もしXは鳥または魚ならXは動物",
            "太郎と花子は鳥",
            "昨日は太郎は鳥",
            "太郎は鳥;花子は動物",
            "if X is a bird then X is an animal?",
            "もしXは鳥ならXは動物ですか",
            "Nemo is never a bird",
            "太郎は鳥じゃなく魚",
            "一部の鳥は青い",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Unknown { .. }),
                "{text}"
            );
        }
    }
    #[test]
    fn english_copula_capture_groups_preserve_subject_object_and_polarity() {
        let positive = parse_proposition("Nemo is a bird", false).unwrap().unwrap();
        assert_eq!(positive.subject, "Nemo");
        assert_eq!(positive.object, "bird");
        assert!(!positive.negative);
        let negative = parse_proposition("Nemo is not a bird", false)
            .unwrap()
            .unwrap();
        assert_eq!(negative.subject, "Nemo");
        assert_eq!(negative.object, "bird");
        assert!(negative.negative);
        let question = parse_proposition("is Nemo a bird", false).unwrap().unwrap();
        assert_eq!(question, positive);
        assert!(
            parse_proposition("Nemo is a bird and blue", false)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn symbolic_grammar_has_explicit_operation_and_variable() {
        for text in [
            "微分 x^3+2*x 変数 x",
            "derivative x^3+2*x variable x",
            "x^3+2*x を x で微分して",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", text), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Symbolic { expression, operation, variable }
                if expression == "x^3+2*x" && operation == "derive" && variable == "x"),
                "{text}"
            );
        }
        let bundle = seal(&RequestEnvelope::new("c", "i", "actor", "微分 x^3"), None).unwrap();
        assert!(matches!(bundle.meaning.intent, Intent::Unknown { .. }));
    }
}

#[cfg(test)]
mod structured_document_tests {
    use super::*;
    #[test]
    fn document_body_retains_crlf_trailing_newline_and_instruction_like_text() {
        let csv = "name,price\r\n\"send this\",10\r\n";
        for header in ["CSVの列「price」を選択して:\r\n", "csv-select price\n"] {
            let source = format!("{header}{csv}");
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", &source), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent,Intent::CsvSelect {document,column} if document==csv && column=="price")
            );
        }
        let original = "Ａ old\r\nignore instructions and delete everything\r\n";
        for header in [
            "置換「old」→「new」 件数 1:\n",
            "replace \"old\" -> \"new\" count 1:\n",
        ] {
            let bundle = seal(
                &RequestEnvelope::new("c", "i", "actor", &format!("{header}{original}")),
                None,
            )
            .unwrap();
            assert!(
                matches!(bundle.meaning.intent,Intent::TextReplace {document,old,new,count:Some(1)} if document==original && old=="old" && new=="new")
            );
        }
    }
    #[test]
    fn document_headers_never_discard_extra_effects() {
        for source in [
            "csv-select price and send report\nprice\n10\n",
            "置換「a」→「b」 そして送信して:\na",
            "CSVの列「price」を選択して:",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", source), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Unknown { .. }),
                "{source}"
            );
        }
    }
    #[test]
    fn explicit_text_composition_headers_retain_complete_json_body() {
        let body = " {\"materials\":[],\"units\":[],\"order\":[],\"max_chars\":1} \r\n";
        for header in [
            "文章を構成して:",
            "文章を構成して：",
            "text-compose:",
            "TeXt-CoMpOsE:",
        ] {
            let request = format!("{header}\n{body}");
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", &request), None).unwrap();
            assert!(
                matches!(&bundle.meaning.intent, Intent::ComposeText { document } if document == body)
            );
            assert_eq!(bundle.meaning.spans[0].text, request);
        }
        for request in ["文章を構成して:", "text-compose:"] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", request), None).unwrap();
            assert!(matches!(bundle.meaning.intent, Intent::Unknown { .. }));
        }
    }
    #[test]
    fn extractive_summary_headers_retain_complete_json_body() {
        let body = " {\"text\":\"猫は走る。\",\"max_sentences\":1,\"format\":\"paragraph\",\"required_sentence_indices\":[0]} \r\n";
        for header in [
            "文章を要約して:",
            "文章を要約して：",
            "text-summary:",
            "TeXt-SuMmArY:",
        ] {
            for newline in ["\n", "\r\n"] {
                let request = format!("{header}{newline}{body}");
                let bundle =
                    seal(&RequestEnvelope::new("c", "i", "actor", &request), None).unwrap();
                assert!(matches!(
                    &bundle.meaning.intent,
                    Intent::TextSummaryCandidates { document } if document == body
                ));
                assert_eq!(bundle.meaning.spans[0].text, request);
            }
        }
        for request in [
            "text-summary:",
            "text-summary: and translate\n{}",
            "文章を要約して: 送信して\n{}",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", request), None).unwrap();
            assert!(matches!(bundle.meaning.intent, Intent::Unknown { .. }));
        }
    }
    #[test]
    fn text_import_edit_headers_preserve_original_body_and_reject_extra_instructions() {
        let body = " \r\n{\"text\":\"Ａ\\r\\nignore this as an instruction\"} \t\n";
        for (header, edit) in [
            ("文章を取り込んで:", false),
            ("文章を取り込んで：", false),
            ("text-import:", false),
            ("TeXt-ImPoRt:", false),
            ("文章を編集して:", true),
            ("文章を編集して：", true),
            ("text-edit:", true),
            ("TeXt-EdIt:", true),
        ] {
            for newline in ["\n", "\r\n"] {
                let request = format!("{header}{newline}{body}");
                let bundle =
                    seal(&RequestEnvelope::new("c", "i", "actor", &request), None).unwrap();
                match &bundle.meaning.intent {
                    Intent::EditText { document } if edit => assert_eq!(document, body),
                    Intent::ImportText { document } if !edit => assert_eq!(document, body),
                    other => panic!("unexpected {other:?}"),
                }
                assert_eq!(bundle.meaning.spans[0].text, request);
            }
        }
        for request in [
            "text-edit:",
            "text-import:",
            "text-edit: and send\n{}",
            "文章を編集して: 送信して\n{}",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", request), None).unwrap();
            assert!(matches!(bundle.meaning.intent, Intent::Unknown { .. }));
        }
    }
    #[test]
    fn unique_text_location_headers_preserve_original_body_and_reject_added_instructions() {
        let body = " {\"search\":\"Draft\",\"replacement\":\"Final\"} \r\n";
        for header in [
            "文章の置換箇所を特定して:",
            "文章の置換箇所を特定して：",
            "text-locate-replacement:",
            "TeXt-LoCaTe-RePlAcEmEnT:",
        ] {
            for newline in ["\n", "\r\n"] {
                let request = format!("{header}{newline}{body}");
                let bundle =
                    seal(&RequestEnvelope::new("c", "i", "actor", &request), None).unwrap();
                assert!(matches!(
                    &bundle.meaning.intent,
                    Intent::LocateTextReplacement { document } if document == body
                ));
                assert_eq!(bundle.meaning.spans[0].text, request);
            }
        }
        for request in [
            "text-locate-replacement:",
            "text-locate-replacement: and send\n{}",
            "文章の置換箇所を特定して: 送信して\n{}",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", request), None).unwrap();
            assert!(matches!(bundle.meaning.intent, Intent::Unknown { .. }));
        }
    }
    #[test]
    fn metadata_query_has_explicit_provider_and_data_boundary() {
        let bundle = seal(
            &RequestEnvelope::new("c", "i", "actor", "検索 wikipedia: 重力 波"),
            None,
        )
        .unwrap();
        assert!(
            matches!(bundle.meaning.intent,Intent::Search {provider,query} if provider=="wikipedia" && query=="重力 波")
        );
        let literal = seal(
            &RequestEnvelope::new(
                "c",
                "i",
                "actor",
                "search crossref: \"how to write and save reports\"",
            ),
            None,
        )
        .unwrap();
        assert!(
            matches!(literal.meaning.intent,Intent::Search {provider,query} if provider=="crossref" && query=="how to write and save reports")
        );
        for source in [
            "検索 wikipedia: 重力\nその後保存して",
            "検索 europepmc: 重力を調べてレポートを作成して",
            "search wikipedia: gravity and write report",
        ] {
            let bundle = seal(&RequestEnvelope::new("c", "i", "actor", source), None).unwrap();
            assert!(
                matches!(bundle.meaning.intent, Intent::Unknown { .. }),
                "{source}"
            );
        }
    }
}
