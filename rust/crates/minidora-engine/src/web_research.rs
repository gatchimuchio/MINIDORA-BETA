//! Compare retained, subject-matched passages from distinct Web bodies.
use crate::{
    ports,
    search::{self, SearchCandidate, SearchProvider},
    web_answer::{self, Passage},
};
use minidora_domain::{Error, EvidencePacket, Permissions, Result, digest, failure};
use serde::Serialize;
use std::collections::BTreeSet;

pub(crate) const MAX_RESEARCH_SOURCES: usize = 4;
const MAX_TOPIC_BYTES: usize = 512;
const MAX_PASSAGES: usize = 12;
const MAX_TERMS: usize = 32;
const DISPLAY_TERMS: usize = 3;
const VERSION: &str = "minidora.web-research.paired-wikipedia-prose.v1";

#[derive(Serialize)]
pub(crate) struct ResearchReport {
    kind: &'static str,
    topic: String,
    citations: Vec<Citation>,
    comparison: Option<Comparison>,
}

#[derive(Serialize)]
struct Citation {
    title: String,
    candidate_id: String,
    search_packet_id: String,
    source_packet_id: String,
    source_uri: String,
    observed_at_unix_ms: u64,
    raw_body_sha256: String,
    passage: Passage,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
enum Comparison {
    ExactPassage,
    Wordings {
        shared_terms: Vec<String>,
        first_only: Vec<String>,
        second_only: Vec<String>,
    },
}

impl ResearchReport {
    pub(crate) fn complete(&self) -> bool {
        self.citations.len() >= 2
    }

    pub(crate) fn units(&self) -> Vec<String> {
        let mut units = vec![format!(
            "「{}」について、本文で対象を確認できた資料は{}件です。",
            self.topic,
            self.citations.len()
        )];
        for (index, citation) in self.citations.iter().enumerate() {
            units.push(format!(
                "【{}】{}「{}」",
                index + 1,
                citation.title,
                citation.passage.text
            ));
            units.push(format!(
                "出典 {}。取得時刻 UNIX {} ミリ秒。本文 SHA-256 {}。引用の原文バイト範囲 {}..{}、範囲 SHA-256 {}。",
                citation.source_uri,
                citation.observed_at_unix_ms,
                citation.raw_body_sha256,
                citation.passage.source_start,
                citation.passage.source_end,
                citation.passage.source_span_sha256
            ));
        }
        match &self.comparison {
            Some(Comparison::ExactPassage) => {
                units.push("【1】【2】の引用には同じ文面が記載されています。".into());
            }
            Some(Comparison::Wordings {
                shared_terms,
                first_only,
                second_only,
            }) => {
                if shared_terms.is_empty() {
                    units.push("両方に明記された比較上の共通点は確認できませんでした。".into());
                } else {
                    units.push(format!(
                        "【1】【2】の引用に共通する語句は{}です。",
                        quoted(shared_terms)
                    ));
                }
                for (index, terms) in [(1, first_only), (2, second_only)] {
                    if !terms.is_empty() {
                        units.push(format!(
                            "【{index}】の引用に固有の語句は{}です。",
                            quoted(terms)
                        ));
                    }
                }
                if first_only.is_empty() && second_only.is_empty() {
                    units.push("【1】【2】は異なる文面で記述しています。".into());
                }
                units.push(
                    "上の違いは引用箇所を照合した結果です。資料間の主張の食い違いは未確認です。"
                        .into(),
                );
            }
            None => units.push("二つの異なる本文で対象を確認できず、比較を保留しました。".into()),
        }
        if !self.citations.is_empty() {
            units.push(
                "表示用の引用からリンクや注記の記法を除きました。掲載・改訂時点と資料の独立性は未確認です。"
                    .into(),
            );
        }
        units
    }
}

struct ReadSource<'a> {
    candidate: &'a SearchCandidate,
    packet: &'a EvidencePacket,
    quotes: Vec<SupportedPassage>,
}

struct SupportedPassage {
    passage: Passage,
    terms: BTreeSet<String>,
}

pub(crate) fn compare(
    topic: &str,
    sources: &[(&SearchCandidate, &EvidencePacket)],
    permissions: &Permissions,
) -> Result<ResearchReport> {
    if topic.trim().is_empty()
        || topic.len() > MAX_TOPIC_BYTES
        || topic.chars().any(char::is_control)
    {
        return Err(failure(
            "WEB_RESEARCH_TOPIC_UNSUPPORTED",
            "本文と照合できる短い調査対象を指定してください。",
        ));
    }
    if sources.len() > MAX_RESEARCH_SOURCES {
        return Err(failure(
            "WEB_RESEARCH_SOURCE_LIMIT",
            "比較に使う候補が上限を超えました。",
        ));
    }
    let mut selected = Vec::new();
    let mut uris = BTreeSet::new();
    let mut digests = BTreeSet::new();
    for &(candidate, packet) in sources {
        ports::validate_successful_packet(packet, permissions)?;
        if candidate.provider != SearchProvider::Wikipedia
            || candidate.provider_id != SearchProvider::Wikipedia.provider_id()
            || candidate.rank == 0
            || candidate.title.trim().is_empty()
            || candidate.title.chars().any(char::is_control)
            || packet.request_uri != search::candidate_body_uri(candidate)?
        {
            return Err(invalid_source());
        }
        if uris.contains(packet.final_uri.as_str()) || digests.contains(packet.raw_sha256.as_str())
        {
            continue;
        }
        uris.insert(packet.final_uri.as_str());
        digests.insert(packet.raw_sha256.as_str());
        let body = packet
            .extracted_text
            .as_deref()
            .ok_or_else(invalid_source)?;
        let mut quotes = Vec::new();
        for passage in web_answer::readable_research_passages(body, topic, MAX_PASSAGES) {
            if !supports_discussion(&passage.text, topic) {
                continue;
            }
            let raw = body
                .get(passage.source_start..passage.source_end)
                .filter(|source| !source.is_empty())
                .ok_or_else(invalid_source)?;
            if digest(raw.as_bytes()) != passage.source_span_sha256 {
                return Err(invalid_source());
            }
            quotes.push(SupportedPassage {
                terms: terms(&passage.text, topic),
                passage,
            });
        }
        if !quotes.is_empty() {
            selected.push(ReadSource {
                candidate,
                packet,
                quotes,
            });
        }
    }
    Ok(report(topic, &selected))
}

fn invalid_source() -> Error {
    failure(
        "WEB_RESEARCH_SOURCE_MISMATCH",
        "検索で選んだ資料のURI、実本文、引用範囲を照合できませんでした。",
    )
}

fn subject_separator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '、' | '。'
                | '，'
                | ','
                | '：'
                | ':'
                | '・'
                | '（'
                | '）'
                | '('
                | ')'
                | '「'
                | '」'
                | '『'
                | '』'
                | '〈'
                | '〉'
                | '《'
                | '》'
                | '【'
                | '】'
                | '“'
                | '”'
                | '"'
        )
}

fn navigation_ending(text: &str) -> bool {
    let text = text.trim_end_matches(['。', '.', '！', '!', '？', '?', ' ']);
    [
        "を参照",
        "を参照のこと",
        "を参照してください",
        "を参照されたい",
        "を参照すること",
        "を見よ",
        "を参照の事",
    ]
    .iter()
    .any(|ending| text.ends_with(ending))
}

fn supports_discussion(text: &str, topic: &str) -> bool {
    let body = text.trim_end_matches(['。', '.', ' ']);
    if navigation_ending(body) {
        return false;
    }
    let body = [('（', '）'), ('(', ')')]
        .into_iter()
        .find_map(|(start, end)| {
            let (main, note) = body.strip_suffix(end)?.rsplit_once(start)?;
            navigation_ending(note).then_some(main)
        })
        .unwrap_or(body);
    contains_subject(body, topic)
}

fn contains_subject(text: &str, topic: &str) -> bool {
    text.match_indices(topic).any(|(start, _)| {
        let before = text[..start].chars().next_back();
        let tail = &text[start + topic.len()..];
        let after = tail.chars().next();
        let boundary = |ch| {
            subject_separator(ch)
                || matches!(
                    ch,
                    'は' | 'が' | 'の' | 'を' | 'に' | 'と' | 'へ' | 'や' | 'も' | 'で'
                )
        };
        (before.is_none_or(boundary)
            || ["対して", "一方で", "例えば", "たとえば", "つまり"]
                .into_iter()
                .any(|transition| text[..start].ends_with(transition)))
            && (after.is_none_or(boundary)
                || [
                    "から",
                    "まで",
                    "より",
                    "速度",
                    "周期",
                    "方向",
                    "比率",
                    "測定値",
                ]
                .into_iter()
                .any(|particle| tail.starts_with(particle)))
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Script {
    Kanji,
    Katakana,
    Latin,
}

fn script(ch: char) -> Option<Script> {
    match ch {
        '一'..='鿿' | '㐀'..='䶿' | '々' => Some(Script::Kanji),
        'ァ'..='ヺ' | 'ー' => Some(Script::Katakana),
        'a'..='z' | 'A'..='Z' => Some(Script::Latin),
        _ => None,
    }
}

fn terms(text: &str, topic: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut start = 0;
    let mut current = None;
    for (offset, next) in text
        .char_indices()
        .map(|(offset, ch)| (offset, script(ch)))
        .chain([(text.len(), None)])
    {
        if next == current {
            continue;
        }
        if let Some(kind) = current {
            let term = &text[start..offset];
            let min = if kind == Script::Latin { 3 } else { 2 };
            if (min..=24).contains(&term.chars().count())
                && !topic.contains(term)
                && !term.contains(topic)
            {
                result.insert(term.to_owned());
                if result.len() == MAX_TERMS {
                    break;
                }
            }
        }
        start = offset;
        current = next;
    }
    result
}

fn salient<'a>(terms: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut words = terms.collect::<Vec<_>>();
    words.sort_by_key(|&term| (std::cmp::Reverse(term.chars().count()), term.as_str()));
    words.into_iter().take(DISPLAY_TERMS).cloned().collect()
}

fn quoted(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("「{term}」"))
        .collect::<Vec<_>>()
        .join("、")
}

fn cite(source: &ReadSource<'_>, quote: &SupportedPassage) -> Citation {
    Citation {
        title: source.candidate.title.clone(),
        candidate_id: source.candidate.id.clone(),
        search_packet_id: source.candidate.search_packet_id.clone(),
        source_packet_id: source.packet.id.clone(),
        source_uri: source.packet.final_uri.clone(),
        observed_at_unix_ms: source.packet.observed_at_unix_ms,
        raw_body_sha256: source.packet.raw_sha256.clone(),
        passage: quote.passage.clone(),
    }
}

fn report(topic: &str, sources: &[ReadSource<'_>]) -> ResearchReport {
    let mut best = None;
    let mut best_score = (false, 0);
    for (index, first) in sources.iter().enumerate() {
        for second in &sources[index + 1..] {
            for a in &first.quotes {
                for b in &second.quotes {
                    let score = (
                        a.passage.text == b.passage.text,
                        a.terms.intersection(&b.terms).count(),
                    );
                    if best.is_none() || score > best_score {
                        best_score = score;
                        best = Some((first, a, second, b));
                    }
                }
            }
        }
    }
    let (citations, comparison) = if let Some((first, a, second, b)) = best {
        let comparison = if best_score.0 {
            Comparison::ExactPassage
        } else {
            Comparison::Wordings {
                shared_terms: salient(a.terms.intersection(&b.terms)),
                first_only: salient(a.terms.difference(&b.terms)),
                second_only: salient(b.terms.difference(&a.terms)),
            }
        };
        (vec![cite(first, a), cite(second, b)], Some(comparison))
    } else {
        let citations = sources
            .first()
            .map(|source| cite(source, &source.quotes[0]));
        (citations.into_iter().collect(), None)
    };
    ResearchReport {
        kind: VERSION,
        topic: topic.into(),
        citations,
        comparison,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_and_topic_only_in_a_reference_are_not_discussion() {
        assert!(!supports_discussion(
            "詳細は閏秒、地球の自転を参照のこと。",
            "地球の自転"
        ));
        assert!(!supports_discussion(
            "一年の長さには変動がある（地球の自転を参照）。",
            "地球の自転"
        ));
        assert!(supports_discussion(
            "対して地球の自転は遅くなる。",
            "地球の自転"
        ));
        assert!(supports_discussion(
            "地球の自転速度は、観測値から算出する。",
            "地球の自転"
        ));
        assert!(!supports_discussion("別の惑星の自転車を展示した。", "自転"));
    }

    fn sources(title: &str, rank: u32, text: &str) -> (SearchCandidate, EvidencePacket) {
        let provider = SearchProvider::Wikipedia;
        let candidate = SearchCandidate {
            provider,
            provider_id: provider.provider_id().into(),
            rank,
            title: title.into(),
            uri: format!("https://ja.wikipedia.org/wiki/{title}"),
            source_key: title.into(),
            id: digest(title.as_bytes()),
            search_packet_id: digest(b"listing"),
        };
        let uri = search::candidate_body_uri(&candidate).unwrap();
        let mut packet = EvidencePacket {
            id: String::new(),
            mode: "LIVE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: uri.clone(),
            final_uri: uri.clone(),
            redirect_chain: vec![uri.clone()],
            observed_at_unix_ms: 100 + u64::from(rank),
            provider_id: "https-direct-url-v1".into(),
            http_status: 200,
            raw_body: text.as_bytes().to_vec(),
            raw_sha256: digest(text.as_bytes()),
            extracted_text: Some(text.into()),
            parser_version: "strict-utf8-identity-v1".into(),
            independence_group: uri,
            failure: None,
            retry_after: None,
        };
        packet.id = ports::fetch_packet_identity(&packet).unwrap();
        (candidate, packet)
    }

    fn permission() -> Permissions {
        Permissions {
            read_hosts: BTreeSet::from(["ja.wikipedia.org".into()]),
            ..Permissions::default()
        }
    }

    #[test]
    fn compares_distinct_articles_and_retains_each_exact_quote_range() {
        let a = sources(
            "Alpha",
            1,
            "== 方式 ==\n[[蓄電池]]は電力を蓄え、変換装置を通じて家庭に供給する。",
        );
        let b = sources(
            "Bravo",
            2,
            "家庭の蓄電池は電力を蓄え、設備の停電対策に使う。",
        );
        let captured = [(&a.0, &a.1), (&b.0, &b.1)];
        let report = compare("蓄電池", &captured, &permission()).unwrap();
        assert!(report.complete());
        let Some(Comparison::Wordings {
            shared_terms,
            first_only,
            second_only,
        }) = &report.comparison
        else {
            panic!("expected independently observed wordings");
        };
        assert!(shared_terms.iter().any(|term| term == "電力"));
        assert!(first_only.iter().any(|term| term == "変換装置"));
        assert!(second_only.iter().any(|term| term == "停電対策"));
        for (citation, (_, packet)) in report.citations.iter().zip(captured) {
            let span = &citation.passage;
            assert_eq!(
                span.source_span_sha256,
                digest(&packet.raw_body[span.source_start..span.source_end])
            );
            assert_eq!(citation.observed_at_unix_ms, packet.observed_at_unix_ms);
        }
    }

    #[test]
    fn mirrored_body_and_compound_name_do_not_make_a_second_source() {
        let a = sources("Alpha", 1, "富士山は静岡県と山梨県にまたがる。");
        let mirror = sources("Bravo", 2, "富士山は静岡県と山梨県にまたがる。");
        let compound = sources(
            "Charlie",
            3,
            "富士山本体の名称です。新富士山は別の名前です。",
        );
        let report = compare(
            "富士山",
            &[
                (&a.0, &a.1),
                (&mirror.0, &mirror.1),
                (&compound.0, &compound.1),
            ],
            &permission(),
        )
        .unwrap();
        assert!(!report.complete());
        assert_eq!(report.citations.len(), 1);
        assert!(compare("富士山", &[(&mirror.0, &a.1)], &permission()).is_err());
    }
}
