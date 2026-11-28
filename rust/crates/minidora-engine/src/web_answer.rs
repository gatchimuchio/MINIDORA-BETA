//! Pure validation and bounded prose extraction for a Core-owned Web goal.

use crate::{
    language, ports,
    scheduler::{GoalNode, GoalNodeKind},
    search::{self, SearchCandidate, SearchObservation, SearchProvider},
};
use minidora_domain::{Calculation, EvidencePacket, Permissions, Result, digest, failure};
use serde::Serialize;

pub(crate) const LISTING_NODE: &str = "web-question-listing";
pub(crate) const BODY_NODE: &str = "web-question-body";
pub(crate) const ANSWER_NODE: &str = "web-question-answer";

const MAX_SCAN_BYTES: usize = 131_072;
const MAX_SUMMARY_PASSAGES: usize = 128;
const MAX_SENTENCE_BYTES: usize = 1_536;
const MAX_TOPIC_BYTES: usize = 512;
const MOUNTAIN_DEFINITION_ANSWER: &str = "minidora.web-answer.wikipedia-prose.v3";

pub(crate) fn nodes() -> [GoalNode; 3] {
    [
        GoalNode {
            id: LISTING_NODE.into(),
            kind: GoalNodeKind::Evidence,
            requires: Vec::new(),
        },
        GoalNode {
            id: BODY_NODE.into(),
            kind: GoalNodeKind::Evidence,
            requires: vec![LISTING_NODE.into()],
        },
        GoalNode {
            id: ANSWER_NODE.into(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![BODY_NODE.into()],
        },
    ]
}

pub(crate) fn select_candidate<'a>(
    observation: &'a SearchObservation,
    permissions: &Permissions,
) -> Result<&'a SearchCandidate> {
    search::validate_stored_observation(observation, Some(permissions))?;
    if observation.provider != SearchProvider::Wikipedia {
        return Err(failure(
            "WEB_ANSWER_PROVIDER_UNSUPPORTED",
            "日本語本文を回答に使える検索接続がありません。",
        ));
    }
    observation
        .candidates
        .iter()
        .filter(|candidate| {
            search::candidate_body_uri(candidate)
                .and_then(|uri| ports::approved_url(&uri, permissions))
                .is_ok()
        })
        .min_by_key(|candidate| candidate.rank)
        .ok_or_else(|| {
            if observation.candidates.is_empty() {
                failure("SEARCH_CANDIDATE_UNAVAILABLE", "検索候補がありません。")
            } else {
                failure(
                    ports::READ_PERMISSION_REQUIRED,
                    "本文取得を許可された検索候補がありません。",
                )
            }
        })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct WebAnswer {
    kind: &'static str,
    topic: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    case_matched_article_title: Option<String>,
    candidate_id: String,
    search_packet_id: String,
    source_packet_id: String,
    source_uri: String,
    observed_at_unix_ms: u64,
    raw_body_sha256: String,
    pub(crate) passage: Passage,
    #[serde(skip_serializing_if = "Option::is_none")]
    elevation: Option<ElevationConversion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct Passage {
    pub(crate) source_start: usize,
    pub(crate) source_end: usize,
    pub(crate) source_span_sha256: String,
    pub(crate) text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ElevationConversion {
    metres: String,
    calculation: Calculation,
}

impl WebAnswer {
    pub(crate) fn elevation_calculation(&self) -> Option<&Calculation> {
        self.elevation
            .as_ref()
            .map(|elevation| &elevation.calculation)
    }

    pub(crate) fn units(&self) -> Vec<String> {
        let mut units = vec![
            format!(
                "「{}」について、取得した本文には次の記述があります。",
                self.topic
            ),
            format!("「{}」", self.passage.text),
            format!("出典: {}", self.source_uri),
            format!(
                "取得時刻: Unix時刻 {} ミリ秒、本文 SHA-256: {}",
                self.observed_at_unix_ms, self.raw_body_sha256
            ),
            "本文の記述に基づく回答です。表示時にリンクや注記などの記法を取り除きました。内容の真偽や網羅性は確認していません。".into(),
        ];
        if let Some(title) = &self.case_matched_article_title {
            units.insert(
                2,
                format!(
                    "質問語「{}」と記事名「{title}」は英字の大文字・小文字だけが異なります。",
                    self.topic
                ),
            );
        }
        if let Some(elevation) = &self.elevation {
            let source = if self.kind == MOUNTAIN_DEFINITION_ANSWER {
                "本文の山の定義文"
            } else {
                "本文"
            };
            units.insert(
                2,
                format!(
                    "{source}が『標高』と記した {} m を換算すると、{} km です（1 km = 1000 m）。",
                    elevation.metres, elevation.calculation.exact_value
                ),
            );
        }
        units
    }

    pub(crate) fn explanation_units(
        &self,
        listing: &EvidencePacket,
        candidate: &SearchCandidate,
        body: &EvidencePacket,
    ) -> Vec<String> {
        let mut units = vec![
            format!(
                "前回は {:?} を {} で検索しました。一覧取得時刻は Unix時刻 {} ミリ秒です。",
                self.topic,
                candidate.provider.provider_id(),
                listing.observed_at_unix_ms
            ),
            format!(
                "検索一覧: {} / packet ID: {} / 原応答 SHA-256: {}",
                listing.final_uri, listing.id, listing.raw_sha256
            ),
            format!(
                "本文取得を許可された候補のうち、掲載順位が最上位だった {} 位の {:?} を選びました。候補URL: {}",
                candidate.rank, candidate.title, candidate.uri
            ),
            format!(
                "本文: {} / HTTP {} / 取得時刻（Unix ms）: {} / packet ID: {} / 原本文 SHA-256: {}",
                body.final_uri,
                body.http_status,
                body.observed_at_unix_ms,
                body.id,
                body.raw_sha256
            ),
            format!(
                "回答に使った箇所は原本文の byte[{}..{}) です。採用範囲 SHA-256: {}",
                self.passage.source_start, self.passage.source_end, self.passage.source_span_sha256
            ),
            format!("記法を除いて表示した箇所: 「{}」", self.passage.text),
        ];
        if let Some(title) = &self.case_matched_article_title {
            units.push(format!(
                "質問語「{}」と記事名「{title}」の英字の大小を照合し、原本文を記事名と同じ表記で確認しました。",
                self.topic,
            ));
        }
        if let Some(elevation) = &self.elevation {
            let source = if self.kind == MOUNTAIN_DEFINITION_ANSWER {
                "本文の山の定義文にある標高"
            } else {
                "本文中の標高"
            };
            units.push(format!(
                "{source} {} m を 1000 で割り、{} km としました。",
                elevation.metres, elevation.calculation.exact_value
            ));
        }
        units.push(
            "今回は前回保存した検索一覧・本文・採用箇所を照合しました。記事全体の網羅性、記述の真偽や最新性は未確認です。"
                .into(),
        );
        units
    }
}

pub(crate) fn answer(
    topic: &str,
    elevation_in_kilometres: bool,
    candidate: &SearchCandidate,
    packet: &EvidencePacket,
    permissions: &Permissions,
) -> Result<WebAnswer> {
    if topic.trim().is_empty()
        || topic.len() > MAX_TOPIC_BYTES
        || topic.chars().any(char::is_control)
    {
        return Err(failure(
            "WEB_ANSWER_TOPIC_UNSUPPORTED",
            "本文と照合できる短い話題を指定してください。",
        ));
    }
    ports::validate_successful_packet(packet, permissions)?;
    if candidate.provider != SearchProvider::Wikipedia
        || packet.request_uri != search::candidate_body_uri(candidate)?
    {
        return Err(failure(
            "WEB_ANSWER_SOURCE_MISMATCH",
            "取得本文と選択した検索候補が一致しません。",
        ));
    }
    let article = packet.extracted_text.as_deref().ok_or_else(|| {
        failure(
            "WEB_ANSWER_SOURCE_MISMATCH",
            "取得本文のUTF-8記録がありません。",
        )
    })?;
    if elevation_in_kilometres && candidate.title != topic {
        return Err(failure(
            "WEB_ELEVATION_SUBJECT_UNRESOLVED",
            "検索候補の記事名と標高を調べる対象が一致しません。",
        ));
    }
    let selected = if elevation_in_kilometres {
        find_elevation_passage(article, topic)
            .map(|(passage, definition)| (passage, None, definition))
    } else {
        find_topic_passage(article, topic, &candidate.title)
            .map(|(passage, title)| (passage, title, false))
    };
    let (passage, case_matched_article_title, uses_mountain_definition) =
        selected.ok_or_else(|| {
            failure(
                "WEB_ANSWER_NO_SUPPORTED_PASSAGE",
                "取得本文で話題に一致する読み取れる日本語の文を確認できませんでした。",
            )
        })?;
    let elevation = if elevation_in_kilometres {
        let metres = if uses_mountain_definition {
            mountain_definition_metres(topic, &passage.text)
        } else {
            stated_elevation_metres(&passage.text)
        }
        .ok_or_else(|| {
            failure(
                "WEB_ELEVATION_UNRESOLVED",
                "本文から標高の値と単位を確認できませんでした。",
            )
        })?;
        Some(ElevationConversion {
            metres: metres.into(),
            calculation: language::calculate(&format!("{metres}/1000"))?,
        })
    } else {
        None
    };
    Ok(WebAnswer {
        kind: if uses_mountain_definition {
            MOUNTAIN_DEFINITION_ANSWER
        } else if case_matched_article_title.is_some() {
            "minidora.web-answer.wikipedia-prose.v2"
        } else {
            "minidora.web-answer.wikipedia-prose.v1"
        },
        topic: topic.into(),
        case_matched_article_title: case_matched_article_title.map(str::to_owned),
        candidate_id: candidate.id.clone(),
        search_packet_id: candidate.search_packet_id.clone(),
        source_packet_id: packet.id.clone(),
        source_uri: packet.final_uri.clone(),
        observed_at_unix_ms: packet.observed_at_unix_ms,
        raw_body_sha256: packet.raw_sha256.clone(),
        passage,
        elevation,
    })
}

fn stated_elevation_metres(sentence: &str) -> Option<&str> {
    let (number, tail) = metre_value(sentence.strip_prefix("標高は")?)?;
    if positive_measurement_end(tail) {
        return Some(number);
    }

    // The article can state a separately named highest point alongside its
    // elevation. Require both direct measurements before accepting the first.
    let (_, peak_tail) = metre_value(tail.strip_prefix("、山体の最高地点は")?)?;
    if positive_measurement_end(peak_tail) {
        return Some(number);
    }
    let (peak_description, recognition) = peak_tail.strip_prefix("で、")?.split_once("で、その")?;
    let peak_description = peak_description.strip_suffix("の独立峰")?;
    let (subject, recognition) = recognition.split_once('は')?;
    let (region, symbol) = recognition
        .strip_suffix("の象徴として広く知られている。")?
        .split_once("でも")?;
    [peak_description, subject, region, symbol]
        .into_iter()
        .all(description_phrase)
        .then_some(number)
}

fn metre_value(value: &str) -> Option<(&str, &str)> {
    let (number, rest) = measured_number(value)?;
    Some((number, rest.strip_prefix('m')?))
}

fn measured_number(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    let end = value
        .bytes()
        .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
        .count();
    let (number, rest) = value.split_at(end);
    let (integral, fraction) = number.split_once('.').unwrap_or((number, "0"));
    if integral.is_empty() || fraction.is_empty() || fraction.contains('.') || number.len() > 24 {
        return None;
    }
    Some((number, rest.trim_start()))
}

fn mountain_definition_metres<'a>(topic: &str, sentence: &'a str) -> Option<&'a str> {
    let mut predicate = sentence.strip_prefix(topic)?;
    if let Some(reading) = predicate.strip_prefix('（') {
        let (reading, rest) = reading.split_once('）')?;
        if reading.is_empty()
            || !reading
                .chars()
                .all(|character| matches!(character, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | 'ー'))
        {
            return None;
        }
        predicate = rest;
    }
    let (location, elevation) = predicate.strip_prefix("は、")?.split_once("にある標高")?;
    if !location.ends_with(['市', '区', '町', '村'])
        || !location
            .chars()
            .all(|character| matches!(character, '一'..='龯' | '々' | 'ヶ'))
        || !description_phrase(location)
        || ["推定", "概算", "未確認", "想定"]
            .into_iter()
            .any(|word| location.contains(word))
    {
        return None;
    }
    let (number, rest) = measured_number(elevation)?;
    (rest == "メートル（m）の山。").then_some(number)
}

fn decimal_value_parts(number: &str) -> (&str, &str) {
    let (integer, fraction) = number.split_once('.').unwrap_or((number, ""));
    (
        integer.trim_start_matches('0'),
        fraction.trim_end_matches('0'),
    )
}

fn positive_measurement_end(tail: &str) -> bool {
    matches!(tail, "。" | "です。" | "だ。" | "である。")
}

fn description_phrase(value: &str) -> bool {
    !value.is_empty()
        && value.chars().all(|character| {
            matches!(character, 'ぁ'..='ゖ' | 'ァ'..='ヺ' | '一'..='龯' | '（' | '）' | '々')
        })
        && !["もし", "ない", "なら", "仮", "疑", "誤", "説", "過去", "以前", "かつて"]
            .into_iter()
            .any(|qualifier| value.contains(qualifier))
}

fn find_passage(
    article: &str,
    unquoted_only: bool,
    matches_topic: &impl Fn(&str) -> bool,
) -> Option<Passage> {
    let mut selected = None;
    let _ = visit_passages(
        article,
        unquoted_only,
        false,
        false,
        matches_topic,
        &mut |passage| {
            selected = Some(passage);
            true
        },
    );
    selected
}

fn find_elevation_passage(article: &str, topic: &str) -> Option<(Passage, bool)> {
    // Preserve the earlier direct statement rule throughout the same bounded
    // scan. A definition is eligible only as a fallback for the named subject.
    let mut direct = None;
    let mut definition: Option<Passage> = None;
    let mut conflicting_definitions = false;
    let _ = visit_passages(
        article,
        true,
        false,
        false,
        &|sentence| {
            stated_elevation_metres(sentence).is_some()
                || mountain_definition_metres(topic, sentence).is_some()
        },
        &mut |passage| {
            if stated_elevation_metres(&passage.text).is_some() {
                direct = Some((passage, false));
                return true;
            }
            if !article[passage.source_start..passage.source_end].contains("{{")
                && let Some(number) = mountain_definition_metres(topic, &passage.text)
            {
                if let Some(previous) = &definition {
                    let previous_number = mountain_definition_metres(topic, &previous.text)
                        .expect("the definition had a checked elevation");
                    conflicting_definitions |=
                        decimal_value_parts(previous_number) != decimal_value_parts(number);
                } else {
                    definition = Some(passage);
                }
            }
            false
        },
    );
    if conflicting_definitions {
        direct
    } else {
        direct.or_else(|| definition.map(|passage| (passage, true)))
    }
}

fn find_topic_passage<'a>(
    article: &str,
    topic: &str,
    title: &'a str,
) -> Option<(Passage, Option<&'a str>)> {
    if !topic.is_ascii() || title == topic || !title.eq_ignore_ascii_case(topic) {
        return find_passage(article, false, &|sentence| sentence.contains(topic))
            .map(|passage| (passage, None));
    }

    // Keep the first title spelling as a possible result while looking for the
    // exact original spelling in the same bounded source scan.
    let mut exact = None;
    let mut by_title = None;
    let _ = visit_passages(
        article,
        false,
        false,
        false,
        &|sentence| sentence.contains(topic) || sentence.contains(title),
        &mut |passage| {
            if passage.text.contains(topic) {
                exact = Some((passage, None));
                return true;
            }
            by_title.get_or_insert((passage, Some(title)));
            false
        },
    );
    exact.or(by_title)
}

pub(crate) fn readable_passages(article: &str, limit: usize) -> Vec<Passage> {
    readable_passages_with_markup(article, "", limit, true, false)
}

pub(crate) fn readable_passages_with_nested_footnotes(article: &str, limit: usize) -> Vec<Passage> {
    readable_passages_with_markup(article, "", limit, true, true)
}

pub(crate) fn readable_research_passages(article: &str, topic: &str, limit: usize) -> Vec<Passage> {
    readable_passages_with_markup(article, topic, limit, false, true)
}

fn readable_passages_with_markup(
    article: &str,
    topic: &str,
    limit: usize,
    summary_lead_only: bool,
    nested_footnotes: bool,
) -> Vec<Passage> {
    let mut passages = Vec::new();
    let limit = limit.min(MAX_SUMMARY_PASSAGES);
    if limit == 0 {
        return passages;
    }
    let _ = visit_passages(
        article,
        true,
        summary_lead_only,
        nested_footnotes,
        &|sentence| sentence.contains(topic),
        &mut |passage| {
            if !passage.text.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
                passages.push(passage);
            }
            passages.len() == limit
        },
    );
    passages
}

fn visit_passages(
    article: &str,
    unquoted_only: bool,
    summary_lead_only: bool,
    nested_footnotes: bool,
    matches_topic: &impl Fn(&str) -> bool,
    stop: &mut impl FnMut(Passage) -> bool,
) -> Option<()> {
    let bounded = &article[..article.floor_char_boundary(article.len().min(MAX_SCAN_BYTES))];
    let mut rest = bounded;
    let mut quotes = QuotationContext::default();
    while !rest.is_empty() {
        rest = rest.trim_start();
        if summary_lead_only && rest.starts_with('=') {
            break;
        }
        if rest.starts_with("{{") {
            rest = &rest[prose_markup_end(rest, unquoted_only, nested_footnotes)?.0..];
            continue;
        }
        if let Some(block_end) = block_end(rest) {
            let end = rest.find(block_end)?;
            if summary_lead_only && block_end == "|}" && rest.get(2..end)?.contains("{|") {
                return None;
            }
            rest = &rest[end + block_end.len()..];
            continue;
        }
        let line_end = prose_line_end(rest, unquoted_only, summary_lead_only, nested_footnotes)?;
        let line = &rest[..line_end];
        let article_offset = bounded.len() - rest.len();
        let is_prose = !line.starts_with(['=', '*', '#', ':', ';', '|', '!', '<'])
            && !(unquoted_only && line.starts_with('>'));
        if (is_prose || unquoted_only)
            && line_passage(
                line,
                article_offset,
                &|sentence| is_prose && matches_topic(sentence),
                unquoted_only.then_some(&mut quotes),
                nested_footnotes,
                stop,
            ) == Some(true)
        {
            return Some(());
        }
        rest = &rest[(line_end + 1).min(rest.len())..];
    }
    Some(())
}

fn block_end(rest: &str) -> Option<&'static str> {
    [
        ("<!--", "-->"),
        ("{|", "|}"),
        ("<gallery", "</gallery>"),
        ("<pre", "</pre>"),
        ("<source", "</source>"),
        ("<syntaxhighlight", "</syntaxhighlight>"),
        ("<nowiki", "</nowiki>"),
        ("<script", "</script>"),
        ("<style", "</style>"),
    ]
    .into_iter()
    .find_map(|(start, end)| rest.starts_with(start).then_some(end))
}

/// Do not start a new candidate line inside a multiline note or template.
fn prose_line_end(
    text: &str,
    unquoted_only: bool,
    stop_at_unknown_tag: bool,
    nested_footnotes: bool,
) -> Option<usize> {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        match rest.as_bytes()[0] {
            b'\n' => return Some(at),
            b'[' if rest.starts_with("[[") => at += markup_end(rest, unquoted_only)?,
            b'{' if stop_at_unknown_tag && rest.starts_with("{|") => return None,
            b'{' if rest.starts_with("{{") || rest.starts_with("{|") => {
                at += prose_markup_end(rest, unquoted_only, nested_footnotes)?.0;
            }
            b'<' if is_reference(rest)
                || block_end(rest).is_some()
                || unquoted_only && quotation_block(rest).is_some() =>
            {
                at += markup_end(rest, unquoted_only)?;
            }
            b'<' if stop_at_unknown_tag => return None,
            _ => at += rest.chars().next()?.len_utf8(),
        }
    }
    Some(at)
}

fn template_end(text: &str) -> Option<usize> {
    let mut depth = 1;
    let mut at = 2;
    let bytes = text.as_bytes();
    while at + 1 < bytes.len() {
        match &bytes[at..at + 2] {
            b"{{" => {
                depth += 1;
                at += 2;
            }
            b"}}" => {
                depth -= 1;
                at += 2;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => at += 1,
        }
    }
    None
}

fn line_passage(
    line: &str,
    article_offset: usize,
    matches_topic: &impl Fn(&str) -> bool,
    mut quotes: Option<&mut QuotationContext>,
    nested_footnotes: bool,
    stop: &mut impl FnMut(Passage) -> bool,
) -> Option<bool> {
    let mut at = 0;
    let mut sentence_start = 0;
    let mut text = String::new();
    let mut recognized = true;
    let mut quoted = quotes.as_deref().is_some_and(QuotationContext::is_open);
    while at < line.len() {
        let remaining = &line[at..];
        if let Some((end, omit_note)) =
            prose_markup_end(remaining, quotes.is_some(), nested_footnotes)
        {
            let markup = &remaining[..end];
            let visible = if omit_note {
                Some("")
            } else {
                visible_markup(markup)
            };
            match visible {
                Some(visible) => {
                    if let Some(context) = quotes.as_deref_mut() {
                        for character in visible.chars() {
                            quoted |= context.observe(character);
                        }
                    }
                    text.push_str(visible);
                }
                None => recognized = false,
            }
            at += end;
            continue;
        }
        let next = remaining.chars().next()?;
        if matches!(next, '[' | ']' | '{' | '}' | '<' | '>' | '&')
            || next.is_control() && next != '\t'
        {
            if quotes.is_none() {
                return None;
            }
            recognized = false;
        }
        if let Some(context) = quotes.as_deref_mut() {
            quoted |= context.observe(next);
        }
        text.push(next);
        at += next.len_utf8();
        if next != '。' {
            continue;
        }
        let visible = text.trim();
        if recognized
            && !quoted
            && !visible.is_empty()
            && visible.len() <= MAX_SENTENCE_BYTES
            && matches_topic(visible)
            && visible.chars().any(|ch| ('ぁ'..='ん').contains(&ch))
            && stop(Passage {
                source_start: article_offset + sentence_start,
                source_end: article_offset + at,
                source_span_sha256: digest(&line.as_bytes()[sentence_start..at]),
                text: visible.into(),
            })
        {
            return Some(true);
        }
        sentence_start = at;
        text.clear();
        recognized = true;
        quoted = quotes.as_deref().is_some_and(QuotationContext::is_open);
    }
    Some(false)
}

/// The second value identifies a verified note that has no visible prose.
fn prose_markup_end(
    text: &str,
    unquoted_only: bool,
    nested_footnotes: bool,
) -> Option<(usize, bool)> {
    if nested_footnotes && let Some(body) = nested_footnote_body(text) {
        let (end, omit_note) = nested_footnote_end(body)?;
        return Some((text.len() - body.len() + end, omit_note));
    }
    markup_end(text, unquoted_only).map(|end| (end, false))
}

fn nested_footnote_body(text: &str) -> Option<&str> {
    ["{{refnest|group=\"注釈\"|", "{{refnest|group=注釈|"]
        .into_iter()
        .find_map(|prefix| text.strip_prefix(prefix))
}

/// Accept a single note argument with a complete nested reference. Its text
/// does not participate in prose recognition, quotation tracking or matching.
fn nested_footnote_end(body: &str) -> Option<(usize, bool)> {
    let mut at = 0;
    let mut has_reference = false;
    let mut single_note_argument = true;
    while let Some(rest) = body.get(at..).filter(|rest| !rest.is_empty()) {
        if rest.starts_with("}}") {
            return Some((at + 2, has_reference && single_note_argument));
        }
        if let Some(reference) = rest.strip_prefix("<ref>") {
            let (contents, _) = reference.split_once("</ref>")?;
            if contents.trim().is_empty() || contents.contains('<') {
                return None;
            }
            at += "<ref>".len() + contents.len() + "</ref>".len();
            has_reference = true;
            continue;
        }
        if let Some(link) = rest.strip_prefix("[[") {
            let (label, _) = link.split_once("]]")?;
            if label.trim().is_empty() || label.contains(['[', ']', '{', '}', '<', '>']) {
                return None;
            }
            at += label.len() + 4;
            continue;
        }
        let character = rest.chars().next()?;
        if character == '|' {
            // The boundary is still known, but this variant is not eligible.
            single_note_argument = false;
        }
        if matches!(character, '[' | ']' | '{' | '}' | '<' | '>')
            || character.is_control() && !matches!(character, '\t' | '\r' | '\n')
        {
            return None;
        }
        at += character.len_utf8();
    }
    None
}

fn markup_end(text: &str, unquoted_only: bool) -> Option<usize> {
    if unquoted_only && let Some((opening, ending)) = quotation_block(text) {
        return quotation_block_end(text, opening, ending);
    }
    if text.starts_with("[[") {
        return text[2..].find("]]").map(|end| end + 4);
    }
    if text.starts_with("{{") {
        return template_end(text);
    }
    if let Some(block_end) = block_end(text) {
        return text.find(block_end).map(|end| end + block_end.len());
    }
    if is_reference(text) {
        let opening_end = opening_tag_end(text)?;
        return if text[..opening_end].ends_with("/>") {
            Some(opening_end)
        } else {
            text[opening_end..]
                .find("</ref>")
                .map(|end| opening_end + end + 6)
        };
    }
    ["'''", "''", "&nbsp;", "&amp;"]
        .into_iter()
        .find_map(|markup| text.starts_with(markup).then_some(markup.len()))
}

fn quotation_block(text: &str) -> Option<(&'static str, &'static str)> {
    [("<blockquote", "</blockquote>"), ("<q", "</q>")]
        .into_iter()
        .find(|(opening, _)| starts_quotation_tag(text, opening))
}

fn starts_quotation_tag(text: &str, opening: &str) -> bool {
    text.get(..opening.len())
        .is_some_and(|part| part.eq_ignore_ascii_case(opening))
        && text[opening.len()..].starts_with(['>', ' ', '\t', '\n'])
}

fn quotation_block_end(text: &str, opening: &str, ending: &str) -> Option<usize> {
    let mut depth = 1;
    let mut at = opening_tag_end(text)?;
    while let Some(next) = text[at..].find('<') {
        at += next;
        let rest = &text[at..];
        if starts_quotation_tag(rest, opening) {
            depth += 1;
            at += opening_tag_end(rest)?;
        } else if rest
            .get(..ending.len())
            .is_some_and(|part| part.eq_ignore_ascii_case(ending))
        {
            depth -= 1;
            at += ending.len();
            if depth == 0 {
                return Some(at);
            }
        } else if let Some(end) = markup_end(rest, false) {
            at += end;
        } else {
            at += opening_tag_end(rest).unwrap_or(1);
        }
    }
    None
}

#[derive(Default)]
struct QuotationContext {
    expected_closers: Vec<char>,
}

impl QuotationContext {
    fn is_open(&self) -> bool {
        !self.expected_closers.is_empty()
    }

    /// Returns true if the character starts, ends or occurs within a quote.
    fn observe(&mut self, character: char) -> bool {
        if self.expected_closers.last() == Some(&character) {
            self.expected_closers.pop();
            return true;
        }
        let closing = match character {
            '「' => '」',
            '『' => '』',
            '“' => '”',
            '‘' => '’',
            '〈' => '〉',
            '《' => '》',
            '"' | '\'' | '＂' => character,
            '」' | '』' | '”' | '’' | '〉' | '》' => return true,
            _ => return self.is_open(),
        };
        self.expected_closers.push(closing);
        true
    }
}

fn is_reference(text: &str) -> bool {
    text.strip_prefix("<ref")
        .is_some_and(|rest| rest.starts_with([' ', '\t', '\n', '>', '/']))
}

fn opening_tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (at, byte) in text.bytes().enumerate() {
        match quote {
            Some(delimiter) if delimiter == byte => quote = None,
            None if matches!(byte, b'\'' | b'"') => quote = Some(byte),
            None if byte == b'>' => return Some(at + 1),
            _ => {}
        }
    }
    None
}

fn visible_markup(markup: &str) -> Option<&str> {
    if let Some(target) = markup.strip_prefix("[[").and_then(|s| s.strip_suffix("]]")) {
        let (destination, label) = target.split_once('|').unwrap_or((target, target));
        let label = label.trim();
        return (!destination.contains(':') && safe_label(label)).then_some(label);
    }
    if let Some(template) = markup.strip_prefix("{{").and_then(|s| s.strip_suffix("}}")) {
        let mut parts = template.split('|').map(str::trim);
        let name = parts.next()?;
        if matches!(name, "lang" | "lang2") {
            parts.next()?;
        } else if !name.starts_with("lang-") {
            return None;
        }
        let label = parts.next()?;
        return (parts.next().is_none() && safe_label(label)).then_some(label);
    }
    match markup {
        "&nbsp;" => Some(" "),
        "&amp;" => Some("&"),
        "''" | "'''" => Some(""),
        _ if markup.starts_with("<!--") || is_reference(markup) => Some(""),
        _ => None,
    }
}

fn safe_label(label: &str) -> bool {
    !label.is_empty()
        && !label.contains(['|', '[', ']', '{', '}', '<', '>', '&'])
        && !label.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_footnotes_preserve_the_passage_range_without_exposing_the_note() {
        let opening = "富士山は活火山である。\n";
        let retained = "富士山は[[富士箱根伊豆国立公園]]に指定されている";
        let note = "{{refnest|group=\"注釈\"|[[1936年]]の補足。<ref>注記だけの語。</ref>。}}";
        let article = format!("{opening}{retained}{note}。\n");

        assert_eq!(readable_passages(&article, 32).len(), 1);
        let passages = readable_passages_with_nested_footnotes(&article, 32);
        let [_, passage] = passages.as_slice() else {
            panic!("expected two readable passages, got {passages:?}");
        };
        assert_eq!(
            passage.text,
            "富士山は富士箱根伊豆国立公園に指定されている。"
        );
        let raw = &article[passage.source_start..passage.source_end];
        assert_eq!(raw, format!("{retained}{note}。"));
        assert_eq!(passage.source_span_sha256, digest(raw.as_bytes()));

        for unsupported in [
            "{{refnest|group=\"注釈\"|<ref>未閉鎖の注記。}}",
            "{{refnest|group=\"注釈\"|<ref>注記。</ref>|name=extra}}",
            "{{othernote|<ref>注記。</ref>}}",
        ] {
            let article = format!("{opening}{retained}{unsupported}。\n");
            assert_eq!(
                readable_passages_with_nested_footnotes(&article, 32).len(),
                1
            );
        }

        let note =
            "{{refnest|group=注釈|<ref>引用内の閉じ括弧 }}。\n注記だけの別行がある。\n</ref>。}}";
        let article = format!("{retained}{note}。\n公園の説明が続く。\n");
        let passages = readable_passages_with_nested_footnotes(&article, 32);
        assert_eq!(passages.len(), 2);
        assert_eq!(
            passages[0].text,
            "富士山は富士箱根伊豆国立公園に指定されている。"
        );
        assert_eq!(passages[1].text, "公園の説明が続く。");
    }

    #[test]
    fn ascii_case_uses_the_exact_article_title_only_without_a_topic_hit() {
        let article = "'''Python'''は、[[プログラミング言語]]である。\n";
        let (passage, matched_title) = find_topic_passage(article, "python", "Python").unwrap();
        assert_eq!(matched_title, Some("Python"));
        assert_eq!(passage.text, "Pythonは、プログラミング言語である。");
        let raw = &article[passage.source_start..passage.source_end];
        assert!(raw.contains("[[プログラミング言語]]"));
        assert_eq!(passage.source_span_sha256, digest(raw.as_bytes()));

        let both = format!("{article}pythonという小文字表記もある。\n");
        let (passage, matched_title) = find_topic_passage(&both, "python", "Python").unwrap();
        assert_eq!(matched_title, None);
        assert_eq!(passage.text, "pythonという小文字表記もある。");
        assert!(find_topic_passage(article, "python", "Python (言語)").is_none());
        assert!(find_topic_passage(article, "py", "Python").is_none());
    }

    #[test]
    fn mountain_definition_uses_its_named_subject_and_unambiguous_metres() {
        let title = "高尾山";
        let raw = "'''高尾山'''（たかおさん）は、[[東京都]][[八王子市]]にある[[高さ#標高|標高]]599[[メートル]]（m）の[[山]]。";
        let (passage, fallback) = find_elevation_passage(raw, title).unwrap();
        assert!(fallback);
        assert_eq!(
            mountain_definition_metres(title, &passage.text),
            Some("599")
        );
        assert_eq!(&raw[passage.source_start..passage.source_end], raw);
        assert_eq!(passage.source_span_sha256, digest(raw.as_bytes()));

        for invalid in [
            raw.replace("高尾山", "別の山"),
            raw.replace("599", "約599"),
            raw.replace("599", "推定599"),
            raw.replace("山]]。", "山]]ではない。"),
            raw.replace("[[八王子市]]", "{{lang-ja|八王子市}}"),
            format!("「{raw}」"),
            format!("{{{{Info|{raw}}}}}"),
        ] {
            assert!(
                find_elevation_passage(&invalid, title).is_none(),
                "{invalid}"
            );
        }

        let same = format!("{raw}\n{}", raw.replace("599", "599.0"));
        assert!(find_elevation_passage(&same, title).is_some());
        let different = format!("{raw}\n{}", raw.replace("599", "599.3"));
        assert!(find_elevation_passage(&different, title).is_none());
        let direct = format!("{different}\n標高は600 mです。\n");
        let (passage, fallback) = find_elevation_passage(&direct, title).unwrap();
        assert!(!fallback);
        assert_eq!(stated_elevation_metres(&passage.text), Some("600"));
    }

    #[test]
    fn elevation_requires_an_unquoted_positive_measurement() {
        let find_elevation = |article: &str| {
            find_passage(article, true, &|line| {
                stated_elevation_metres(line).is_some()
            })
        };
        for not_asserted in [
            "標高は1000 mではない。",
            "標高は1000 mであると仮定する。",
            "標高は1000 m、という説が広まった。",
            "標高は1000 m、山体の最高地点は1100 mではない。",
            "記録には「数値は不明である。標高は1000 mです。」とある。",
            "記録には「数値は不明である。\n標高は1000 mです。」とある。",
            "<BLOCKQUOTE>\n<blockquote>調査中。</blockquote>\n標高は1000 mです。\n</BLOCKQUOTE>",
            "<blockquote>\n<!-- </blockquote> -->\n標高は1000 mです。\n</blockquote>",
        ] {
            assert!(find_elevation(not_asserted).is_none(), "{not_asserted}");
        }

        let stated = "標高は3775.56 m、山体の最高地点は3776.12 mで、日本最高峰（剣ヶ峰）の独立峰で、その優美な風貌は日本国外でも日本の象徴として広く知られている。";
        let article = format!("標高は3000 mではない。\n{stated}\n");
        let passage = find_elevation(&article).unwrap();
        assert_eq!(stated_elevation_metres(&passage.text), Some("3775.56"));
        assert_eq!(&article[passage.source_start..passage.source_end], stated);
        assert_eq!(passage.source_span_sha256, digest(stated.as_bytes()));
    }

    #[test]
    fn verified_elevation_exposes_the_legacy_quotient_without_making_prose_a_quantity() {
        let provider = SearchProvider::Wikipedia;
        let mut permissions = Permissions::default();
        permissions.read_hosts.insert(provider.host().into());
        for (title, raw, expression, quotient, cm) in [
            (
                "高尾山",
                "'''高尾山'''（たかおさん）は、[[東京都]][[八王子市]]にある[[高さ#標高|標高]]599[[メートル]]（m）の[[山]]。",
                "599/1000",
                "599/1000",
                "59900 cm",
            ),
            (
                "富士山",
                "'''富士山'''は日本の山である。\n標高は3775.56 mです。\n",
                "3775.56/1000",
                "94389/25000",
                "377556 cm",
            ),
        ] {
            let candidate = SearchCandidate {
                id: digest(title.as_bytes()),
                rank: 1,
                title: title.into(),
                uri: format!("https://{}/wiki/{title}", provider.host()),
                provider,
                provider_id: provider.provider_id().into(),
                source_key: title.into(),
                search_packet_id: "f".repeat(64),
            };
            let uri = search::candidate_body_uri(&candidate).unwrap();
            let mut packet = EvidencePacket {
                id: String::new(),
                mode: "LIVE".into(),
                packet_kind: "FETCHED_BODY".into(),
                request_uri: uri.clone(),
                final_uri: uri.clone(),
                redirect_chain: vec![uri.clone()],
                observed_at_unix_ms: 1,
                provider_id: "https-direct-url-v1".into(),
                http_status: 200,
                raw_body: raw.as_bytes().into(),
                raw_sha256: digest(raw.as_bytes()),
                extracted_text: Some(raw.into()),
                parser_version: "strict-utf8-identity-v1".into(),
                independence_group: uri,
                failure: None,
                retry_after: None,
            };
            packet.id = ports::fetch_packet_identity(&packet).unwrap();

            let ordinary = answer(title, false, &candidate, &packet, &permissions).unwrap();
            assert!(ordinary.elevation_calculation().is_none());
            assert!(
                serde_json::to_value(ordinary)
                    .unwrap()
                    .get("elevation")
                    .is_none()
            );

            let elevation = answer(title, true, &candidate, &packet, &permissions).unwrap();
            let calculation = elevation.elevation_calculation().unwrap();
            assert_eq!(calculation.expression, expression);
            assert_eq!(calculation.exact_value, quotient);
            assert_eq!(
                serde_json::to_value(&elevation).unwrap()["elevation"]["calculation"],
                serde_json::to_value(calculation).unwrap()
            );
            assert!(language::convert_exact_length(calculation, "cm").is_err());

            let mut length = calculation.clone();
            length.exact_value.push_str(" km");
            let converted = language::convert_exact_length(&length, "cm").unwrap();
            assert_eq!(converted.exact_value, cm);
        }
    }
}
