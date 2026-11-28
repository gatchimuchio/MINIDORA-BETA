//! Core-owned evidence selection for limited, dated public news requests.
use crate::{
    core::read_case_changed,
    news_source::{NewsArticle, NewsCandidate, NewsPublisher},
    ports::{self, NewsWebFormat},
    scheduler::{GoalNode, GoalNodeKind},
};
use minidora_domain::{
    Error, EvidencePacket, Guarantee, Intent, NewsPeriod, Permissions, Result, ResultRecord,
    failure,
};
use serde::Serialize;
use std::{
    collections::BTreeSet,
    time::{SystemTime, UNIX_EPOCH},
};
use unicode_normalization::UnicodeNormalization;

pub(crate) const PREDICATE: &str = "GROUNDED_CURRENT_NEWS_V1";
pub(crate) const EXPLANATION_PREDICATE: &str = "CURRENT_NEWS_EXPLANATION_V1";
pub(crate) const MAX_CANDIDATES_PER_PUBLISHER: usize = 4;
const DAY_MS: i64 = 86_400_000;
const JST_OFFSET_MS: i64 = 9 * 3_600_000;
const HALF_DAY_MS: i64 = DAY_MS / 2;

impl NewsPublisher {
    pub(crate) fn listing_format(self) -> NewsWebFormat {
        match self {
            Self::Nhk => NewsWebFormat::ListingXml,
            Self::Sankei => NewsWebFormat::ListingHtml,
        }
    }

    pub(crate) fn listing_node(self) -> &'static str {
        match self {
            Self::Nhk => "news-nhk-listing",
            Self::Sankei => "news-sankei-listing",
        }
    }

    pub(crate) fn review_node(self) -> &'static str {
        match self {
            Self::Nhk => "news-nhk-article-review",
            Self::Sankei => "news-sankei-article-review",
        }
    }

    pub(crate) fn nodes(self) -> [GoalNode; 2] {
        [
            GoalNode {
                id: self.listing_node().into(),
                kind: GoalNodeKind::Evidence,
                requires: Vec::new(),
            },
            GoalNode {
                id: self.review_node().into(),
                kind: GoalNodeKind::ComputedValue,
                requires: vec![self.listing_node().into()],
            },
        ]
    }

    pub(crate) fn read_permissions(self) -> Permissions {
        Permissions {
            read_hosts: BTreeSet::from([self.host().into()]),
            write_paths: BTreeSet::new(),
            wasm_execution: false,
        }
    }
}

pub(crate) fn admitted_publishers(permissions: &Permissions) -> Vec<NewsPublisher> {
    NewsPublisher::ALL
        .into_iter()
        .filter(|publisher| ports::approved_url(publisher.listing_url(), permissions).is_ok())
        .collect()
}

pub(crate) struct NewsAction<'a> {
    pub(crate) topic: Option<&'a str>,
    pub(crate) period: NewsPeriod,
}

/// One bounded fetch and the scoped artifact published for its response.
pub(crate) struct NewsFetch {
    pub(crate) packet: EvidencePacket,
    pub(crate) reference: String,
    pub(crate) redirect_wait: Option<String>,
}

impl<'a> NewsAction<'a> {
    pub(crate) fn bind(intent: &'a Intent) -> Option<Self> {
        let Intent::AskCurrentNews { topic, period } = intent else {
            return None;
        };
        Some(Self {
            topic: topic.as_deref(),
            period: *period,
        })
    }

    pub(crate) fn report<'b>(
        &'b self,
        window: &'b NewsWindow,
        reviews: &'b [NewsReview],
    ) -> NewsReport<'b> {
        NewsReport {
            kind: "minidora.core.current-news.v1",
            window,
            topic: self.topic,
            reviews,
        }
    }

    pub(crate) fn review_publisher(
        &self,
        publisher: NewsPublisher,
        parent_permissions: &Permissions,
        window: &NewsWindow,
        candidate_limit: usize,
        fetch: &mut dyn FnMut(&str, NewsWebFormat, &Permissions) -> Result<NewsFetch>,
    ) -> Result<NewsReview> {
        // Narrow the admitted Goal permission to this publisher before asking
        // the transport to start. The same checks also apply to replay.
        ports::approved_url(publisher.listing_url(), parent_permissions)?;
        let candidate_limit = candidate_limit.min(MAX_CANDIDATES_PER_PUBLISHER);
        let mut review = NewsReview::new(publisher);
        let permissions = publisher.read_permissions();
        let listing = (|| {
            let listed = fetch(
                publisher.listing_url(),
                publisher.listing_format(),
                &permissions,
            )?;
            let packet = listed.packet;
            review.listing_ref = Some(listed.reference);
            if let Some(wait) = listed.redirect_wait {
                return Err(failure(ports::RETRY_DEFERRED, wait));
            }
            if let Some(code) = &packet.failure {
                return Err(failure(code, "公開記事一覧を取得できませんでした。"));
            }
            ports::validate_news_packet(&packet, publisher.listing_format(), &permissions)?;
            if packet.request_uri != publisher.listing_url()
                || packet.final_uri != publisher.listing_url()
            {
                return Err(failure(
                    "NEWS_LISTING_ROUTE",
                    "公開記事一覧の取得元を確認できません。",
                ));
            }
            publisher.listing(packet.extracted_text.as_deref().unwrap_or_default())
        })();
        match listing {
            Err(error) if read_case_changed(&error) => return Err(error),
            Err(Error::Contract { code, .. }) => review.unavailable(code),
            Err(error) => return Err(error),
            Ok(mut candidates) => {
                window.prioritize_candidates(publisher, &mut candidates, self.topic);
                for candidate in candidates.into_iter().take(candidate_limit) {
                    let article = (|| {
                        if !publisher.valid_article_uri(&candidate.uri) {
                            return Err(failure(
                                "NEWS_ARTICLE_ROUTE",
                                "記事候補の接続先を確認できません。",
                            ));
                        }
                        let fetched =
                            fetch(&candidate.uri, NewsWebFormat::ArticleHtml, &permissions)?;
                        let packet = fetched.packet;
                        let body_ref = fetched.reference;
                        review.attempted_articles.push(body_ref.clone());
                        if let Some(wait) = fetched.redirect_wait {
                            return Err(failure(ports::RETRY_DEFERRED, wait));
                        }
                        if let Some(code) = &packet.failure {
                            return Err(failure(code, "記事の本文を取得できませんでした。"));
                        }
                        ports::validate_news_packet(
                            &packet,
                            NewsWebFormat::ArticleHtml,
                            &permissions,
                        )?;
                        if packet.request_uri != candidate.uri || packet.final_uri != candidate.uri
                        {
                            return Err(failure(
                                "NEWS_ARTICLE_ROUTE",
                                "一覧で見つけた記事と取得した記事を照合できません。",
                            ));
                        }
                        let article = publisher.article(
                            &packet.final_uri,
                            packet.extracted_text.as_deref().unwrap_or_default(),
                            self.topic,
                        )?;
                        let published_at_unix_ms = window.admits(&article, &packet)?;
                        if !matches_topic(&article, self.topic) {
                            return Err(failure(
                                "NEWS_TOPIC_UNCONFIRMED",
                                "指定された話題を記事本文と照合できません。",
                            ));
                        }
                        Ok(NewsCitation {
                            source: publisher,
                            body_ref,
                            uri: packet.final_uri,
                            observed_at_unix_ms: packet.observed_at_unix_ms,
                            published_at_unix_ms,
                            candidate,
                            article,
                        })
                    })();
                    match article {
                        Ok(citation) => {
                            review.status = "NEWS_ARTICLE_CONFIRMED";
                            review.diagnostic = None;
                            review.citation = Some(citation);
                            break;
                        }
                        Err(error) if read_case_changed(&error) => return Err(error),
                        Err(Error::Contract { code, .. }) => {
                            let end_publisher = matches!(
                                code.as_str(),
                                ports::DEADLINE_EXCEEDED
                                    | ports::RETRY_DEFERRED
                                    | ports::REDIRECT_RETRY_DEFERRED
                                    | "HTTP_RATE_LIMITED"
                                    | "HTTP_START_BUDGET_EXHAUSTED"
                                    | "RESOURCE_LIMIT"
                            );
                            review.unavailable(code);
                            if end_publisher {
                                break;
                            }
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
        Ok(review)
    }
}

#[derive(Serialize)]
pub(crate) struct NewsWindow {
    period: NewsPeriod,
    start_unix_ms: i64,
    end_unix_ms: i64,
    #[serde(skip)]
    captured_at_unix_ms: i64,
}

impl NewsWindow {
    pub(crate) fn new(period: NewsPeriod) -> Result<Self> {
        let elapsed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|value| i64::try_from(value.as_millis()).ok())
            .ok_or_else(|| {
                failure(
                    "NEWS_CLOCK_UNAVAILABLE",
                    "ニュースの時間範囲を確定できません。",
                )
            })?;
        Self::at(period, elapsed)
    }

    pub(crate) fn at(period: NewsPeriod, captured_at_unix_ms: i64) -> Result<Self> {
        let jst_clock = captured_at_unix_ms.checked_add(JST_OFFSET_MS).filter(|_| {
            // Publication times are parsed from four-digit ISO 8601 years.
            (0..=253_402_300_799_999).contains(&captured_at_unix_ms)
        });
        let midnight = jst_clock
            .ok_or_else(|| {
                failure(
                    "NEWS_CLOCK_UNAVAILABLE",
                    "ニュースの時間範囲を確定できません。",
                )
            })?
            .div_euclid(DAY_MS)
            * DAY_MS
            - JST_OFFSET_MS;
        let (start_unix_ms, end_unix_ms) = match period {
            NewsPeriod::Today => (midnight, captured_at_unix_ms),
            NewsPeriod::Latest => (captured_at_unix_ms - DAY_MS, captured_at_unix_ms),
            NewsPeriod::Yesterday => (midnight - DAY_MS, midnight - 1),
            NewsPeriod::ThisMorning => (
                midnight,
                captured_at_unix_ms.min(midnight + HALF_DAY_MS - 1),
            ),
        };
        Ok(Self {
            period,
            start_unix_ms,
            end_unix_ms,
            captured_at_unix_ms,
        })
    }

    pub(crate) fn captured_at_unix_ms(&self) -> i64 {
        self.captured_at_unix_ms
    }

    pub(crate) fn admits(&self, article: &NewsArticle, packet: &EvidencePacket) -> Result<i64> {
        let timestamp = publication_time(&article.published).ok_or_else(|| {
            failure(
                "NEWS_PUBLICATION_UNKNOWN",
                "記事が示す掲載日時と時差を確認できません。",
            )
        })?;
        if !(self.start_unix_ms..=self.end_unix_ms).contains(&timestamp)
            || u64::try_from(timestamp).is_ok_and(|time| time > packet.observed_at_unix_ms)
        {
            return Err(failure(
                "NEWS_PUBLICATION_OUTSIDE_WINDOW",
                "記事の掲載日時が依頼時点の時間範囲に入りません。",
            ));
        }
        Ok(timestamp)
    }

    pub(crate) fn prioritize_candidates(
        &self,
        publisher: NewsPublisher,
        candidates: &mut [NewsCandidate],
        topic: Option<&str>,
    ) {
        if topic.is_none() && matches!(self.period, NewsPeriod::Today | NewsPeriod::Latest) {
            return;
        }
        let topic = topic.map(normalize);
        // These hints choose a bounded fetch order. Only article body evidence
        // can satisfy the publication-window and topic checks.
        candidates.sort_by_key(|candidate| {
            let in_window = match publisher {
                NewsPublisher::Nhk => candidate
                    .feed_pub_date
                    .as_deref()
                    .and_then(listed_publication_time)
                    .map(|time| (self.start_unix_ms..=self.end_unix_ms).contains(&time)),
                NewsPublisher::Sankei => listed_sankei_day(&candidate.uri)
                    .map(|start| start <= self.end_unix_ms && start + DAY_MS > self.start_unix_ms),
            };
            let date = match in_window {
                Some(true) => 0,
                None => 1,
                Some(false) => 2,
            };
            let title_matches = topic.as_ref().is_some_and(|topic| {
                candidate
                    .feed_title
                    .as_ref()
                    .is_some_and(|title| normalize(title).contains(topic))
            });
            (date, !title_matches)
        });
    }

    fn label(&self) -> &'static str {
        match self.period {
            NewsPeriod::Today => "本日（日本時間）",
            NewsPeriod::Latest => "依頼時点から直近24時間",
            NewsPeriod::Yesterday => "昨日（日本時間）",
            NewsPeriod::ThisMorning => "今朝（日本時間の正午より前）",
        }
    }
}

fn listed_publication_time(raw: &str) -> Option<i64> {
    if let Some(parsed) = publication_time(raw) {
        return Some(parsed);
    }
    let raw = raw.split_once(',').map_or(raw, |(_, date)| date);
    let mut parts = raw.split_ascii_whitespace();
    let day = parts.next()?.parse::<u8>().ok()?;
    let listed_month = parts.next()?;
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|month| month.eq_ignore_ascii_case(listed_month));
    let month = month? + 1;
    let year = parts.next()?;
    let time = parts.next()?;
    let zone = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let zone = match zone {
        "GMT" | "UTC" => "Z".into(),
        zone if zone.len() == 5 && zone.starts_with(['+', '-']) && zone.is_ascii() => {
            format!("{}:{}", &zone[..3], &zone[3..])
        }
        _ => return None,
    };
    publication_time(&format!("{year}-{month:02}-{day:02}T{time}{zone}"))
}

fn listed_sankei_day(uri: &str) -> Option<i64> {
    if !NewsPublisher::Sankei.valid_article_uri(uri) {
        return None;
    }
    let (date, _) = uri.split_once("/article/")?.1.split_once('-')?;
    if date.len() != 8 || !date.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    publication_time(&format!(
        "{}-{}-{}T00:00:00+09:00",
        &date[..4],
        &date[4..6],
        &date[6..]
    ))
}

fn publication_time(source: &str) -> Option<i64> {
    fn number(part: &str, count: usize) -> Option<i64> {
        (part.len() == count && part.bytes().all(|byte| byte.is_ascii_digit()))
            .then(|| part.parse().ok())
            .flatten()
    }
    let (date, rest) = source.split_once('T')?;
    let (year, date) = date.split_once('-')?;
    let (month, day) = date.split_once('-')?;
    let year = number(year, 4)?;
    let month = usize::try_from(number(month, 2)?.checked_sub(1)?).ok()?;
    let day = number(day, 2)?;
    let mut lengths = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) {
        lengths[1] = 29;
    }
    if year == 0 || !(1..=*lengths.get(month)?).contains(&day) {
        return None;
    }
    let (time, offset) = if let Some(time) = rest.strip_suffix('Z') {
        (time, 0)
    } else {
        let split = rest.find(['+', '-'])?;
        let (time, zone) = rest.split_at(split);
        let (hours, minutes) = zone.get(1..)?.split_once(':')?;
        let hours = number(hours, 2)?;
        let minutes = number(minutes, 2)?;
        if hours > 23 || minutes > 59 || zone == "-00:00" {
            return None;
        }
        let offset = (hours * 60 + minutes) * 60;
        (
            time,
            if zone.starts_with('-') {
                -offset
            } else {
                offset
            },
        )
    };
    let (hour, tail) = time.split_once(':')?;
    let (minute, tail) = tail.split_once(':')?;
    let (second, millis) = if let Some((second, fraction)) = tail.split_once('.') {
        if fraction.is_empty()
            || fraction.len() > 9
            || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        {
            return None;
        }
        let (ms, digits) = fraction.bytes().take(3).fold((0, 0), |(ms, digits), byte| {
            (ms * 10 + i64::from(byte - b'0'), digits + 1)
        });
        (second, ms * 10_i64.pow(3 - digits))
    } else {
        (tail, 0)
    };
    let (hour, minute, second) = (number(hour, 2)?, number(minute, 2)?, number(second, 2)?);
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let leap_years = |year: i64| year / 4 - year / 100 + year / 400;
    let days = (year - 1970) * 365 + leap_years(year - 1) - leap_years(1969)
        + lengths[..month].iter().sum::<i64>()
        + day
        - 1;
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second - offset) * 1_000 + millis)
}

fn normalize(text: &str) -> String {
    text.nfkc()
        .flat_map(char::to_lowercase)
        .filter(|character| !character.is_whitespace())
        .collect()
}

pub(crate) fn matches_topic(article: &NewsArticle, topic: Option<&str>) -> bool {
    topic.is_none_or(|topic| {
        contains_normalized_topic(&article.title, topic)
            || contains_normalized_topic(&article.passage, topic)
    })
}

pub(crate) fn contains_normalized_topic(text: &str, topic: &str) -> bool {
    let topic = normalize(topic);
    !topic.is_empty() && normalize(text).contains(&topic)
}

#[derive(Serialize)]
pub(crate) struct NewsCitation {
    pub(crate) source: NewsPublisher,
    pub(crate) body_ref: String,
    pub(crate) uri: String,
    pub(crate) observed_at_unix_ms: u64,
    pub(crate) published_at_unix_ms: i64,
    pub(crate) candidate: NewsCandidate,
    pub(crate) article: NewsArticle,
}

#[derive(Serialize)]
pub(crate) struct NewsReview {
    pub(crate) source: NewsPublisher,
    pub(crate) listing_ref: Option<String>,
    pub(crate) attempted_articles: Vec<String>,
    pub(crate) citation: Option<NewsCitation>,
    pub(crate) status: &'static str,
    pub(crate) diagnostic: Option<String>,
}

impl NewsReview {
    pub(crate) fn new(source: NewsPublisher) -> Self {
        Self {
            source,
            listing_ref: None,
            attempted_articles: Vec::new(),
            citation: None,
            status: "NEWS_ARTICLE_UNCONFIRMED",
            diagnostic: None,
        }
    }

    pub(crate) fn unavailable(&mut self, reason: impl Into<String>) {
        self.diagnostic = Some(reason.into());
    }

    fn unavailable_text(&self, english: bool) -> &'static str {
        let (ja, en) = match self.diagnostic.as_deref() {
            Some("NEWS_PUBLICATION_OUTSIDE_WINDOW") => (
                "調べた記事に指定時間内と確認できる掲載がありませんでした。",
                "The articles checked contained no confirmed publication within the requested time window.",
            ),
            Some("NEWS_PUBLICATION_UNKNOWN" | "NEWS_ARTICLE_PUBLICATION_UNAVAILABLE") => (
                "記事の掲載日時を確認できませんでした。",
                "The article's publication time could not be confirmed.",
            ),
            Some("NEWS_TOPIC_UNCONFIRMED") => (
                "調べた記事では、指定した話題との一致を確認できませんでした。",
                "The articles checked did not establish a match for the requested topic.",
            ),
            Some(ports::RETRY_DEFERRED | ports::REDIRECT_RETRY_DEFERRED | "HTTP_RATE_LIMITED") => (
                "取得先から待機を求められています。",
                "The publisher has asked us to wait before retrying.",
            ),
            Some(ports::DEADLINE_EXCEEDED | "DEADLINE_EXCEEDED") => (
                "時間内に記事を確認できませんでした。",
                "The article could not be verified before the deadline.",
            ),
            Some("PRIVATE_READ_TARGET") => (
                "配信元の接続先が通信の安全条件を満たしませんでした。",
                "The publisher's address failed the connection safety checks.",
            ),
            Some(
                "HTTP_REQUEST_FAILED" | "HTTP_TRANSPORT_FAILURE" | "HTTP_CLIENT_OR_DNS_FAILED",
            ) => (
                "配信元との通信を完了できませんでした。",
                "The connection to the publisher could not be completed.",
            ),
            Some(ports::READ_PERMISSION_REQUIRED) => (
                "記事の取得経路に許可されていない接続先がありました。",
                "The route to the article required access to an unapproved host.",
            ),
            Some("NEWS_LISTING_UNAVAILABLE" | "NEWS_LISTING_ROUTE") => (
                "公開記事一覧を確認できませんでした。",
                "The publisher's public article list could not be verified.",
            ),
            _ => (
                "掲載日時と本文を確認できる記事を取得できませんでした。",
                "No article body with a verifiable publication time was retrieved.",
            ),
        };
        if english { en } else { ja }
    }
}

#[derive(Serialize)]
pub(crate) struct NewsReport<'a> {
    pub(crate) kind: &'static str,
    pub(crate) window: &'a NewsWindow,
    pub(crate) topic: Option<&'a str>,
    pub(crate) reviews: &'a [NewsReview],
}

impl NewsReport<'_> {
    pub(crate) fn localized_unavailable(&self, english: bool) -> Vec<String> {
        self.reviews
            .iter()
            .filter(|review| review.citation.is_none())
            .map(|review| {
                let source = review.source.label();
                let reason = review.unavailable_text(english);
                if english {
                    format!("{source}: {reason}")
                } else {
                    format!("【{source}】{reason}")
                }
            })
            .collect()
    }

    pub(crate) fn localized_requested_scope(&self, english: bool) -> String {
        if english {
            let period = match self.window.period {
                NewsPeriod::Today => "today (Japan Standard Time)",
                NewsPeriod::Latest => "the 24 hours before your request",
                NewsPeriod::Yesterday => "yesterday (Japan Standard Time)",
                NewsPeriod::ThisMorning => "this morning (before noon Japan Standard Time)",
            };
            let topic = self
                .topic
                .map_or_else(String::new, |topic| format!(" about “{topic}”"));
            format!("{period}{topic}")
        } else {
            let topic = self
                .topic
                .map_or_else(String::new, |topic| format!("の「{topic}」"));
            format!("{}{topic}", self.window.label())
        }
    }

    pub(crate) fn localized_units(&self, english: bool) -> Vec<String> {
        let scope = self.localized_requested_scope(english);
        let mut units = vec![if english {
            format!("I confirmed these public news articles for {scope}.")
        } else {
            format!("{scope}のニュースです。確認できた公開記事を紹介します。")
        }];
        let mut citations = self
            .reviews
            .iter()
            .filter_map(|review| review.citation.as_ref())
            .collect::<Vec<_>>();
        citations.sort_by_key(|citation| std::cmp::Reverse(citation.published_at_unix_ms));
        for citation in &citations {
            let source = citation.source.label();
            let title = &citation.article.title;
            let published = &citation.article.published;
            let passage = &citation.article.passage;
            let uri = &citation.uri;
            units.push(if english {
                format!("{source}: {title} (published {published}). Article excerpt: “{passage}” Source: {uri}")
            } else {
                format!("【{source}】{title}（掲載 {published}）。本文の一部「{passage}」 出典 {uri}")
            });
        }
        if let [left, right] = citations.as_slice() {
            let same = normalize(&left.article.title) == normalize(&right.article.title)
                && normalize(&left.article.passage) == normalize(&right.article.passage);
            let comparison = match (english, same) {
                (true, true) => {
                    "The headlines and quoted passages match. Independent reporting remains unverified."
                }
                (true, false) => {
                    "These two articles did not establish independent confirmation of the same event."
                }
                (false, true) => {
                    "両記事の見出しと採用箇所は一致しています。別々に取材されたかは未確認です。"
                }
                (false, false) => {
                    "取得した2件からは、同じ出来事についての裏取りまでは確認できませんでした。"
                }
            };
            units.push(comparison.into());
        }
        units.extend(self.localized_unavailable(english));
        units
    }
}

pub(crate) fn localized_explanation_units(original: &ResultRecord, english: bool) -> Vec<String> {
    let reason = original
        .reason
        .as_deref()
        .and_then(|value| value.split_once(": ").map(|(code, _)| code));
    let (ja, en) = match reason {
        Some(ports::READ_PERMISSION_REQUIRED) => (
            "前のニュースの質問は理解できました。記事を調べるにはWeb接続先の許可が必要ですが、そのときは許可された配信元がなかったため、取得できていません。",
            "I understood the previous news request. No publisher was authorized for that request, so no articles were retrieved.",
        ),
        Some("NEWS_ARTICLES_UNAVAILABLE") => (
            "記事一覧や本文を確認しようとしましたが、指定の時間や話題に合う記事を確認できなかったため、ニュースの内容は確定していません。",
            "I could not confirm an article that matched the requested time and topic when checking publisher listings and article bodies.",
        ),
        Some("NEWS_PARTIALLY_CONFIRMED") => (
            "前の回答に出した記事は本文と掲載日時を確認できました。確認できなかった配信元も回答に記載しています。",
            "I verified the body and publication time of the articles shown. The previous answer also identified the publishers I could not verify.",
        ),
        _ if original.guarantee == Guarantee::SourceReport => (
            "前の回答では、取得できた公開記事の本文と、配信元が掲載した日時を確認しました。出典は前の回答の各記事に付けています。",
            "The previous answer checked each retrieved article's body and the publication time stated by its publisher. It included a source for each article.",
        ),
        _ => (
            "前の依頼には、確認済みとして紹介できるニュースの記事がありません。",
            "The previous request has no verified news articles to report.",
        ),
    };
    let guidance = match (english, reason == Some(ports::READ_PERMISSION_REQUIRED)) {
        (true, true) => {
            "To look up the news, allow read access to your chosen publisher and ask again."
        }
        (true, false) => {
            "Publication time is the time stated by the publisher. Retrieval time records when Minidora read the page. This follow-up reused the existing result."
        }
        (false, true) => {
            "通信してよければ、使う配信元への接続を許可してから、もう一度ニュースを聞いてください。"
        }
        (false, false) => {
            "掲載日時は配信元が示した日時です。取得時刻はミニドラがページを読んだ時刻で、今回の追問では再取得していません。"
        }
    };
    let status = if english { en } else { ja };
    vec![status.into(), guidance.into()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn news_window_uses_japan_midnight_and_publication_offsets() {
        let now = publication_time("2026-11-27T00:04:00+09:00").unwrap();
        let before = publication_time("2026-11-26T14:59:59.999Z").unwrap();
        let midnight = publication_time("2026-11-26T15:00:00Z").unwrap();
        assert_eq!(midnight, before + 1);
        let today = NewsWindow::at(NewsPeriod::Today, now).unwrap();
        assert_eq!(today.start_unix_ms, midnight);
        assert!(before < today.start_unix_ms);
        let latest = NewsWindow::at(NewsPeriod::Latest, now).unwrap();
        assert!(before > latest.start_unix_ms);
        assert_eq!(publication_time("2026-02-29T14:00:00Z"), None);
        assert_eq!(publication_time("2026-11-27T08:00:00-00:00"), None);
        assert_eq!(
            publication_time("2026-11-26T23:00:00-02:00"),
            publication_time("2026-11-27T01:00:00Z")
        );
    }

    #[test]
    fn earlier_news_windows_use_closed_bounds_inside_the_requested_day() {
        let moment = |text| publication_time(text).unwrap();
        let now = moment("2026-11-28T16:20:00+09:00");
        let midnight = moment("2026-11-28T00:00:00+09:00");
        let noon = moment("2026-11-28T12:00:00+09:00");
        let yesterday = NewsWindow::at(NewsPeriod::Yesterday, now).unwrap();
        assert_eq!(yesterday.start_unix_ms, midnight - DAY_MS);
        assert_eq!(yesterday.end_unix_ms, midnight - 1);
        let morning = NewsWindow::at(NewsPeriod::ThisMorning, now).unwrap();
        assert_eq!(morning.start_unix_ms, midnight);
        assert_eq!(morning.end_unix_ms, noon - 1);
        let early = moment("2026-11-28T05:00:00+09:00");
        assert_eq!(
            NewsWindow::at(NewsPeriod::ThisMorning, early)
                .unwrap()
                .end_unix_ms,
            early
        );
    }

    #[test]
    fn news_candidate_hints_are_stable_and_keep_unknown_dates() {
        use crate::news_source::NewsSourceSpan;

        let at = publication_time("2026-11-28T10:20:00+09:00").unwrap();
        let window = NewsWindow::at(NewsPeriod::Yesterday, at).unwrap();
        let item = |uri: &str, title: &str, date: &str| NewsCandidate {
            uri: uri.into(),
            uri_span: NewsSourceSpan {
                start: 0,
                end: 0,
                sha256: String::new(),
            },
            feed_title: Some(title.into()),
            feed_title_span: None,
            feed_pub_date: Some(date.into()),
            feed_pub_date_span: None,
        };
        let mut candidates = [
            item("today", "鉄道", "Sat, 28 Nov 2026 05:03:23 +0900"),
            item("unknown", "鉄道", "未確認"),
            item("other", "別の話題", "Fri, 27 Nov 2026 11:40:00 GMT"),
            item(
                "target",
                "新しい鉄道計画",
                "Fri, 27 Nov 2026 18:10:00 +0900",
            ),
        ];
        window.prioritize_candidates(NewsPublisher::Nhk, &mut candidates, Some("鉄道"));
        assert_eq!(
            candidates.map(|candidate| candidate.uri),
            ["target", "other", "unknown", "today"]
        );
        assert_eq!(
            listed_sankei_day("https://www.sankei.com/article/20261127-ABCDEFGHIJKLMNOPQRST/"),
            Some(window.start_unix_ms)
        );
    }
}
