//! Core-owned evidence selection for limited, dated public news requests.
use crate::{
    news_source::{NewsArticle, NewsCandidate, NewsPublisher},
    ports::{self, NewsWebFormat},
    scheduler::{GoalNode, GoalNodeKind},
};
use minidora_domain::{
    EvidencePacket, Guarantee, NewsPeriod, Permissions, Result, ResultRecord, failure,
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

#[derive(Serialize)]
pub(crate) struct NewsWindow {
    period: NewsPeriod,
    start_unix_ms: i64,
    end_unix_ms: i64,
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
        Ok(Self::at(period, elapsed))
    }

    fn at(period: NewsPeriod, end_unix_ms: i64) -> Self {
        let midnight = (end_unix_ms + JST_OFFSET_MS).div_euclid(DAY_MS) * DAY_MS - JST_OFFSET_MS;
        let (start_unix_ms, end_unix_ms) = match period {
            NewsPeriod::Today => (midnight, end_unix_ms),
            NewsPeriod::Latest => (end_unix_ms - DAY_MS, end_unix_ms),
            NewsPeriod::Yesterday => (midnight - DAY_MS, midnight - 1),
            NewsPeriod::ThisMorning => (midnight, end_unix_ms.min(midnight + HALF_DAY_MS - 1)),
        };
        Self {
            period,
            start_unix_ms,
            end_unix_ms,
        }
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

    fn unavailable_text(&self) -> &'static str {
        match self.diagnostic.as_deref() {
            Some("NEWS_PUBLICATION_OUTSIDE_WINDOW") => {
                "調べた記事に指定時間内と確認できる掲載がありませんでした。"
            }
            Some("NEWS_PUBLICATION_UNKNOWN" | "NEWS_ARTICLE_PUBLICATION_UNAVAILABLE") => {
                "記事の掲載日時を確認できませんでした。"
            }
            Some("NEWS_TOPIC_UNCONFIRMED") => {
                "調べた記事では、指定した話題との一致を確認できませんでした。"
            }
            Some(ports::RETRY_DEFERRED | ports::REDIRECT_RETRY_DEFERRED | "HTTP_RATE_LIMITED") => {
                "取得先から待機を求められています。"
            }
            Some(ports::DEADLINE_EXCEEDED | "DEADLINE_EXCEEDED") => {
                "時間内に記事を確認できませんでした。"
            }
            Some("PRIVATE_READ_TARGET") => "配信元の接続先が通信の安全条件を満たしませんでした。",
            Some(
                "HTTP_REQUEST_FAILED" | "HTTP_TRANSPORT_FAILURE" | "HTTP_CLIENT_OR_DNS_FAILED",
            ) => "配信元との通信を完了できませんでした。",
            Some(ports::READ_PERMISSION_REQUIRED) => {
                "記事の取得経路に許可されていない接続先がありました。"
            }
            Some("NEWS_LISTING_UNAVAILABLE" | "NEWS_LISTING_ROUTE") => {
                "公開記事一覧を確認できませんでした。"
            }
            _ => "掲載日時と本文を確認できる記事を取得できませんでした。",
        }
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
    pub(crate) fn unavailable_units(&self) -> Vec<String> {
        self.reviews
            .iter()
            .filter(|review| review.citation.is_none())
            .map(|review| format!("【{}】{}", review.source.label(), review.unavailable_text()))
            .collect()
    }

    pub(crate) fn requested_scope(&self) -> String {
        let scope = self
            .topic
            .map_or_else(String::new, |topic| format!("の「{topic}」"));
        format!("{}{scope}", self.window.label())
    }

    pub(crate) fn units(&self) -> Vec<String> {
        let mut units = vec![format!(
            "{}のニュースです。確認できた公開記事を紹介します。",
            self.requested_scope()
        )];
        let mut citations = self
            .reviews
            .iter()
            .filter_map(|review| review.citation.as_ref())
            .collect::<Vec<_>>();
        citations.sort_by_key(|citation| std::cmp::Reverse(citation.published_at_unix_ms));
        for citation in &citations {
            units.push(format!(
                "【{}】{}（掲載 {}）。本文の一部「{}」 出典 {}",
                citation.source.label(),
                citation.article.title,
                citation.article.published,
                citation.article.passage,
                citation.uri,
            ));
        }
        if let [left, right] = citations.as_slice() {
            if normalize(&left.article.title) == normalize(&right.article.title)
                && normalize(&left.article.passage) == normalize(&right.article.passage)
            {
                units.push(
                    "両記事の見出しと採用箇所は一致しています。別々に取材されたかは未確認です。"
                        .into(),
                );
            } else {
                units.push(
                    "取得した2件からは、同じ出来事についての裏取りまでは確認できませんでした。"
                        .into(),
                );
            }
        }
        units.extend(self.unavailable_units());
        units
    }
}

pub(crate) fn explanation_units(original: &ResultRecord) -> Vec<String> {
    let reason = original
        .reason
        .as_deref()
        .and_then(|value| value.split_once(": ").map(|(code, _)| code));
    let status = match reason {
        Some(ports::READ_PERMISSION_REQUIRED) => {
            "前のニュースの質問は理解できました。記事を調べるにはWeb接続先の許可が必要ですが、そのときは許可された配信元がなかったため、取得できていません。"
        }
        Some("NEWS_ARTICLES_UNAVAILABLE") => {
            "記事一覧や本文を確認しようとしましたが、指定の時間や話題に合う記事を確認できなかったため、ニュースの内容は確定していません。"
        }
        Some("NEWS_PARTIALLY_CONFIRMED") => {
            "前の回答に出した記事は本文と掲載日時を確認できました。確認できなかった配信元も回答に記載しています。"
        }
        _ if original.guarantee == Guarantee::SourceReport => {
            "前の回答では、取得できた公開記事の本文と、配信元が掲載した日時を確認しました。出典は前の回答の各記事に付けています。"
        }
        _ => "前の依頼には、確認済みとして紹介できるニュースの記事がありません。",
    };
    let guidance = if reason == Some(ports::READ_PERMISSION_REQUIRED) {
        "通信してよければ、使う配信元への接続を許可してから、もう一度ニュースを聞いてください。"
    } else {
        "掲載日時は配信元が示した日時です。取得時刻はミニドラがページを読んだ時刻で、今回の追問では再取得していません。"
    };
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
        let today = NewsWindow::at(NewsPeriod::Today, now);
        assert_eq!(today.start_unix_ms, midnight);
        assert!(before < today.start_unix_ms);
        let latest = NewsWindow::at(NewsPeriod::Latest, now);
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
        let yesterday = NewsWindow::at(NewsPeriod::Yesterday, now);
        assert_eq!(yesterday.start_unix_ms, midnight - DAY_MS);
        assert_eq!(yesterday.end_unix_ms, midnight - 1);
        let morning = NewsWindow::at(NewsPeriod::ThisMorning, now);
        assert_eq!(morning.start_unix_ms, midnight);
        assert_eq!(morning.end_unix_ms, noon - 1);
        let early = moment("2026-11-28T05:00:00+09:00");
        assert_eq!(
            NewsWindow::at(NewsPeriod::ThisMorning, early).end_unix_ms,
            early
        );
    }

    #[test]
    fn news_candidate_hints_are_stable_and_keep_unknown_dates() {
        use crate::news_source::NewsSourceSpan;

        let at = publication_time("2026-11-28T10:20:00+09:00").unwrap();
        let window = NewsWindow::at(NewsPeriod::Yesterday, at);
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
