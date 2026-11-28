//! Bounded Japanese counterfactual questions. Hypotheses remain Input data;
//! Core owns the case projection and the effect of any temporary replacement.
use std::{collections::BTreeMap, sync::LazyLock};

use minidora_domain::{Intent, Proposition, Result, SourceSpan};
use regex::Regex;

use crate::input;

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v254-counterfactual-relation";

const ROOT: &str = "/AskCounterfactualRelation";
const MAX_REQUEST_BYTES: usize = 4096;
const MAX_ASSUMPTIONS: usize = 8;
const MAX_CONNECTORS: usize = 8;
const INTRODUCTIONS: &[&str] = &["もしも", "もし", "仮に"];
const TEMPORARY_SCOPES: &[&str] = &["この仮定の中でだけ", "この仮定の中だけ", "今回の仮定だけ"];
const TEMPORARY_REPLACEMENTS: &[&str] =
    &["保存した前提を仮に置き換えて", "元の前提を仮に置き換えて"];

type Roles = BTreeMap<String, Vec<SourceSpan>>;

static CONDITIONAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"という前提であれば|という前提ならば|という前提なら|",
        r"だと仮定した場合|と仮定した場合|だと仮定すると|と仮定すると|",
        r"だと仮定すれば|と仮定すれば|だったとしたら|",
        r"だとしたら|だとすると|だとすれば|だったら|としたら|とすると|",
        r"とすれば|であれば|であるなら|ならば|なら|の場合"
    ))
    .expect("static counterfactual conditional grammar")
});

static CONJUNCTION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:[、，,]\s*)?(?:かつ|および|及び)(?:\s*[、，,])?")
        .expect("static counterfactual conjunction grammar")
});

fn strip_particle(text: &str) -> &str {
    let text = text.trim_start();
    text.strip_prefix(['、', '，', ','])
        .map_or(text, str::trim_start)
}

fn strip_leader<'a>(text: &'a str, prefixes: &[&str]) -> Option<(&'a str, &'a str)> {
    prefixes.iter().find_map(|prefix| {
        text.strip_prefix(prefix)
            .map(|tail| (&text[..prefix.len()], tail))
    })
}

fn replacement_clause(text: &str) -> Option<(&str, &str)> {
    let remaining = if let Some((_, rest)) = strip_leader(text, TEMPORARY_SCOPES) {
        let (_, rest) = strip_leader(strip_particle(rest), &["元の前提", "保存した前提"])?;
        let (_, rest) = strip_leader(rest, &["を置き換えて", "を入れ替えて"])?;
        rest
    } else {
        let (_, rest) = strip_leader(text, TEMPORARY_REPLACEMENTS)?;
        rest
    };
    let cue = &text[..text.len() - remaining.len()];
    let remaining = remaining
        .trim_start()
        .strip_prefix(['、', '，', ','])?
        .trim_start();
    Some((cue, remaining))
}

fn introduction(text: &str) -> Option<(&str, &str)> {
    if text.starts_with("もしもし") || text.starts_with("もしか") {
        return None;
    }
    strip_leader(text, INTRODUCTIONS)
}

pub(crate) fn explicitly_hypothetical(raw: &str) -> bool {
    let text = raw.trim_start();
    let question = text.trim_end().trim_end_matches('。').trim_end();
    text.starts_with("仮に")
        || strip_leader(text, TEMPORARY_SCOPES).is_some()
        || strip_leader(text, TEMPORARY_REPLACEMENTS).is_some()
        || (introduction(text).is_some()
            && (question.ends_with(['？', '?']) || question_clause(question, false).is_some()))
}

fn question_clause(text: &str, has_mark: bool) -> Option<(&str, Option<&str>)> {
    for ending in [
        "とは言えますか",
        "と言えますか",
        "は成り立ちますか",
        "でしょうか",
        "なのか",
    ] {
        if let Some(prefix) = text.strip_suffix(ending) {
            return Some((prefix.trim_end(), Some(&text[prefix.len()..])));
        }
    }
    if let Some(prefix) = text.strip_suffix("ですか")
        && prefix.trim_end().ends_with('」')
    {
        return Some((prefix.trim_end(), Some(&text[prefix.len()..])));
    }
    if let Some(prefix) = text.strip_suffix('か')
        && [
            "です",
            "ます",
            "ません",
            "である",
            "ではない",
            "でない",
            "じゃない",
        ]
        .iter()
        .any(|ending| prefix.ends_with(ending))
    {
        return Some((prefix.trim_end(), Some(&text[prefix.len()..])));
    }
    has_mark.then_some((text, None))
}

fn atom_is_supported(term: &str) -> bool {
    term.chars()
        .all(|ch| ch.is_alphanumeric() || matches!(ch, ' ' | '　' | '_' | '-' | 'ー' | '・'))
        && ![
            "して",
            "しろ",
            "せよ",
            "してくれ",
            "してください",
            "して下さい",
            "くれ",
            "ください",
            "下さい",
            "なさい",
            "教えて",
            "書いて",
            "送って",
            "返せ",
        ]
        .iter()
        .any(|ending| term.ends_with(ending))
}

fn proposition_clause(fragment: &str) -> Result<Option<(Proposition, &str)>> {
    let fragment = fragment.trim();
    let statement = if let Some(quoted) = fragment.strip_prefix('「') {
        let Some(quoted) = quoted.strip_suffix('」') else {
            return Ok(None);
        };
        quoted.trim()
    } else {
        fragment
    };
    if statement.is_empty() || statement.contains(['「', '」', '。', '？', '?', '！', '!']) {
        return Ok(None);
    }
    let Some(proposition) = input::parse_proposition(statement, false)? else {
        return Ok(None);
    };
    if !crate::relations::is_ground(&proposition)
        || input::singular_previous_subject(&proposition.subject)
        || proposition.subject.contains('を')
        || [&proposition.subject, &proposition.object]
            .into_iter()
            .any(|term| matches!(term.as_str(), "X" | "Y" | "Z") || !atom_is_supported(term))
    {
        return Ok(None);
    }
    Ok(Some((proposition, statement)))
}

fn span(raw: &str, exact: &str) -> SourceSpan {
    let start = exact.as_ptr() as usize - raw.as_ptr() as usize;
    input::source_span_at(raw, start, start + exact.len())
        .expect("counterfactual role is an exact slice of the submitted text")
}

fn add_roles(
    raw: &str,
    statement: &str,
    proposition: &Proposition,
    path: &str,
    roles: &mut Roles,
) -> Result<bool> {
    let Some(relative) = input::counterfactual_proposition_roles(statement, proposition, path)?
    else {
        return Ok(false);
    };
    let clause_span = span(raw, statement);
    let offset = clause_span.start;
    roles.insert(path.into(), vec![clause_span]);
    for (key, mut spans) in relative {
        for span in &mut spans {
            span.start += offset;
            span.end += offset;
        }
        roles.insert(key, spans);
    }
    Ok(true)
}

fn interpretation(
    raw: &str,
    left: &str,
    right: &str,
    question_mark: Option<&str>,
) -> Result<Option<(Vec<Proposition>, Proposition, Roles)>> {
    let Some((target, question)) = question_clause(right, question_mark.is_some()) else {
        return Ok(None);
    };
    let parts: Vec<_> = CONJUNCTION.split(left).take(MAX_ASSUMPTIONS + 1).collect();
    if parts.len() > MAX_ASSUMPTIONS {
        return Ok(None);
    }
    let mut assumptions = Vec::with_capacity(parts.len());
    let mut roles = BTreeMap::new();
    for (index, part) in parts.into_iter().enumerate() {
        let Some((assumption, statement)) = proposition_clause(part)? else {
            return Ok(None);
        };
        if !add_roles(
            raw,
            statement,
            &assumption,
            &format!("{ROOT}/assumptions/{index}"),
            &mut roles,
        )? {
            return Ok(None);
        }
        assumptions.push(assumption);
    }
    if assumptions.len() > 1 {
        roles.insert(
            format!("{ROOT}/assumption_conjunction"),
            CONJUNCTION
                .find_iter(left)
                .map(|separator| span(raw, separator.as_str()))
                .collect(),
        );
    }
    let Some((target, statement)) = proposition_clause(target)? else {
        return Ok(None);
    };
    if !add_roles(
        raw,
        statement,
        &target,
        &format!("{ROOT}/proposition"),
        &mut roles,
    )? {
        return Ok(None);
    }
    let question_spans = question
        .into_iter()
        .chain(question_mark)
        .map(|part| span(raw, part))
        .collect();
    roles.insert(format!("{ROOT}/question"), question_spans);
    Ok(Some((assumptions, target, roles)))
}

pub(crate) fn parse(raw: &str) -> Result<Option<(Intent, Roles)>> {
    if raw.len() > MAX_REQUEST_BYTES || raw.chars().any(char::is_control) {
        return Ok(None);
    }
    let text = raw.trim();
    let text = text.strip_suffix('。').map_or(text, str::trim_end);
    let (text, question_mark) = if let Some(body) = text.strip_suffix(['？', '?']) {
        (body.trim_end(), Some(&text[body.len()..]))
    } else {
        (text, None)
    };
    // Most turns have no scenario connector. Keep them on their existing
    // parsing paths and avoid constructing any proposition roles.
    let (replacement, text) =
        replacement_clause(text).map_or((None, text), |(cue, remaining)| (Some(cue), remaining));
    let (intro, text) = introduction(text).map_or((None, text), |(cue, remaining)| {
        (Some(cue), strip_particle(remaining))
    });
    let connectors: Vec<_> = CONDITIONAL
        .find_iter(text)
        .take(MAX_CONNECTORS + 1)
        .collect();
    if connectors.len() > MAX_CONNECTORS {
        return Ok(None);
    }
    let mut found = None;
    for connector in connectors {
        let left = text[..connector.start()].trim_end();
        let right = strip_particle(&text[connector.end()..]);
        let Some((assumptions, proposition, mut roles)) =
            interpretation(raw, left, right, question_mark)?
        else {
            continue;
        };
        if found.is_some() {
            return Ok(None);
        }
        roles.insert(
            format!("{ROOT}/conditional"),
            vec![span(raw, connector.as_str())],
        );
        if let Some(intro) = intro {
            roles.insert(format!("{ROOT}/introduction"), vec![span(raw, intro)]);
        }
        if let Some(replacement) = replacement {
            roles.insert(
                format!("{ROOT}/replace_opposites"),
                vec![span(raw, replacement)],
            );
        }
        found = Some((
            Intent::AskCounterfactualRelation {
                assumptions,
                proposition,
                replace_opposites: replacement.is_some(),
            },
            roles,
        ));
    }
    Ok(found)
}
