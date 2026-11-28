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
const ENGLISH_DEFINITION_ANSWER: &str = "minidora.web-answer.wikipedia-en-prose.v1";

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProseMode {
    Japanese,
    English,
    EnglishDefinition,
}

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
    expected_provider: SearchProvider,
) -> Result<&'a SearchCandidate> {
    search::validate_stored_observation(observation, Some(permissions))?;
    let english = expected_provider == SearchProvider::WikipediaEnglish;
    if !expected_provider.is_wikipedia() || observation.provider != expected_provider {
        return Err(failure(
            "WEB_ANSWER_PROVIDER_UNSUPPORTED",
            if english {
                "The saved search uses a different source from the English Wikipedia edition required by this question."
            } else {
                "日本語本文を回答に使える検索接続がありません。"
            },
        ));
    }
    let matches_title = |candidate: &SearchCandidate| {
        !english || english_title_matches(&observation.query, &candidate.title)
    };
    observation
        .candidates
        .iter()
        .filter(|candidate| matches_title(candidate))
        .filter(|candidate| {
            search::candidate_body_uri(candidate)
                .and_then(|uri| ports::approved_url(&uri, permissions))
                .is_ok()
        })
        .min_by_key(|candidate| {
            (
                english && !candidate.title.eq_ignore_ascii_case(&observation.query),
                candidate.rank,
            )
        })
        .ok_or_else(|| {
            if english && !observation.candidates.iter().any(matches_title) {
                failure(
                    "SEARCH_CANDIDATE_UNAVAILABLE",
                    "No English Wikipedia article title matches the requested topic.",
                )
            } else if observation.candidates.is_empty() {
                failure("SEARCH_CANDIDATE_UNAVAILABLE", "検索候補がありません。")
            } else if english {
                failure(
                    ports::READ_PERMISSION_REQUIRED,
                    "The matching English Wikipedia article requires permission to read its exact host.",
                )
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
        if self.kind == ENGLISH_DEFINITION_ANSWER {
            return vec![
                format!("The retrieved article describes {} as follows.", self.topic),
                format!("“{}”", self.passage.text),
                format!("Source: {}", self.source_uri),
                format!(
                    "Retrieved at Unix time {} ms. Body SHA-256: {}",
                    self.observed_at_unix_ms, self.raw_body_sha256
                ),
                "Links and notes were removed from this excerpt. Independent verification and broader coverage remain open.".into(),
            ];
        }
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
        english: bool,
    ) -> Vec<String> {
        if english {
            let mut units = vec![
                format!(
                    "The previous search looked up {:?} with {} at Unix time {} ms.",
                    self.topic,
                    candidate.provider.provider_id(),
                    listing.observed_at_unix_ms
                ),
                format!(
                    "Search listing: {} / packet ID: {} / raw SHA-256: {}",
                    listing.final_uri, listing.id, listing.raw_sha256
                ),
                format!(
                    "Selected article: {:?} / search rank: {} / candidate URL: {}",
                    candidate.title, candidate.rank, candidate.uri
                ),
                format!(
                    "Article body: {} / HTTP {} / retrieved at Unix time {} ms / packet ID: {} / raw SHA-256: {}",
                    body.final_uri,
                    body.http_status,
                    body.observed_at_unix_ms,
                    body.id,
                    body.raw_sha256
                ),
                format!(
                    "The answer used article bytes [{}..{}). Span SHA-256: {}",
                    self.passage.source_start,
                    self.passage.source_end,
                    self.passage.source_span_sha256
                ),
                format!("Excerpt after removing markup: “{}”", self.passage.text),
            ];
            if let Some(title) = &self.case_matched_article_title {
                units.push(format!(
                    "The query {:?} and the article title {title:?} match ignoring letter case.",
                    self.topic
                ));
            }
            if let Some(elevation) = &self.elevation {
                units.push(format!(
                    "The elevation stated in the article is {} m. Dividing by 1000 gives {} km.",
                    elevation.metres, elevation.calculation.exact_value
                ));
            }
            units.push(
                "This explanation checks the retained listing, article body and excerpt. The article's completeness, accuracy and currency remain unverified.".into(),
            );
            return units;
        }
        let candidate_reason = if self.kind == ENGLISH_DEFINITION_ANSWER {
            format!(
                "取得した候補から、英語の質問語と記事名を照合して {} 位の {:?} を選びました。候補URL: {}",
                candidate.rank, candidate.title, candidate.uri
            )
        } else {
            format!(
                "本文取得を許可された候補のうち、掲載順位が最上位だった {} 位の {:?} を選びました。候補URL: {}",
                candidate.rank, candidate.title, candidate.uri
            )
        };
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
            candidate_reason,
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
            units.push(if self.kind == ENGLISH_DEFINITION_ANSWER {
                format!(
                    "質問語「{}」と記事名「{title}」は英字の大小を区別せず照合し、本文で同じ話題の定義を確認しました。",
                    self.topic
                )
            } else {
                format!(
                    "質問語「{}」と記事名「{title}」の英字の大小を照合し、原本文を記事名と同じ表記で確認しました。",
                    self.topic
                )
            });
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
    expected_provider: SearchProvider,
) -> Result<WebAnswer> {
    let english = expected_provider == SearchProvider::WikipediaEnglish;
    if topic.trim().is_empty()
        || topic.len() > MAX_TOPIC_BYTES
        || topic.chars().any(char::is_control)
    {
        return Err(failure(
            "WEB_ANSWER_TOPIC_UNSUPPORTED",
            if english {
                "Specify one short topic that can be matched to the article."
            } else {
                "本文と照合できる短い話題を指定してください。"
            },
        ));
    }
    if english {
        search::validate_research_body(candidate, packet, permissions)?;
    } else {
        ports::validate_successful_packet(packet, permissions)?;
    }
    if !expected_provider.is_wikipedia()
        || candidate.provider != expected_provider
        || packet.request_uri != search::candidate_body_uri(candidate)?
        || (english && elevation_in_kilometres)
    {
        return Err(failure(
            "WEB_ANSWER_SOURCE_MISMATCH",
            if english {
                "The retrieved article or requested operation differs from the supported English source question."
            } else {
                "取得本文と選択した検索候補が一致しません。"
            },
        ));
    }
    let article = packet.extracted_text.as_deref().ok_or_else(|| {
        failure(
            "WEB_ANSWER_SOURCE_MISMATCH",
            if english {
                "The retrieved article has no verified UTF-8 text."
            } else {
                "取得本文のUTF-8記録がありません。"
            },
        )
    })?;
    if elevation_in_kilometres && candidate.title != topic {
        return Err(failure(
            "WEB_ELEVATION_SUBJECT_UNRESOLVED",
            "検索候補の記事名と標高を調べる対象が一致しません。",
        ));
    }
    let selected = if english {
        find_english_definition(article, topic, &candidate.title).map(|passage| {
            let matched_title = (candidate.title != topic
                && candidate.title.eq_ignore_ascii_case(topic))
            .then_some(candidate.title.as_str());
            (passage, matched_title, false)
        })
    } else if elevation_in_kilometres {
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
                if english {
                    "The beginning of the retrieved article does not contain a readable definition of the requested topic."
                } else {
                    "取得本文で話題に一致する読み取れる日本語の文を確認できませんでした。"
                },
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
        kind: if english {
            ENGLISH_DEFINITION_ANSWER
        } else if uses_mountain_definition {
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
        ProseMode::Japanese,
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
        ProseMode::Japanese,
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

fn strip_ascii_case_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.get(..prefix.len())
        .filter(|opening| opening.eq_ignore_ascii_case(prefix))
        .map(|_| &text[prefix.len()..])
}

fn english_definition_sentence(sentence: &str, title: &str) -> bool {
    let Some(sentence) = sentence.strip_suffix('.') else {
        return false;
    };
    ["", "the ", "an ", "a "].into_iter().any(|article| {
        let Some(mut predicate) = strip_ascii_case_prefix(sentence, article)
            .and_then(|body| strip_ascii_case_prefix(body, title))
            .and_then(|rest| rest.strip_prefix(' '))
            .map(str::trim_start)
        else {
            return false;
        };
        if let Some(rest) = predicate.strip_prefix('(')
            && let Some((aside, after)) = rest.split_once(')')
            && !aside.contains('(')
        {
            predicate = after.trim_start();
        }
        ["is ", "are "].into_iter().any(|copula| {
            let Some(definition) = strip_ascii_case_prefix(predicate, copula).map(str::trim) else {
                return false;
            };
            let normalized = definition.to_ascii_lowercase();
            definition.contains(char::is_alphabetic)
                && !normalized.starts_with("not ")
                && !normalized.contains(" may refer to")
                && !normalized.contains(" can refer to")
        })
    })
}

fn english_title_matches(topic: &str, title: &str) -> bool {
    topic.eq_ignore_ascii_case(title)
        || ["a ", "an "].into_iter().any(|article| {
            strip_ascii_case_prefix(topic, article)
                .is_some_and(|title_query| title_query.eq_ignore_ascii_case(title))
        })
}

fn find_english_definition(article: &str, topic: &str, title: &str) -> Option<Passage> {
    if !english_title_matches(topic, title) {
        return None;
    }
    let mut definition = None;
    let _ = visit_passages(
        article,
        true,
        true,
        true,
        ProseMode::EnglishDefinition,
        &|sentence| english_definition_sentence(sentence, title),
        &mut |passage| {
            definition = Some(passage);
            true
        },
    );
    definition
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
        ProseMode::Japanese,
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
    readable_passages_with_markup(article, limit, true, false, false, |_| true)
}

pub(crate) fn readable_passages_with_nested_footnotes(article: &str, limit: usize) -> Vec<Passage> {
    readable_passages_with_markup(article, limit, true, true, false, |_| true)
}

pub(crate) fn readable_research_passages(article: &str, topic: &str, limit: usize) -> Vec<Passage> {
    readable_passages_with_markup(article, limit, false, true, false, |sentence| {
        sentence.contains(topic)
    })
}

pub(crate) fn readable_research_passages_matching(
    article: &str,
    limit: usize,
    matches_topic: impl Fn(&str) -> bool,
) -> Vec<Passage> {
    readable_passages_with_markup(article, limit, false, true, true, matches_topic)
}

fn readable_passages_with_markup(
    article: &str,
    limit: usize,
    summary_lead_only: bool,
    nested_footnotes: bool,
    english_prose: bool,
    matches_topic: impl Fn(&str) -> bool,
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
        if english_prose {
            ProseMode::English
        } else {
            ProseMode::Japanese
        },
        &matches_topic,
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
    mode: ProseMode,
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
        if mode == ProseMode::EnglishDefinition && is_figure(rest) {
            rest = &rest[figure_end(rest)?..];
            continue;
        }
        if rest.starts_with("{{") {
            rest = &rest[prose_markup_end(rest, unquoted_only, nested_footnotes, mode)?.0..];
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
        let line_end = prose_line_end(
            rest,
            unquoted_only,
            summary_lead_only && mode != ProseMode::EnglishDefinition,
            nested_footnotes,
            mode,
        )?;
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
                mode,
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

fn is_figure(text: &str) -> bool {
    text.strip_prefix("[[").is_some_and(|contents| {
        ["file:", "image:"]
            .into_iter()
            .any(|prefix| strip_ascii_case_prefix(contents.trim_start(), prefix).is_some())
    })
}

/// Figure captions can contain links and references. Consume the complete
/// figure before looking for article prose, including a multiline caption.
fn figure_end(text: &str) -> Option<usize> {
    let mut depth = 1;
    let mut at = 2;
    while let Some(rest) = text.get(at..).filter(|rest| !rest.is_empty()) {
        if rest.starts_with("[[") {
            at += 2;
            depth += 1;
        } else if rest.starts_with("]]") {
            at += 2;
            depth -= 1;
            if depth == 0 {
                return Some(at);
            }
        } else if rest.starts_with("{{") {
            at += template_end(rest, true)?;
        } else if is_reference(rest) || rest.starts_with("<!--") {
            at += markup_end(rest, true)?;
        } else {
            at += rest.chars().next()?.len_utf8();
        }
    }
    None
}

/// Do not start a new candidate line inside a multiline note or template.
fn prose_line_end(
    text: &str,
    unquoted_only: bool,
    stop_at_unknown_tag: bool,
    nested_footnotes: bool,
    mode: ProseMode,
) -> Option<usize> {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        match rest.as_bytes()[0] {
            b'\n' => return Some(at),
            b'[' if rest.starts_with("[[") => at += markup_end(rest, unquoted_only)?,
            b'{' if stop_at_unknown_tag && rest.starts_with("{|") => return None,
            b'{' if rest.starts_with("{{") || rest.starts_with("{|") => {
                at += prose_markup_end(rest, unquoted_only, nested_footnotes, mode)?.0;
            }
            b'(' if mode == ProseMode::EnglishDefinition => {
                at += pronunciation_end(rest).unwrap_or(1);
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

fn template_end(text: &str, opaque_notes: bool) -> Option<usize> {
    let mut depth = 1;
    let mut at = 2;
    let bytes = text.as_bytes();
    while at + 1 < bytes.len() {
        if opaque_notes && bytes[at] == b'<' {
            let tail = &text[at..];
            if is_reference(tail) || tail.starts_with("<!--") {
                at += markup_end(tail, true)?;
                continue;
            }
        }
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
    mode: ProseMode,
    stop: &mut impl FnMut(Passage) -> bool,
) -> Option<bool> {
    let mut at = 0;
    let mut sentence_start = 0;
    let mut text = String::new();
    let mut recognized = true;
    let english_prose = mode != ProseMode::Japanese;
    let mut quoted = quotes.as_deref().is_some_and(QuotationContext::is_open);
    while at < line.len() {
        let remaining = &line[at..];
        if let Some((end, omit_note)) =
            prose_markup_end(remaining, quotes.is_some(), nested_footnotes, mode)
        {
            let markup = &remaining[..end];
            let visible = if omit_note {
                if mode == ProseMode::EnglishDefinition
                    && text.ends_with(' ')
                    && remaining[end..].starts_with(' ')
                {
                    text.pop();
                }
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
        let english_end = if english_prose && matches!(next, '.' | '!' | '?') {
            if mode == ProseMode::EnglishDefinition {
                after_definition_references(line, at)
            } else {
                line[at..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
                    .then_some(at)
            }
        } else {
            None
        };
        if let Some(end) = english_end {
            at = end;
        } else if next != '。' {
            continue;
        }
        let visible = text.trim();
        if recognized
            && !quoted
            && !visible.is_empty()
            && visible.len() <= MAX_SENTENCE_BYTES
            && matches_topic(visible)
            && visible
                .chars()
                .any(|ch| ('ぁ'..='ん').contains(&ch) || english_prose && ch.is_ascii_alphabetic())
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

fn after_definition_references(text: &str, mut at: usize) -> Option<usize> {
    while is_reference(&text[at..]) {
        at += markup_end(&text[at..], true)?;
    }
    text[at..]
        .chars()
        .next()
        .is_none_or(char::is_whitespace)
        .then_some(at)
}

fn template_named(text: &str, name: &str) -> bool {
    text.strip_prefix("{{")
        .and_then(|body| strip_ascii_case_prefix(body, name))
        .is_some_and(|tail| tail.trim_start().starts_with('|'))
}

fn pronunciation_template(text: &str) -> Option<usize> {
    ["IPAc-en", "IPA", "IPA-en", "Respell"]
        .into_iter()
        .any(|name| template_named(text, name))
        .then(|| template_end(text, true))
        .flatten()
}

fn pronunciation_end(text: &str) -> Option<usize> {
    let mut rest = text.strip_prefix('(')?.trim_start();
    loop {
        let end = pronunciation_template(rest)?;
        rest = rest[end..].trim_start();
        if rest.starts_with(')') {
            return Some(text.len() - rest.len() + 1);
        }
        rest = rest.strip_prefix(',').unwrap_or(rest).trim_start();
    }
}

/// The second value identifies a verified note that has no visible prose.
fn prose_markup_end(
    text: &str,
    unquoted_only: bool,
    nested_footnotes: bool,
    mode: ProseMode,
) -> Option<(usize, bool)> {
    if mode == ProseMode::EnglishDefinition {
        let note = if text.starts_with('(') {
            pronunciation_end(text)
        } else if template_named(text, "notetag") {
            template_end(text, true)
        } else {
            pronunciation_template(text)
        };
        if let Some(end) = note {
            return Some((end, true));
        }
    }
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
        return template_end(text, false);
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

    fn wiki_body(
        provider: SearchProvider,
        title: &str,
        raw: &str,
    ) -> (SearchCandidate, EvidencePacket) {
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
        (candidate, packet)
    }

    #[test]
    fn english_definition_checks_named_lead_prose_and_bound_source_edition() {
        let title = "Thermocouple";
        let provider = SearchProvider::WikipediaEnglish;
        let mut permissions = Permissions::default();
        permissions.read_hosts.insert(provider.host().into());
        permissions
            .read_hosts
            .insert(SearchProvider::Wikipedia.host().into());
        let lead = "A '''Thermocouple''' is a [[sensor]] for measuring temperature<ref>Note with internal markup.</ref>.";
        let raw = format!("{{{{Short description|Temperature sensor}}}}\n{lead}\n== Use ==\n");
        let (candidate, body) = wiki_body(provider, title, &raw);
        let result = answer(
            "thermocouple",
            false,
            &candidate,
            &body,
            &permissions,
            provider,
        )
        .unwrap();
        assert_eq!(result.kind, ENGLISH_DEFINITION_ANSWER);
        assert_eq!(
            result.passage.text,
            "A Thermocouple is a sensor for measuring temperature."
        );
        assert_eq!(
            &raw[result.passage.source_start..result.passage.source_end],
            lead
        );
        assert_eq!(result.passage.source_span_sha256, digest(lead.as_bytes()));
        assert!(
            result
                .units()
                .iter()
                .any(|unit| unit.starts_with("Source: https://en.wikipedia.org/"))
        );
        assert!(
            answer(
                "thermocouple",
                false,
                &candidate,
                &body,
                &permissions,
                SearchProvider::Wikipedia,
            )
            .is_err()
        );
        let mut redirected_body = body.clone();
        redirected_body.final_uri = redirected_body
            .request_uri
            .replace("en.wikipedia.org", "ja.wikipedia.org");
        redirected_body
            .redirect_chain
            .push(redirected_body.final_uri.clone());
        redirected_body.independence_group = redirected_body.final_uri.clone();
        redirected_body.id = ports::fetch_packet_identity(&redirected_body).unwrap();
        assert!(
            answer(
                "thermocouple",
                false,
                &candidate,
                &redirected_body,
                &permissions,
                provider
            )
            .is_err()
        );
        for raw in [
            "The thermocouplehouse is a fictional structure.",
            "Thermocouple may refer to the following items.",
            "A thermocouple is a term that may refer to these things.",
            "== History ==\nA thermocouple is a temperature sensor.",
            "\"A thermocouple is a temperature sensor.\"",
        ] {
            assert!(
                find_english_definition(raw, "thermocouple", title).is_none(),
                "{raw}"
            );
        }
        assert!(find_english_definition(&raw, "couple", title).is_none());
        assert!(find_english_definition(&raw, "a thermocouple", title).is_some());
        assert!(find_english_definition(&raw, "the thermocouple", title).is_none());
    }

    #[test]
    fn english_definitions_keep_provenance_through_ordinary_wiki_lead_markup() {
        let preamble = concat!(
            "{{Short description|A sample definition}}\n",
            "[[File:diagram.png|thumb|Caption with [[nested link]] and a newline.\n",
            "Sample term is a claim in the caption.<ref>]] is a note delimiter here.</ref>]]\n\n",
        );
        let explanation = "Sample term is a measurable property.";
        for (intro, reference) in [
            (
                "'''Sample term'''{{notetag|{{IPAc-en|s|a|m|p|l}} {{Respell|SAM|pul}}<ref>Unrelated }} in a citation.</ref>}}",
                "",
            ),
            (
                "'''Sample term''' ({{IPAc-en|s|a|m|p|l}} {{respell|SAM|pul}})",
                "",
            ),
            (
                "'''Sample term'''",
                "<ref>{{Cite book|title=Source}}</ref><ref name=\"second\"/>",
            ),
        ] {
            let lead = format!("{intro} is a [[measurement|measurable]] property.{reference}");
            let article =
                format!("{preamble}{lead} An unrelated exponent is m<sup>−1</sup>.\n== Body ==\n");
            let definition =
                find_english_definition(&article, "sample term", "Sample term").unwrap();
            assert_eq!(definition.text, explanation);
            assert_eq!(
                &article[definition.source_start..definition.source_end],
                lead
            );
            assert_eq!(definition.source_span_sha256, digest(lead.as_bytes()));
        }
        assert!(find_english_definition(preamble, "sample term", "Sample term").is_none());
        assert!(find_english_definition(
            "[[File:diagram.png|caption starts.\nSample term is a claim in an unclosed caption.",
            "sample term",
            "Sample term",
        ).is_none());
        assert!(
            find_english_definition(
                "'''Sample term''' {{unsupported|definition}} is a measurable property.",
                "sample term",
                "Sample term",
            )
            .is_none()
        );
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
            let (candidate, packet) = wiki_body(provider, title, raw);

            let ordinary =
                answer(title, false, &candidate, &packet, &permissions, provider).unwrap();
            assert!(ordinary.elevation_calculation().is_none());
            assert!(
                serde_json::to_value(ordinary)
                    .unwrap()
                    .get("elevation")
                    .is_none()
            );

            let elevation =
                answer(title, true, &candidate, &packet, &permissions, provider).unwrap();
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
