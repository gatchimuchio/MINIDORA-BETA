//! Source-backed parsing of bounded choices. Core evaluates the sealed options.

use std::{collections::BTreeMap, sync::LazyLock};

use minidora_domain::{
    DecisionCandidate, DecisionChange, DecisionDirection, DecisionFact, DecisionPreference,
    DecisionRelation, DecisionRequest, DecisionRequirement, DecisionRevision, DecisionValue,
    Result, SourceSpan,
};
use regex::Regex;

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v221-compare-options";
pub(crate) const REVISION_PARSER_VERSION: &str = "declarative-ja-v223-decision-revision";
pub(crate) const ATTACHED_REVISION_PARSER_VERSION: &str =
    "declarative-ja-v252-attached-decision-revision";

pub(crate) struct ParsedDecision {
    pub(crate) decision: DecisionRequest,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

pub(crate) struct ParsedDecisionRevision {
    pub(crate) revision: DecisionRevision,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

pub(crate) struct ParsedAttachedDecisionRevision {
    pub(crate) candidate: SourceSpan,
    pub(crate) field: SourceSpan,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

type Roles = BTreeMap<String, Vec<SourceSpan>>;

const MAX_CANDIDATES: usize = 8;
const MAX_FACTS: usize = 12;
const MAX_REQUIREMENTS: usize = 6;
const MAX_PRIORITIES: usize = 4;
const LIST_SEPARATORS: [char; 3] = ['、', ',', '，'];

static QUANTITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?P<property>[\p{L}\p{N}_ー・-]{1,48}?)\s*(?:(?:は|が|[:：=＝])\s*)?",
        r"(?P<amount>[+＋\-−－]?(?:[0-9０-９]+(?:[.．][0-9０-９]+)?|[.．][0-9０-９]+)",
        r"(?:[eEｅＥ][+＋\-−－]?[0-9０-９]+|[/／][0-9０-９]+)?)",
        r"\s*(?P<unit>[\p{Han}\p{Katakana}\p{Latin}%％°℃μµΩ]{1,16})?$"
    ))
    .expect("static decision quantity grammar")
});

fn span(raw: &str, part: &str) -> SourceSpan {
    let start = part.as_ptr() as usize - raw.as_ptr() as usize;
    crate::input::source_span_at(raw, start, start + part.len())
        .expect("decision role is a slice of the original input")
}

fn role(roles: &mut Roles, key: String, value: &SourceSpan) {
    roles.entry(key).or_default().push(value.clone());
}

fn suffix<'a>(text: &'a str, endings: &[&str]) -> Option<(&'a str, &'a str)> {
    endings.iter().find_map(|ending| {
        text.strip_suffix(ending)
            .map(|stem| (stem, &text[stem.len()..]))
    })
}

fn label(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= 48
        && text.chars().any(char::is_alphabetic)
        && text
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '・' | 'ー'))
}

fn bare_property(text: &str) -> &str {
    text.trim_end()
        .trim_end_matches([':', '：', '=', '＝'])
        .trim_end()
}

fn flag(raw: &str, text: &str) -> Option<DecisionFact> {
    for (present, endings) in [
        (true, &["があります", "がある", "あり", "有り"][..]),
        (
            false,
            &["がありません", "ではない", "がない", "なし", "無し"][..],
        ),
    ] {
        if let Some((property, literal)) = suffix(text, endings) {
            let property = bare_property(property);
            let property = property.strip_suffix('は').unwrap_or(property).trim_end();
            if label(property) {
                return Some(DecisionFact {
                    property: span(raw, property),
                    value: DecisionValue::Flag {
                        present,
                        span: span(raw, literal),
                    },
                });
            }
        }
    }
    None
}

const COMPARISONS: [(&str, DecisionRelation); 10] = [
    ("より小さい", DecisionRelation::Less),
    ("より大きい", DecisionRelation::Greater),
    ("ちょうど", DecisionRelation::Equal),
    ("を超える", DecisionRelation::Greater),
    ("以上", DecisionRelation::AtLeast),
    ("以下", DecisionRelation::AtMost),
    ("以内", DecisionRelation::AtMost),
    ("未満", DecisionRelation::Less),
    ("超過", DecisionRelation::Greater),
    ("超", DecisionRelation::Greater),
];

fn quantity(raw: &str, text: &str) -> Option<DecisionFact> {
    let parts = QUANTITY.captures(text)?;
    let property = parts.name("property")?.as_str();
    let amount = parts.name("amount")?.as_str();
    let unit = parts.name("unit").map(|part| part.as_str());
    if !label(property)
        || unit.is_some_and(|unit| COMPARISONS.iter().any(|(end, _)| unit.ends_with(end)))
    {
        return None;
    }
    Some(DecisionFact {
        property: span(raw, property),
        value: DecisionValue::Quantity {
            amount: span(raw, amount),
            unit: unit.map(|unit| span(raw, unit)),
        },
    })
}

fn fact(raw: &str, text: &str) -> Option<DecisionFact> {
    if let Some((property, marker)) = suffix(text, &["不明", "未確認", "未記入"]) {
        let property = bare_property(property);
        let property = property
            .strip_suffix(['は', 'が'])
            .unwrap_or(property)
            .trim_end();
        if label(property) {
            return Some(DecisionFact {
                property: span(raw, property),
                value: DecisionValue::Unknown {
                    span: span(raw, marker),
                },
            });
        }
    }
    quantity(raw, text).or_else(|| flag(raw, text))
}

pub(crate) fn parse_attached_decision_value(
    raw_value: &str,
    value_start: usize,
) -> Option<DecisionValue> {
    let text = raw_value.trim();
    let start = value_start.checked_add(raw_value.len() - raw_value.trim_start().len())?;
    const PREFIX: &str = "値：";
    let synthetic = format!("{PREFIX}{text}");
    let parsed = fact(&synthetic, &synthetic)?;
    if parsed.property.text != "値" {
        return None;
    }
    let original_span = |value: SourceSpan| {
        let from = value.start.checked_sub(PREFIX.len())?;
        let to = value.end.checked_sub(PREFIX.len())?;
        if text.get(from..to)? != value.text {
            return None;
        }
        Some(SourceSpan {
            start: start.checked_add(from)?,
            end: start.checked_add(to)?,
            text: value.text,
        })
    };
    match parsed.value {
        DecisionValue::Quantity { amount, unit } => Some(DecisionValue::Quantity {
            amount: original_span(amount)?,
            unit: match unit {
                Some(unit) => Some(original_span(unit)?),
                None => None,
            },
        }),
        DecisionValue::Flag { present, span }
            if matches!(text, "あり" | "なし" | "有り" | "無し") =>
        {
            Some(DecisionValue::Flag {
                present,
                span: original_span(span)?,
            })
        }
        _ => None,
    }
}

fn parts(text: &str) -> impl Iterator<Item = &str> {
    text.split(LIST_SEPARATORS).map(str::trim)
}

fn candidate(raw: &str, text: &str) -> Option<DecisionCandidate> {
    let (name, values) = text
        .split_once('は')
        .or_else(|| text.split_once([':', '：']))?;
    let name = name.trim();
    if !label(name) {
        return None;
    }
    let mut facts = Vec::new();
    for value in parts(values) {
        if facts.len() == MAX_FACTS {
            return None;
        }
        facts.push(fact(raw, value)?);
    }
    Some(DecisionCandidate {
        name: span(raw, name),
        facts,
    })
}

fn value_roles(roles: &mut Roles, key: &str, value: &DecisionValue) {
    match value {
        DecisionValue::Quantity { amount, unit } => {
            role(roles, format!("{key}/amount"), amount);
            if let Some(unit) = unit {
                role(roles, format!("{key}/unit"), unit);
            }
        }
        DecisionValue::Flag { span, .. } => role(roles, format!("{key}/flag"), span),
        DecisionValue::Unknown { span } => role(roles, format!("{key}/unknown"), span),
    }
}

fn requirement(
    raw: &str,
    text: &str,
    default_marker: &str,
) -> Option<(DecisionRequirement, SourceSpan)> {
    for (ending, relation) in COMPARISONS {
        if let Some((body, marker)) = suffix(text, &[ending]) {
            let value = quantity(raw, body.trim_end())?;
            return Some((
                DecisionRequirement {
                    property: value.property,
                    relation,
                    expected: value.value,
                },
                span(raw, marker),
            ));
        }
    }
    let value = fact(raw, text)?;
    if matches!(value.value, DecisionValue::Unknown { .. }) {
        return None;
    }
    Some((
        DecisionRequirement {
            property: value.property,
            relation: DecisionRelation::Equal,
            expected: value.value,
        },
        span(raw, default_marker),
    ))
}

fn preference(raw: &str, text: &str) -> Option<(DecisionPreference, SourceSpan)> {
    let min = [
        "安い方",
        "短い方",
        "低い方",
        "少ない方",
        "小さい方",
        "軽い方",
        "早い方",
        "安い順",
        "短い順",
        "低い順",
        "少ない順",
        "小さい順",
        "軽い順",
        "早い順",
        "最小",
        "最小に",
        "最小化",
    ];
    let max = [
        "高い方",
        "長い方",
        "多い方",
        "大きい方",
        "重い方",
        "速い方",
        "高い順",
        "長い順",
        "多い順",
        "大きい順",
        "重い順",
        "速い順",
        "最大",
        "最大に",
        "最大化",
    ];
    for (direction, endings) in [
        (DecisionDirection::Minimize, min.as_slice()),
        (DecisionDirection::Maximize, max.as_slice()),
    ] {
        if let Some((property, surface)) = suffix(text, endings) {
            let property = property
                .trim_end()
                .strip_suffix(['が', 'は', 'を'])
                .unwrap_or(property)
                .trim_end();
            if label(property) {
                return Some((
                    DecisionPreference {
                        property: span(raw, property),
                        direction,
                    },
                    span(raw, surface),
                ));
            }
        }
    }
    None
}

fn advice(text: &str) -> bool {
    matches!(
        text,
        "候補を比較して"
            | "候補を比較してください"
            | "候補を比べて"
            | "候補を比べてください"
            | "比較して"
            | "比較してください"
            | "どれを選べばいい"
            | "どれを選べばいいか教えて"
            | "どれを選べばよいか教えて"
            | "どれがいい"
            | "どれがおすすめ"
            | "どちらがおすすめ"
    ) || suffix(
        text,
        &[
            "を選びたい",
            "の選択を手伝って",
            "について相談したい",
            "について相談です",
            "選びの相談です",
            "で相談",
            "の相談です",
        ],
    )
    .is_some_and(|(topic, _)| label(topic.trim()))
}

fn revision_context_with<'a>(text: &'a str, targets: &[&str]) -> Option<(&'a str, &'a str)> {
    let rest = ["前の", "前回の", "直前の", "さっきの", "先ほどの", "その"]
        .into_iter()
        .find_map(|prefix| text.strip_prefix(prefix))?;
    let rest = targets
        .iter()
        .find_map(|target| rest.strip_prefix(*target))?;
    let marker = &text[..text.len() - rest.len()];
    Some((
        marker,
        rest.trim_start()
            .strip_prefix(LIST_SEPARATORS)
            .unwrap_or(rest)
            .trim_start(),
    ))
}

fn revision_context(text: &str) -> Option<(&str, &str)> {
    revision_context_with(text, &["比較で", "相談で", "候補比較で"])
}

fn attachment_revision_source(text: &str) -> Option<(&str, &str)> {
    let rest = text.strip_prefix("添付")?;
    let rest = rest
        .strip_prefix('の')
        .or_else(|| rest.strip_prefix("した"))
        .unwrap_or(rest);
    let rest = ["更新された", "新しい", "最新の", "最新"]
        .into_iter()
        .find_map(|qualifier| rest.strip_prefix(qualifier))
        .unwrap_or(rest);
    let rest = ["資料", "案内", "文書", "ファイル", "お知らせ", "文面"]
        .into_iter()
        .find_map(|name| rest.strip_prefix(name))?;
    let tail = [
        "をもとに",
        "を基に",
        "に基づいて",
        "を根拠に",
        "を使って",
        "で",
    ]
    .into_iter()
    .find_map(|particle| rest.strip_prefix(particle))?;
    let marker = &text[..text.len() - tail.len()];
    let tail = tail.trim_start();
    Some((
        marker,
        tail.strip_prefix(LIST_SEPARATORS)
            .unwrap_or(tail)
            .trim_start(),
    ))
}

fn attachment_revision_target(body: &str) -> Option<(&str, &str)> {
    let (candidate, rest) = if let Some(quoted) = body.strip_prefix('「') {
        let (candidate, rest) = quoted.split_once('」')?;
        (candidate, rest.trim_start().strip_prefix('の')?)
    } else {
        body.split_once('の')?
    };
    let candidate = candidate.trim_end();
    let rest = rest.trim_start();
    let field = if let Some(quoted) = rest.strip_prefix('「') {
        quoted.strip_suffix('」')?
    } else {
        if rest.contains('の') {
            return None;
        }
        rest
    };
    (label(candidate) && label(field)).then_some((candidate, field))
}

pub(crate) fn parse_attached_revision(raw: &str) -> Option<ParsedAttachedDecisionRevision> {
    if raw.len() > 4096
        || raw.chars().any(char::is_control)
        || ["または", "もしくは", "あるいは", "又は"]
            .iter()
            .any(|marker| raw.contains(marker))
    {
        return None;
    }
    let text = raw
        .trim()
        .trim_end_matches(['。', '.', '！', '!'])
        .trim_end();
    let (source, rest) = attachment_revision_source(text)?;
    let (context, operation) =
        revision_context_with(rest, &["候補比較の", "候補比較で", "比較の", "比較で"])?;
    let (target, operation) = suffix(
        operation,
        &[
            "を更新してください",
            "を更新して",
            "を訂正してください",
            "を訂正して",
            "を修正してください",
            "を修正して",
        ],
    )?;
    let (candidate, field) = attachment_revision_target(target.trim_end())?;
    let candidate = span(raw, candidate);
    let field = span(raw, field);
    let mut roles = Roles::new();
    for (key, source) in [
        ("/ReviseDecisionFromAttachment/source", span(raw, source)),
        ("/ReviseDecisionFromAttachment/context", span(raw, context)),
        ("/ReviseDecisionFromAttachment/candidate", candidate.clone()),
        ("/ReviseDecisionFromAttachment/field", field.clone()),
        (
            "/ReviseDecisionFromAttachment/operation",
            span(raw, operation),
        ),
    ] {
        role(&mut roles, key.into(), &source);
    }
    Some(ParsedAttachedDecisionRevision {
        candidate,
        field,
        roles,
    })
}

fn explicit_revision_fact(raw: &str, text: &str) -> Option<DecisionFact> {
    let value = fact(raw, text)?;
    let text_end = span(raw, text).end;
    raw[value.property.end..text_end]
        .trim_start()
        .starts_with('は')
        .then_some(value)
}

fn revised_candidate_fact(raw: &str, body: &str, roles: &mut Roles) -> Option<DecisionChange> {
    let (name, property_value) = body.split_once('の')?;
    let name = name.trim_end();
    let (property, _) = property_value.split_once('は')?;
    // Multiple possessive boundaries can change which named candidate is
    // edited. Do not choose one without a unique syntactic target.
    if !label(name) || property.contains('の') {
        return None;
    }
    let candidate = span(raw, name);
    let fact = explicit_revision_fact(raw, property_value.trim())?;
    role(roles, "/ReviseDecision/fact/candidate".into(), &candidate);
    role(
        roles,
        "/ReviseDecision/fact/property".into(),
        &fact.property,
    );
    value_roles(roles, "/ReviseDecision/fact/value", &fact.value);
    Some(DecisionChange::Fact { candidate, fact })
}

fn revised_requirement(raw: &str, body: &str, roles: &mut Roles) -> Option<DecisionChange> {
    let comparison = COMPARISONS.into_iter().find_map(|(ending, relation)| {
        suffix(body, &[ending]).map(|(stem, literal)| (stem.trim_end(), literal, relation))
    });
    let (body, relation) = match comparison {
        Some((stem, literal, relation)) => {
            role(
                roles,
                "/ReviseDecision/requirement/relation".into(),
                &span(raw, literal),
            );
            (stem, Some(relation))
        }
        None => (body, None),
    };
    let fact = explicit_revision_fact(raw, body)?;
    match &fact.value {
        DecisionValue::Unknown { .. } => return None,
        DecisionValue::Flag { .. } if relation.is_some() => return None,
        _ => {}
    }
    role(
        roles,
        "/ReviseDecision/requirement/property".into(),
        &fact.property,
    );
    value_roles(roles, "/ReviseDecision/requirement/expected", &fact.value);
    Some(DecisionChange::Requirement {
        property: fact.property,
        relation,
        expected: fact.value,
    })
}

fn revised_preference(
    raw: &str,
    body: &str,
    only: bool,
    allow_implicit_direction: bool,
    roles: &mut Roles,
) -> Option<DecisionChange> {
    let (property, direction) = if let Some((preference, direction_span)) = preference(raw, body) {
        role(
            roles,
            "/ReviseDecision/preference/direction".into(),
            &direction_span,
        );
        (preference.property, Some(preference.direction))
    } else if allow_implicit_direction && label(body) {
        (span(raw, body), None)
    } else {
        return None;
    };
    role(
        roles,
        "/ReviseDecision/preference/property".into(),
        &property,
    );
    Some(DecisionChange::Preference {
        property,
        direction,
        only,
    })
}

pub(crate) fn parse_revision(raw: &str) -> Option<ParsedDecisionRevision> {
    if raw.len() > 4096
        || raw.chars().any(char::is_control)
        || raw.contains([
            '「', '」', '『', '』', '"', '\'', '“', '”', '‘', '’', '〈', '《', '>', '＞',
        ])
        || [
            "ではなく",
            "じゃなく",
            "しないで",
            "変更するな",
            "送信",
            "送って",
            "予約して",
            "購入して",
            "注文して",
            "支払って",
            "決済して",
            "実行して",
            "登録して",
            "申し込んで",
            "保存して",
            "削除して",
            "または",
            "もしくは",
            "あるいは",
        ]
        .iter()
        .any(|marker| raw.contains(marker))
    {
        return None;
    }
    let text = raw
        .trim()
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim_end();
    let mut roles = Roles::new();
    let (named, operation) = match revision_context(text) {
        Some((context, rest)) => {
            role(
                &mut roles,
                "/ReviseDecision/context".into(),
                &span(raw, context),
            );
            (true, rest)
        }
        None => (false, text),
    };
    let correction = ["訂正：", "訂正:"]
        .into_iter()
        .find_map(|marker| operation.strip_prefix(marker).map(str::trim_start));
    let body = correction.unwrap_or(operation);
    if body.contains(['、', ',', '，', '。', '！', '!', '？', '?', '；', ';']) {
        return None;
    }
    let (change, marker) = if correction.is_some() {
        let corrected = body.strip_suffix("です").unwrap_or(body).trim_end();
        let marker = &operation[..operation.len() - body.len()];
        (revised_candidate_fact(raw, corrected, &mut roles)?, marker)
    } else if let Some((body, marker)) = suffix(
        body,
        &[
            "に訂正してください",
            "に訂正して",
            "に修正してください",
            "に修正して",
        ],
    ) {
        (
            revised_candidate_fact(raw, body.trim_end(), &mut roles)?,
            marker,
        )
    } else if let Some((body, marker)) = suffix(body, &["に変更してください", "に変更して"])
    {
        (
            revised_requirement(raw, body.trim_end(), &mut roles)?,
            marker,
        )
    } else if let Some((body, marker)) = suffix(
        body,
        &[
            "だけを重視してください",
            "だけ重視してください",
            "だけを重視して",
            "だけ重視して",
            "だけを優先してください",
            "だけ優先してください",
            "だけを優先して",
            "だけ優先して",
        ],
    ) {
        (
            revised_preference(raw, body.trim_end(), true, true, &mut roles)?,
            marker,
        )
    } else if let Some((body, marker)) = suffix(
        body,
        &[
            "を優先してください",
            "を優先して",
            "を重視してください",
            "を重視して",
        ],
    ) {
        (
            revised_preference(raw, body.trim_end(), false, true, &mut roles)?,
            marker,
        )
    } else {
        let (body, marker) = suffix(body, &["してください", "して"])?;
        (
            revised_preference(raw, body.trim_end(), false, false, &mut roles)?,
            marker,
        )
    };
    role(
        &mut roles,
        "/ReviseDecision/operation".into(),
        &span(raw, marker),
    );
    Some(ParsedDecisionRevision {
        revision: DecisionRevision { named, change },
        roles,
    })
}

pub(crate) fn parse(raw: &str) -> Result<Option<ParsedDecision>> {
    if raw.len() > 8192
        || raw.contains([
            '「', '」', '『', '』', '"', '\'', '“', '”', '‘', '’', '〈', '《',
        ])
        || raw
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\r' | '\n'))
        || [
            "送信して",
            "送って",
            "実行して",
            "予約して",
            "注文して",
            "購入して",
            "買って",
            "支払って",
            "決済して",
            "削除して",
            "保存して",
            "契約して",
            "申し込んで",
            "登録して",
            "または",
            "もしくは",
            "あるいは",
            "又は",
        ]
        .iter()
        .any(|marker| raw.contains(marker))
    {
        return Ok(None);
    }
    let mut candidates = Vec::new();
    let mut requirements = Vec::new();
    let mut preferences = Vec::new();
    let mut roles = Roles::new();
    let mut advised = false;
    for clause in raw
        .split(['。', '\r', '\n', '；', ';', '！', '!', '？', '?'])
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
    {
        if advice(clause) {
            advised = true;
            role(
                &mut roles,
                "/CompareOptions/intent".into(),
                &span(raw, clause),
            );
        } else if let Some((body, marker)) = suffix(
            clause,
            &["を必須とする", "が必須", "は必須", "が必要", "が必要です"],
        ) {
            for item in parts(body)
                .flat_map(|part| part.split("かつ"))
                .flat_map(|part| part.split('と'))
            {
                if requirements.len() == MAX_REQUIREMENTS {
                    return Ok(None);
                }
                let Some((requirement, relation)) = requirement(raw, item.trim(), marker) else {
                    return Ok(None);
                };
                let key = format!("/CompareOptions/requirements/{}", requirements.len());
                role(&mut roles, format!("{key}/property"), &requirement.property);
                role(&mut roles, format!("{key}/relation"), &relation);
                value_roles(
                    &mut roles,
                    &format!("{key}/expected"),
                    &requirement.expected,
                );
                requirements.push(requirement);
            }
        } else if let Some((body, command)) = suffix(
            clause,
            &[
                "を選んでください",
                "を選んで",
                "で選んでください",
                "で選んで",
                "を優先してください",
                "を優先して",
                "を優先",
            ],
        ) {
            for item in parts(body) {
                if preferences.len() == MAX_PRIORITIES {
                    return Ok(None);
                }
                let continuation = [
                    "同じ場合は",
                    "同じなら",
                    "同額なら",
                    "同値なら",
                    "同点なら",
                    "同順位なら",
                    "その次に",
                    "次に",
                ]
                .iter()
                .find_map(|prefix| item.strip_prefix(prefix).map(|rest| (prefix, rest)));
                if preferences.is_empty() != continuation.is_none() {
                    return Ok(None);
                }
                let (preference_text, priority) = continuation.map_or((item, None), |(_, rest)| {
                    (rest.trim_start(), Some(&item[..item.len() - rest.len()]))
                });
                let Some((preference, direction)) = preference(raw, preference_text) else {
                    return Ok(None);
                };
                let key = format!("/CompareOptions/preferences/{}", preferences.len());
                role(&mut roles, format!("{key}/property"), &preference.property);
                role(&mut roles, format!("{key}/direction"), &direction);
                if let Some(priority) = priority {
                    role(&mut roles, format!("{key}/priority"), &span(raw, priority));
                }
                preferences.push(preference);
            }
            advised = true;
            role(
                &mut roles,
                "/CompareOptions/intent".into(),
                &span(raw, command),
            );
        } else {
            let Some(candidate) = candidate(raw, clause) else {
                return Ok(None);
            };
            let name = crate::decision::property_key(&candidate.name.text);
            let existing = candidates.iter().position(|earlier: &DecisionCandidate| {
                crate::decision::property_key(&earlier.name.text) == name
            });
            let first_fact = existing.map_or(0, |index| candidates[index].facts.len());
            if (existing.is_none() && candidates.len() == MAX_CANDIDATES)
                || first_fact + candidate.facts.len() > MAX_FACTS
            {
                return Ok(None);
            }
            let key = format!(
                "/CompareOptions/candidates/{}",
                existing.unwrap_or(candidates.len())
            );
            role(&mut roles, format!("{key}/name"), &candidate.name);
            for (index, fact) in candidate.facts.iter().enumerate() {
                let key = format!("{key}/facts/{}", first_fact + index);
                role(&mut roles, format!("{key}/property"), &fact.property);
                value_roles(&mut roles, &format!("{key}/value"), &fact.value);
            }
            if let Some(index) = existing {
                candidates[index].facts.extend(candidate.facts);
            } else {
                candidates.push(candidate);
            }
        }
    }
    Ok(
        (advised && candidates.len() >= 2 && (!requirements.is_empty() || !preferences.is_empty()))
            .then_some(ParsedDecision {
                decision: DecisionRequest {
                    candidates,
                    requirements,
                    preferences,
                },
                roles,
            }),
    )
}
