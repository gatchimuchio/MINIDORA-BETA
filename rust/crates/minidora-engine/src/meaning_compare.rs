//! Bounded Japanese meaning-difference reports for explicitly labelled A/B text.
//!
//! This component reports only a small, inspectable set of linguistic markers.
//! It does not claim whole-sentence equivalence, infer omitted facts, or use a
//! score/model preference to resolve ambiguous scope.
use minidora_domain::{Result, failure};
use std::collections::BTreeMap;
use unicode_normalization::UnicodeNormalization;

type Features = BTreeMap<&'static str, String>;

const ORDER: &[&str] = &[
    "scope",
    "quantifier",
    "polarity",
    "modality",
    "condition",
    "agent",
    "recipient",
    "voice",
    "tense",
    "aspect",
    "time",
    "validity",
    "sequence",
    "attribution",
    "causality",
    "quantity",
    "deadline",
    "action",
];

pub fn compare(a: &str, b: &str) -> Result<String> {
    if a.is_empty()
        || b.is_empty()
        || a.len() > 8_192
        || b.len() > 8_192
        || a.contains('\0')
        || b.contains('\0')
    {
        return Err(failure(
            "MEANING_COMPARE_INPUT_LIMIT",
            "each labelled statement must contain 1..=8192 UTF-8 bytes and no NUL",
        ));
    }
    if a == b {
        return Ok(format!(
            "文Aと文Bは同じ文字列です。\n文A・文B: {a}\nこの比較では表記上の差はありません。"
        ));
    }

    let left = features(a);
    let right = features(b);
    let changes = ORDER
        .iter()
        .filter_map(|key| match (left.get(key), right.get(key)) {
            (Some(av), Some(bv)) if av != bv => Some((*key, av.clone(), bv.clone())),
            (None, Some(bv)) => Some((*key, absent_feature(key).into(), bv.clone())),
            (Some(av), None) => Some((*key, av.clone(), absent_feature(key).into())),
            _ => None,
        })
        .collect::<Vec<_>>();
    if changes.is_empty() {
        return Err(failure(
            "MEANING_COMPARE_UNRESOLVED",
            "the bounded feature reader found no comparable difference; no equivalence claim was made",
        ));
    }

    let mut out = format!("文A: {a}\n文B: {b}\n認識した意味の差:");
    for (key, av, bv) in changes {
        out.push_str(&format!(
            "\n- {}: 文Aは「{av}」、文Bは「{bv}」。",
            label(key)
        ));
    }
    let common = common_roles(&left, &right);
    if !common.is_empty() {
        out.push_str(&format!(
            "\n両方に共通して明示された要素: {}。",
            common.join("、")
        ));
    }
    out.push_str(
        "\nこの報告は認識できた限定的な特徴だけを比較しています。文全体の同値性や未認識語句の意味までは確認していません。",
    );
    Ok(out)
}

fn features(text: &str) -> Features {
    let mut map = Features::new();
    let modal_negation = [
        "必要はない",
        "必要がない",
        "必要ではない",
        "なければならない",
    ]
    .iter()
    .any(|needle| text.contains(needle));
    let has_negative = !modal_negation
        && [
            "わけではない",
            "とは限らない",
            "なかった",
            "しない",
            "ない",
            "ません",
        ]
        .iter()
        .any(|needle| text.contains(needle));
    map.insert(
        "polarity",
        if has_negative { "否定" } else { "肯定形" }.into(),
    );

    let scope = if text.contains("とは限らない") {
        Some("必ずそうだという断定を否定")
    } else if (text.contains("誰も") || text.contains("だれも")) && has_negative {
        Some("該当者が一人もいない")
    } else if text.contains("全員") && has_negative && !text.contains("わけではない") {
        Some("全員についての否定だが、否定の作用域は文脈なしでは曖昧")
    } else {
        None
    };
    if let Some(scope) = scope {
        map.insert("scope", scope.into());
    }

    if text.contains("わけではない") && text.contains("全員") {
        map.insert(
            "quantifier",
            "全員とは限らない（少なくとも一人は例外で、誰も回答しないとは限らない）".into(),
        );
    } else if text.contains("最大") || text.contains("多くても") {
        map.insert("quantifier", format!("{}（上限）", nearby_count(text)));
    } else if text.contains("より多") {
        map.insert(
            "quantifier",
            format!("{}より多い（整数なら次の人数以上）", nearby_count(text)),
        );
    } else if text.contains("最低") || text.contains("少なくとも") || text.contains("以上")
    {
        map.insert(
            "quantifier",
            format!("{}以上（下限、境界を含む）", nearby_count(text)),
        );
    } else if text.contains("のは") && text.contains("だけ") && text.contains("できる") {
        map.insert("quantifier", "その条件を満たす人に限定".into());
    } else if text.contains("だけ") || text.contains("のみ") {
        map.insert("quantifier", "それだけに限定（排他的な焦点）".into());
    } else if text.contains("も")
        && (text.contains("運ん") || text.contains("含") || text.contains("参加"))
    {
        map.insert("quantifier", "それも含む（ほかの対象を排除しない）".into());
    } else if text.contains("全員") || text.contains("すべて") || text.contains("全て") {
        map.insert("quantifier", "全員・全対象に及ぶ全称".into());
    } else if text.contains("毎回") {
        map.insert("quantifier", "各回で反復する".into());
    } else if text.contains("一部") || (text.contains("商品がある") || text.contains("人がいる"))
    {
        map.insert("quantifier", "少なくとも一部・一つの存在を述べる".into());
    } else if (text.contains("誰も") || text.contains("だれも")) && has_negative {
        map.insert("quantifier", "該当者なし".into());
    }

    if text.contains("必要はない") || text.contains("必要がない") {
        map.insert("modality", "義務・必要はない（禁止とは限らない）".into());
    } else if text.contains("なければならない") || text.contains("必要がある") {
        map.insert("modality", "義務・必要がある".into());
    } else if text.contains("てもよい") {
        map.insert("modality", "許可されている".into());
    } else if text.contains("可能性がある") {
        map.insert("modality", "起こる可能性がある".into());
    } else if text.contains("必ず") {
        map.insert("modality", "必ず起こるという断定".into());
    }

    if text.contains("ときに限り") {
        map.insert("condition", "この条件が成立に必要（逆向きの条件）".into());
    } else if text.contains("できるのは") && text.contains("だけ") {
        map.insert("condition", "条件を満たすことが許可の必要条件".into());
    } else if text.contains("なければ") && !text.contains("なければならない") {
        map.insert("condition", "連絡がない場合を条件とする".into());
    } else if text.contains("あれば") {
        map.insert("condition", "連絡がある場合を条件とする".into());
    } else if (text.contains("なら") && !text.contains("ならない"))
        || text.contains("すれば")
        || text.contains("であれば")
    {
        map.insert("condition", "前件が後件の十分条件（前件なら後件）".into());
    }

    let passive = ["によって", "された", "される", "られた", "られる"]
        .iter()
        .any(|needle| text.contains(needle));
    if let Some(action) = action(text) {
        map.insert("action", action.into());
        map.insert(
            "voice",
            if passive {
                "受動形"
            } else {
                "能動形・通常形"
            }
            .into(),
        );
        if passive {
            let explicit_agent = text
                .split_once("によって")
                .and_then(|(before, _)| last_role(before, &["は", "が", "を", "に", "へ"]));
            map.insert(
                "agent",
                explicit_agent
                    .unwrap_or("原文では行為者が明示されない")
                    .into(),
            );
            if let Some(patient) = last_role_before(text, "は") {
                map.insert("patient", patient.into());
            } else if let Some(patient) = last_role_before(text, "が") {
                map.insert("patient", patient.into());
            }
        } else {
            if let Some(agent) = active_agent(text) {
                map.insert("agent", agent.into());
            }
            if let Some(recipient) = text
                .split_once('が')
                .and_then(|(_, after_subject)| last_role_before(after_subject, "に"))
            {
                map.insert("recipient", recipient.into());
            }
        }
    }
    if let Some(object) = object(text) {
        map.insert("object", object.into());
    }

    if text.contains("去年") {
        map.insert("tense", "去年の一回の出来事".into());
    } else if text.contains("毎年") {
        map.insert("tense", "毎年の反復・習慣".into());
    }
    if text.contains("読み終えた") || text.contains("終えた") {
        map.insert("aspect", "動作の完了".into());
    } else if text.contains("読んでいる") || text.contains("進めている") {
        map.insert("aspect", "動作中・継続中".into());
    }
    if let Some((number, unit)) = number_and_unit(text, "時") {
        if text.contains("前に始まる") {
            map.insert("time", format!("{number}{unit}より前に開始"));
        } else if text.contains("から始まる") {
            map.insert("time", format!("{number}{unit}に開始"));
        }
    }
    if text.contains("から有効") {
        if let Some((month, _)) = number_and_unit(text, "月") {
            map.insert("validity", format!("{month}月から効力が始まる"));
        }
    } else if text.contains("まで有効")
        && let Some((month, _)) = number_and_unit(text, "月")
    {
        map.insert(
            "validity",
            format!("{month}月まで効力が続く（境界の厳密な扱いは文脈依存）"),
        );
    }
    if text.contains("点検後に電源") {
        map.insert("sequence", "点検の後に電源を入れる".into());
    } else if text.contains("電源を入れた後に点検") {
        map.insert("sequence", "電源を入れた後に点検する".into());
    }
    if text.contains("と述べた") || text.contains("と報告した") {
        map.insert("attribution", "引用・報告として話者に帰属する主張".into());
    } else if text.contains('「') && text.contains('」') {
        map.insert(
            "attribution",
            "引用された文言。真偽はこの引用だけでは確定しない".into(),
        );
    } else if text.contains('。')
        || text.contains('.')
        || text.contains("だ")
        || text.contains("です")
        || text.contains("だった")
    {
        map.insert("attribution", "話者が直接述べる断定".into());
    }
    if text.contains("ため") {
        map.insert("causality", "因果関係を明示".into());
    } else if text.contains("後") {
        map.insert(
            "causality",
            "時間的な後先のみを明示（因果は断定しない）".into(),
        );
    }
    if let Some((number, unit)) = number_and_unit(text, "点") {
        map.insert("quantity", format!("得点の絶対差 {number}{unit}"));
    } else if let Some((number, unit)) = number_and_unit(text, "%") {
        map.insert("quantity", format!("基準値に対する相対変化 {number}{unit}"));
    }
    if text.contains("営業日") {
        map.insert("deadline", "営業日で数える期限".into());
    } else if text.contains("以内に返信") || text.contains("以内に返事") {
        map.insert("deadline", "営業日とは指定されていない日数期限".into());
    }

    // A simple explicit topic is retained for common-element reporting only.
    // It is not treated as a semantic difference from grammatical subject.
    let subject_source = if text.contains("と述べた") || text.contains("と報告した") {
        text.split_once('「')
            .and_then(|(_, rest)| rest.split_once('」').map(|(quoted, _)| quoted))
            .unwrap_or(text)
    } else {
        text
    };
    if let Some(topic) = topic(subject_source) {
        map.insert("topic", topic.into());
    }
    map
}

fn action(text: &str) -> Option<&'static str> {
    action_frame(text).map(|(_, label)| label)
}

fn action_frame(text: &str) -> Option<(usize, &'static str)> {
    [
        ("承認", "承認"),
        ("設置", "設置"),
        ("説明", "説明"),
        ("電話", "電話"),
        ("渡し", "受け渡し"),
        ("提出", "提出"),
        ("回答", "回答"),
        ("返却", "返却"),
        ("転倒", "転倒"),
        ("開始", "開始"),
        ("運ん", "運搬"),
        ("読み", "読書"),
        ("読ん", "読書"),
    ]
    .into_iter()
    .filter_map(|(needle, label)| text.find(needle).map(|position| (position, label)))
    .min_by_key(|(position, _)| *position)
}

fn action_position(text: &str) -> Option<usize> {
    action_frame(text).map(|(position, _)| position)
}

fn active_agent(text: &str) -> Option<&str> {
    if text.contains("誰も") {
        return Some("誰も");
    }
    let prefix = &text[..action_position(text)?];
    let before_object = prefix.split_once('を').map_or(prefix, |(before, _)| before);
    let marker = ["が", "は"]
        .into_iter()
        .filter_map(|particle| {
            before_object
                .match_indices(particle)
                .map(|(position, _)| (position, particle))
                .last()
        })
        .max_by_key(|(position, _)| *position)?;
    let actor = before_object[..marker.0]
        .rsplit(['。', '、'])
        .next()
        .map(str::trim)?;
    (!actor.is_empty()).then_some(actor)
}

fn object(text: &str) -> Option<&str> {
    let prefix = &text[..action_position(text)?];
    if let Some((before, _)) = prefix.split_once('を') {
        let value = before
            .rsplit(['。', '、'])
            .next()
            .map(trim_condition_prefix)
            .and_then(|phrase| last_role(phrase, &["は", "が", "に", "へ", "と", "で"]))?;
        return (!value.is_empty()).then_some(value);
    }
    for particle in ["だけ", "のみ", "も"] {
        if let Some(position) = prefix.rfind(particle) {
            let value = prefix[..position]
                .rsplit(['。', '、', 'は', 'が', 'に', 'へ', 'と'])
                .next()
                .map(trim_focus)?
                .trim();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    None
}

fn trim_condition_prefix(value: &str) -> &str {
    [
        "なければ",
        "あれば",
        "すれば",
        "ならば",
        "なら",
        "たら",
        "れば",
    ]
    .into_iter()
    .filter_map(|marker| value.rfind(marker).map(|position| position + marker.len()))
    .max()
    .map_or(value, |position| &value[position..])
}

fn topic(text: &str) -> Option<&str> {
    let position = text.match_indices('は').find_map(|(position, _)| {
        let previous = text[..position].chars().next_back()?;
        (!matches!(previous, 'で' | '要')).then_some(position)
    })?;
    let value = text[..position].rsplit(['。', '、']).next()?.trim();
    (!value.is_empty()).then_some(value)
}

fn nearby_count(text: &str) -> String {
    let normalized = text.nfkc().collect::<String>();
    for unit in ['人', '回'] {
        for (position, _) in normalized
            .match_indices(unit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if let Some(value) = numeric_suffix(&normalized[..position]) {
                return format!("{value}{unit}");
            }
        }
    }
    let Some(suffix) = normalized
        .char_indices()
        .find(|(_, character)| is_numeric_character(*character))
        .map(|(position, _)| &normalized[position..])
    else {
        return "指定値".into();
    };
    numeric_suffix(suffix).unwrap_or_else(|| "指定値".into())
}

fn number_and_unit(text: &str, unit: &str) -> Option<(String, &'static str)> {
    let label = match unit {
        "点" => "点",
        "%" => "%",
        "時" => "時",
        "月" => "月",
        _ => return None,
    };
    let normalized = text.nfkc().collect::<String>();
    for (position, _) in normalized
        .match_indices(unit)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if let Some(number) = numeric_suffix(&normalized[..position]) {
            return Some((number, label));
        }
    }
    None
}

fn numeric_suffix(text: &str) -> Option<String> {
    let normalized = text.nfkc().collect::<String>();
    let suffix = normalized
        .chars()
        .rev()
        .take_while(|character| is_numeric_character(*character))
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    if suffix.is_empty() {
        return None;
    }
    if suffix
        .chars()
        .all(|character| character.is_ascii_digit() || character == '.')
    {
        if !suffix.chars().any(|character| character.is_ascii_digit())
            || suffix.matches('.').count() > 1
            || suffix
                .split_once('.')
                .is_some_and(|(whole, fraction)| whole.is_empty() && fraction.is_empty())
        {
            return None;
        }
        return Some(suffix);
    }
    if suffix.contains('.') {
        return None;
    }
    kanji_integer(&suffix).map(|value| value.to_string())
}

fn is_numeric_character(character: char) -> bool {
    character.is_ascii_digit()
        || character == '.'
        || digit_value(character).is_some()
        || matches!(character, '十' | '百' | '千' | '万' | '億' | '兆')
}

fn digit_value(character: char) -> Option<u64> {
    character.to_digit(10).map(u64::from).or(match character {
        '〇' | '零' => Some(0),
        '一' => Some(1),
        '二' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    })
}

fn kanji_integer(text: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut section = 0u64;
    let mut number = 0u64;
    let mut saw_number = false;
    for character in text.chars() {
        if let Some(digit) = digit_value(character) {
            number = number.checked_mul(10)?.checked_add(digit)?;
            saw_number = true;
            continue;
        }
        let unit = match character {
            '十' => 10,
            '百' => 100,
            '千' => 1_000,
            '万' => 10_000,
            '億' => 100_000_000,
            '兆' => 1_000_000_000_000,
            _ => return None,
        };
        saw_number = true;
        if unit < 10_000 {
            let multiplier = if number == 0 { 1 } else { number };
            section = section.checked_add(multiplier.checked_mul(unit)?)?;
            number = 0;
        } else {
            let multiplier = section.checked_add(number)?;
            total = total
                .checked_add((if multiplier == 0 { 1 } else { multiplier }).checked_mul(unit)?)?;
            section = 0;
            number = 0;
        }
    }
    saw_number
        .then(|| total.checked_add(section)?.checked_add(number))
        .flatten()
}

fn last_role_before<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let (before, _) = text.split_once(marker)?;
    last_role(
        before,
        &[
            "なければ",
            "あれば",
            "すれば",
            "ならば",
            "なら",
            "たら",
            "れば",
            "とき",
            "、",
            "は",
            "が",
            "を",
            "に",
            "へ",
            "と",
        ],
    )
}

fn last_role<'a>(text: &'a str, markers: &[&str]) -> Option<&'a str> {
    let start = markers
        .iter()
        .filter_map(|marker| text.rfind(marker).map(|position| position + marker.len()))
        .max()
        .unwrap_or(0);
    let value = trim_focus(text[start..].trim());
    (!value.is_empty()).then_some(value)
}

fn trim_focus(value: &str) -> &str {
    value
        .trim_start_matches(['「', '『', '"'])
        .trim_end_matches(['」', '』', '"'])
        .trim_end_matches("だけ")
        .trim_end_matches("のみ")
        .trim_end_matches('も')
        .trim()
}

fn common_roles(left: &Features, right: &Features) -> Vec<String> {
    let mut values = std::collections::BTreeSet::new();
    for key in ["agent", "recipient", "topic", "action"] {
        if let (Some(a), Some(b)) = (left.get(key), right.get(key))
            && a == b
            && !a.contains("明示されない")
        {
            values.insert(a.clone());
        }
    }
    let left_patient = left.get("patient").or_else(|| left.get("object"));
    let right_patient = right.get("patient").or_else(|| right.get("object"));
    if let (Some(a), Some(b)) = (left_patient, right_patient)
        && a == b
    {
        values.insert(a.clone());
    }
    values.into_iter().collect()
}

fn label(key: &str) -> &'static str {
    match key {
        "scope" => "否定・範囲",
        "quantifier" => "量化・数量範囲",
        "polarity" => "肯否",
        "modality" => "可能性・義務・許可",
        "condition" => "条件の向き",
        "agent" => "行為者",
        "recipient" => "相手役割",
        "topic" => "話題",
        "patient" | "object" => "対象",
        "voice" => "能動・受動",
        "tense" => "時制・反復",
        "aspect" => "動作の完了・進行",
        "time" => "時刻",
        "validity" => "有効期間",
        "sequence" => "動作の順序",
        "attribution" => "発言の帰属",
        "causality" => "因果と時間順序",
        "quantity" => "数値の意味",
        "deadline" => "期限の数え方",
        "action" => "行為",
        _ => "意味特徴",
    }
}

fn absent_feature(key: &str) -> &'static str {
    match key {
        "scope" => "明示的な否定・例外表現なし",
        "quantifier" => "明示的な数量限定なし",
        "polarity" => "否定表現なし（肯定形）",
        "modality" => "義務・許可・可能性の表現なし",
        "condition" => "条件表現なし",
        "agent" => "行為者は明示されない",
        "recipient" => "相手役割は明示されない",
        "voice" => "対応する行為の形を特定できない",
        "tense" => "明示的な時制・反復表現なし",
        "aspect" => "完了・進行表現なし",
        "time" => "時刻表現なし",
        "validity" => "有効期間表現なし",
        "sequence" => "動作順序表現なし",
        "attribution" => "明示的な引用・帰属表現なし",
        "causality" => "因果・時間順序表現なし",
        "quantity" => "比較可能な数値表現なし",
        "deadline" => "期限表現なし",
        "action" => "対応する行為を特定できない",
        _ => "明示的な特徴なし",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_partial_negation_from_none_and_preserves_ambiguity() {
        let not_all = compare("全員が回答した。", "全員が回答したわけではない。").unwrap();
        assert!(not_all.contains("全員とは限らない"));
        assert!(not_all.contains("文Aは「全員・全対象に及ぶ全称」"));
        let ambiguity = compare("誰も回答しなかった。", "全員が回答しなかった。").unwrap();
        assert!(ambiguity.contains("作用域は文脈なしでは曖昧"));
    }

    #[test]
    fn compares_participant_roles_and_active_passive_wording() {
        let roles = compare("玲奈が健に本を渡した。", "健が玲奈に本を渡した。").unwrap();
        assert!(roles.contains("文Aは「玲奈」"));
        assert!(roles.contains("文Bは「健」"));
        let voice = compare("住民が看板を設置した。", "看板は住民によって設置された。").unwrap();
        assert!(voice.contains("能動形"));
        assert!(voice.contains("受動形"));
        assert!(voice.contains("住民"));
        assert!(voice.contains("看板"));
    }

    #[test]
    fn distinguishes_condition_direction_attribution_cause_and_units() {
        let bound = compare("参加者は3人以上だった。", "参加者は3人より多かった。").unwrap();
        assert!(bound.contains("3人より多い"));
        assert!(
            compare("雨なら試合を中止する。", "雨のときに限り試合を中止する。")
                .unwrap()
                .contains("十分条件")
        );
        assert!(
            compare(
                "田中さんは「この装置は安全だ」と述べた。",
                "この装置は安全だ。"
            )
            .unwrap()
            .contains("帰属する主張")
        );
        let quote = compare(
            "田中さんは「この装置は安全だ」と述べた。",
            "この装置は安全だ。",
        )
        .unwrap();
        assert!(!quote.contains("話題・主語"));
        assert!(
            compare(
                "道路が凍ったため、歩行者が転倒した。",
                "道路が凍った後、歩行者が転倒した。"
            )
            .unwrap()
            .contains("因果関係を明示")
        );
        assert!(
            compare("得点が10点上がった。", "得点が10%上がった。")
                .unwrap()
                .contains("相対変化")
        );
    }

    #[test]
    fn compares_ascii_fullwidth_and_kanji_counts_as_the_same_numeric_scale() {
        assert_eq!(nearby_count("参加者は三人以上だった。"), "3人");
        assert_eq!(nearby_count("参加者は３人以上だった。"), "3人");
        assert_eq!(
            nearby_count("参加者は一万二千三百四十五人だった。"),
            "12345人"
        );
        assert_eq!(nearby_count("参加者は1.5万人以上だった。"), "指定値");
        assert_eq!(
            number_and_unit("得点が１０点上がった。", "点").unwrap().0,
            "10"
        );

        let report = compare("参加者は三人以上だった。", "参加者は４人以上だった。").unwrap();
        assert!(report.contains("文Aは「3人以上"));
        assert!(report.contains("文Bは「4人以上"));
    }

    #[test]
    fn refuses_unrecognized_semantic_differences_and_oversized_inputs() {
        assert!(compare("これは適切です。", "これは妥当です。").is_err());
        assert!(compare(&"あ".repeat(3_000), "A").is_err());
    }

    #[test]
    fn keeps_modality_negation_separate_from_clause_polarity_and_conditions() {
        let obligation =
            compare("資料を提出してもよい。", "資料を提出しなければならない。").unwrap();
        assert!(obligation.contains("許可されている"));
        assert!(obligation.contains("義務・必要がある"));
        assert!(!obligation.contains("肯否"));
        assert!(!obligation.contains("条件の向き"));
        assert!(obligation.contains("資料"));
        assert!(obligation.contains("提出"));

        let requirement = compare(
            "利用者は申請書を再提出する必要がある。",
            "利用者は申請書を再提出する必要はない。",
        )
        .unwrap();
        assert!(requirement.contains("義務・必要がある"));
        assert!(requirement.contains("義務・必要はない"));
        assert!(!requirement.contains("行為者: 文Aは「利用者は申請書を再提出する必要」"));
        assert!(requirement.contains("利用者"));
        assert!(requirement.contains("申請書"));
    }

    #[test]
    fn reports_common_topics_and_objects_without_duplicating_subject_differences() {
        let not_all = compare(
            "すべての会議で議事録が必要だ。",
            "一部の会議では議事録が必要だ。",
        )
        .unwrap();
        assert!(not_all.contains("全称"));
        assert!(not_all.contains("存在を述べる"));
        assert!(!not_all.contains("話題・主語"));

        let exclusive_additive = compare("赤い箱だけを運んだ。", "赤い箱も運んだ。").unwrap();
        assert!(exclusive_additive.contains("赤い箱"));

        let conditional = compare(
            "連絡がなければ作業を開始する。",
            "連絡があれば作業を開始する。",
        )
        .unwrap();
        assert!(conditional.contains("作業"));

        let active_passive = compare("事務局が申請を承認した。", "申請が承認された。").unwrap();
        assert!(active_passive.contains("申請"));
    }

    #[test]
    fn reports_absent_markers_and_avoids_duplicate_annual_frequency() {
        let condition =
            compare("合格すれば入場できる。", "入場できるのは合格した人だけだ。").unwrap();
        assert!(condition.contains("明示的な数量限定なし"));

        let annual = compare("去年、桜が咲いた。", "毎年、桜が咲く。").unwrap();
        assert!(annual.contains("時制・反復"));
        assert!(!annual.contains("量化・数量範囲"));
    }
}
