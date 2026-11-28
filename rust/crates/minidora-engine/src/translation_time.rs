//! Clock, calendar, and positional time expressions shared by both directions.
use std::sync::LazyLock;

use minidora_domain::TranslationLanguage;
use regex::Regex;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Meridian {
    Am,
    Pm,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Relation {
    At,
    From,
    By,
    Until,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Timing {
    Relative(u8),
    Weekday {
        day: u8,
        next: bool,
    },
    Date {
        month: u8,
        day: u8,
        year: Option<u16>,
    },
    Clock {
        hour: u8,
        minute: Option<u8>,
        meridian: Option<Meridian>,
        relation: Relation,
    },
    AfterHours,
}

const DAYS: [(&str, &str); 7] = [
    ("月曜日", "Monday"),
    ("火曜日", "Tuesday"),
    ("水曜日", "Wednesday"),
    ("木曜日", "Thursday"),
    ("金曜日", "Friday"),
    ("土曜日", "Saturday"),
    ("日曜日", "Sunday"),
];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const RELATIVE: [(&str, &str); 4] = [
    ("今日", "today"),
    ("明日", "tomorrow"),
    ("昨日", "yesterday"),
    ("明後日", "the day after tomorrow"),
];

pub(super) fn small_number(text: &str) -> Option<u8> {
    const ENGLISH: [&str; 25] = [
        "zero",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
        "twenty-one",
        "twenty-two",
        "twenty-three",
        "twenty-four",
    ];
    const NUMERALS: [char; 10] = ['〇', '一', '二', '三', '四', '五', '六', '七', '八', '九'];
    let digits = |value: &str| match value {
        "零" => Some(0),
        _ => {
            let mut chars = value.chars();
            let head = chars.next()?;
            if chars.next().is_some() {
                return None;
            }
            NUMERALS.iter().position(|x| *x == head).map(|i| i as u8)
        }
    };
    let normalized: String = text
        .chars()
        .map(|ch| match ch {
            '０'..='９' => {
                char::from_u32(ch as u32 - '０' as u32 + '0' as u32).expect("ASCII digit")
            }
            _ => ch,
        })
        .collect();
    if let Ok(number) = normalized.parse::<u8>() {
        return Some(number);
    }
    if let Some(index) = ENGLISH.iter().position(|word| *word == text) {
        return Some(index as u8);
    }
    if let Some((tens, ones)) = text.split_once('十') {
        let tens = if tens.is_empty() { 1 } else { digits(tens)? };
        let ones = if ones.is_empty() { 0 } else { digits(ones)? };
        return Some(tens * 10 + ones);
    }
    digits(text)
}

pub(super) fn parse_timing(mut text: &str, language: TranslationLanguage) -> Option<Vec<Timing>> {
    let mut parts = Vec::new();
    while !text.trim().is_empty() {
        if parts.len() == 2 {
            return None;
        }
        let (part, rest) = match language {
            TranslationLanguage::Japanese => next_japanese(text.trim_start())?,
            TranslationLanguage::English => next_english(text.trim_start())?,
        };
        // Separate calendar and time slots. Conflicting expressions are not silently dropped.
        let clock = matches!(part, Timing::Clock { .. } | Timing::AfterHours);
        if parts
            .iter()
            .any(|existing| matches!(existing, Timing::Clock { .. } | Timing::AfterHours) == clock)
        {
            return None;
        }
        parts.push(part);
        text = rest;
    }
    Some(parts)
}

fn next_japanese(text: &str) -> Option<(Timing, &str)> {
    static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"^(午前|午後)?([0-9０-９〇零一二三四五六七八九十]{1,3})時(?:(半)|([0-9０-９〇零一二三四五六七八九十]{1,3})分)?(までに|から|まで|に)?"
    ).expect("Japanese clock regex")
    });
    static DATE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:([0-9]{4})年)?([0-9]{1,2})月([0-9]{1,2})日(?:に|の)?")
            .expect("Japanese date regex")
    });
    if let Some(m) = CLOCK.captures(text) {
        let hour = small_number(&m[2])?;
        let minute = if m.get(3).is_some() {
            Some(30)
        } else {
            m.get(4).and_then(|v| small_number(v.as_str()))
        };
        if m.get(4).is_some() && minute.is_none() {
            return None;
        }
        let meridian = match m.get(1).map(|m| m.as_str()) {
            Some("午前") => Some(Meridian::Am),
            Some("午後") => Some(Meridian::Pm),
            _ => None,
        };
        let relation = match m.get(5).map(|m| m.as_str()) {
            Some("までに") => Relation::By,
            Some("まで") => Relation::Until,
            Some("から") => Relation::From,
            _ => Relation::At,
        };
        return checked_clock(hour, minute, meridian, relation)
            .map(|value| (value, &text[m[0].len()..]));
    }
    if let Some(m) = DATE.captures(text) {
        let month = m[2].parse().ok()?;
        let day = m[3].parse().ok()?;
        let year = m.get(1).and_then(|v| v.as_str().parse().ok());
        return checked_date(month, day, year).map(|value| (value, &text[m[0].len()..]));
    }
    if let Some(rest) = text.strip_prefix("営業時間外") {
        return Some((Timing::AfterHours, skip_japanese_particle(rest)));
    }
    let (next, weekday_text) = text
        .strip_prefix("来週の")
        .map_or((false, text), |rest| (true, rest));
    for (index, (day, _)) in DAYS.iter().enumerate() {
        if let Some(rest) = weekday_text.strip_prefix(day) {
            return Some((
                Timing::Weekday {
                    day: index as u8,
                    next,
                },
                skip_japanese_particle(rest),
            ));
        }
    }
    for (index, (word, _)) in RELATIVE.iter().enumerate() {
        if let Some(rest) = text.strip_prefix(word) {
            return Some((Timing::Relative(index as u8), skip_japanese_particle(rest)));
        }
    }
    None
}

fn skip_japanese_particle(rest: &str) -> &str {
    rest.strip_prefix('に')
        .or_else(|| rest.strip_prefix('の'))
        .unwrap_or(rest)
}

fn next_english(text: &str) -> Option<(Timing, &str)> {
    static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(at|by|from|until) ([0-9]{1,2}|[a-z-]+)(?::([0-9]{2}))?(?: (am|pm))?(?:\b|$)")
            .expect("English clock regex")
    });
    static DATE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
        r"^(?:on )?(january|february|march|april|may|june|july|august|september|october|november|december)",
        r" ([0-9]{1,2})(?:(?:, )([0-9]{4}))?(?:\b|$)"
    )).expect("English date regex")
    });
    if let Some(m) = CLOCK.captures(text) {
        let hour = small_number(&m[2])?;
        let minute = m.get(3).and_then(|v| v.as_str().parse().ok());
        let meridian = m.get(4).map(|v| {
            if v.as_str() == "am" {
                Meridian::Am
            } else {
                Meridian::Pm
            }
        });
        let relation = match &m[1] {
            "at" => Relation::At,
            "by" => Relation::By,
            "from" => Relation::From,
            "until" => Relation::Until,
            _ => unreachable!("clock relation capture"),
        };
        return checked_clock(hour, minute, meridian, relation)
            .map(|value| (value, &text[m[0].len()..]));
    }
    if let Some(m) = DATE.captures(text) {
        let month = MONTHS.iter().position(|v| v.eq_ignore_ascii_case(&m[1]))? as u8 + 1;
        let day = m[2].parse().ok()?;
        let year = m.get(3).and_then(|v| v.as_str().parse().ok());
        return checked_date(month, day, year).map(|value| (value, &text[m[0].len()..]));
    }
    if let Some(rest) = strip_english_word(text, "after hours") {
        return Some((Timing::AfterHours, rest));
    }
    let (week, next) = if let Some(rest) = text.strip_prefix("next ") {
        (rest, true)
    } else {
        (text.strip_prefix("on ").unwrap_or(text), false)
    };
    for (index, (_, day)) in DAYS.iter().enumerate() {
        if let Some(rest) = strip_english_word(week, &day.to_ascii_lowercase()) {
            return Some((
                Timing::Weekday {
                    day: index as u8,
                    next,
                },
                rest,
            ));
        }
    }
    for (index, (_, word)) in RELATIVE.iter().enumerate() {
        if let Some(rest) = strip_english_word(text, word) {
            return Some((Timing::Relative(index as u8), rest));
        }
    }
    None
}

fn strip_english_word<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    text.strip_prefix(prefix)
        .filter(|tail| tail.is_empty() || tail.starts_with(' '))
}

fn checked_clock(
    hour: u8,
    minute: Option<u8>,
    meridian: Option<Meridian>,
    relation: Relation,
) -> Option<Timing> {
    let valid_hour = if meridian.is_some() {
        (1..=12).contains(&hour)
    } else {
        hour < 24 || (hour == 24 && minute.unwrap_or(0) == 0)
    };
    (valid_hour && minute.is_none_or(|m| m < 60)).then_some(Timing::Clock {
        hour,
        minute,
        meridian,
        relation,
    })
}

fn checked_date(month: u8, day: u8, year: Option<u16>) -> Option<Timing> {
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_some_and(|y| y % 4 != 0 || (y % 100 == 0 && y % 400 != 0)) => 28,
        2 => 29,
        _ => return None,
    };
    ((1..=days).contains(&day) && year != Some(0)).then_some(Timing::Date { month, day, year })
}

impl Timing {
    pub(super) fn is_clock(&self) -> bool {
        matches!(self, Self::Clock { .. } | Self::AfterHours)
    }

    pub(super) fn english(&self) -> String {
        match *self {
            Self::Relative(rel) => RELATIVE[rel as usize].1.into(),
            Self::Weekday { day, next } => format!(
                "{}{}",
                if next { "next " } else { "on " },
                DAYS[day as usize].1
            ),
            Self::Date { month, day, year } => {
                let date = format!("on {} {day}", MONTHS[(month - 1) as usize]);
                year.map_or_else(|| date.clone(), |year| format!("{date}, {year}"))
            }
            Self::AfterHours => "after hours".into(),
            Self::Clock {
                hour,
                minute,
                meridian,
                relation,
            } => {
                let prep = match relation {
                    Relation::At => "at",
                    Relation::From => "from",
                    Relation::By => "by",
                    Relation::Until => "until",
                };
                let min = minute.map(|m| format!(":{m:02}")).unwrap_or_default();
                let suffix = match meridian {
                    Some(Meridian::Am) => " AM",
                    Some(Meridian::Pm) => " PM",
                    None => "",
                };
                format!("{prep} {hour}{min}{suffix}")
            }
        }
    }

    pub(super) fn japanese(&self, copula: bool) -> String {
        let particle = if copula { "" } else { "に" };
        match *self {
            Self::Relative(rel) => RELATIVE[rel as usize].0.into(),
            Self::Weekday { day, next } => format!(
                "{}{}{particle}",
                if next { "来週の" } else { "" },
                DAYS[day as usize].0
            ),
            Self::Date { month, day, year } => {
                let year = year.map(|y| format!("{y}年")).unwrap_or_default();
                format!("{year}{month}月{day}日{particle}")
            }
            Self::AfterHours => format!("営業時間外{particle}"),
            Self::Clock {
                hour,
                minute,
                meridian,
                relation,
            } => {
                let start = match meridian {
                    Some(Meridian::Am) => "午前",
                    Some(Meridian::Pm) => "午後",
                    None => "",
                };
                let min = minute.map(|m| format!("{m:02}分")).unwrap_or_default();
                let end = match relation {
                    Relation::At => particle,
                    Relation::From => "から",
                    Relation::By => "までに",
                    Relation::Until => "まで",
                };
                format!("{start}{hour}時{min}{end}")
            }
        }
    }
}
