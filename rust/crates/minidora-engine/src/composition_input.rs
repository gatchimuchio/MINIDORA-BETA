//! Text boundaries and bounded item references inside a composed request.
use crate::input::{arithmetic_expression, calculation_step_references};
use minidora_domain::{Intent, Result, failure};
use regex::Regex;
use std::{ops::Range, sync::LazyLock};
use unicode_normalization::UnicodeNormalization;

const SELECTORS: &[&str] = &[
    "mean",
    "median",
    "sum",
    "min",
    "max",
    "population-variance",
    "sample-variance",
    "expected-value",
    "variance",
];

static ARITHMETIC_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?i)^(?:(?:please\s+)?(?:calculate|compute|evaluate)\s+|(?:計算してください|計算して|計算)[:：\s]+)(.+)$",
    )
    .expect("arithmetic request prefix is static")
});
static ARITHMETIC_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(.+?)(?:を)?\s*(?:計算してください|計算して|計算しろ|計算せよ|計算|はいくつですか|はいくつ|は何ですか|は何)$",
    )
    .expect("arithmetic request suffix is static")
});
static JAPANESE_OPERATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^依頼\s*(?P<ordinal>[0-9]+)\s*の\s*",
        r"(?P<field>結果|答え|平均値?|中央値|合計|最小値|最大値|母分散|標本分散|期待値|分散)\s*",
        r"(?P<from>から|に|と|を)\s*(?P<operand>[-+−]?[0-9]+(?:\.[0-9]+)?)\s*",
        r"(?P<to>を|で)?\s*",
        r"(?P<operation>足して|足す|加えて|引いて|引く|掛けて|掛ける|かけて|かける|割って|割る)(?:ください)?$"
    ))
    .expect("Japanese arithmetic reference grammar is static")
});
static JAPANESE_JSON_NUMBER_PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A依頼\s*[0-9０-９]*\s*の\s*(?i:JSON)\s*「")
        .expect("Japanese JSON-number prefix is static")
});
static JAPANESE_JSON_NUMBER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\A依頼\s*([0-9０-９]*)\s*の\s*(?i:JSON)\s*「([^「」\r\n]*)」\s*の\s*数値を取り出して(?:ください|下さい)?。?\z",
    )
    .expect("Japanese JSON-number grammar is static")
});
static JSON_POINTER: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\A(?:/(?:[^~/\p{Cc}]|~[01])*)*\z").expect("JSON pointer grammar is static")
});

fn invalid_reference() -> minidora_domain::Error {
    failure(
        "BRANCH_REFERENCE_INVALID",
        "数式には @{request-1} や @{request-1-mean} の形式で参照先を指定してください。",
    )
}

fn not_prior() -> minidora_domain::Error {
    failure(
        "BRANCH_REFERENCE_NOT_PRIOR",
        "今回の依頼で、この項目より前にある項目番号を指定してください。",
    )
}

fn prior_item(id: &str, item_index: usize) -> Result<String> {
    let tail = id.strip_prefix("request-").ok_or_else(invalid_reference)?;
    let (ordinal, selector) = tail
        .split_once('-')
        .map_or((tail, None), |(ordinal, selector)| {
            (ordinal, Some(selector))
        });
    if ordinal.is_empty() {
        return Err(not_prior());
    }
    if !ordinal.bytes().all(|byte| byte.is_ascii_digit())
        || (ordinal.len() > 1 && ordinal.starts_with('0'))
        || selector.is_some_and(|selector| !SELECTORS.contains(&selector))
    {
        return Err(invalid_reference());
    }
    let ordinal = ordinal.parse::<usize>().map_err(|_| not_prior())?;
    if ordinal == 0 || ordinal > item_index {
        return Err(not_prior());
    }
    Ok(format!("request-{ordinal}"))
}

fn invalid_json_reference() -> minidora_domain::Error {
    failure(
        "BRANCH_REFERENCE_INVALID",
        "先行項目とJSON Pointerを「json-number @{request-1} /value」の形式で指定してください。",
    )
}

fn json_selection(source: &str, pointer: &str, item_index: usize) -> Result<Intent> {
    let ordinal = source
        .strip_prefix("request-")
        .ok_or_else(invalid_json_reference)?;
    if ordinal.contains('-') || !JSON_POINTER.is_match(pointer) {
        return Err(invalid_json_reference());
    }
    Ok(Intent::SelectRequestJsonNumber {
        source: prior_item(source, item_index)?,
        pointer: pointer.to_owned(),
    })
}

fn parse_json_number(text: &str, item_index: usize) -> Result<Option<Intent>> {
    let mut tokens = text.split_whitespace();
    if tokens
        .next()
        .is_some_and(|command| command.eq_ignore_ascii_case("json-number"))
    {
        let source = tokens
            .next()
            .and_then(|token| token.strip_prefix("@{"))
            .and_then(|token| token.strip_suffix('}'))
            .ok_or_else(invalid_json_reference)?;
        let pointer = tokens.next().ok_or_else(invalid_json_reference)?;
        if tokens.next().is_some() || text.contains(['\r', '\n']) {
            return Err(invalid_json_reference());
        }
        let pointer = if pointer == "\"\"" { "" } else { pointer };
        return json_selection(source, pointer, item_index).map(Some);
    }
    if !JAPANESE_JSON_NUMBER_PREFIX.is_match(text) {
        return Ok(None);
    }
    if text.contains(['\r', '\n']) {
        return Err(invalid_json_reference());
    }
    let parts = JAPANESE_JSON_NUMBER
        .captures(text)
        .ok_or_else(invalid_json_reference)?;
    let ordinal: String = parts[1].nfkc().collect();
    json_selection(&format!("request-{ordinal}"), &parts[2], item_index).map(Some)
}

fn japanese_operation(text: &str) -> Result<Option<String>> {
    let Some(parts) = JAPANESE_OPERATION.captures(text) else {
        return Ok(None);
    };
    let to = parts.name("to").map(|part| part.as_str());
    let op = match (&parts["from"], to, &parts["operation"]) {
        ("に" | "と", None | Some("を"), "足して" | "足す" | "加えて") => "+",
        ("から", None | Some("を"), "引いて" | "引く") => "-",
        ("に" | "を", None | Some("を"), "掛けて" | "掛ける" | "かけて" | "かける") => {
            "*"
        }
        ("を", Some("で"), "割って" | "割る") => "/",
        _ => return Err(invalid_reference()),
    };
    let field = match &parts["field"] {
        "結果" | "答え" => "",
        "平均" | "平均値" => "-mean",
        "中央値" => "-median",
        "合計" => "-sum",
        "最小値" => "-min",
        "最大値" => "-max",
        "母分散" => "-population-variance",
        "標本分散" => "-sample-variance",
        "期待値" => "-expected-value",
        "分散" => "-variance",
        _ => return Err(invalid_reference()),
    };
    Ok(Some(format!(
        "@{{request-{}{field}}}{op}{}",
        &parts["ordinal"], &parts["operand"]
    )))
}

fn arithmetic_surface(text: &str) -> Option<&str> {
    if let Some(fields) = ARITHMETIC_PREFIX
        .captures(text)
        .or_else(|| ARITHMETIC_SUFFIX.captures(text))
    {
        return fields.get(1).map(|field| field.as_str().trim());
    }
    text.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_digit() || matches!(ch, '+' | '-' | '−' | '.' | '(' | '@'))
        .then_some(text)
}

/// Parse only a complete reference item. Referring to an item does not grant
/// permission to reinterpret any text produced by that item as an instruction.
pub(crate) fn parse(text: &str, item_index: usize) -> Result<Option<Intent>> {
    if let Some(intent) = parse_json_number(text.trim(), item_index)? {
        return Ok(Some(intent));
    }
    if text.contains(['\r', '\n']) {
        return Ok(None);
    }
    let normalized: String = text.nfkc().collect();
    let text = normalized
        .trim()
        .trim_end_matches(['。', '!', '?'])
        .trim_end();
    if text.contains([
        '"', '\x27', '\x60', '「', '」', '『', '』', '“', '”', '‘', '’',
    ]) {
        return Ok(None);
    }
    let natural = japanese_operation(text)?;
    let expression = match natural.as_deref() {
        Some(expression) => expression,
        None => {
            let Some(expression) = arithmetic_surface(text).filter(|text| text.contains('@'))
            else {
                return Ok(None);
            };
            expression
        }
    };
    let references = calculation_step_references(expression).map_err(|_| invalid_reference())?;
    if references.is_empty() {
        return Err(invalid_reference());
    }
    let mut requires = Vec::new();
    let mut rest = expression;
    let mut scalar_surface = String::new();
    let mut quantity_surface = String::new();
    for reference in references {
        let marker = format!("@{{{reference}}}");
        let (before, after) = rest.split_once(&marker).ok_or_else(invalid_reference)?;
        scalar_surface.push_str(before);
        scalar_surface.push('1');
        quantity_surface.push_str(before);
        quantity_surface.push_str("1 m");
        rest = after;
        let producer = prior_item(&reference, item_index)?;
        if !requires.contains(&producer) {
            requires.push(producer);
        }
    }
    scalar_surface.push_str(rest);
    quantity_surface.push_str(rest);
    if arithmetic_expression(&scalar_surface)?.as_deref() != Some(scalar_surface.as_str())
        && arithmetic_expression(&quantity_surface)?.as_deref() != Some(quantity_surface.as_str())
    {
        return Err(invalid_reference());
    }
    Ok(Some(Intent::CalculateFromRequestItems {
        expression: expression.into(),
        requires,
    }))
}

const MAX_INLINE_ITEMS: usize = 8;
const MAX_INLINE_ITEM_BYTES: usize = 8192;

// A boundary must identify a new request, rather than a sentence within one.
const INLINE_BOUNDARIES: &[&str] = &[
    "。次に、",
    "。そのあと、",
    "。その後、",
    "、次に、",
    "、そのあと、",
    "、その後、",
    ". then ",
    ". then, ",
    ", and then ",
    "; then ",
    "; then, ",
];

#[derive(Clone, Copy)]
enum InlineDelimiter {
    Quote(char),
    Group(char),
}

fn inline_quote_close(ch: char) -> Option<char> {
    Some(match ch {
        '「' => '」',
        '『' => '』',
        '“' => '”',
        '‘' => '’',
        '"' | '\'' => ch,
        _ => return None,
    })
}

fn inline_group_close(ch: char) -> Option<char> {
    Some(match ch {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        '（' => '）',
        '［' => '］',
        '｛' => '｝',
        '【' => '】',
        '〈' => '〉',
        '《' => '》',
        '〔' => '〕',
        _ => return None,
    })
}

fn inline_source_range(text: &str, start: usize, end: usize) -> Option<Range<usize>> {
    let source = text.get(start..end)?;
    let trimmed = source.trim_start();
    let item = trimmed.trim_end();
    if item.is_empty() || item.len() > MAX_INLINE_ITEM_BYTES {
        return None;
    }
    let start = end - trimmed.len();
    Some(start..start + item.len())
}

fn inline_markup(ch: char, before: &str, after: &str) -> bool {
    if matches!(ch, '\u{60}' | '<' | '>' | '\\') {
        return true;
    }
    if !matches!(ch, '*' | '_' | '~') {
        return false;
    }
    let previous = before.chars().next_back();
    let next = after.chars().next();
    if previous == Some(ch) || next == Some(ch) {
        return true;
    }
    let emphasis = previous.is_none_or(|ch| ch.is_whitespace() || ch.is_ascii_punctuation())
        && next.is_some_and(|ch| !ch.is_whitespace());
    let multiplication = ch == '*'
        && before
            .trim_end()
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_numeric() || matches!(ch, ')' | '）' | '}'))
        && after
            .trim_start()
            .trim_start_matches(['+', '-', '−'])
            .trim_start()
            .chars()
            .next()
            .is_some_and(|ch| ch.is_numeric() || matches!(ch, '(' | '（' | '.' | '@'));
    emphasis && !multiplication
}

/// Identify explicit sequencing cues in plain prose, using the authored byte ranges.
pub(crate) fn inline_ranges(text: &str) -> Option<Vec<Range<usize>>> {
    if text.len() > MAX_INLINE_ITEMS * (MAX_INLINE_ITEM_BYTES + 32)
        || text.trim().chars().any(char::is_control)
    {
        return None;
    }
    let trimmed = text.trim_start();
    if trimmed.starts_with(['#', '>'])
        || ["- ", "+ ", "* "]
            .iter()
            .any(|prefix| trimmed.starts_with(prefix))
    {
        return None;
    }
    let first = ["まず、", "first, "]
        .iter()
        .find(|marker| {
            trimmed
                .get(..marker.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(marker))
        })
        .map_or(trimmed, |marker| &trimmed[marker.len()..]);
    let origin = text.len() - first.len();
    let mut start = origin;
    let mut ranges = Vec::new();
    let mut delimiters = Vec::new();
    let mut chars = first.char_indices();
    while let Some((offset, ch)) = chars.next() {
        let index = origin + offset;
        if index < start {
            continue;
        }
        let before = &text[..index];
        let after = &text[index + ch.len_utf8()..];
        let word_apostrophe = matches!(ch, '\'' | '’')
            && before
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric)
            && after.chars().next().is_some_and(char::is_alphanumeric);

        if let Some(InlineDelimiter::Quote(close)) = delimiters.last().copied() {
            if ch == '\\' {
                chars.next()?;
            } else if ch == close && !word_apostrophe {
                delimiters.pop();
            } else if !matches!(close, '"' | '\'')
                && !word_apostrophe
                && let Some(close) = inline_quote_close(ch)
            {
                delimiters.push(InlineDelimiter::Quote(close));
            }
            continue;
        }
        if word_apostrophe {
            continue;
        }
        if let Some(close) = inline_quote_close(ch) {
            delimiters.push(InlineDelimiter::Quote(close));
            continue;
        }
        if let Some(close) = inline_group_close(ch) {
            delimiters.push(InlineDelimiter::Group(close));
            continue;
        }
        if matches!(
            ch,
            ')' | ']'
                | '}'
                | '）'
                | '］'
                | '｝'
                | '】'
                | '〉'
                | '》'
                | '〕'
                | '」'
                | '』'
                | '”'
                | '’'
        ) {
            if !matches!(delimiters.pop(), Some(InlineDelimiter::Group(close)) if close == ch) {
                return None;
            }
            continue;
        }
        if inline_markup(ch, before, after) {
            return None;
        }
        if delimiters.is_empty()
            && matches!(ch, '。' | '、' | '.' | ',' | ';')
            && let Some(marker) = INLINE_BOUNDARIES.iter().find(|marker| {
                text[index..]
                    .get(..marker.len())
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case(marker))
            })
        {
            if ranges.len() == MAX_INLINE_ITEMS - 1 {
                return None;
            }
            ranges.push(inline_source_range(text, start, index)?);
            start = index + marker.len();
        }
    }
    if ranges.is_empty() || !delimiters.is_empty() {
        return None;
    }
    ranges.push(inline_source_range(text, start, text.len())?);
    Some(ranges)
}

#[cfg(test)]
mod tests {
    use super::inline_ranges;

    #[test]
    fn inline_ranges_preserve_quoted_and_grouped_utf8() {
        for (text, expected) in [
            (
                "まず、「一杯です。次に、『二杯です』と言います。」を英訳して。次に、17 *23を計算して",
                [
                    "「一杯です。次に、『二杯です』と言います。」を英訳して",
                    "17 *23を計算して",
                ],
            ),
            (
                "First, Translate \"I won't arrive. Then, I will call.\" to Japanese. THEN, calculate 17*23",
                [
                    "Translate \"I won't arrive. Then, I will call.\" to Japanese",
                    "calculate 17*23",
                ],
            ),
            (
                "Use [\"I won't know. Then, ask\", [1; then 2]]. Then calculate (2 + 3)\n",
                [
                    "Use [\"I won't know. Then, ask\", [1; then 2]]",
                    "calculate (2 + 3)",
                ],
            ),
            (
                "Calculate 17+(2*3), and then calculate (@{request-1}+4)",
                ["Calculate 17+(2*3)", "calculate (@{request-1}+4)"],
            ),
        ] {
            let ranges = inline_ranges(text).unwrap();
            assert_eq!(
                ranges
                    .iter()
                    .map(|span| &text[span.clone()])
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }

    #[test]
    fn inline_ranges_decline_unbalanced_markup_and_plain_conjunctions() {
        for text in [
            "Calculate 17 and then calculate 23",
            "\"Calculate 17. Then calculate 23\"",
            "Translate 「一杯です。次に、2*3",
            "Calculate (17 + 3]. Then calculate 23",
            "\u{60}Calculate 17. Then calculate 23\u{60}",
            "# Calculate 17. Then calculate 23",
            "Please _calculate 17. Then calculate 23_",
            "**Calculate 17. Then calculate 23**",
            "1 *Calculate 17. Then calculate 23*",
            "Calculate 17. Then ",
        ] {
            assert!(inline_ranges(text).is_none(), "{text}");
        }
    }
}
