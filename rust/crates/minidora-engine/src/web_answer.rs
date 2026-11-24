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
const MAX_SENTENCE_BYTES: usize = 1_536;
const MAX_TOPIC_BYTES: usize = 512;

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
    candidate_id: String,
    search_packet_id: String,
    source_packet_id: String,
    source_uri: String,
    observed_at_unix_ms: u64,
    raw_body_sha256: String,
    passage: Passage,
    #[serde(skip_serializing_if = "Option::is_none")]
    elevation: Option<ElevationConversion>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct Passage {
    source_start: usize,
    source_end: usize,
    source_span_sha256: String,
    text: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct ElevationConversion {
    metres: String,
    calculation: Calculation,
}

impl WebAnswer {
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
        if let Some(elevation) = &self.elevation {
            units.insert(
                2,
                format!(
                    "本文が『標高』と記した {} m を換算すると、{} km です（1 km = 1000 m）。",
                    elevation.metres, elevation.calculation.exact_value
                ),
            );
        }
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
    let passage = find_passage(article, &|line| {
        if elevation_in_kilometres {
            stated_elevation_metres(line).is_some()
        } else {
            line.contains(topic)
        }
    })
    .ok_or_else(|| {
        failure(
            "WEB_ANSWER_NO_SUPPORTED_PASSAGE",
            "取得本文で話題に一致する読み取れる日本語の文を確認できませんでした。",
        )
    })?;
    let elevation = if elevation_in_kilometres {
        let metres = stated_elevation_metres(&passage.text).ok_or_else(|| {
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
        kind: "minidora.web-answer.wikipedia-prose.v1",
        topic: topic.into(),
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
    let value = sentence.strip_prefix("標高は")?.trim_start();
    let end = value
        .bytes()
        .take_while(|byte| byte.is_ascii_digit() || *byte == b'.')
        .count();
    let (number, rest) = value.split_at(end);
    let (integral, fraction) = number.split_once('.').unwrap_or((number, "0"));
    if integral.is_empty() || fraction.is_empty() || fraction.contains('.') || number.len() > 24 {
        return None;
    }
    let after_unit = rest.trim_start().strip_prefix('m')?;
    after_unit
        .starts_with(['、', '。', 'で', '（'])
        .then_some(number)
}

fn find_passage(article: &str, matches_topic: &impl Fn(&str) -> bool) -> Option<Passage> {
    let bounded = &article[..article.floor_char_boundary(article.len().min(MAX_SCAN_BYTES))];
    let mut rest = bounded;
    while !rest.is_empty() {
        rest = rest.trim_start();
        if rest.starts_with("{{") {
            rest = &rest[template_end(rest)?..];
            continue;
        }
        if let Some(block_end) = block_end(rest) {
            rest = &rest[rest.find(block_end)? + block_end.len()..];
            continue;
        }
        let line_end = prose_line_end(rest)?;
        let line = &rest[..line_end];
        let article_offset = bounded.len() - rest.len();
        if !line.starts_with(['=', '*', '#', ':', ';', '|', '!', '<'])
            && let Some(passage) = line_passage(line, article_offset, matches_topic)
        {
            return Some(passage);
        }
        rest = &rest[(line_end + 1).min(rest.len())..];
    }
    None
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
fn prose_line_end(text: &str) -> Option<usize> {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        match rest.as_bytes()[0] {
            b'\n' => return Some(at),
            b'[' if rest.starts_with("[[") => at += markup_end(rest)?,
            b'{' if rest.starts_with("{{") || rest.starts_with("{|") => at += markup_end(rest)?,
            b'<' if is_reference(rest) || block_end(rest).is_some() => {
                at += markup_end(rest)?;
            }
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
) -> Option<Passage> {
    let mut at = 0;
    let mut sentence_start = 0;
    let mut text = String::new();
    let mut recognized = true;
    while at < line.len() {
        let remaining = &line[at..];
        if let Some(end) = markup_end(remaining) {
            match visible_markup(&remaining[..end]) {
                Some(visible) => text.push_str(visible),
                None => recognized = false,
            }
            at += end;
            continue;
        }
        let next = remaining.chars().next()?;
        if matches!(next, '[' | ']' | '{' | '}' | '<' | '>' | '&')
            || next.is_control() && next != '\t'
        {
            return None;
        }
        text.push(next);
        at += next.len_utf8();
        if next != '。' {
            continue;
        }
        let visible = text.trim();
        if recognized
            && !visible.is_empty()
            && visible.len() <= MAX_SENTENCE_BYTES
            && matches_topic(visible)
            && visible.chars().any(|ch| ('ぁ'..='ん').contains(&ch))
        {
            return Some(Passage {
                source_start: article_offset + sentence_start,
                source_end: article_offset + at,
                source_span_sha256: digest(&line.as_bytes()[sentence_start..at]),
                text: visible.into(),
            });
        }
        sentence_start = at;
        text.clear();
        recognized = true;
    }
    None
}

fn markup_end(text: &str) -> Option<usize> {
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
