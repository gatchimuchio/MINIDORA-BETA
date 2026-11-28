//! Whole-clause edits to a previously rendered message.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use minidora_domain::{
    MessageFormat, MessageRevision, Result, SentenceLimit, SourceSpan, WritingRegister,
};
use regex::Regex;

use crate::writing::english_input;

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v220-message-revision";
pub(crate) const ENGLISH_PARSER_VERSION: &str = "declarative-en-v1-message-revision";

pub(crate) struct ParsedRevision {
    pub(crate) revision: MessageRevision,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

enum Edit {
    SentenceLimit(SentenceLimit),
    OmitSubject(bool),
    Register(WritingRegister),
    Shorten,
}

fn named_target(text: &str) -> Option<(Option<MessageFormat>, &str, &str)> {
    let referent = ["前回の", "直前の", "さっきの", "先ほどの", "前の", "その"]
        .into_iter()
        .find_map(|prefix| text.strip_prefix(prefix))?;
    for (noun, format) in [
        ("メール", Some(MessageFormat::Email)),
        ("メッセージ", Some(MessageFormat::Message)),
        ("連絡文", Some(MessageFormat::Message)),
        ("案内文", Some(MessageFormat::Message)),
        ("文案", None),
        ("下書き", None),
    ] {
        let Some(rest) = referent
            .strip_prefix(noun)
            .and_then(|rest| rest.strip_prefix('を'))
        else {
            continue;
        };
        let target_end = text.len() - rest.len() - 'を'.len_utf8();
        return Some((format, &text[..target_end], rest));
    }
    None
}

fn sentence_limit(clause: &str) -> Option<SentenceLimit> {
    let (number, ending) = clause.split_once('文')?;
    let number = match number {
        "1" | "１" | "一" => 1,
        "2" | "２" | "二" => 2,
        "3" | "３" | "三" => 3,
        _ => return None,
    };
    if let Some(ending) = ending.strip_prefix("以内") {
        return matches!(ending, "で" | "に" | "にして" | "に収めて")
            .then_some(SentenceLimit::AtMost(number));
    }
    let ending = ending.strip_prefix("ぴったり").unwrap_or(ending);
    matches!(ending, "で" | "に" | "にして").then_some(SentenceLimit::Exact(number))
}

fn edit(clause: &str) -> Option<Edit> {
    let clause = ["ください", "下さい", "お願いします"]
        .into_iter()
        .find_map(|suffix| clause.strip_suffix(suffix))
        .unwrap_or(clause)
        .trim_end();
    Some(match clause {
        "件名なし"
        | "件名なしで"
        | "件名なしにして"
        | "件名は不要"
        | "件名を外して"
        | "件名を省いて"
        | "件名を消して"
        | "件名をなくして"
        | "件名は入れないで" => Edit::OmitSubject(true),
        "件名あり"
        | "件名ありで"
        | "件名ありにして"
        | "件名を戻して"
        | "件名を入れて"
        | "件名を付けて"
        | "件名をつけて"
        | "件名も付けて"
        | "件名もつけて" => Edit::OmitSubject(false),
        "丁寧に" | "丁寧にして" | "ていねいに" | "ていねいにして" | "丁寧な文体で" => {
            Edit::Register(WritingRegister::Polite)
        }
        "フォーマルに" | "フォーマルにして" | "改まった文体で" | "改まった文体にして" => {
            Edit::Register(WritingRegister::Formal)
        }
        "カジュアルに" | "カジュアルにして" | "くだけた文体で" | "くだけた文体にして" => {
            Edit::Register(WritingRegister::Casual)
        }
        "短く"
        | "短くして"
        | "少し短くして"
        | "もう少し短くして"
        | "もっと短くして"
        | "簡潔に"
        | "簡潔にして" => Edit::Shorten,
        _ => Edit::SentenceLimit(sentence_limit(clause)?),
    })
}

fn apply_setting<T: Copy + Eq>(setting: &mut Option<T>, incoming: T) -> bool {
    if setting.is_some_and(|current| current != incoming) {
        return false;
    }
    *setting = Some(incoming);
    true
}

fn apply_edit(revision: &mut MessageRevision, edit: Edit) -> (&'static str, bool) {
    match edit {
        Edit::SentenceLimit(limit) => (
            "sentence_limit",
            apply_setting(&mut revision.sentence_limit, limit),
        ),
        Edit::OmitSubject(omit) => (
            "omit_subject",
            apply_setting(&mut revision.omit_subject, omit),
        ),
        Edit::Register(register) => ("register", apply_setting(&mut revision.register, register)),
        Edit::Shorten => {
            revision.shorten = true;
            ("shorten", true)
        }
    }
}

fn push_role(raw: &str, roles: &mut BTreeMap<String, Vec<SourceSpan>>, role: &str, part: &str) {
    let start = part.as_ptr() as usize - raw.as_ptr() as usize;
    roles
        .entry(format!("/ReviseMessage/{role}"))
        .or_default()
        .push(
            crate::input::source_span_at(raw, start, start + part.len())
                .expect("the revision field is a nonempty slice from the original input"),
        );
}

pub(crate) fn parse(raw: &str) -> Result<Option<ParsedRevision>> {
    if raw.len() > 4096
        || raw.contains(['「', '」', '『', '』', '"', '“', '”'])
        || raw.chars().any(char::is_control)
    {
        return Ok(None);
    }
    let text = raw
        .trim()
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim_end();
    let mut roles = BTreeMap::new();
    let (named, format, clauses) = match named_target(text) {
        Some((format, target, rest)) => {
            push_role(raw, &mut roles, "target", target);
            let earlier_draft = !target.starts_with("その") && !target.starts_with("直前の");
            (earlier_draft, format, rest)
        }
        None => (false, None, text),
    };
    let mut revision = MessageRevision {
        named,
        format,
        sentence_limit: None,
        omit_subject: None,
        register: None,
        shorten: false,
    };
    let mut has_edit = false;
    for clause in clauses
        .split(['、', '。', ',', '，', '；', ';'])
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
    {
        let Some(edit) = edit(clause) else {
            return Ok(None);
        };
        let (role, consistent) = apply_edit(&mut revision, edit);
        if !consistent {
            return Ok(None);
        }
        has_edit = true;
        push_role(raw, &mut roles, role, clause);
    }
    Ok(has_edit.then_some(ParsedRevision { revision, roles }))
}

fn edit_english(clause: &str) -> Option<Edit> {
    static REGISTER: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)\A(?:make it\s+)?(?:(?:more|in an?|with an?)\s+(?:more\s+)?)?",
            r"(?P<register>formal|polite|casual|friendly)(?:\s+tone)?\z"
        ))
        .expect("English message revision register")
    });
    if english_input::omit_subject(clause) {
        return Some(Edit::OmitSubject(true));
    }
    let lower = clause.to_ascii_lowercase();
    match lower.as_str() {
        "add a subject line" | "include a subject line" | "restore the subject line" => {
            Some(Edit::OmitSubject(false))
        }
        "shorter" | "more concise" | "make it shorter" | "make it more concise" | "shorten it" => {
            Some(Edit::Shorten)
        }
        _ => {
            if let Some(parts) = REGISTER.captures(clause) {
                return english_input::register(&parts["register"]).map(Edit::Register);
            }
            english_input::sentence_limit(clause).map(Edit::SentenceLimit)
        }
    }
}

pub(crate) fn parse_english(raw: &str) -> Option<ParsedRevision> {
    static TARGET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?i)\A(?:please\s+)?(?:make|revise|rewrite|edit|shorten)\s+",
            r"(?P<target>(?P<anchor>the\s+previous|the\s+earlier|the\s+last|(?:the\s+)?immediately\s+previous|that|this)\s+",
            r"(?P<format>e-?mail|message|draft))(?:\s+(?P<edit>.+))?\z"
        ))
        .expect("English message revision target")
    });
    static SHORTEN_TARGET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)\A(?:please\s+)?(?P<shorten>shorten)\s+")
            .expect("English message revision shortening command")
    });

    let clauses = english_input::clauses(raw)?;
    let (head, tail) = clauses.split_first()?;
    let parts = TARGET.captures(head);
    let mut roles = BTreeMap::new();
    let (named, format, first_edit, shortening) = if let Some(parts) = &parts {
        push_role(raw, &mut roles, "target", parts.name("target")?.as_str());
        let anchor = parts.name("anchor")?.as_str();
        let mut anchor_words = anchor.split_whitespace();
        let named = anchor_words
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("the"))
            && anchor_words.next().is_some_and(|word| {
                ["previous", "earlier", "last"]
                    .iter()
                    .any(|value| word.eq_ignore_ascii_case(value))
            });
        let format = match parts["format"].to_ascii_lowercase().as_str() {
            "email" | "e-mail" => Some(MessageFormat::Email),
            "message" => Some(MessageFormat::Message),
            _ => None,
        };
        let shortening = SHORTEN_TARGET
            .captures(head)
            .and_then(|parts| parts.name("shorten"))
            .map(|field| field.as_str());
        (
            named,
            format,
            parts.name("edit").map(|field| field.as_str()),
            shortening,
        )
    } else {
        let lower = head.to_ascii_lowercase();
        if !lower.starts_with("make it ") && lower != "shorten it" {
            return None;
        }
        (false, None, Some(*head), None)
    };
    let mut revision = MessageRevision {
        named,
        format,
        sentence_limit: None,
        omit_subject: None,
        register: None,
        shorten: shortening.is_some(),
    };
    if let Some(shorten) = shortening {
        push_role(raw, &mut roles, "shorten", shorten);
    }
    let mut has_edit = revision.shorten;
    for clause in first_edit.into_iter().chain(tail.iter().copied()) {
        let (role, consistent) = if english_input::no_send(clause) {
            ("constraints", true)
        } else {
            has_edit = true;
            apply_edit(&mut revision, edit_english(clause)?)
        };
        if !consistent {
            return None;
        }
        push_role(raw, &mut roles, role, clause);
    }
    has_edit.then_some(ParsedRevision { revision, roles })
}
