//! Structural readers for the two supported publishers. Listing dates are
//! retained separately from the publisher's article publication field.
use crate::ports::{JsonKind, JsonNode, strict_json_document};
use minidora_domain::{Result, digest, failure};
use regex::{Match, Regex};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, ops::Range, sync::OnceLock};

const MAX_DOCUMENT_BYTES: usize = 1_500_000;
const MAX_HTML_TAGS: usize = 24_000;
const MAX_CANDIDATES: usize = 12;
const MAX_PASSAGE_CHARS: usize = 110;
const MAX_TOPIC_PASSAGES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum NewsPublisher {
    Nhk,
    Sankei,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewsSourceSpan {
    pub start: usize,
    pub end: usize,
    pub sha256: String,
}

impl NewsSourceSpan {
    fn new(source: &str, range: Range<usize>) -> Self {
        Self {
            sha256: digest(&source.as_bytes()[range.clone()]),
            start: range.start,
            end: range.end,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewsCandidate {
    pub uri: String,
    pub uri_span: NewsSourceSpan,
    pub feed_title: Option<String>,
    pub feed_title_span: Option<NewsSourceSpan>,
    pub feed_pub_date: Option<String>,
    pub feed_pub_date_span: Option<NewsSourceSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NewsArticle {
    pub title: String,
    pub published: String,
    pub passage: String,
    pub title_span: NewsSourceSpan,
    pub published_span: NewsSourceSpan,
    pub passage_span: NewsSourceSpan,
}

impl NewsPublisher {
    pub(crate) const ALL: [Self; 2] = [Self::Nhk, Self::Sankei];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Nhk => "NHK",
            Self::Sankei => "産経新聞",
        }
    }

    pub(crate) fn host(self) -> &'static str {
        match self {
            Self::Nhk => "news.web.nhk",
            Self::Sankei => "www.sankei.com",
        }
    }

    pub(crate) fn listing_url(self) -> &'static str {
        match self {
            Self::Nhk => "https://news.web.nhk/n-data/conf/na/rss/cat0.xml",
            Self::Sankei => "https://www.sankei.com/flash/",
        }
    }

    pub(crate) fn valid_article_uri(self, uri: &str) -> bool {
        match self {
            Self::Nhk => uri
                .strip_prefix("https://news.web.nhk/newsweb/na/nd-")
                .is_some_and(|id| {
                    (1..=64).contains(&id.len())
                        && id.bytes().all(|byte| {
                            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
                        })
                        && id.as_bytes()[0].is_ascii_digit()
                }),
            Self::Sankei => uri
                .strip_prefix("https://www.sankei.com/article/")
                .and_then(|path| path.strip_suffix('/'))
                .and_then(|path| path.split_once('-'))
                .is_some_and(|(date, id)| {
                    date.len() == 8
                        && date.bytes().all(|byte| byte.is_ascii_digit())
                        && (20..=32).contains(&id.len())
                        && id
                            .bytes()
                            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                }),
        }
    }

    pub(crate) fn listing(&self, raw: &str) -> Result<Vec<NewsCandidate>> {
        admit(raw)?;
        let candidates = match self {
            Self::Nhk => nhk_listing(raw),
            Self::Sankei => sankei_listing(raw)?,
        };
        if candidates.is_empty() {
            return Err(failure(
                "NEWS_LISTING_UNAVAILABLE",
                "掲載一覧から対応する記事URLを確認できません。",
            ));
        }
        Ok(candidates)
    }

    pub(crate) fn article(&self, uri: &str, raw: &str, topic: Option<&str>) -> Result<NewsArticle> {
        admit(raw)?;
        if !self.valid_article_uri(uri) {
            return Err(failure(
                "NEWS_ARTICLE_URI",
                "対応する配信元の記事URLではありません。",
            ));
        }
        let tags = html_tags(raw)?;
        let (title, title_range, published, publication_range) =
            article_metadata(*self, &tags, raw, uri)?;
        let body = match self {
            Self::Nhk => element_contents(&tags, "div", |tag| tag.has_class("_1i1d7sh0"))?.1,
            Self::Sankei => {
                let (wrapper, article) =
                    element_contents(&tags, "article", |tag| tag.has_class("article-wrapper"))?;
                if !wrapper.has_class("free") || wrapper.has_class("paid_members") {
                    return Err(article_unavailable());
                }
                element_contents(article, "div", |tag| {
                    tag.has_class("article-body") && tag.has_class("clearfix")
                })?
                .1
            }
        };
        let passage_topic =
            topic.filter(|topic| !crate::news::contains_normalized_topic(&title, topic));
        let (passage, passage_range) =
            article_passage(*self, raw, body, topic.is_some(), passage_topic)?;
        Ok(NewsArticle {
            title,
            published,
            passage,
            title_span: NewsSourceSpan::new(raw, title_range),
            published_span: NewsSourceSpan::new(raw, publication_range),
            passage_span: NewsSourceSpan::new(raw, passage_range),
        })
    }
}

fn admit(raw: &str) -> Result<()> {
    if raw.is_empty() || raw.len() > MAX_DOCUMENT_BYTES || raw.contains('\0') {
        return Err(failure(
            "NEWS_SOURCE_LIMIT",
            "配信元の資料が空、または対応する本文予算外です。",
        ));
    }
    Ok(())
}

fn news_structure() -> minidora_domain::Error {
    failure(
        "NEWS_SOURCE_STRUCTURE",
        "配信元の一覧または記事の構造を一意に確認できません。",
    )
}

fn article_unavailable() -> minidora_domain::Error {
    failure(
        "NEWS_ARTICLE_BODY_UNAVAILABLE",
        "対応する公開記事本文の段落を確認できません。",
    )
}

fn static_regex(cache: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cache.get_or_init(|| Regex::new(pattern).expect("static news source grammar"))
}

fn nhk_listing(raw: &str) -> Vec<NewsCandidate> {
    static ITEM: OnceLock<Regex> = OnceLock::new();
    static FIELD: OnceLock<Regex> = OnceLock::new();
    // Do not expand declared entities or read any fetched URL through XML.
    if !raw.contains("<rss ") || raw.contains("<!DOCTYPE") || raw.contains("<!ENTITY") {
        return Vec::new();
    }
    let Some((header, channel)) = raw.split_once("<channel>") else {
        return Vec::new();
    };
    let Some((channel, end)) = channel.split_once("</channel>") else {
        return Vec::new();
    };
    if channel.contains("<channel>") || !end.contains("</rss>") {
        return Vec::new();
    }
    let channel_offset = header.len() + "<channel>".len();
    let item = static_regex(
        &ITEM,
        r"(?s)<!--.*?(?:-->|\z)|<!\[CDATA\[.*?(?:\]\]>|\z)|<item>(.*?)</item>",
    );
    let field = static_regex(
        &FIELD,
        r"(?s)<(?P<key>title|link|pubDate)>(?P<value>[^<]*)</(?P<close>title|link|pubDate)>",
    );
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    for item_content in item
        .captures_iter(channel)
        .filter_map(|captures| captures.get(1))
        .take(96)
    {
        let mut title = None;
        let mut link = None;
        let mut pub_date = None;
        let mut ambiguous = false;
        for entry in field.captures_iter(item_content.as_str()) {
            let target = match &entry["key"] {
                "title" => &mut title,
                "link" => &mut link,
                _ => &mut pub_date,
            };
            if entry["key"] != entry["close"] || target.is_some() {
                ambiguous = true;
                break;
            }
            *target = entry.name("value");
        }
        let (Some(title), Some(link), Some(pub_date)) = (title, link, pub_date) else {
            continue;
        };
        let uri = link.as_str();
        let Some(title_text) = decode_prefix(title.as_str(), 512).map(|(text, _)| text) else {
            continue;
        };
        if ambiguous
            || title_text.is_empty()
            || !NewsPublisher::Nhk.valid_article_uri(uri)
            || pub_date.as_str().is_empty()
            || pub_date.as_str().len() > 96
            || pub_date.as_str().chars().any(char::is_control)
            || !seen.insert(uri)
        {
            continue;
        }
        let span = |value: Match<'_>| {
            NewsSourceSpan::new(
                raw,
                channel_offset + item_content.start() + value.start()
                    ..channel_offset + item_content.start() + value.end(),
            )
        };
        candidates.push(NewsCandidate {
            uri: uri.into(),
            uri_span: span(link),
            feed_title: Some(title_text),
            feed_title_span: Some(span(title)),
            feed_pub_date: Some(pub_date.as_str().to_owned()),
            feed_pub_date_span: Some(span(pub_date)),
        });
        if candidates.len() == MAX_CANDIDATES {
            break;
        }
    }
    candidates
}

fn sankei_listing(raw: &str) -> Result<Vec<NewsCandidate>> {
    let tags = html_tags(raw)?;
    let (_, body) = element_contents(&tags, "main", |tag| tag.has_class("main-column"))?;
    let mut seen = BTreeSet::new();
    let mut candidates = Vec::new();
    let mut cards = Vec::new();
    for tag in body {
        if tag.is("article") {
            if tag.closing {
                cards.pop().ok_or_else(news_structure)?;
            } else {
                cards.push(tag.has_class("storycard") && !tag.hidden());
            }
            continue;
        }
        if cards.last() != Some(&true) || !tag.is("a") || tag.closing || tag.hidden() {
            continue;
        }
        let Some(href) = tag.attribute("href") else {
            continue;
        };
        let uri = href.as_str();
        if !NewsPublisher::Sankei.valid_article_uri(uri) || !seen.insert(uri) {
            continue;
        }
        candidates.push(NewsCandidate {
            uri: uri.to_owned(),
            uri_span: NewsSourceSpan::new(
                raw,
                tag.attributes.start() + href.start()..tag.attributes.start() + href.end(),
            ),
            feed_title: None,
            feed_title_span: None,
            feed_pub_date: None,
            feed_pub_date_span: None,
        });
        if candidates.len() == MAX_CANDIDATES {
            break;
        }
    }
    Ok(candidates)
}

struct HtmlTag<'a> {
    name: &'a str,
    closing: bool,
    attributes: Match<'a>,
    range: Range<usize>,
    raw_text: Option<Range<usize>>,
    concealed_parent: bool,
}

impl<'a> HtmlTag<'a> {
    fn is(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    fn attribute(&self, name: &str) -> Option<Match<'a>> {
        let mut found = None;
        for capture in html_attribute().captures_iter(self.attributes.as_str()) {
            if !capture[1].eq_ignore_ascii_case(name) {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(
                capture
                    .get(2)
                    .or_else(|| capture.get(3))
                    .or_else(|| capture.get(4)),
            );
        }
        found.flatten()
    }

    fn has_class(&self, class: &str) -> bool {
        self.attribute("class").is_some_and(|value| {
            value
                .as_str()
                .split_ascii_whitespace()
                .any(|word| word == class)
        })
    }

    fn hidden(&self) -> bool {
        html_attribute()
            .captures_iter(self.attributes.as_str())
            .any(|attr| {
                let name = &attr[1];
                let value = attr.get(2).or_else(|| attr.get(3)).or_else(|| attr.get(4));
                name.eq_ignore_ascii_case("hidden")
                    || (name.eq_ignore_ascii_case("aria-hidden")
                        && value.is_some_and(|value| value.as_str().eq_ignore_ascii_case("true")))
                    || (name.eq_ignore_ascii_case("style")
                        && value.is_some_and(|value| {
                            value.as_str().split(';').any(|declaration| {
                                declaration.split_once(':').is_some_and(|(key, value)| {
                                    let key = key.trim();
                                    let value = value.split('!').next().unwrap_or_default().trim();
                                    (key.eq_ignore_ascii_case("display")
                                        && value.eq_ignore_ascii_case("none"))
                                        || (key.eq_ignore_ascii_case("visibility")
                                            && value.eq_ignore_ascii_case("hidden"))
                                })
                            })
                        }))
            })
    }
}

fn html_attribute() -> &'static Regex {
    static ATTRIBUTE: OnceLock<Regex> = OnceLock::new();
    // A token consumes its whole quoted value, including fake attribute names.
    static_regex(
        &ATTRIBUTE,
        r#"\s+([a-zA-Z_:][a-zA-Z0-9:_.-]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?"#,
    )
}

fn html_token() -> &'static Regex {
    static TAG: OnceLock<Regex> = OnceLock::new();
    static_regex(
        &TAG,
        r#"(?s)<!--.*?(?:-->|\z)|<(?P<close>/)?(?P<name>[a-zA-Z][a-zA-Z0-9:_-]*)(?P<attributes>(?:[^<>"']|"[^"]*"|'[^']*')*)>"#,
    )
}

fn html_tags(raw: &str) -> Result<Vec<HtmlTag<'_>>> {
    static RAW_END: OnceLock<Regex> = OnceLock::new();
    let raw_end = static_regex(&RAW_END, r"(?i)</(script|style|textarea|title)\s*>");
    let mut tags = Vec::new();
    let mut at = 0;
    let mut opaque = Vec::new();
    let mut concealed: Option<(&str, usize)> = None;
    for _ in 0..MAX_HTML_TAGS {
        let Some(token) = html_token().captures_at(raw, at) else {
            return if opaque.is_empty() {
                Ok(tags)
            } else {
                Err(news_structure())
            };
        };
        let whole = token.get(0).expect("HTML token");
        at = whole.end();
        let Some(name) = token.name("name").map(|name| name.as_str()) else {
            continue;
        };
        let closing = token.name("close").is_some();
        let mut tag = HtmlTag {
            name,
            closing,
            attributes: token.name("attributes").expect("HTML tag attributes"),
            range: whole.range(),
            raw_text: None,
            concealed_parent: concealed.is_some(),
        };
        if !closing
            && ["script", "style", "textarea", "title"]
                .iter()
                .any(|candidate| tag.is(candidate))
        {
            let end = raw_end
                .captures_iter(&raw[at..])
                .find(|end| name.eq_ignore_ascii_case(&end[1]))
                .ok_or_else(news_structure)?;
            let close_tag = end.get(0).expect("raw HTML element closing tag");
            tag.raw_text = Some(at..at + close_tag.start());
            at += close_tag.end();
            tag.range.end = at;
        }
        if tag.is("template") || tag.is("noscript") {
            if closing {
                if !opaque
                    .pop()
                    .is_some_and(|parent: &str| parent.eq_ignore_ascii_case(name))
                {
                    return Err(news_structure());
                }
            } else {
                opaque.push(name);
            }
        } else if opaque.is_empty() {
            if let Some((name, depth)) = &mut concealed {
                if tag.is(name) {
                    if tag.closing {
                        *depth -= 1;
                    } else {
                        *depth += 1;
                    }
                    if *depth == 0 {
                        concealed = None;
                    }
                }
            } else if !tag.closing
                && tag.raw_text.is_none()
                && ![
                    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta",
                    "param", "source", "track", "wbr",
                ]
                .iter()
                .any(|name| tag.is(name))
                && tag.hidden()
            {
                concealed = Some((tag.name, 1));
            }
            tags.push(tag);
        }
    }
    Err(failure(
        "NEWS_SOURCE_LIMIT",
        "配信元資料内のHTML要素数が予算を超えました。",
    ))
}

fn element_contents<'tag, 'source>(
    tags: &'tag [HtmlTag<'source>],
    name: &str,
    matches_class: impl Fn(&HtmlTag<'source>) -> bool,
) -> Result<(&'tag HtmlTag<'source>, &'tag [HtmlTag<'source>])> {
    let mut containers = tags
        .iter()
        .enumerate()
        .filter(|(_, tag)| tag.is(name) && !tag.closing && matches_class(tag));
    let (start, first) = containers.next().ok_or_else(news_structure)?;
    if first.hidden() || containers.next().is_some() {
        return Err(news_structure());
    }
    let mut depth = 1;
    for (index, tag) in tags
        .iter()
        .enumerate()
        .skip(start + 1)
        .filter(|(_, tag)| tag.is(name))
    {
        if tag.closing {
            depth -= 1;
        } else {
            depth += 1;
        }
        if depth == 0 {
            return Ok((first, &tags[start + 1..index]));
        }
    }
    Err(news_structure())
}

fn json_string(text: &str, node: &JsonNode) -> Result<String> {
    serde_json::from_str::<String>(&text[node.span.clone()]).map_err(|_| news_structure())
}

fn article_metadata(
    publisher: NewsPublisher,
    tags: &[HtmlTag<'_>],
    raw: &str,
    uri: &str,
) -> Result<(String, Range<usize>, String, Range<usize>)> {
    let mut metadata = None;
    for tag in tags.iter().filter(|tag| tag.is("script") && !tag.closing) {
        if !tag
            .attribute("type")
            .is_some_and(|kind| kind.as_str().eq_ignore_ascii_case("application/ld+json"))
        {
            continue;
        }
        let range = tag.raw_text.as_ref().ok_or_else(news_structure)?;
        let text = &raw[range.clone()];
        let root = strict_json_document(text)?;
        let entries = match &root.kind {
            JsonKind::Object(_) => std::slice::from_ref(&root),
            // The Sankei script places NewsArticle next to BreadcrumbList.
            JsonKind::Array(entries) if publisher == NewsPublisher::Sankei => entries,
            _ => continue,
        };
        let mut articles = entries.iter().filter_map(|node| {
            let JsonKind::Object(fields) = &node.kind else {
                return None;
            };
            (json_string(text, fields.get("@type")?).ok().as_deref() == Some("NewsArticle"))
                .then_some(fields)
        });
        let Some(fields) = articles.next() else {
            continue;
        };
        if metadata.is_some() || articles.next().is_some() {
            return Err(news_structure());
        }
        if fields
            .get("isAccessibleForFree")
            .is_some_and(|node| !matches!(&text[node.span.clone()], "true" | "\"true\""))
        {
            return Err(failure(
                "NEWS_ARTICLE_BODY_UNAVAILABLE",
                "公開本文として利用できる記事を確認できません。",
            ));
        }
        let uri_node = match publisher {
            NewsPublisher::Nhk => {
                let entity = fields.get("mainEntityOfPage").ok_or_else(news_structure)?;
                match &entity.kind {
                    JsonKind::Object(values) => values.get("@id").ok_or_else(news_structure)?,
                    _ => entity,
                }
            }
            NewsPublisher::Sankei => fields.get("url").ok_or_else(news_structure)?,
        };
        if json_string(text, uri_node)? != uri {
            return Err(news_structure());
        }
        let headline = fields.get("headline").ok_or_else(news_structure)?;
        // NHK's original JSON-LD declares a Japanese language-tagged value.
        // Keep the lexical string span inside that value, not the wrapper.
        let headline = if let (NewsPublisher::Nhk, JsonKind::Object(language_value)) =
            (publisher, &headline.kind)
        {
            let language = language_value.get("@language").ok_or_else(news_structure)?;
            if language_value.len() != 2 || json_string(text, language)? != "ja" {
                return Err(news_structure());
            }
            language_value.get("@value").ok_or_else(news_structure)?
        } else {
            headline
        };
        let date = fields.get("datePublished").ok_or_else(news_structure)?;
        let title = json_string(text, headline)?;
        let published = json_string(text, date)?;
        if title.trim().is_empty()
            || title.len() > 2048
            || title.chars().any(char::is_control)
            || !publication_date(&published)
        {
            return Err(failure(
                "NEWS_ARTICLE_PUBLICATION_UNAVAILABLE",
                "配信元が示した記事名と時差付き公開時刻を確認できません。",
            ));
        }
        let in_raw = |node: &JsonNode| range.start + node.span.start..range.start + node.span.end;
        metadata = Some((title, in_raw(headline), published, in_raw(date)));
    }
    metadata.ok_or_else(news_structure)
}

fn publication_date(text: &str) -> bool {
    static DATE: OnceLock<Regex> = OnceLock::new();
    let date = static_regex(
        &DATE,
        r"^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})(?:\.[0-9]{1,9})?(Z|[+-][0-9]{2}:[0-9]{2})$",
    );
    let Some(value) = date.captures(text) else {
        return false;
    };
    let number = |index: usize| value[index].parse::<u16>().expect("bound date field");
    let (year, month, day) = (number(1), number(2), number(3));
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => 28 + u16::from(leap),
        _ => return false,
    };
    let offset = &value[7];
    (1..=days).contains(&day)
        && year > 0
        && number(4) < 24
        && number(5) < 60
        && number(6) < 60
        && (offset == "Z" || (offset != "-00:00" && &offset[1..3] <= "23" && &offset[4..6] <= "59"))
}

fn article_passage(
    publisher: NewsPublisher,
    raw: &str,
    body: &[HtmlTag<'_>],
    check_ancestors: bool,
    topic: Option<&str>,
) -> Result<(String, Range<usize>)> {
    const BODY_BLOCKS: &[&str] = &[
        "article",
        "aside",
        "blockquote",
        "details",
        "dialog",
        "div",
        "figure",
        "figcaption",
        "footer",
        "form",
        "header",
        "li",
        "nav",
        "ol",
        "section",
        "table",
        "ul",
    ];
    if body.iter().any(|tag| {
        !tag.closing
            && ["paywall", "article-paywall", "login-required"]
                .iter()
                .any(|class| tag.has_class(class))
    }) {
        return Err(failure(
            "NEWS_ARTICLE_BODY_UNAVAILABLE",
            "公開本文として利用できる記事を確認できません。",
        ));
    }
    let mut blocks = Vec::new();
    let mut eligible = 0;
    for (index, paragraph) in body.iter().enumerate() {
        if BODY_BLOCKS.iter().any(|name| paragraph.is(name)) {
            if paragraph.closing {
                if !blocks.pop().is_some_and(|name: &str| paragraph.is(name)) {
                    return Err(news_structure());
                }
            } else {
                blocks.push(paragraph.name);
            }
            continue;
        }
        if !paragraph.is("p")
            || paragraph.closing
            || !blocks.is_empty()
            || (check_ancestors && paragraph.concealed_parent)
            || paragraph.hidden()
            || !paragraph.has_class(match publisher {
                NewsPublisher::Nhk => "_1i1d7sh2",
                NewsPublisher::Sankei => "article-text",
            })
        {
            continue;
        }
        let end = body[index + 1..].iter().find(|tag| tag.is("p"));
        let Some(end) = end.filter(|tag| tag.closing) else {
            continue;
        };
        let start = paragraph.range.end;
        let content = &raw[start..end.range.start];
        let Some((text, consumed)) = passage_prefix(content) else {
            continue;
        };
        if text.chars().count() >= 16
            && ![
                "続きを読むには",
                "続きはログイン",
                "全文を読むには",
                "会員登録してください",
                "ログインしてください",
            ]
            .iter()
            .any(|cue| text.contains(cue))
        {
            if topic.is_none_or(|topic| crate::news::contains_normalized_topic(&text, topic)) {
                return Ok((text, start..start + consumed));
            }
            eligible += 1;
            if eligible == MAX_TOPIC_PASSAGES {
                break;
            }
        }
    }
    if eligible == 0 {
        Err(article_unavailable())
    } else {
        Err(failure(
            "NEWS_TOPIC_UNCONFIRMED",
            "指定された話題を記事本文と照合できません。",
        ))
    }
}

fn passage_prefix(content: &str) -> Option<(String, usize)> {
    let mut rendered = String::new();
    let mut consumed = 0;
    let mut remaining = MAX_PASSAGE_CHARS;
    for token in html_token().captures_iter(content) {
        let tag = token.get(0)?;
        let (part, bytes) = decode_prefix(&content[consumed..tag.start()], remaining)?;
        remaining -= part.chars().count();
        rendered.push_str(&part);
        if remaining == 0 {
            return Some((rendered.trim().to_owned(), consumed + bytes));
        }
        if let Some(name) = token.name("name") {
            let inline = HtmlTag {
                name: name.as_str(),
                closing: token.name("close").is_some(),
                attributes: token.name("attributes")?,
                range: tag.range(),
                raw_text: None,
                concealed_parent: false,
            };
            if inline.hidden() {
                return None;
            }
            if name.as_str().eq_ignore_ascii_case("br") {
                rendered.push(' ');
                remaining -= 1;
            } else if !["b", "strong", "em", "i", "span", "a"]
                .iter()
                .any(|inline| name.as_str().eq_ignore_ascii_case(inline))
            {
                return None;
            }
        }
        consumed = tag.end();
        if remaining == 0 {
            return Some((rendered.trim().to_owned(), consumed));
        }
    }
    let (tail, bytes) = decode_prefix(&content[consumed..], remaining)?;
    rendered.push_str(&tail);
    Some((rendered.trim().to_owned(), consumed + bytes))
}

fn decode_prefix(raw: &str, limit: usize) -> Option<(String, usize)> {
    let mut text = String::new();
    let mut at = 0;
    for _ in 0..limit {
        let Some(mut character) = raw[at..].chars().next() else {
            break;
        };
        let mut bytes = character.len_utf8();
        if character == '&'
            && let Some(end) = raw[at..].find(';').filter(|end| (2..=12).contains(end))
            && let Some(decoded) = decode_entity(&raw[at + 1..at + end])
        {
            character = decoded;
            bytes = end + 1;
        }
        if character.is_control() && !character.is_whitespace() {
            return None;
        }
        text.push(if character.is_whitespace() {
            ' '
        } else {
            character
        });
        at += bytes;
    }
    Some((text, at))
}

fn decode_entity(name: &str) -> Option<char> {
    if let Some(hex) = name.strip_prefix("#x").or_else(|| name.strip_prefix("#X")) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    if let Some(decimal) = name.strip_prefix('#') {
        return decimal.parse::<u32>().ok().and_then(char::from_u32);
    }
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some(' '),
        "ndash" => Some('–'),
        "mdash" => Some('—'),
        "hellip" => Some('…'),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nhk_language_tagged_headline_keeps_lexical_value_and_body_spans() {
        let uri = "https://news.web.nhk/newsweb/na/nd-20261127sample";
        let raw = format!(
            r#"<script type="application/ld+json">{{"@type":"NewsArticle","mainEntityOfPage":{{"@id":"{uri}"}},"headline":{{"@value":"観測塔の新しい照明","@language":"ja"}},"datePublished":"2026-11-27T12:51:49+09:00","isAccessibleForFree":true}}</script><div class="esl7kn2s _1i1d7sh0"><p class="_1i1d7sh2">古い観測塔の照明を冬の運用に合わせて調整しました。</p></div>"#
        );
        let article = NewsPublisher::Nhk.article(uri, &raw, None).unwrap();
        assert_eq!(article.title, "観測塔の新しい照明");
        assert_eq!(
            &raw[article.title_span.start..article.title_span.end],
            r#""観測塔の新しい照明""#
        );
        assert_eq!(
            &raw[article.published_span.start..article.published_span.end],
            r#""2026-11-27T12:51:49+09:00""#
        );
        let passage = &raw[article.passage_span.start..article.passage_span.end];
        assert_eq!(passage, article.passage);
        assert_eq!(article.passage_span.sha256, digest(passage.as_bytes()));
        let wrong_language = raw.replace(r#""@language":"ja""#, r#""@language":"en""#);
        assert!(
            NewsPublisher::Nhk
                .article(uri, &wrong_language, None)
                .is_err()
        );
    }

    #[test]
    fn sankei_admits_only_storycard_uris_and_free_original_body() {
        let uri = "https://www.sankei.com/article/20261127-ABCDEF123456ABCDEF123456/";
        let listing = format!(
            r#"<nav><a href="{uri}">outside</a></nav><main class="main-column"><article class="storycard"><a href="{uri}?outputType=amp">variant</a><a href="{uri}">story</a><a href="{uri}">duplicate</a></article></main>"#
        );
        let candidates = NewsPublisher::Sankei.listing(&listing).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].uri, uri);
        let raw = format!(
            r#"<script type="application/ld+json">[{{"@type":"NewsArticle","url":"{uri}","headline":"観測塔の新しい照明","datePublished":"2026-11-27T12:51:49+09:00"}},{{"@type":"BreadcrumbList"}}]</script><article class="article-wrapper free"><div class="article-body clearfix "><p class="article-text ">古い観測塔の照明を冬の運用に合わせて調整しました。</p></div></article>"#
        );
        let article = NewsPublisher::Sankei.article(uri, &raw, None).unwrap();
        assert_eq!(
            article.passage,
            "古い観測塔の照明を冬の運用に合わせて調整しました。"
        );
        let paid = raw.replace("article-wrapper free", "article-wrapper paid_members");
        assert!(NewsPublisher::Sankei.article(uri, &paid, None).is_err());
        let duplicate = raw.replace("BreadcrumbList", "NewsArticle");
        assert!(
            NewsPublisher::Sankei
                .article(uri, &duplicate, None)
                .is_err()
        );
    }

    #[test]
    fn topic_uses_first_matching_public_passage_with_exact_source_span() {
        let uri = "https://news.web.nhk/newsweb/na/nd-20261127sample";
        let first = "観測塔の照明について運用を見直す計画です。";
        let matching = "地域向けの解説会では渡り鳥の調査結果も紹介します。";
        let metadata = format!(
            r#"<script type="application/ld+json">{{"@type":"NewsArticle","mainEntityOfPage":{{"@id":"{uri}"}},"headline":{{"@value":"観測塔の新しい照明","@language":"ja"}},"datePublished":"2026-11-27T12:51:49+09:00","isAccessibleForFree":true}}</script>"#
        );
        let raw = format!(
            r#"{metadata}<div class="_1i1d7sh0"><p class="_1i1d7sh2">{first}</p><fieldset aria-hidden="true"><fieldset><p class="_1i1d7sh2">渡り鳥と秘密の案内を含む非表示の関連資料です。</p></fieldset><p class="_1i1d7sh2">秘密の案内を別の段落にも表示しない設定です。</p></fieldset><p class="_1i1d7sh2">{matching}</p></div>"#
        );
        let selected = NewsPublisher::Nhk
            .article(uri, &raw, Some("渡り鳥"))
            .unwrap();
        assert_eq!(selected.passage, matching);
        assert_eq!(
            &raw[selected.passage_span.start..selected.passage_span.end],
            matching
        );
        assert_eq!(selected.passage_span.sha256, digest(matching.as_bytes()));
        for topic in [None, Some("観測塔")] {
            assert_eq!(
                NewsPublisher::Nhk
                    .article(uri, &raw, topic)
                    .unwrap()
                    .passage,
                first
            );
        }
        assert!(matches!(
            NewsPublisher::Nhk.article(uri, &raw, Some("秘密の案内")),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "NEWS_TOPIC_UNCONFIRMED"
        ));

        let filler = format!(r#"<p class="_1i1d7sh2">{first}</p>"#);
        let after_limit = raw.replacen(&filler, &filler.repeat(MAX_TOPIC_PASSAGES), 1);
        assert!(matches!(
            NewsPublisher::Nhk.article(uri, &after_limit, Some("渡り鳥")),
            Err(minidora_domain::Error::Contract { code, .. }) if code == "NEWS_TOPIC_UNCONFIRMED"
        ));
    }
}
