//! Interpret bounded requests for a dated, source-backed news report.

use std::{collections::BTreeMap, sync::LazyLock};

use minidora_domain::{Intent, NewsPeriod, SourceSpan};
use regex::Regex;

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v222-current-news";
pub(crate) const NATURAL_PARSER_VERSION: &str = "declarative-ja-v253-natural-news";
pub(crate) const ENGLISH_PARSER_VERSION: &str = "declarative-en-v1-current-news";

struct NewsRequest<'a> {
    period: NewsPeriod,
    time: Option<&'a str>,
    topic: Option<&'a str>,
    request: &'a str,
}

fn remove_time_particle(text: &str) -> &str {
    let text = text.trim_start();
    text.strip_prefix(['の', '、', ',', 'は'])
        .map_or(text, str::trim_start)
}

fn time_reference<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let following = text.strip_prefix(marker)?;
    if !following.starts_with(char::is_whitespace)
        && !following.starts_with(['の', '、', ',', 'は'])
        && !following.starts_with("ニュース")
    {
        return None;
    }
    Some(following)
}

fn requested_period(text: &str) -> (NewsPeriod, Option<&str>, &str) {
    for marker in ["今日一日", "今日", "本日", "きょう"] {
        if let Some(rest) = time_reference(text, marker) {
            return (
                NewsPeriod::Today,
                Some(&text[..marker.len()]),
                remove_time_particle(rest),
            );
        }
    }
    for marker in [
        "過去24時間",
        "過去２４時間",
        "直近24時間",
        "直近２４時間",
        "最新",
        "新着",
        "今",
        "いま",
    ] {
        if let Some(rest) = time_reference(text, marker) {
            return (
                NewsPeriod::Latest,
                Some(&text[..marker.len()]),
                remove_time_particle(rest),
            );
        }
    }
    (NewsPeriod::Latest, None, text)
}

fn supported_topic(prefix: &str) -> Option<Option<&str>> {
    let prefix = prefix.trim();
    if matches!(
        prefix,
        "" | "主な" | "主要" | "最新" | "新着" | "どんな" | "どのような"
    ) {
        return Some(None);
    }
    let topic = ["に関する", "についての", "の"]
        .into_iter()
        .find_map(|ending| prefix.strip_suffix(ending))
        .unwrap_or(prefix)
        .trim_end();
    if topic.is_empty()
        || topic.chars().count() > 48
        || !topic.chars().any(char::is_alphabetic)
        || !topic
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, 'ー' | '・'))
        || [
            "昨日",
            "明日",
            "一昨日",
            "おととい",
            "先週",
            "来週",
            "今週",
            "先月",
            "今月",
            "来月",
            "昨年",
            "今年",
            "来年",
            "先日",
            "昨夜",
            "今朝",
            "今夜",
            "午前",
            "午後",
            "以前",
            "前回",
            "今日",
            "本日",
            "きょう",
            "過去",
            "直近",
        ]
        .iter()
        .any(|marker| topic.starts_with(marker))
        || [
            "以外",
            "抜き",
            "除く",
            "除いた",
            "ではなく",
            "じゃなく",
            "以降",
            "までの",
        ]
        .iter()
        .any(|marker| topic.contains(marker))
    {
        return None;
    }
    Some(Some(topic))
}

fn asks_for_news(tail: &str, has_question_mark: bool, explicit_time: bool) -> bool {
    let tail = tail.trim();
    if tail.is_empty() {
        return explicit_time || has_question_mark;
    }
    if matches!(tail, "は" | "って") {
        return has_question_mark;
    }
    if matches!(
        tail,
        "は何"
            | "は何ですか"
            | "はありますか"
            | "がありますか"
            | "がある"
            | "はどんな内容"
            | "はどんな内容ですか"
            | "で何が起きている"
            | "で何が起きていますか"
            | "で何が起きてる"
            | "は何が起きている"
            | "って何があった"
    ) {
        return true;
    }
    let tail = tail
        .strip_prefix(['を', 'が', 'は'])
        .unwrap_or(tail)
        .trim_start()
        .trim_start_matches(['、', ','])
        .trim_start();
    let tail = [
        "出典付きで",
        "出典つきで",
        "出典を付けて",
        "出典をつけて",
        "出典を示して",
        "出典も含めて",
        "出典を添えて",
    ]
    .into_iter()
    .find_map(|source| tail.strip_prefix(source))
    .unwrap_or(tail)
    .trim_start()
    .trim_start_matches(['、', ','])
    .trim_start();
    matches!(
        tail,
        "教えて"
            | "教えてください"
            | "調べて"
            | "調べてください"
            | "知りたい"
            | "まとめて"
            | "まとめてください"
            | "見せて"
            | "見せてください"
    )
}

fn parse_request(raw: &str) -> Option<NewsRequest<'_>> {
    if raw.len() > 1024 || raw.chars().any(char::is_control) {
        return None;
    }
    let trimmed = raw.trim();
    let text = trimmed
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim_end();
    let has_question_mark = trimmed[text.len()..].contains(['？', '?']);
    let (period, time, subject) = requested_period(text);
    let (prefix, tail) = subject.split_once("ニュース")?;
    // A bare news request defaults to the latest window. A topic needs a
    // time reference so that unsupported dates or scope qualifiers do not
    // become literal topic filters.
    if time.is_none() && !prefix.trim().is_empty() {
        return None;
    }
    let topic = supported_topic(prefix)?;
    if !asks_for_news(tail, has_question_mark, time.is_some()) {
        return None;
    }
    Some(NewsRequest {
        period,
        time,
        topic,
        request: subject,
    })
}

pub(crate) fn parse(text: &str) -> Option<Intent> {
    let request = parse_request(text)?;
    Some(Intent::AskCurrentNews {
        topic: request.topic.map(str::to_owned),
        period: request.period,
    })
}

pub(crate) fn source_roles(
    text: &str,
    intent: &Intent,
) -> Option<BTreeMap<String, Vec<SourceSpan>>> {
    roles_for_request(text, intent, parse_request(text)?)
}

fn roles_for_request(
    text: &str,
    intent: &Intent,
    request: NewsRequest<'_>,
) -> Option<BTreeMap<String, Vec<SourceSpan>>> {
    if intent
        != &(Intent::AskCurrentNews {
            topic: request.topic.map(str::to_owned),
            period: request.period,
        })
    {
        return None;
    }
    let mut roles = BTreeMap::new();
    for (key, fragment) in [
        ("/AskCurrentNews/period", request.time),
        ("/AskCurrentNews/topic", request.topic),
        ("/AskCurrentNews/request", Some(request.request)),
    ] {
        if let Some(fragment) = fragment {
            let start = fragment.as_ptr() as usize - text.as_ptr() as usize;
            roles.insert(
                key.to_owned(),
                vec![crate::input::source_span_at(
                    text,
                    start,
                    start + fragment.len(),
                )?],
            );
        }
    }
    Some(roles)
}

const SELECTORS: &[&str] = &[
    "何か",
    "なにか",
    "なんか",
    "どんな",
    "どのような",
    "大きな",
    "主な",
    "主要な",
    "新しい",
    "気になる",
    "目立った",
    "あった",
    "起きた",
];

fn trim_separator(text: &str) -> &str {
    text.trim_start()
        .trim_start_matches(['、', '，', ','])
        .trim_start()
}

fn strip_leading<'a>(text: &'a str, choices: &[&str]) -> Option<(&'a str, &'a str)> {
    choices.iter().find_map(|choice| {
        let rest = text.strip_prefix(choice)?;
        Some((&text[..choice.len()], rest))
    })
}

fn natural_period(text: &str) -> Option<(NewsPeriod, &str, &str)> {
    let (period, marker, tail) = [
        (
            NewsPeriod::ThisMorning,
            &["今日の午前中", "きょうの午前中", "今日の朝", "今朝", "けさ"][..],
        ),
        (
            NewsPeriod::Yesterday,
            &["昨日一日", "きのう一日", "昨日", "きのう"][..],
        ),
        (
            NewsPeriod::Today,
            &["今日一日", "本日中", "今日", "本日", "きょう"][..],
        ),
        (
            NewsPeriod::Latest,
            &[
                "ここ24時間",
                "ここ２４時間",
                "この24時間",
                "この２４時間",
                "過去24時間",
                "過去２４時間",
                "直近24時間",
                "直近２４時間",
                "現在",
                "最新",
                "新着",
                "今",
                "いま",
            ][..],
        ),
    ]
    .into_iter()
    .find_map(|(period, markers)| {
        let (marker, rest) = strip_leading(text, markers)?;
        Some((period, marker, rest))
    })?;
    // A one-character marker cannot claim the start of a different date or
    // topic. Other intervals also keep range qualifiers outside the grammar.
    if [
        "から",
        "まで",
        "以降",
        "以前",
        "以後",
        "より前",
        "と",
        "か",
        "や",
    ]
    .iter()
    .any(|part| tail.trim_start().starts_with(part))
        || matches!(marker, "今" | "いま")
            && !tail.starts_with(|ch: char| ch.is_whitespace() || "のは、，,".contains(ch))
            && ![
                "世の中で",
                "社会で",
                "日本で",
                "世界で",
                "国内で",
                "海外で",
                "ニュース",
                "報道",
                "何",
                "なに",
                "なん",
                "どんな",
                "どのような",
            ]
            .iter()
            .any(|part| tail.starts_with(part))
    {
        return None;
    }
    let tail = trim_separator(tail);
    let tail = tail.strip_prefix(['の', 'は']).map_or(tail, trim_separator);
    Some((period, marker, tail))
}

fn unsupported_topic(topic: &str) -> bool {
    let after_number =
        topic.trim_start_matches(|ch: char| ch.is_ascii_digit() || ('０'..='９').contains(&ch));
    (after_number != topic
        && ["日", "月", "年", "週", "時", "分"]
            .iter()
            .any(|unit| after_number.starts_with(unit)))
        || [
            "ここ数",
            "この数",
            "ここしばらく",
            "今まで",
            "最近",
            "数日間",
        ]
        .iter()
        .any(|marker| topic.starts_with(marker))
        || [
            "昨晩",
            "昨朝",
            "きのう",
            "けさ",
            "あした",
            "あす",
            "ゆうべ",
            "この前",
            "数日",
            "現在",
            "正午",
            "夕方",
            "朝",
            "昼",
            "夜",
        ]
        .iter()
        .any(|marker| {
            topic == *marker
                || topic.strip_prefix(marker).is_some_and(|rest| {
                    rest.starts_with(['の', 'に', 'は']) || rest.starts_with("から")
                })
        })
        || [
            "以外",
            "抜き",
            "じゃなく",
            "ではなく",
            "除く",
            "除き",
            "除外",
            "以後",
            "より前",
            "まで",
            "以降",
            "不要",
            "だけ",
            "のうち",
            "または",
            "あるいは",
            "に限",
            "ついで",
            "そして",
            "もしくは",
            "そのあたり",
            "あなた",
            "いちばん",
            "一番",
            "最も",
        ]
        .iter()
        .any(|marker| topic.contains(marker))
        || topic
            .chars()
            .zip(topic.chars().skip(1))
            .zip(topic.chars().skip(2))
            .any(|((left, particle), right)| {
                let token = |ch: char| ch.is_alphanumeric() && !('ぁ'..='ゖ').contains(&ch);
                matches!(particle, 'と' | 'や') && token(left) && token(right)
            })
}

fn natural_topic(mut prefix: &str) -> Option<Option<&str>> {
    prefix = prefix.trim();
    // An inquiry can use an indeterminate selector and a digest selector
    // together. Neither introduces an invented subject to look up.
    for _ in 0..3 {
        let Some((_, rest)) = strip_leading(prefix, SELECTORS) else {
            break;
        };
        prefix = rest.trim_start();
    }
    if let Some(rest) = SELECTORS
        .iter()
        .find_map(|marker| prefix.strip_suffix(marker))
    {
        prefix = rest.trim_end();
    }
    let prefix = ["に関連する", "関連についての", "関連の", "関連"]
        .into_iter()
        .find_map(|marker| prefix.strip_suffix(marker))
        .unwrap_or(prefix)
        .trim_end();
    if matches!(
        prefix,
        "この" | "その" | "あの" | "どの" | "前の" | "さっきの"
    ) {
        return None;
    }
    let topic = supported_topic(prefix)?;
    if topic.is_some_and(unsupported_topic) {
        return None;
    }
    Some(topic)
}

fn reporting_request(text: &str) -> bool {
    let text = [
        "出典付きで",
        "出典つきで",
        "出典を付けて",
        "出典をつけて",
        "出典を示して",
        "出典も含めて",
        "出典を添えて",
    ]
    .into_iter()
    .find_map(|cue| text.strip_prefix(cue))
    .map_or(text, trim_separator);
    if matches!(text, "知りたい" | "知りたいです") {
        return true;
    }
    let Some((_, ending)) = strip_leading(text, &["教えて", "調べて", "まとめて", "見せて"])
    else {
        return false;
    };
    matches!(
        ending,
        "" | "ください"
            | "ほしい"
            | "ほしいです"
            | "もらえる"
            | "もらえますか"
            | "くれる"
            | "くれますか"
            | "いただけますか"
    )
}

fn question_stem(text: &str, marked: bool) -> Option<&str> {
    if let Some(stem) = [
        "のでしょうか",
        "んでしょうか",
        "のですか",
        "んですか",
        "でしょうか",
        "ですか",
        "のかな",
        "かな",
        "のか",
        "か",
    ]
    .into_iter()
    .find_map(|ending| text.strip_suffix(ending))
    {
        return Some(stem);
    }
    if let Some((stem, request)) = text.rsplit_once('か')
        && reporting_request(trim_separator(request))
    {
        return Some(stem);
    }
    marked.then(|| text.strip_suffix('の').unwrap_or(text))
}

fn news_tail(tail: &str, marked: bool, explicit_time: bool) -> bool {
    let tail = trim_separator(tail);
    if tail.is_empty() {
        return explicit_time || marked;
    }
    let predicate = strip_leading(tail, &["って", "について", "が", "は", "を"])
        .map_or(tail, |(_, rest)| trim_separator(rest));
    if predicate.is_empty() {
        return marked;
    }
    if reporting_request(predicate) {
        return true;
    }
    let Some(stem) = question_stem(predicate, marked) else {
        return false;
    };
    let stem = strip_leading(stem, &["何か", "なにか", "なんか", "何が", "なにが"])
        .map_or(stem, |(_, rest)| trim_separator(rest));
    matches!(
        stem,
        "あった"
            | "ありました"
            | "ある"
            | "あります"
            | "出た"
            | "出ました"
            | "出てる"
            | "出ています"
            | "何"
            | "なに"
            | "どんな内容"
            | "どのような内容"
            | "どんなもの"
            | "どのようなもの"
    )
}

fn event_question<'a>(
    original: &'a str,
    subject: &'a str,
    marked: bool,
    period: Option<(NewsPeriod, &'a str)>,
) -> Option<NewsRequest<'a>> {
    // General scope can use the public digest. A specific event location
    // cannot be established by matching a place name as a topic.
    let (_, tail) = strip_leading(subject, &["世の中で", "社会で", "世界で"])?;
    let tail = trim_separator(tail);
    let tail = tail.strip_prefix('は').map_or(tail, trim_separator);
    let (period, time, tail) = match (period, natural_period(tail)) {
        (Some((period, time)), None) => (period, Some(time), tail),
        (None, Some((period, time, rest))) => (period, Some(time), rest),
        (None, None) => (NewsPeriod::Latest, None, tail),
        _ => return None,
    };
    let (_, predicate) = strip_leading(
        tail,
        &[
            "どのような出来事が",
            "どんな出来事が",
            "どのようなことが",
            "どんなことが",
            "何が",
            "なにが",
        ],
    )?;
    let stem = question_stem(trim_separator(predicate), marked)?;
    if !matches!(
        stem,
        "起きてる"
            | "起きている"
            | "起きています"
            | "起きた"
            | "起きました"
            | "起こってる"
            | "起こっている"
            | "起こっています"
            | "起こった"
            | "起こりました"
            | "あった"
            | "ありました"
            | "ある"
            | "あります"
    ) {
        return None;
    }
    Some(NewsRequest {
        period,
        time,
        topic: None,
        request: original,
    })
}

fn natural_news(raw: &str) -> Option<NewsRequest<'_>> {
    if raw.len() > 1024 || raw.chars().any(char::is_control) {
        return None;
    }
    let trimmed = raw.trim();
    let text = trimmed
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim_end();
    let marked = trimmed[text.len()..].contains(['？', '?']);
    if text.contains([
        '。', '！', '!', '？', '?', '「', '」', '『', '』', '"', '“', '”', '；', ';',
    ]) {
        return None;
    }

    let mut subject = text;
    let mut front_topic = None;
    if let Some((head, tail)) = text.split_once(['、', '，', ','])
        && let Some(candidate) = ["について", "に関して", "関連で"]
            .into_iter()
            .find_map(|ending| head.trim_end().strip_suffix(ending))
    {
        front_topic = Some(natural_topic(candidate)??);
        subject = trim_separator(tail);
    }
    let leading_period = natural_period(subject);
    if let Some((_, _, rest)) = leading_period {
        subject = rest;
    }
    if front_topic.is_none()
        && let Some(request) = event_question(
            text,
            subject,
            marked,
            leading_period.map(|(period, span, _)| (period, span)),
        )
    {
        return Some(request);
    }

    let (prefix, tail) = subject
        .split_once("ニュース")
        .or_else(|| subject.split_once("報道"))?;
    let topic = match (front_topic, natural_topic(prefix)?) {
        (Some(topic), None) | (None, Some(topic)) => Some(topic),
        (None, None) => None,
        (Some(_), Some(_)) => return None,
    };
    let (period, time, tail) = if let Some((period, time, _)) = leading_period {
        (period, Some(time), tail)
    } else {
        let after_news = trim_separator(tail);
        let after_news = strip_leading(after_news, &["って", "は", "なら"])
            .map_or(after_news, |(_, rest)| trim_separator(rest));
        natural_period(after_news)
            .map_or((NewsPeriod::Latest, None, tail), |(period, time, rest)| {
                (period, Some(time), rest)
            })
    };
    if !news_tail(tail, marked, time.is_some()) {
        return None;
    }
    Some(NewsRequest {
        period,
        time,
        topic,
        request: text,
    })
}

fn english_news(raw: &str) -> Option<NewsRequest<'_>> {
    const REQUEST: &str = r"(?i)\A(?:(?<cue>(?:please +)?(?:what(?:['’]s| +is| +are)|(?:show|tell|give) +me|(?:summarize|summarise))) +)?(?:the +)?";
    const TOPIC: &str = r"(?: +(?:about|on) +(?<topic>[\p{L}\p{N}][\p{L}\p{N} -]{0,95}?))?";
    const SOURCES: &str = r"(?: +with +(?:sources|citations))?\z";
    static PERIOD_FIRST: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"{REQUEST}(?:(?<calendar>today|yesterday|this +morning)['’]s|(?<latest>latest)) +(?:news|headlines){TOPIC}{SOURCES}"
        ))
        .expect("English news with a leading date")
    });
    static PERIOD_LAST: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(&format!(
            r"{REQUEST}(?:news|headlines)(?: +(?<calendar>(?:from +)?(?:today|yesterday|this +morning)|(?:in|over) +the +(?:last|past) +24 +hours))?{TOPIC}{SOURCES}"
        ))
        .expect("English news with a following date")
    });
    if raw.len() > 1024 || raw.chars().any(char::is_control) {
        return None;
    }
    let trimmed = raw.trim();
    let text = trimmed
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim_end();
    let fields = PERIOD_FIRST
        .captures(text)
        .or_else(|| PERIOD_LAST.captures(text))?;
    let time = fields
        .name("calendar")
        .or_else(|| fields.name("latest"))
        .map(|found| found.as_str());
    if time.is_none()
        && fields.name("cue").is_none()
        && !trimmed[text.len()..].contains(['?', '？'])
    {
        return None;
    }
    let period = time.map_or(NewsPeriod::Latest, |marker| {
        let mut words = marker.split_ascii_whitespace();
        match words
            .find(|word| !word.eq_ignore_ascii_case("from"))
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("today") => NewsPeriod::Today,
            Some("yesterday") => NewsPeriod::Yesterday,
            Some("this") => NewsPeriod::ThisMorning,
            _ => NewsPeriod::Latest,
        }
    });
    let topic = fields.name("topic").map(|found| found.as_str().trim_end());
    if topic.is_some_and(|topic| {
        topic.chars().count() > 48
            || !topic.chars().any(char::is_alphabetic)
            || unsupported_topic(topic)
            || topic.split([' ', '-']).any(|word| {
                matches!(
                    word.to_ascii_lowercase().as_str(),
                    "except"
                        | "excluding"
                        | "not"
                        | "without"
                        | "non"
                        | "only"
                        | "instead"
                        | "no"
                        | "and"
                        | "or"
                        | "but"
                        | "today"
                        | "yesterday"
                        | "tomorrow"
                        | "morning"
                        | "tonight"
                        | "latest"
                        | "last"
                        | "past"
                        | "week"
                        | "month"
                        | "year"
                        | "from"
                        | "until"
                        | "since"
                        | "translate"
                        | "send"
                        | "post"
                        | "report"
                )
            })
    }) {
        return None;
    }
    Some(NewsRequest {
        period,
        time,
        topic,
        request: text,
    })
}

/// Choose the already published interpretation before broadening the grammar.
fn preferred_legacy(text: &str) -> Option<Intent> {
    let intent = parse(text)?;
    let Intent::AskCurrentNews { topic, .. } = &intent else {
        return None;
    };
    // Replay still uses v222 verbatim. A new request must not turn an
    // unsupported time qualifier or a digest cue into a topic word.
    if topic
        .as_deref()
        .is_some_and(|topic| unsupported_topic(topic) || natural_topic(topic) == Some(None))
    {
        return None;
    }
    Some(intent)
}

pub(crate) fn parse_user(text: &str) -> Option<(&'static str, Intent)> {
    if let Some(prior) = preferred_legacy(text) {
        return Some((PARSER_VERSION, prior));
    }
    let (version, request) = if let Some(request) = natural_news(text) {
        (NATURAL_PARSER_VERSION, request)
    } else {
        (ENGLISH_PARSER_VERSION, english_news(text)?)
    };
    Some((
        version,
        Intent::AskCurrentNews {
            topic: request.topic.map(str::to_owned),
            period: request.period,
        },
    ))
}

pub(crate) fn source_roles_for(
    text: &str,
    intent: &Intent,
    version: &str,
) -> Option<BTreeMap<String, Vec<SourceSpan>>> {
    match version {
        PARSER_VERSION => source_roles(text, intent),
        NATURAL_PARSER_VERSION if preferred_legacy(text).is_none() => {
            roles_for_request(text, intent, natural_news(text)?)
        }
        ENGLISH_PARSER_VERSION
            if preferred_legacy(text).is_none() && natural_news(text).is_none() =>
        {
            roles_for_request(text, intent, english_news(text)?)
        }
        _ => None,
    }
}
