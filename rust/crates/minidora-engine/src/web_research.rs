//! Compare retained, subject-matched passages from distinct Web bodies.
use crate::{
    language, math, ports,
    search::{self, SearchCandidate, SearchProvider},
    web_answer::{self, Passage},
};
use minidora_domain::{Error, EvidencePacket, Permissions, Result, digest, failure};
use regex::Regex;
use roxmltree::{Document, Node, ParsingOptions};
use serde::Serialize;
use std::{collections::BTreeSet, sync::LazyLock};

pub(crate) const MAX_RESEARCH_SOURCES: usize = 4;
const MAX_TOPIC_BYTES: usize = 512;
const MAX_PASSAGES: usize = 12;
const MAX_TERMS: usize = 32;
const DISPLAY_TERMS: usize = 3;
const VERSION: &str = "minidora.web-research.paired-wikipedia-prose.v1";
const CURRENT_VERSION: &str = "minidora.web-research.paired-wikipedia-prose.v2";
const DIVERSE_VERSION: &str = "minidora.web-research.distinct-source-families.v3";
const MAX_JATS_BYTES: usize = 800_000;
const MAX_JATS_NODES: u32 = 40_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ReportVersion {
    Legacy,
    Current,
    Diverse,
}

impl ReportVersion {
    fn kind(self) -> &'static str {
        match self {
            Self::Legacy => VERSION,
            Self::Current => CURRENT_VERSION,
            Self::Diverse => DIVERSE_VERSION,
        }
    }
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    journal: Option<JournalProvenance>,
}

#[derive(Clone, Serialize)]
struct JournalProvenance {
    journal: String,
    publisher: String,
    pmcid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    doi: Option<String>,
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
    StatedValues {
        first: ValueCitation,
        second: ValueCitation,
        unit: String,
        equal: bool,
    },
}

#[derive(Serialize)]
struct ValueCitation {
    literal: String,
    exact: String,
    source_start: usize,
    source_end: usize,
}

impl ResearchReport {
    pub(crate) fn has_citations(&self) -> bool {
        !self.citations.is_empty()
    }

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
            if let Some(source) = &citation.journal {
                units.push(format!(
                    "掲載誌 {}。刊行元 {}。保存先の論文識別子 {}。",
                    source.journal, source.publisher, source.pmcid
                ));
                if let Some(doi) = &source.doi {
                    units.push(format!("論文 DOI {doi}。"));
                }
            }
        }
        match &self.comparison {
            Some(Comparison::ExactPassage) => {
                units.push("【1】【2】の引用には同じ文面が記載されています。".into());
            }
            Some(Comparison::StatedValues {
                first,
                second,
                unit,
                equal,
            }) => {
                let relation = if *equal {
                    "一致しています"
                } else {
                    "異なります"
                };
                units.push(format!(
                    "数値以外が同じ文面で、【1】は {} {}、【2】は {} {} と記載しています。記載値は{}。",
                    first.literal, unit, second.literal, unit, relation
                ));
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
            None if self.kind == DIVERSE_VERSION => units.push(
                "百科事典と刊行元を確認できる論文の本文が揃わず、比較を保留しました。".into(),
            ),
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

    pub(crate) fn localized_units(&self, english: bool) -> Vec<String> {
        if !english {
            return self.units();
        }
        let mut units = vec![format!(
            "Passages discussing “{}” were found in {} source bodies.",
            self.topic,
            self.citations.len()
        )];
        for (index, citation) in self.citations.iter().enumerate() {
            units.push(format!(
                "[{}] {} “{}”",
                index + 1,
                citation.title,
                citation.passage.text
            ));
            units.push(format!(
                "Source {}. Retrieved at {} Unix milliseconds. Body SHA-256 {}. Original byte range {}..{} with SHA-256 {}.",
                citation.source_uri,
                citation.observed_at_unix_ms,
                citation.raw_body_sha256,
                citation.passage.source_start,
                citation.passage.source_end,
                citation.passage.source_span_sha256
            ));
            if let Some(source) = &citation.journal {
                units.push(format!(
                    "Journal {}. Publisher {}. Archived article {}.",
                    source.journal, source.publisher, source.pmcid
                ));
                if let Some(doi) = &source.doi {
                    units.push(format!("Article DOI {doi}."));
                }
            }
        }
        match &self.comparison {
            Some(Comparison::ExactPassage) => {
                units.push("The displayed excerpts [1] and [2] have the same wording.".into());
            }
            Some(Comparison::StatedValues {
                first,
                second,
                unit,
                equal,
            }) => {
                let relation = if *equal { "match" } else { "differ" };
                units.push(format!(
                    "The surrounding wording is identical. [1] states {} {} and [2] states {} {}. The stated values {relation}.",
                    first.literal, unit, second.literal, unit
                ));
            }
            Some(Comparison::Wordings {
                shared_terms,
                first_only,
                second_only,
            }) => {
                if shared_terms.is_empty() {
                    units.push(
                        "The selected excerpts have no shared terms in this comparison.".into(),
                    );
                } else {
                    units.push(format!(
                        "The terms shared by both excerpts are {}.",
                        quoted_english(shared_terms)
                    ));
                }
                for (index, terms) in [(1, first_only), (2, second_only)] {
                    if !terms.is_empty() {
                        units.push(format!(
                            "Only excerpt [{index}] uses {}.",
                            quoted_english(terms)
                        ));
                    }
                }
                if first_only.is_empty() && second_only.is_empty() {
                    units.push("The two excerpts use different wording.".into());
                }
                units.push("Whether the sources make conflicting claims is unverified.".into());
            }
            None if self.kind == DIVERSE_VERSION => units.push(
                "The comparison is pending until both an encyclopedia and a journal article with an identified publisher provide usable passages.".into(),
            ),
            None => {
                units.push("Comparing the topic requires passages from two distinct bodies.".into())
            }
        }
        if self.has_citations() {
            units.push(
                "Links and note markup were removed from the displayed excerpts. Publication dates, revisions and source independence are unverified."
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
    journal: Option<JournalProvenance>,
}

struct SupportedPassage {
    passage: Passage,
    terms: BTreeSet<String>,
    stated: Option<StatedNumber>,
}

pub(crate) fn compare(
    topic: &str,
    sources: &[(&SearchCandidate, &EvidencePacket)],
    permissions: &Permissions,
) -> Result<ResearchReport> {
    compare_sources(topic, sources, permissions, ReportVersion::Legacy)
}

pub(crate) fn compare_current(
    topic: &str,
    sources: &[(&SearchCandidate, &EvidencePacket)],
    permissions: &Permissions,
) -> Result<ResearchReport> {
    compare_sources(topic, sources, permissions, ReportVersion::Current)
}

pub(crate) fn compare_diverse(
    topic: &str,
    sources: &[(&SearchCandidate, &EvidencePacket)],
    permissions: &Permissions,
) -> Result<ResearchReport> {
    compare_sources(topic, sources, permissions, ReportVersion::Diverse)
}

fn compare_sources(
    topic: &str,
    sources: &[(&SearchCandidate, &EvidencePacket)],
    permissions: &Permissions,
    version: ReportVersion,
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
    let mut pmcids = BTreeSet::new();
    let mut dois = BTreeSet::new();
    let current = version != ReportVersion::Legacy;
    for &(candidate, packet) in sources {
        let journal_source = version == ReportVersion::Diverse
            && candidate.provider == SearchProvider::EuropePmcOpenAccess;
        if version == ReportVersion::Diverse {
            search::validate_research_body(candidate, packet, permissions)?;
        } else {
            ports::validate_successful_packet(packet, permissions)?;
        }
        if !(candidate.provider == SearchProvider::Wikipedia
            || (current && candidate.provider == SearchProvider::WikipediaEnglish)
            || journal_source)
            || candidate.provider_id != candidate.provider.provider_id()
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
        let (passages, journal) = if journal_source {
            let Some((source, passages)) = journal_body(body, &candidate.source_key, topic) else {
                continue;
            };
            if pmcids.contains(&source.pmcid)
                || source.doi.as_ref().is_some_and(|doi| dois.contains(doi))
            {
                continue;
            }
            (passages, Some(source))
        } else if current {
            (current_passages(body, topic, MAX_PASSAGES), None)
        } else {
            (
                web_answer::readable_research_passages(body, topic, MAX_PASSAGES),
                None,
            )
        };
        for passage in passages {
            let discusses = if current {
                discussion_body(&passage.text).is_some_and(|text| {
                    !text
                        .get(..4)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("see "))
                        && topic_positions(text, topic)
                            .any(|start| current_subject_at(text, topic, start))
                })
            } else {
                supports_discussion(&passage.text, topic)
            };
            if !discusses {
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
                stated: if current && raw.trim() == passage.text {
                    let start = passage.source_start + raw.len() - raw.trim_start().len();
                    stated_number(&passage.text, start)
                } else {
                    None
                },
                passage,
            });
        }
        if !quotes.is_empty() {
            if let Some(source) = &journal {
                pmcids.insert(source.pmcid.clone());
                dois.extend(source.doi.iter().cloned());
            }
            selected.push(ReadSource {
                candidate,
                packet,
                quotes,
                journal,
            });
        }
    }
    Ok(report(topic, &selected, version))
}

fn current_passages(text: &str, topic: &str, limit: usize) -> Vec<Passage> {
    web_answer::readable_research_passages_matching(text, limit, |sentence| {
        topic_positions(sentence, topic).next().is_some()
    })
}

fn journal_body(
    xml: &str,
    expected_pmcid: &str,
    topic: &str,
) -> Option<(JournalProvenance, Vec<Passage>)> {
    if xml.len() > MAX_JATS_BYTES || xml.contains("<!ENTITY") {
        return None;
    }
    let allow_dtd = exterior_jats_declaration(xml)?;
    let document = Document::parse_with_options(
        xml,
        ParsingOptions {
            allow_dtd,
            nodes_limit: MAX_JATS_NODES,
            entity_resolver: None,
        },
    )
    .ok()?;
    let article = document.root_element();
    if !jats_tag(article, "article") {
        return None;
    }
    let source = journal_provenance(one_jats_child(article, "front")?, expected_pmcid)?;
    let body = one_jats_child(article, "body")?;
    let mut passages = Vec::new();
    for paragraph in body.descendants().filter(|node| jats_tag(*node, "p")) {
        if !paragraph
            .ancestors()
            .skip(1)
            .take_while(|ancestor| *ancestor != body)
            .all(|ancestor| jats_tag(ancestor, "sec"))
        {
            continue;
        }
        let Some((start, visible)) = journal_paragraph(xml, paragraph) else {
            continue;
        };
        for mut passage in current_passages(&visible, topic, MAX_PASSAGES - passages.len()) {
            passage.source_start += start;
            passage.source_end += start;
            passage.source_span_sha256 = digest(
                xml.get(passage.source_start..passage.source_end)?
                    .as_bytes(),
            );
            passage.text = passage
                .text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if let Some(prefix) = passage.text.strip_suffix(['.', '!', '?', '。']) {
                let punctuation = &passage.text[prefix.len()..];
                passage.text = format!("{}{punctuation}", prefix.trim_end());
            }
            passages.push(passage);
        }
        if passages.len() == MAX_PASSAGES {
            break;
        }
    }
    Some((source, passages))
}

/// Project plain paragraph text and bracketed bibliographic markers to the
/// same byte length so the sentence range still identifies the original XML.
fn journal_paragraph(xml: &str, paragraph: Node<'_, '_>) -> Option<(usize, String)> {
    let start = paragraph.first_child()?.range().start;
    let end = paragraph.last_child()?.range().end;
    let mut visible = xml.get(start..end)?.as_bytes().to_vec();
    let mut references = Vec::new();
    for child in paragraph.children() {
        let range = child.range();
        if child.is_text() && !xml.get(range.clone())?.contains(['<', '>', '&']) {
            continue;
        }
        if jats_tag(child, "xref")
            && child.attribute("ref-type") == Some("bibr")
            && single_text_child(child)
                .and_then(|text| text.text())
                .is_some_and(|text| {
                    !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit())
                })
        {
            references.push(range);
            continue;
        }
        return None;
    }

    let mut references = references.into_iter().peekable();
    while let Some(reference) = references.next() {
        let before = xml.get(start..reference.start)?.trim_ascii_end();
        let group_start = start + before.strip_suffix('[')?.len();
        let mut group_end = reference.end;
        while let Some(next) = references.peek() {
            let separator = xml.get(group_end..next.start)?.trim_ascii();
            if !matches!(separator, "," | ";" | "-" | "–") {
                break;
            }
            group_end = references.next()?.end;
        }
        let remainder = xml.get(group_end..end)?;
        let closing = remainder.trim_ascii_start();
        let after_bracket = closing.strip_prefix(']')?.trim_ascii_start();
        if !after_bracket.starts_with(['.', '?', '!', '。']) {
            return None;
        }
        group_end += remainder.len() - closing.len() + 1;
        visible
            .get_mut(group_start - start..group_end - start)?
            .fill(b' ');
    }
    for byte in &mut visible {
        if matches!(*byte, b'\t' | b'\r' | b'\n') {
            *byte = b' ';
        }
    }
    Some((start, String::from_utf8(visible).ok()?))
}

fn exterior_jats_declaration(xml: &str) -> Option<bool> {
    // A bare external declaration is inert. Never load its identifier or
    // admit an internal subset, entity definition or another declaration.
    static EXTERNAL_DTD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r#"^<!DOCTYPE[ \t\r\n]+article[ \t\r\n]+(?:SYSTEM[ \t\r\n]+"[^"<>\[\]]+""#,
            r#"|PUBLIC[ \t\r\n]+"[^"<>\[\]]+"[ \t\r\n]+"[^"<>\[\]]+")[ \t\r\n]*>"#
        ))
        .expect("bare external JATS declaration")
    });
    let mut declarations = xml.match_indices("<!DOCTYPE");
    let Some((start, _)) = declarations.next() else {
        return Some(false);
    };
    if declarations.next().is_some() || start > 1024 || !EXTERNAL_DTD.is_match(&xml[start..]) {
        return None;
    }
    Some(true)
}

fn jats_tag(node: Node<'_, '_>, name: &str) -> bool {
    node.has_tag_name(name) && node.tag_name().namespace().is_none()
}

fn one_jats_child<'a, 'input: 'a>(
    parent: Node<'a, 'input>,
    name: &str,
) -> Option<Node<'a, 'input>> {
    let mut named = parent.children().filter(|node| jats_tag(*node, name));
    let node = named.next()?;
    named.next().is_none().then_some(node)
}

fn single_text_child<'a, 'input: 'a>(parent: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    let mut children = parent.children();
    let node = children.next()?;
    (node.is_text() && children.next().is_none()).then_some(node)
}

fn metadata_text(node: Node<'_, '_>) -> Option<String> {
    let child = single_text_child(node)?;
    let text = child.text()?.trim();
    (!text.is_empty() && text.len() <= 256 && !text.chars().any(char::is_control))
        .then(|| text.to_owned())
}

fn journal_provenance(front: Node<'_, '_>, expected_pmcid: &str) -> Option<JournalProvenance> {
    let expected_number = expected_pmcid.strip_prefix("PMC")?;
    let article_meta = one_jats_child(front, "article-meta")?;
    let mut matched = false;
    let mut doi = None;
    for identifier in article_meta
        .children()
        .filter(|node| jats_tag(*node, "article-id"))
    {
        match identifier.attribute("pub-id-type") {
            Some("pmc" | "pmcid") => {
                let id = metadata_text(identifier)?;
                if id.strip_prefix("PMC").unwrap_or(&id) != expected_number {
                    return None;
                }
                matched = true;
            }
            Some("doi") => {
                let id = metadata_text(identifier)?.to_ascii_lowercase();
                if !id.starts_with("10.")
                    || !id.contains('/')
                    || id.chars().any(char::is_whitespace)
                    || doi.as_ref().is_some_and(|previous| previous != &id)
                {
                    return None;
                }
                doi = Some(id);
            }
            _ => {}
        }
    }
    if !matched {
        return None;
    }
    let journal_meta = one_jats_child(front, "journal-meta")?;
    let title_parent = one_jats_child(journal_meta, "journal-title-group").unwrap_or(journal_meta);
    let journal = metadata_text(one_jats_child(title_parent, "journal-title")?)?;
    let publisher = metadata_text(one_jats_child(
        one_jats_child(journal_meta, "publisher")?,
        "publisher-name",
    )?)?;
    Some(JournalProvenance {
        journal,
        publisher,
        pmcid: expected_pmcid.into(),
        doi,
    })
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
    discussion_body(text).is_some_and(|body| contains_subject(body, topic))
}

fn discussion_body(text: &str) -> Option<&str> {
    let body = text.trim_end_matches(['。', '.', ' ']);
    if navigation_ending(body) {
        return None;
    }
    Some(
        [('（', '）'), ('(', ')')]
            .into_iter()
            .find_map(|(start, end)| {
                let (main, note) = body.strip_suffix(end)?.rsplit_once(start)?;
                navigation_ending(note).then_some(main)
            })
            .unwrap_or(body),
    )
}

fn contains_subject(text: &str, topic: &str) -> bool {
    text.match_indices(topic)
        .any(|(start, _)| subject_at(text, topic, start))
}

fn topic_positions<'a>(text: &'a str, topic: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.char_indices()
        .map(|(start, _)| start)
        .filter(move |&start| {
            text.get(start..start + topic.len())
                .is_some_and(|part| part.eq_ignore_ascii_case(topic))
        })
}

fn current_subject_at(text: &str, topic: &str, start: usize) -> bool {
    if !topic.is_ascii() {
        return subject_at(text, topic, start);
    }
    let boundary = |ch: char| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-';
    text[..start].chars().next_back().is_none_or(boundary)
        && text[start + topic.len()..]
            .chars()
            .next()
            .is_none_or(boundary)
}

fn subject_at(text: &str, topic: &str, start: usize) -> bool {
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
}

struct StatedNumber {
    start: usize,
    end: usize,
    source_base: usize,
    exact: String,
    unit: String,
}

impl StatedNumber {
    fn citation(&self, text: &str) -> ValueCitation {
        ValueCitation {
            literal: text[self.start..self.end].into(),
            exact: self.exact.clone(),
            source_start: self.source_base + self.start,
            source_end: self.source_base + self.end,
        }
    }
}

fn stated_number(text: &str, source_base: usize) -> Option<StatedNumber> {
    static QUANTITY: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?P<number>[+-]?[0-9]{1,32}(?:\.[0-9]{1,32})?)[ \t　]*",
            r"(?P<unit>km/h|m/s|MHz|kHz|Hz|mm|cm|km|mg|kg|mL|°C|℃|%|％|g|m|L)"
        ))
        .expect("bounded exact quantity grammar")
    });
    let found = QUANTITY.captures(text)?;
    let number = found.name("number")?;
    let unit = found.name("unit")?;
    let prefix = &text[..number.start()];
    let suffix = &text[unit.end()..];
    let other_number = prefix.chars().chain(suffix.chars()).any(char::is_numeric);
    let prefix_word = prefix.trim_end().to_ascii_lowercase();
    let suffix_word = suffix.trim_start().to_ascii_lowercase();
    if other_number
        || prefix
            .chars()
            .next_back()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || ".,/+-−~～±≈≒<>≤≥".contains(ch))
        || suffix
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || "/^+-−~～±".contains(ch))
        || [
            "about",
            "around",
            "approximately",
            "at least",
            "at most",
            "less than",
            "more than",
            "約",
            "およそ",
            "少なくとも",
        ]
        .into_iter()
        .any(|qualifier| prefix_word.ends_with(qualifier))
        || [
            "or more", "or less", "at least", "at most", "以上", "以下", "未満", "超", "程度",
            "ほど", "前後",
        ]
        .into_iter()
        .any(|qualifier| suffix_word.starts_with(qualifier))
    {
        return None;
    }
    let exact = math::scalar(number.as_str()).ok()?;
    Some(StatedNumber {
        start: number.start(),
        end: number.end(),
        source_base,
        exact: language::rational_text(&exact),
        unit: unit.as_str().into(),
    })
}

fn comparable_numbers<'a>(
    first: &'a SupportedPassage,
    second: &'a SupportedPassage,
) -> Option<(&'a StatedNumber, &'a StatedNumber)> {
    let (left, right) = (first.stated.as_ref()?, second.stated.as_ref()?);
    let (first, second) = (&first.passage.text, &second.passage.text);
    (left.unit == right.unit
        && first[..left.start] == second[..right.start]
        && first[left.end..] == second[right.end..])
        .then_some((left, right))
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

fn quoted_english(terms: &[String]) -> String {
    terms
        .iter()
        .map(|term| format!("“{term}”"))
        .collect::<Vec<_>>()
        .join(", ")
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
        journal: source.journal.clone(),
    }
}

fn report(topic: &str, sources: &[ReadSource<'_>], version: ReportVersion) -> ResearchReport {
    let mut best = None;
    let mut best_score = (false, false, 0);
    for (index, first) in sources.iter().enumerate() {
        for second in &sources[index + 1..] {
            if version == ReportVersion::Diverse
                && first.journal.is_some() == second.journal.is_some()
            {
                continue;
            }
            for a in &first.quotes {
                for b in &second.quotes {
                    if version == ReportVersion::Diverse
                        && a.passage.text.eq_ignore_ascii_case(&b.passage.text)
                    {
                        continue;
                    }
                    let score = (
                        comparable_numbers(a, b).is_some(),
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
        let comparison = if let Some((left, right)) = comparable_numbers(a, b) {
            Comparison::StatedValues {
                first: left.citation(&a.passage.text),
                second: right.citation(&b.passage.text),
                unit: left.unit.clone(),
                equal: left.exact == right.exact,
            }
        } else if best_score.1 {
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
        kind: version.kind(),
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
        sources_in(SearchProvider::Wikipedia, title, rank, text)
    }

    fn sources_in(
        provider: SearchProvider,
        title: &str,
        rank: u32,
        text: &str,
    ) -> (SearchCandidate, EvidencePacket) {
        let candidate = SearchCandidate {
            provider,
            provider_id: provider.provider_id().into(),
            rank,
            title: title.into(),
            uri: format!("https://{}/wiki/{title}", provider.host()),
            source_key: title.into(),
            id: digest(title.as_bytes()),
            search_packet_id: digest(b"listing"),
        };
        let uri = search::candidate_body_uri(&candidate).unwrap();
        (candidate, text_packet(&uri, rank, text))
    }

    fn text_packet(uri: &str, rank: u32, text: &str) -> EvidencePacket {
        let mut packet = EvidencePacket {
            id: String::new(),
            mode: "LIVE".into(),
            packet_kind: "FETCHED_BODY".into(),
            request_uri: uri.into(),
            final_uri: uri.into(),
            redirect_chain: vec![uri.into()],
            observed_at_unix_ms: 100 + u64::from(rank),
            provider_id: "https-direct-url-v1".into(),
            http_status: 200,
            raw_body: text.as_bytes().to_vec(),
            raw_sha256: digest(text.as_bytes()),
            extracted_text: Some(text.into()),
            parser_version: "strict-utf8-identity-v1".into(),
            independence_group: uri.into(),
            failure: None,
            retry_after: None,
        };
        packet.id = ports::fetch_packet_identity(&packet).unwrap();
        packet
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

    #[test]
    fn current_comparison_uses_exact_stated_values_only_for_the_same_wording() {
        let provider = SearchProvider::WikipediaEnglish;
        let permission = Permissions {
            read_hosts: BTreeSet::from([provider.host().into()]),
            ..Permissions::default()
        };
        let first = sources_in(
            provider,
            "Alpha",
            1,
            "The Battery cable is 30.0 m long at noon.",
        );
        for (text, expected) in [
            ("The Battery cable is 30 m long at noon.", Some(true)),
            ("The Battery cable is 33 m long at noon.", Some(false)),
            ("The Battery cable is 33 m long at dusk.", None),
        ] {
            let second = sources_in(provider, "Bravo", 2, text);
            let packets = [(&first.0, &first.1), (&second.0, &second.1)];
            let report = compare_current("battery", &packets, &permission).unwrap();
            assert!(report.complete());
            assert_eq!(report.kind, CURRENT_VERSION);
            if let Some(equal) = expected {
                let Some(Comparison::StatedValues {
                    first: left,
                    second: right,
                    unit,
                    equal: actual,
                }) = &report.comparison
                else {
                    panic!("expected both exactly comparable quantities");
                };
                assert_eq!(*actual, equal);
                assert_eq!(unit, "m");
                assert_eq!(left.exact, "30");
                for (value, (_, packet)) in [left, right].iter().zip(packets) {
                    assert_eq!(
                        &packet.raw_body[value.source_start..value.source_end],
                        value.literal.as_bytes()
                    );
                }
            } else {
                assert!(matches!(
                    report.comparison,
                    Some(Comparison::Wordings { .. })
                ));
            }
            assert!(
                report
                    .localized_units(true)
                    .iter()
                    .any(|unit| unit.contains("source independence are unverified"))
            );
        }
        let unrelated = sources_in(provider, "Bravo", 2, "The batteryhouse cable is 33 m long.");
        let report = compare_current(
            "battery",
            &[(&first.0, &first.1), (&unrelated.0, &unrelated.1)],
            &permission,
        )
        .unwrap();
        assert!(report.has_citations());
        assert!(!report.complete());
        assert!(compare("battery", &[(&first.0, &first.1)], &permission).is_err());
    }

    #[test]
    fn diverse_comparison_requires_the_journal_own_id_publisher_and_original_body() {
        let topic = "heat pump";
        let permissions = Permissions {
            read_hosts: BTreeSet::from(["en.wikipedia.org".into(), "www.ebi.ac.uk".into()]),
            ..Permissions::default()
        };
        let wiki = sources_in(
            SearchProvider::WikipediaEnglish,
            "Heat pump",
            1,
            "A heat pump transfers heat from a colder area to a warmer area.",
        );
        let provider = SearchProvider::EuropePmcOpenAccess;
        let listing_uri = "https://www.ebi.ac.uk/europepmc/webservices/rest/search?query=%22heat+pump%22+AND+OPEN_ACCESS%3AY+AND+IN_EPMC%3AY&format=json&pageSize=5&resultType=core";
        let listing = text_packet(
            listing_uri,
            1,
            r#"{"resultList":{"result":[{"title":"Heat pump trial","pmcid":"PMC123","isOpenAccess":"Y","inEPMC":"Y"}]}}"#,
        );
        let parsed = search::parse_listing(provider, topic, listing).unwrap();
        let [journal] = parsed.candidates.as_slice() else {
            panic!("expected one admissible open-access candidate");
        };
        let uri = search::candidate_body_uri(journal).unwrap();
        let fetch_article = |body: &str| {
            let mut packet = text_packet(&uri, journal.rank, body);
            packet.packet_kind = ports::RESEARCH_XML_PACKET_KIND.into();
            packet.parser_version = ports::RESEARCH_XML_PARSER_VERSION.into();
            packet.id = ports::fetch_packet_identity(&packet).unwrap();
            packet
        };
        let body = concat!(
            r#"<!DOCTYPE article SYSTEM "https://entities.invalid/article.dtd">"#,
            "<article><front><journal-meta><journal-title-group>",
            "<journal-title>Applied Energy Notes</journal-title></journal-title-group>",
            "<publisher><publisher-name>Example Scientific Press</publisher-name></publisher>",
            "</journal-meta><article-meta>",
            r#"<article-id pub-id-type="pmcid">PMC123</article-id>"#,
            r#"<article-id pub-id-type="doi">10.1234/Trial</article-id>"#,
            "<abstract><p>An abstract about a heat pump is metadata.</p></abstract>",
            "</article-meta></front><body><sec>",
            "<p>The heat pump delivered a steady output in winter.</p>",
            "<fig><caption><p>A heat pump in a caption is not an excerpt.</p></caption></fig>",
            "</sec></body></article>"
        );
        let packet = fetch_article(body);
        let report = compare_diverse(
            topic,
            &[(&wiki.0, &wiki.1), (journal, &packet)],
            &permissions,
        )
        .unwrap();
        assert!(report.complete());
        assert_eq!(report.kind, DIVERSE_VERSION);
        let citation = &report.citations[1];
        let provenance = citation.journal.as_ref().unwrap();
        assert_eq!(provenance.publisher, "Example Scientific Press");
        assert_eq!(provenance.pmcid, "PMC123");
        assert_eq!(provenance.doi.as_deref(), Some("10.1234/trial"));
        let span = &citation.passage;
        assert_eq!(
            &packet.raw_body[span.source_start..span.source_end],
            b"The heat pump delivered a steady output in winter."
        );
        assert_eq!(span.source_span_sha256, digest(span.text.as_bytes()));
        assert!(
            report
                .localized_units(true)
                .iter()
                .any(|unit| unit.contains("Publisher Example Scientific Press"))
        );
        let cited_body = body.replace(
            "The heat pump delivered a steady output in winter.",
            concat!(
                "The heat pump delivered a steady output\nin winter [",
                r#"<xref ref-type="bibr" rid="r1">1</xref>, <xref ref-type="bibr" rid="r2">2</xref>]."#
            ),
        );
        let cited = fetch_article(&cited_body);
        let report = compare_diverse(
            topic,
            &[(&wiki.0, &wiki.1), (journal, &cited)],
            &permissions,
        )
        .unwrap();
        assert!(report.complete());
        let span = &report.citations[1].passage;
        assert_eq!(
            span.text,
            "The heat pump delivered a steady output in winter."
        );
        let raw_span = &cited.raw_body[span.source_start..span.source_end];
        assert!(str::from_utf8(raw_span).unwrap().contains("<xref"));
        assert_eq!(span.source_span_sha256, digest(raw_span));
        for incomplete in [
            cited_body.replace(r#"ref-type="bibr""#, r#"ref-type="fig""#),
            cited_body
                .replace("delivered a steady output", "did not deliver output")
                .replace("winter [", "winter ("),
            body.replace(">PMC123<", ">PMC456<"),
            body.replace(
                "<publisher-name>Example Scientific Press</publisher-name>",
                "",
            ),
            body.replace(
                "The heat pump delivered a steady output in winter.",
                "A heat pump transfers heat from a colder area to a warmer area.",
            ),
            body.replace(
                "The heat pump delivered a steady output in winter.",
                "No evidence here.",
            ),
        ] {
            let packet = fetch_article(&incomplete);
            let report = compare_diverse(
                topic,
                &[(&wiki.0, &wiki.1), (journal, &packet)],
                &permissions,
            )
            .unwrap();
            assert!(!report.complete());
            assert_eq!(report.citations.len(), 1);
        }
        assert!(journal_body(
            r#"<!DOCTYPE article [<!ENTITY x SYSTEM "https://entities.invalid/source">]><article>&x;</article>"#,
            "PMC123",
            topic,
        ).is_none());
    }
}
