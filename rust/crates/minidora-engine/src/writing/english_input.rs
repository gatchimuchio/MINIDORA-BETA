//! Source spans for ordinary English email and message requests.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use minidora_domain::{
    MessageDraft, MessageFormat, MessagePurpose, SentenceLimit, WritingRegister,
};
use regex::Regex;

use super::{ParsedMessage, original_span};

pub(crate) const PARSER_VERSION: &str = "declarative-en-v1-draft-message";

pub(crate) fn clauses(raw: &str) -> Option<Vec<&str>> {
    if raw.len() > 4096
        || raw.contains(['「', '」', '『', '』', '"', '“', '”'])
        || raw
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return None;
    }
    let mut clauses = Vec::new();
    let mut start = 0;
    for (index, marker) in raw.match_indices(['.', ',', ';', '\n', '\r', '!', '?']) {
        if marker == "." && !sentence_period(raw, index) {
            continue;
        }
        let clause = raw[start..index].trim();
        if !clause.is_empty() {
            clauses.push(clause);
        }
        start = index + marker.len();
    }
    let last = raw[start..].trim();
    if !last.is_empty() {
        clauses.push(last);
    }
    Some(clauses)
}

fn sentence_period(raw: &str, index: usize) -> bool {
    let tail = &raw[index + 1..];
    if tail
        .chars()
        .next()
        .is_some_and(|next| !next.is_whitespace())
    {
        return false;
    }
    let word = raw[..index]
        .rsplit_once(char::is_whitespace)
        .map_or(&raw[..index], |(_, word)| word);
    !["a.m", "p.m", "mr", "mrs", "ms", "dr", "prof"]
        .iter()
        .any(|abbrev| word.eq_ignore_ascii_case(abbrev))
}

pub(crate) fn sentence_limit(clause: &str) -> Option<SentenceLimit> {
    static LIMIT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)\A(?:(?:in|use|write|keep it to|make it)\s+)?",
            r"(?P<mode>(?:exactly|at most|no more than)\s+)?(?P<count>[1-3]|one|two|three)\s+sentences?\z"
        ))
        .expect("English draft sentence limits")
    });
    let parts = LIMIT.captures(clause)?;
    let count = match parts["count"].to_ascii_lowercase().as_str() {
        "1" | "one" => 1,
        "2" | "two" => 2,
        "3" | "three" => 3,
        _ => return None,
    };
    Some(
        if parts
            .name("mode")
            .is_some_and(|m| !m.as_str().trim().eq_ignore_ascii_case("exactly"))
        {
            SentenceLimit::AtMost(count)
        } else {
            SentenceLimit::Exact(count)
        },
    )
}

pub(crate) fn register(value: &str) -> Option<WritingRegister> {
    Some(match value.to_ascii_lowercase().as_str() {
        "formal" => WritingRegister::Formal,
        "polite" => WritingRegister::Polite,
        "casual" | "friendly" => WritingRegister::Casual,
        _ => return None,
    })
}

pub(crate) fn omit_subject(value: &str) -> bool {
    [
        "no subject",
        "no subject line",
        "without a subject line",
        "omit the subject",
        "omit the subject line",
        "remove the subject",
        "remove the subject line",
        "do not include a subject line",
        "don't include a subject line",
    ]
    .iter()
    .any(|phrase| value.eq_ignore_ascii_case(phrase))
}

pub(crate) fn no_send(value: &str) -> bool {
    [
        "do not send it",
        "don't send it",
        "do not send",
        "don't send",
        "draft only",
    ]
    .iter()
    .any(|phrase| value.eq_ignore_ascii_case(phrase))
}

fn field(part: &str) -> Option<&str> {
    let part = part.trim();
    (!part.is_empty() && part.len() <= 512 && !part.contains([';', '；', '。', '？', '！']))
        .then_some(part)
}

fn strip_please(part: &str) -> &str {
    part.split_once(char::is_whitespace)
        .filter(|(word, _)| word.eq_ignore_ascii_case("please"))
        .map_or(part, |(_, remainder)| remainder.trim_start())
}

fn rest_after_ascii<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))?;
    Some(text[prefix.len()..].trim_start())
}

pub(crate) fn parse(raw: &str) -> Option<ParsedMessage> {
    static REQUEST: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)\A(?:write|draft|compose)\s+(?:an?\s+)?(?:(?P<register>formal|polite|casual|friendly)\s+)?",
            r"(?P<format>e-?mail|message)(?:\s+to\s+(?P<recipient>.+?))?\s+",
            r"(?:(?:asking|to ask)\s+(?:them|her|him|the recipient)\s+to\s+(?P<action>.+)|",
            r"(?:declining|to decline)\s+(?:the\s+|an?\s+)?invitation\s+to\s+(?P<occasion>.+))\z"
        ))
        .expect("English draft request")
    });
    static GRATITUDE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\A(?:thank (?:her|him|them|the recipient)|express (?:my )?(?:thanks|gratitude)) for (?:the invitation|inviting me)\z")
            .expect("English draft invitation gratitude")
    });
    static UNSCOPED_MODIFIER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)\b(?:body only|subject line|(?:(?:and|but)\s+(?:(?:also|then)\s+)?|then\s+)",
            r"(?:send|save|delete)\s+(?:it|this(?: (?:email|message))?|the (?:draft|email|message))|",
            r"(?:and|but)\s+(?:don't|do not)\s+(?:send|save|mention|include)|",
            r"without\s+(?:giving|showing|mentioning)\s+(?:a|the|any)\s+reason|",
            r"(?:in\s+(?:(?:exactly|at most|no more than)\s+)?|(?:keep it to|exactly|at most|no more than)\s+)",
            r"(?:[0-9]+|one|two|three|four|five|six|seven|eight|nine|ten)\s+sentences?)\b"
        ))
        .expect("unscoped English draft constraints")
    });
    static REASON_CLAUSE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)\A(?:(?:i|we|you|he|she|it|they)\s+\p{L}[\p{L}'’-]*|",
            r"(?:my|our|your|his|her|their|the|a|an)\s+[\p{L}\p{N} -]+?\s+",
            r"(?:is|are|was|were|has|have|had|will|can|cannot|conflicts?))\b(?:\s+.+)?\z"
        ))
        .expect("English draft reason clause")
    });
    let clauses = clauses(raw)?;
    let (first, rest) = clauses.split_first()?;
    let first = strip_please(first);
    let parts = REQUEST.captures(first)?;
    let mut roles = BTreeMap::new();
    let mut role = |key, value| {
        roles
            .entry(format!("/DraftMessage/{key}"))
            .or_insert_with(Vec::new)
            .push(original_span(raw, value));
    };
    let format = parts.name("format")?.as_str();
    role("format", format);
    let format = if format.eq_ignore_ascii_case("message") {
        MessageFormat::Message
    } else {
        MessageFormat::Email
    };
    let register = if let Some(value) = parts.name("register") {
        role("register", value.as_str());
        register(value.as_str())?
    } else {
        WritingRegister::Polite
    };
    let recipient = if let Some(value) = parts.name("recipient") {
        let part = field(value.as_str())?;
        if part.len() > 128 || UNSCOPED_MODIFIER.is_match(part) {
            return None;
        }
        role("recipient", part);
        Some(original_span(raw, part))
    } else {
        None
    };
    let mut purpose = if let Some(value) = parts.name("action") {
        let part = field(value.as_str())?;
        if UNSCOPED_MODIFIER.is_match(part) {
            return None;
        }
        role("purpose", part);
        MessagePurpose::Request {
            action: original_span(raw, part),
        }
    } else {
        let part = field(parts.name("occasion")?.as_str())?;
        if UNSCOPED_MODIFIER.is_match(part) || part.to_ascii_lowercase().contains(" because ") {
            return None;
        }
        role("purpose", part);
        role("occasion", part);
        MessagePurpose::DeclineInvitation {
            occasion: original_span(raw, part),
            gratitude: None,
        }
    };

    let mut reason = None;
    let mut omit_reason = false;
    let mut body_only = false;
    let mut omit_subject_line = false;
    let mut limit = None;
    for clause in rest.iter().map(|part| strip_please(part)) {
        let lowered = clause.to_ascii_lowercase();
        match lowered.as_str() {
            "body only" | "just the body" | "only the body" | "email body only" => body_only = true,
            "no reason"
            | "omit the reason"
            | "do not mention a reason"
            | "do not give a reason"
            | "do not mention the reason"
            | "don't give a reason"
            | "don't mention the reason"
            | "don't mention a reason"
            | "without giving a reason" => omit_reason = true,
            "in english" => {}
            _ if no_send(clause) => {}
            _ if omit_subject(clause) => omit_subject_line = true,
            _ if GRATITUDE.is_match(clause) => {
                let MessagePurpose::DeclineInvitation { gratitude, .. } = &mut purpose else {
                    return None;
                };
                if gratitude.is_some() {
                    return None;
                }
                *gratitude = Some(original_span(raw, clause));
                role("gratitude", clause);
            }
            _ => {
                if let Some(next) = sentence_limit(clause) {
                    if limit.is_some_and(|old| old != next) {
                        return None;
                    }
                    limit = Some(next);
                    role("sentence_count", clause);
                } else if let Some(detail) = rest_after_ascii(clause, "reason:") {
                    let detail = field(detail)?;
                    if reason.is_some()
                        || !REASON_CLAUSE.is_match(detail)
                        || UNSCOPED_MODIFIER.is_match(detail)
                    {
                        return None;
                    }
                    reason = Some(original_span(raw, detail));
                    role("reason", detail);
                } else {
                    return None;
                }
            }
        }
        role("constraints", clause);
    }
    Some(ParsedMessage {
        draft: MessageDraft {
            recipient,
            purpose,
            circumstance: None,
            reason,
            omit_reason,
            format,
            register,
            sentence_limit: limit,
            omit_subject: omit_subject_line,
            body_only,
        },
        roles,
    })
}
