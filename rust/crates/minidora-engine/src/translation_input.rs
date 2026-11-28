//! Source-preserving interpretation of explicit translation requests.
use std::collections::BTreeMap;

use minidora_domain::{
    Intent, Result, SourceSpan, TranslationLanguage, TranslationRegister, TranslationRequest,
};

use TranslationLanguage::{English, Japanese};
use TranslationRegister::{Plain, Polite, Preserve};

pub(crate) const PARSER_VERSION: &str = "declarative-ja-en-v244-translation";
pub(crate) const REVISION_PARSER_VERSION: &str = "declarative-ja-en-v245-translation-register";
pub(crate) const HISTORY_REVISION_PARSER_VERSION: &str =
    "declarative-ja-v1-translation-history-revision";

pub(crate) struct ParsedTranslation {
    pub(crate) intent: Intent,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

struct Command<'a> {
    target: TranslationLanguage,
    target_evidence: &'a str,
    source_evidence: Option<&'a str>,
    register: Option<(TranslationRegister, &'a str)>,
}

const REGISTERS: &[(&str, TranslationRegister)] = &[
    ("自然な文体で", Preserve),
    ("自然な", Preserve),
    ("自然に", Preserve),
    ("丁寧な文体で", Polite),
    ("ていねいな文体で", Polite),
    ("です・ます調で", Polite),
    ("丁寧な", Polite),
    ("丁寧に", Polite),
    ("敬体で", Polite),
    ("だ・である調で", Plain),
    ("くだけた文体で", Plain),
    ("常体で", Plain),
    ("常体の", Plain),
];

const JAPANESE_HISTORY_ANCHORS: &[&str] = &["前回の", "先ほどの", "さっきの", "前の"];
const JAPANESE_REVISION_SUBJECTS: &[&str] = &["翻訳結果を", "訳文を", "翻訳を", "訳を"];
const ENGLISH_HISTORY_ANCHORS: &[&str] = &[
    "the earlier translation",
    "the previous translation",
    "the last translation",
    "the translation from earlier",
];
const ENGLISH_IMMEDIATE_ANCHORS: &[&str] = &[
    "the immediately previous translation",
    "the immediately preceding translation",
    "the translation in your last reply",
    "the translation in my last reply",
    "the translation from your last reply",
    "the translation from my last reply",
    "that translation",
];
const ENGLISH_REVISION_REGISTERS: &[(&str, TranslationRegister)] = &[
    ("polite", Polite),
    ("more polite", Polite),
    ("formal", Polite),
    ("more formal", Polite),
    ("plain", Plain),
    ("casual", Plain),
    ("more casual", Plain),
    ("less formal", Plain),
];

pub(crate) fn parser_version(intent: &Intent) -> Option<&'static str> {
    match intent {
        Intent::TranslateText { .. } => Some(PARSER_VERSION),
        Intent::ReviseTranslation { .. } => Some(REVISION_PARSER_VERSION),
        _ => None,
    }
}

pub(crate) fn version_matches(version: &str, intent: &Intent) -> bool {
    match parser_version(intent) {
        Some(expected) => {
            expected == version
                || (version == HISTORY_REVISION_PARSER_VERSION
                    && matches!(intent, Intent::ReviseTranslation { .. }))
        }
        None => !matches!(
            version,
            PARSER_VERSION | REVISION_PARSER_VERSION | HISTORY_REVISION_PARSER_VERSION
        ),
    }
}

pub(crate) fn has_historical_revision_anchor(
    intent: &Intent,
    roles: &BTreeMap<String, Vec<SourceSpan>>,
) -> bool {
    matches!(intent, Intent::ReviseTranslation { .. })
        && roles
            .get("/ReviseTranslation/anchor")
            .is_some_and(|anchors| {
                matches!(anchors.as_slice(), [anchor] if JAPANESE_HISTORY_ANCHORS
                .iter()
                .any(|prefix| anchor.text.strip_prefix(*prefix)
                    .is_some_and(|subject| JAPANESE_REVISION_SUBJECTS.contains(&subject)))
                || ENGLISH_HISTORY_ANCHORS
                    .iter()
                    .any(|phrase| anchor.text.eq_ignore_ascii_case(phrase)))
            })
}

fn original_span(raw: &str, part: &str) -> SourceSpan {
    let start = part.as_ptr() as usize - raw.as_ptr() as usize;
    SourceSpan {
        start,
        end: start + part.len(),
        text: part.into(),
    }
}

fn terminal_trim(text: &str) -> &str {
    text.trim()
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim_end()
}

fn literal_prefix<'a>(text: &'a str, phrases: &[&str]) -> Option<&'a str> {
    phrases.iter().find_map(|phrase| text.strip_prefix(*phrase))
}

fn literal_suffix<'a>(text: &'a str, phrases: &[&str]) -> Option<&'a str> {
    phrases.iter().find_map(|phrase| text.strip_suffix(*phrase))
}

fn take_register(text: &str) -> (&str, Option<(TranslationRegister, &str)>) {
    REGISTERS
        .iter()
        .find_map(|(phrase, register)| {
            let rest = text.strip_prefix(*phrase)?;
            Some((rest.trim_start(), Some((*register, &text[..phrase.len()]))))
        })
        .unwrap_or((text, None))
}

fn japanese_command(text: &str, after_quote: bool) -> Option<Command<'_>> {
    let text = terminal_trim(text);
    let command = if after_quote {
        text.strip_prefix('を')?.trim_start()
    } else {
        literal_prefix(
            text,
            &[
                "次の文章を",
                "以下の文章を",
                "この文章を",
                "次のテキストを",
                "以下のテキストを",
                "次の文を",
                "以下の文を",
                "この文を",
                "次を",
                "以下を",
            ],
        )
        .unwrap_or(text)
        .trim_start()
    };
    let short_head = literal_suffix(command, &["してください", "して", "せよ"]).map(str::trim_end);
    let head = short_head
        .filter(|head| head.ends_with("英訳") || head.ends_with("和訳"))
        .or_else(|| {
            literal_suffix(
                command,
                &[
                    "翻訳してください",
                    "訳してください",
                    "翻訳して",
                    "訳して",
                    "翻訳せよ",
                    "訳せよ",
                ],
            )
            .map(str::trim_end)
        })
        .or(short_head)?;
    let (mut direction, mut register) = take_register(head);
    let explicit_source = [("日本語から", Japanese), ("英語から", English)]
        .into_iter()
        .find_map(|(phrase, language)| {
            let tail = direction.strip_prefix(phrase)?;
            Some((language, &direction[..phrase.len()], tail.trim_start()))
        });
    if let Some((_, _, tail)) = explicit_source {
        direction = tail;
    }
    let (without_register, leading_register) = take_register(direction);
    if leading_register.is_some() {
        if register.is_some() {
            return None;
        }
        register = leading_register;
        direction = without_register;
    }
    if let Some((inner, evidence)) = REGISTERS.iter().find_map(|(phrase, mode)| {
        let inner = direction.strip_suffix(*phrase)?;
        Some((inner.trim_end(), (*mode, &direction[inner.len()..])))
    }) {
        if register.is_some() {
            return None;
        }
        register = Some(evidence);
        direction = inner;
    }
    let target = match direction {
        "英訳" | "英語に" | "英語へ" | "英文に" => English,
        "和訳" | "日本語に" | "日本語へ" | "日本文に" => Japanese,
        _ => return None,
    };
    if explicit_source.is_some_and(|(language, _, _)| language == target) {
        return None;
    }
    Some(Command {
        target,
        target_evidence: direction,
        source_evidence: explicit_source.map(|(_, evidence, _)| evidence),
        register,
    })
}

fn ascii_prefix<'a>(text: &'a str, phrase: &str) -> Option<&'a str> {
    text.get(..phrase.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(phrase))
        .map(|_| &text[phrase.len()..])
}

fn english_command(text: &str) -> Option<Command<'_>> {
    let text = terminal_trim(text);
    let text = ascii_prefix(text, "please ").unwrap_or(text);
    let rest = ascii_prefix(text, "translate ")?;
    let rest = [
        "the following sentence ",
        "the following text ",
        "the following ",
        "this sentence ",
        "this text ",
        "this ",
    ]
    .into_iter()
    .find_map(|descriptor| ascii_prefix(rest, descriptor))
    .unwrap_or(rest)
    .trim_start();
    let rest = ascii_prefix(rest, "into ")
        .or_else(|| ascii_prefix(rest, "to "))?
        .trim_start();
    let (direction, register) = [("polite ", Polite), ("plain ", Plain)]
        .into_iter()
        .find_map(|(phrase, register)| {
            let remainder = ascii_prefix(rest, phrase)?;
            Some((
                remainder.trim_start(),
                Some((register, &rest[..phrase.trim_end().len()])),
            ))
        })
        .unwrap_or((rest, None));
    let target = if direction.eq_ignore_ascii_case("English") {
        English
    } else if direction.eq_ignore_ascii_case("Japanese") {
        Japanese
    } else {
        return None;
    };
    Some(Command {
        target,
        target_evidence: direction,
        source_evidence: None,
        register,
    })
}

fn quoted_parts(text: &str) -> Option<(&str, &str)> {
    let opening = text.chars().next()?;
    let closing = match opening {
        '「' => '」',
        '『' => '』',
        '"' => '"',
        '“' => '”',
        _ => return None,
    };
    text[opening.len_utf8()..].split_once(closing)
}

fn prefixed_parts(text: &str) -> Option<(&str, &str)> {
    let (boundary, character) = text
        .char_indices()
        .find(|(_, character)| matches!(character, ':' | '：' | '\r' | '\n'))?;
    let following = &text[boundary + character.len_utf8()..];
    let following = match character {
        '\r' => following.strip_prefix('\n').unwrap_or(following),
        ':' | '：' => {
            let line_break = following.trim_start_matches([' ', '\t', '\u{3000}']);
            line_break
                .strip_prefix("\r\n")
                .or_else(|| line_break.strip_prefix(['\r', '\n']))
                .unwrap_or(following)
        }
        _ => following,
    };
    Some((&text[..boundary], following))
}

fn make_translation(raw: &str, source: &str, command: Command<'_>) -> Option<ParsedTranslation> {
    if source.trim().is_empty()
        || source.len() > 4096
        || source
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\r' | '\n' | '\t'))
    {
        return None;
    }
    let source = original_span(raw, source);
    let mut roles = BTreeMap::from([
        ("/TranslateText/source".into(), vec![source.clone()]),
        (
            "/TranslateText/target_language".into(),
            vec![original_span(raw, command.target_evidence)],
        ),
    ]);
    if let Some(evidence) = command.source_evidence {
        roles.insert(
            "/TranslateText/source_language".into(),
            vec![original_span(raw, evidence)],
        );
    }
    if let Some((_, evidence)) = command.register {
        roles.insert(
            "/TranslateText/register".into(),
            vec![original_span(raw, evidence)],
        );
    }
    Some(ParsedTranslation {
        intent: Intent::TranslateText {
            request: TranslationRequest {
                source,
                source_language: match command.target {
                    Japanese => English,
                    English => Japanese,
                },
                target_language: command.target,
                register: command.register.map_or(Preserve, |(register, _)| register),
            },
        },
        roles,
    })
}

fn revise(raw: &str) -> Option<ParsedTranslation> {
    let text = terminal_trim(raw);
    let subject_end = ["前回の", "直前の", "先ほどの", "さっきの", "前の", "その"]
        .into_iter()
        .find_map(|prefix| {
            let tail = text.strip_prefix(prefix)?;
            literal_prefix(tail, JAPANESE_REVISION_SUBJECTS)
        })?;
    let style = literal_suffix(subject_end.trim_start(), &["にしてください", "にして"])?;
    let register = match style {
        "丁寧" | "ていねい" | "もっと丁寧" | "丁寧な言い方" | "です・ます調" | "ですます調"
        | "敬体" => Polite,
        "常体" | "だ・である調" | "である調" | "くだけた文体" => Plain,
        _ => return None,
    };
    Some(revised_translation(
        register,
        original_span(raw, &text[..text.len() - subject_end.len()]),
        original_span(raw, style),
    ))
}

fn revised_translation(
    register: TranslationRegister,
    anchor: SourceSpan,
    style: SourceSpan,
) -> ParsedTranslation {
    ParsedTranslation {
        intent: Intent::ReviseTranslation { register },
        roles: BTreeMap::from([
            ("/ReviseTranslation/anchor".into(), vec![anchor]),
            ("/ReviseTranslation/register".into(), vec![style]),
        ]),
    }
}

fn revise_english(raw: &str) -> Option<ParsedTranslation> {
    let body = terminal_trim(raw);
    if body.len() > 256 || body.chars().any(char::is_control) {
        return None;
    }
    let request = ["could you ", "can you ", "would you "]
        .iter()
        .find_map(|prefix| ascii_prefix(body, prefix))
        .unwrap_or(body)
        .trim_start();
    let request = ascii_prefix(request, "please ")
        .unwrap_or(request)
        .trim_start();
    let (target, make) = if let Some(target) = ascii_prefix(request, "make ") {
        (target.trim_start(), true)
    } else {
        let target = ["rewrite ", "revise ", "put "]
            .iter()
            .find_map(|verb| ascii_prefix(request, verb))?;
        (target.trim_start(), false)
    };
    let (anchor, modifier) = ENGLISH_HISTORY_ANCHORS
        .iter()
        .chain(ENGLISH_IMMEDIATE_ANCHORS)
        .find_map(|anchor| {
            let modifier = ascii_prefix(target, anchor)?.strip_prefix(' ')?;
            Some((&target[..anchor.len()], modifier.trim_start()))
        })?;
    let modifier = if make {
        modifier
    } else {
        ["in ", "into ", "to sound ", "to be "]
            .iter()
            .find_map(|phrase| ascii_prefix(modifier, phrase))?
            .trim_start()
    };
    let modifier = ascii_prefix(modifier, "a ")
        .unwrap_or(modifier)
        .trim_start();
    let style = [" style", " register", " language"]
        .iter()
        .find_map(|suffix| {
            let end = modifier.len().checked_sub(suffix.len())?;
            let trailing = modifier.get(end..)?;
            trailing
                .eq_ignore_ascii_case(suffix)
                .then(|| modifier[..end].trim_end())
        })
        .unwrap_or(modifier);
    let register = ENGLISH_REVISION_REGISTERS
        .iter()
        .find_map(|(phrase, register)| style.eq_ignore_ascii_case(phrase).then_some(*register))?;

    let start = raw.len() - raw.trim_start().len();
    let span = |offset, value: &str| SourceSpan {
        start: offset,
        end: offset + value.len(),
        text: value.into(),
    };
    Some(revised_translation(
        register,
        span(start + body.len() - target.len(), anchor),
        span(start + body.len() - modifier.len(), style),
    ))
}

pub(crate) fn parse(raw: &str) -> Result<Option<ParsedTranslation>> {
    let text = raw.trim_start();
    if text.len() > 4608 {
        return Ok(None);
    }
    if let Some((source, command)) = quoted_parts(text) {
        return Ok(japanese_command(command, true)
            .and_then(|command| make_translation(raw, source, command)));
    }
    if let Some((header, source)) = prefixed_parts(text)
        && let Some(command) = japanese_command(header, false).or_else(|| english_command(header))
    {
        // Explicit delimiters separate a quoted source from any outer instruction.
        // An unquoted body is entirely source data and cannot trigger actions.
        let trimmed_source = source.trim_start();
        if trimmed_source.starts_with(['「', '『', '"', '“']) {
            return Ok(quoted_parts(trimmed_source)
                .filter(|(_, tail)| terminal_trim(tail).is_empty())
                .and_then(|(source, _)| make_translation(raw, source, command)));
        }
        return Ok(make_translation(raw, source, command));
    }
    Ok(revise(raw).or_else(|| revise_english(raw)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_register_revisions_preserve_source_roles_and_reference_scope() {
        for (raw, anchor, style, register, historical) in [
            (
                "　前回の翻訳結果をもっと丁寧にしてください。",
                "前回の翻訳結果を",
                "もっと丁寧",
                Polite,
                true,
            ),
            (
                "先ほどの訳文を敬体にして",
                "先ほどの訳文を",
                "敬体",
                Polite,
                true,
            ),
            (
                "さっきの翻訳を常体にして",
                "さっきの翻訳を",
                "常体",
                Plain,
                true,
            ),
            ("前の訳を丁寧にして", "前の訳を", "丁寧", Polite, true),
            (
                "直前の訳文を常体にして",
                "直前の訳文を",
                "常体",
                Plain,
                false,
            ),
            (
                "その訳をくだけた文体にして",
                "その訳を",
                "くだけた文体",
                Plain,
                false,
            ),
        ] {
            check_revision(raw, anchor, style, register, historical);
        }
    }

    #[test]
    fn english_register_revisions_keep_exact_anchors_and_register_words() {
        for (raw, anchor, style, register, historical) in [
            (
                "  Please make the earlier translation more polite. ",
                "the earlier translation",
                "more polite",
                Polite,
                true,
            ),
            (
                "Rewrite the previous translation in a plain style.",
                "the previous translation",
                "plain",
                Plain,
                true,
            ),
            (
                "Can you make the LAST translation more casual?",
                "the LAST translation",
                "more casual",
                Plain,
                true,
            ),
            (
                "Could you please revise the translation from earlier to sound more formal?",
                "the translation from earlier",
                "more formal",
                Polite,
                true,
            ),
            (
                "Would you make that translation less formal?",
                "that translation",
                "less formal",
                Plain,
                false,
            ),
            (
                "Make the immediately previous translation formal.",
                "the immediately previous translation",
                "formal",
                Polite,
                false,
            ),
            (
                "Put the translation in my last reply into a casual register.",
                "the translation in my last reply",
                "casual",
                Plain,
                false,
            ),
        ] {
            check_revision(raw, anchor, style, register, historical);
        }
    }

    fn check_revision(
        raw: &str,
        anchor: &str,
        style: &str,
        register: TranslationRegister,
        historical: bool,
    ) {
        let parsed = parse(raw).unwrap().unwrap();
        assert_eq!(parsed.intent, Intent::ReviseTranslation { register });
        assert_eq!(
            parser_version(&parsed.intent),
            Some(REVISION_PARSER_VERSION)
        );
        assert!(version_matches(
            HISTORY_REVISION_PARSER_VERSION,
            &parsed.intent
        ));
        assert_eq!(
            has_historical_revision_anchor(&parsed.intent, &parsed.roles),
            historical
        );
        for (role, expected) in [
            ("/ReviseTranslation/anchor", anchor),
            ("/ReviseTranslation/register", style),
        ] {
            let [span] = parsed.roles[role].as_slice() else {
                panic!("one exact source span per role")
            };
            assert_eq!(span.text, expected);
            assert_eq!(&raw[span.start..span.end], expected);
        }
    }

    #[test]
    fn register_revision_rejects_mixed_requests_and_different_output_languages() {
        for raw in [
            "Rewrite the earlier translation in English.",
            "Make the earlier translation more polite in Japanese.",
            "Make the previous translation more formal and add an example.",
            "Rewrite that translation in a plain style without the last sentence.",
            "Don't make the last translation more formal.",
            "Is the earlier translation more polite?",
            "Please make the earlier translation more formal. Save it.",
        ] {
            assert!(parse(raw).unwrap().is_none(), "{raw}");
        }
        let original = parse("Translate into Japanese: We check the report.")
            .unwrap()
            .unwrap();
        assert!(version_matches(PARSER_VERSION, &original.intent));
        assert!(!version_matches(REVISION_PARSER_VERSION, &original.intent));
        assert!(!version_matches(
            HISTORY_REVISION_PARSER_VERSION,
            &original.intent
        ));
        assert!(!version_matches(
            HISTORY_REVISION_PARSER_VERSION,
            &Intent::Cancel
        ));
    }
}
