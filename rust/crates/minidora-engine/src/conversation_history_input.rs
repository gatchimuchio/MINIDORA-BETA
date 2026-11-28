//! Extract a named conversation topic and a separate remark from the current turn.
use std::sync::LazyLock;

use minidora_domain::SourceSpan;
use regex::{Match, Regex};

use crate::input::source_span_at;

const MAX_REQUEST_BYTES: usize = 4096;
const MAX_TOPIC_CHARS: usize = 96;
const SPACING: [char; 2] = [' ', '　'];

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct NamedConversationReturn {
    pub(crate) topic: SourceSpan,
    pub(crate) remark: Option<SourceSpan>,
}

const SELECTOR: &str = concat!(
    r"\A[ 　]*(?:(?:(?:さっき|さきほど|先ほど|先程|この前|前回|以前|前)の|",
    r"(?:先ほど|先程|さっき|この前|以前に|前に)話した)[ 　]*)?",
    r"(?:「(?P<corner>[^「」『』]+)」|『(?P<double>[^「」『』]+)』|",
    r#"(?P<bare>[^「」『』"'“”‘’、，,。．.!！?？]+?))"#,
    r"[ 　]*の(?:話題|話)に",
);
const VERB: &str = concat!(
    r"(?P<verb>戻るんだけど|戻るんですけど|戻るのですが|戻るけど|戻りますが|",
    r"戻ってもいいですか|戻ってください|戻りたいです|戻りましょう|戻りたい|",
    r"戻れますか|戻ろう|戻ります|戻って|戻る)",
);

static NAMED_RETURN: LazyLock<Regex> = LazyLock::new(|| {
    let tail = r"(?:[ 　]*(?P<separator>[、，,。．.!！?？])(?P<remark>.*))?[ 　]*\z";
    Regex::new(&format!("{SELECTOR}{VERB}{tail}"))
        .expect("static named conversation return grammar")
});
static RETURN_START: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!("{SELECTOR}{VERB}")).expect("static named conversation return prefix")
});

pub(crate) fn has_return_prefix(text: &str) -> bool {
    RETURN_START.is_match(text)
}

fn source_phrase(raw: &str, phrase: Match<'_>) -> Option<SourceSpan> {
    let tail = phrase.as_str().trim_start_matches(SPACING);
    let phrase_text = tail.trim_end_matches(SPACING);
    let start = phrase.end() - tail.len();
    if phrase_text.is_empty() {
        return None;
    }
    source_span_at(raw, start, start + phrase_text.len())
}

pub(crate) fn parse(raw: &str) -> Option<NamedConversationReturn> {
    if raw.len() > MAX_REQUEST_BYTES || raw.chars().any(char::is_control) {
        return None;
    }
    let capture = NAMED_RETURN.captures(raw)?;
    let bare = capture.name("bare");
    let topic = source_phrase(
        raw,
        capture
            .name("corner")
            .or_else(|| capture.name("double"))
            .or(bare)?,
    )?;
    if topic.text.chars().count() > MAX_TOPIC_CHARS
        || (bare.is_some()
            && matches!(
                topic.text.as_str(),
                "こ" | "そ"
                    | "あ"
                    | "ど"
                    | "例"
                    | "別"
                    | "さっき"
                    | "さきほど"
                    | "先ほど"
                    | "先程"
                    | "この前"
                    | "前回"
                    | "以前"
                    | "前"
            ))
    {
        return None;
    }
    let verb = capture.name("verb")?.as_str();
    let remark = capture
        .name("remark")
        .and_then(|remark| source_phrase(raw, remark));
    let requires_remark = verb.ends_with("けど")
        || verb.ends_with('が')
        || capture
            .name("separator")
            .is_some_and(|separator| matches!(separator.as_str(), "、" | "，" | ","));
    if requires_remark && remark.is_none() {
        return None;
    }
    Some(NamedConversationReturn { topic, remark })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verify_span(raw: &str, span: &SourceSpan, expected: &str) {
        assert_eq!(span.text, expected);
        assert_eq!(raw.get(span.start..span.end), Some(expected));
    }

    #[test]
    fn named_conversation_return_keeps_exact_topic_and_remark_spans() {
        for (raw, topic, remark) in [
            (
                "　さっきの虫の声の話に戻るけど、今朝は聞こえませんでした。　",
                "虫の声",
                "今朝は聞こえませんでした。",
            ),
            (
                "前に話した「虫、鳥の声」の話題に戻るんだけど、 今朝は静かでした。",
                "虫、鳥の声",
                "今朝は静かでした。",
            ),
            (
                "『帰り道の話』の話に戻ろう。 今日は歩きました。　",
                "帰り道の話",
                "今日は歩きました。",
            ),
        ] {
            let parsed = parse(raw).unwrap();
            verify_span(raw, &parsed.topic, topic);
            verify_span(raw, parsed.remark.as_ref().unwrap(), remark);
        }
    }

    #[test]
    fn named_conversation_return_accepts_a_complete_selector_without_a_remark() {
        for raw in [
            "虫の声の話に戻ろう。",
            "先ほどの「虫の声」の話題に戻ってください。",
            "　『虫の声』の話に戻れますか？　",
        ] {
            let parsed = parse(raw).unwrap();
            verify_span(raw, &parsed.topic, "虫の声");
            assert!(parsed.remark.is_none());
        }
    }

    #[test]
    fn named_conversation_return_requires_a_named_topic_and_a_complete_transition() {
        for raw in [
            "その話に戻ろう。",
            "さっきの話に戻りたい。",
            "「虫の声の話に戻るけど、今朝は聞こえませんでした。",
            "虫の声の話に戻らない。",
            "虫の声の話に戻るけど",
            "虫の声の話に戻るけど、",
            "虫の声の話に戻ろう今朝は聞こえませんでした。",
            "先にファイルを閉じて。虫の声の話に戻ろう。",
            "虫の声の話に戻ろう。\n今朝は聞こえませんでした。",
        ] {
            assert!(parse(raw).is_none(), "{raw}");
        }
        assert!(
            parse(&format!(
                "{}の話に戻ろう。",
                "鳥".repeat(MAX_TOPIC_CHARS + 1)
            ))
            .is_none()
        );
    }
}
