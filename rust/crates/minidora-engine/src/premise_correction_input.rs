//! Extract the correction cue and original replacement clause from one user turn.
use minidora_domain::SourceSpan;

use crate::input::source_span_at;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct PremiseCorrectionRequest {
    pub(crate) cue: SourceSpan,
    pub(crate) replacement: SourceSpan,
    pub(crate) explicit_previous: bool,
}

const PREVIOUS: &[&str] = &[
    "直前の",
    "先ほどの",
    "さきほどの",
    "先程の",
    "さっきの",
    "前の",
];
const TARGETS: &[&str] = &["前提を", "発言を", "記述を"];
const COMMANDS: &[&str] = &[
    "訂正してください",
    "訂正いたします",
    "訂正します",
    "訂正して",
    "訂正する",
];
const HORIZONTAL_SPACE: [char; 3] = [' ', '　', '\t'];
const SEPARATORS: [char; 8] = ['。', '．', '.', '、', '，', ',', '：', ':'];

fn after_separator(text: &str) -> Option<&str> {
    let text = text.trim_start_matches(HORIZONTAL_SPACE);
    text.strip_prefix(SEPARATORS)
        .or_else(|| text.strip_prefix("\r\n"))
        .or_else(|| text.strip_prefix('\n'))
}

fn cue_prefix(text: &str) -> Option<(&str, bool)> {
    let previous = PREVIOUS.iter().find_map(|prefix| {
        let after_prefix = text.strip_prefix(prefix)?;
        TARGETS
            .iter()
            .find_map(|target| after_prefix.strip_prefix(target))
    });
    let command = previous.unwrap_or(text);
    if let Some(tail) = COMMANDS.iter().find_map(|cue| command.strip_prefix(cue)) {
        // Reported or nominalized wording is ordinary text, even without quotes.
        if [
            "と言",
            "と伝",
            "と書",
            "と述",
            "と話",
            "という",
            "こと",
            "方法",
        ]
        .iter()
        .any(|marker| tail.starts_with(marker))
        {
            return None;
        }
        return Some((tail, previous.is_some()));
    }
    let tail = command.strip_prefix("訂正")?;
    if tail.trim().is_empty() || after_separator(tail).is_some() {
        Some((tail, previous.is_some()))
    } else {
        None
    }
}

/// Identifies malformed correction openings that must not reach assertion parsing.
pub(crate) fn has_correction_prefix(raw: &str) -> bool {
    let text = raw.trim_start();
    let text = text
        .strip_prefix("[scope=")
        .and_then(|scoped| scoped.split_once(']'))
        .map_or(text, |(_, body)| body.trim_start());
    cue_prefix(text).is_some()
}

pub(crate) fn parse(raw: &str) -> Option<PremiseCorrectionRequest> {
    if raw.contains(['?', '？']) {
        return None;
    }
    let text = raw.trim_start();
    let (tail, explicit_previous) = cue_prefix(text)?;
    let after_separator = after_separator(tail)?;
    let replacement = after_separator.trim();
    if replacement.is_empty()
        || replacement.starts_with(SEPARATORS)
        || replacement.chars().any(char::is_control)
    {
        return None;
    }
    let cue = source_span_at(
        raw,
        raw.len() - text.len(),
        raw.len() - after_separator.len(),
    )?;
    let replacement_start = raw.len() - after_separator.trim_start().len();
    let replacement = source_span_at(
        raw,
        replacement_start,
        replacement_start + replacement.len(),
    )?;
    Some(PremiseCorrectionRequest {
        cue,
        replacement,
        explicit_previous,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_prefix_preserves_original_utf8_spans() {
        for (raw, cue, replacement, explicit_previous) in [
            (
                "　訂正します。 砂浜倉庫は区分赤丙です。　",
                "訂正します。",
                "砂浜倉庫は区分赤丙です。",
                false,
            ),
            (
                "訂正: [scope=港] 砂浜倉庫は区分赤丙です。",
                "訂正:",
                "[scope=港] 砂浜倉庫は区分赤丙です。",
                false,
            ),
            (
                "先ほどの前提を訂正します。\n砂浜倉庫は区分赤丙です。",
                "先ほどの前提を訂正します。",
                "砂浜倉庫は区分赤丙です。",
                true,
            ),
            (
                "前の前提を訂正して、砂浜倉庫は区分赤丙です。",
                "前の前提を訂正して、",
                "砂浜倉庫は区分赤丙です。",
                true,
            ),
        ] {
            let parsed = parse(raw).expect("a complete correction prefix");
            for (span, expected) in [(&parsed.cue, cue), (&parsed.replacement, replacement)] {
                assert_eq!(span.text, expected);
                assert_eq!(raw.get(span.start..span.end), Some(expected));
            }
            assert_eq!(parsed.explicit_previous, explicit_previous);
        }
    }

    #[test]
    fn incomplete_correction_openings_do_not_fall_through() {
        for raw in [
            "訂正します",
            "訂正:",
            "訂正します砂浜倉庫は区分赤丙です。",
            "前の前提を訂正して、",
            " [scope=港] 訂正します。砂浜倉庫は区分赤丙です。",
            "訂正します。砂浜倉庫は区分赤丙ですか？",
        ] {
            assert!(has_correction_prefix(raw), "{raw}");
            assert!(parse(raw).is_none(), "{raw}");
        }
    }

    #[test]
    fn legacy_correction_and_ordinary_discussion_keep_their_own_entry_point() {
        for raw in [
            "砂浜倉庫は区分青ではなく区分赤丙です。",
            "訂正記事は資料です。",
            "「訂正します。」と伝えました。",
            "訂正しますと伝えました。",
            "前の発言を訂正する方法はありますか？",
        ] {
            assert!(!has_correction_prefix(raw), "{raw}");
            assert!(parse(raw).is_none(), "{raw}");
        }
    }
}
