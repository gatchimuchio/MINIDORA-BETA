//! Offline bidirectional translation of bounded Japanese and English statements.
use std::sync::LazyLock;

use minidora_domain::{
    Result, TranslationLanguage, TranslationRegister, TranslationRequest, failure, identity,
};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::{
    language::{
        MAX_EXACT_FIELD_GENERATION_BYTES, MAX_EXACT_FIELD_GENERATION_TOKENS,
        MeaningCompositionFormat, MeaningCompositionPurpose, MeaningCompositionStyle,
        MeaningGenerationReport, MeaningGenerationRequest, MeaningGrammarFragment,
        MeaningGrammarUnit, MeaningRole,
    },
    translation_lexicon::{NOUNS, Tense, VERBS, Verb, japanese_verb},
    translation_time::{Timing, parse_timing, small_number},
};

pub(crate) const ARTIFACT_KIND: &str = "minidora.core-computation.translation.ja-en.v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranslationRevisionProof {
    pub(crate) request_bundle: String,
    pub(crate) prior_result: String,
    pub(crate) prior_compute: String,
    pub(crate) prior_render: String,
    pub(crate) register: TranslationRegister,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TranslationMeaningPlan {
    pub(crate) request: TranslationRequest,
    pub(crate) source_bundle: String,
    pub(crate) revision: Option<TranslationRevisionProof>,
    lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Line {
    prefix: String,
    sentences: Vec<Sentence>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sentence {
    condition: Option<Condition>,
    main: Clause,
    joined: Option<(Connector, Clause)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Connector {
    Contrast,
    Addition,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Condition {
    Weather(Weather),
    Clause(Clause),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Weather {
    snow: bool,
    negative: bool,
}

impl Weather {
    fn japanese_condition(self) -> &'static str {
        match (self.snow, self.negative) {
            (false, false) => "雨が降る場合は、",
            (false, true) => "雨が降らない場合は、",
            (true, false) => "雪が降る場合は、",
            (true, true) => "雪が降らない場合は、",
        }
    }
    fn english_condition(self, plain: bool) -> &'static str {
        match (self.snow, self.negative, plain) {
            (false, false, _) => "If it rains, ",
            (false, true, false) => "If it does not rain, ",
            (false, true, true) => "If it doesn't rain, ",
            (true, false, _) => "If it snows, ",
            (true, true, false) => "If it does not snow, ",
            (true, true, true) => "If it doesn't snow, ",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Clause {
    subject: Nominal,
    verb: Verb,
    object: Option<Nominal>,
    timing: Vec<Timing>,
    amount: Option<Amount>,
    tense: Tense,
    negative: bool,
    polite: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Article {
    Definite,
    Indefinite,
    Bare,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Nominal {
    Noun {
        key: String,
        plural: bool,
        article: Article,
    },
    Pronoun(u8),
    Name(String),
}

const PRONOUNS: &[(&str, &str)] = &[
    ("I", "私"),
    ("you", "あなた"),
    ("we", "私たち"),
    ("they", "彼ら"),
    ("he", "彼"),
    ("she", "彼女"),
    ("it", "それ"),
];
const I: u8 = 0;
const YOU: u8 = 1;
const WE: u8 = 2;
const THEY: u8 = 3;
const IT: u8 = 6;

impl Nominal {
    fn name(text: &str) -> Option<Self> {
        let mut words = text.split_ascii_whitespace();
        let first = words.next()?;
        (text.len() <= 80
            && first.starts_with(|ch: char| ch.is_ascii_uppercase())
            && !first.eq_ignore_ascii_case("the")
            && !first.eq_ignore_ascii_case("an")
            && !first.eq_ignore_ascii_case("a")
            && text.split(' ').all(|word| {
                (word.starts_with(|ch: char| ch.is_ascii_uppercase())
                    || matches!(word, "of" | "and"))
                    && word
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'\''))
            })
            && words.count() <= 4)
            .then(|| Self::Name(text.into()))
    }

    fn parse_japanese(text: &str, object: bool) -> Option<Self> {
        if let Some(index) = PRONOUNS.iter().position(|(_, ja)| *ja == text) {
            return Some(Self::Pronoun(index as u8));
        }
        let (article, text) = if let Some(rest) = text.strip_prefix("ある") {
            (Article::Indefinite, rest)
        } else {
            (
                if object {
                    Article::Bare
                } else {
                    Article::Definite
                },
                text,
            )
        };
        if let Some((key, _, _)) = NOUNS.iter().find(|(_, _, ja)| *ja == text) {
            let plural = object
                && article == Article::Bare
                && matches!(*key, "book" | "ticket" | "document" | "form" | "item");
            return Some(Self::Noun {
                key: (*key).into(),
                plural,
                article,
            });
        }
        Self::name(text)
    }

    fn parse_english(text: &str, object: bool) -> Option<Self> {
        if let Some(index) = PRONOUNS
            .iter()
            .position(|(en, _)| en.eq_ignore_ascii_case(text))
        {
            return Some(Self::Pronoun(index as u8));
        }
        let lowered = text.to_ascii_lowercase();
        let (term, article) = if let Some(rest) = lowered.strip_prefix("the ") {
            (rest, Article::Definite)
        } else if let Some(rest) = lowered
            .strip_prefix("a ")
            .or_else(|| lowered.strip_prefix("an "))
        {
            (rest, Article::Indefinite)
        } else {
            (lowered.as_str(), Article::Bare)
        };
        for (key, plural, _) in NOUNS {
            if term == *key || term == *plural {
                // Reject spurious bare singulars except uncountable nouns.
                if article == Article::Bare && term == *key && key != plural && !object {
                    break;
                }
                return Some(Self::Noun {
                    key: (*key).into(),
                    plural: term == *plural && key != plural,
                    article,
                });
            }
        }
        Self::name(text)
    }

    fn key(&self) -> Option<&str> {
        match self {
            Self::Noun { key, .. } => Some(key),
            Self::Name(value) => value.rsplit(' ').next(),
            _ => None,
        }
    }
    fn third_singular(&self) -> bool {
        match self {
            Self::Pronoun(index) => !matches!(*index, I | YOU | WE | THEY),
            Self::Noun { plural, .. } => !plural,
            Self::Name(_) => true,
        }
    }
    fn english(&self, object: bool) -> String {
        match self {
            Self::Name(value) => value.clone(),
            Self::Pronoun(index) => {
                if object {
                    ["me", "you", "us", "them", "him", "her", "it"][*index as usize].into()
                } else {
                    PRONOUNS[*index as usize].0.into()
                }
            }
            Self::Noun {
                key,
                plural,
                article,
            } => {
                let lexical = NOUNS
                    .iter()
                    .find(|(word, _, _)| *word == key)
                    .expect("recorded noun");
                let term = if *plural { lexical.1 } else { lexical.0 };
                let prefix = match article {
                    Article::Definite => "the ",
                    Article::Indefinite if term.starts_with(['a', 'e', 'i', 'o', 'u']) => "an ",
                    Article::Indefinite => "a ",
                    Article::Bare if !plural && lexical.0 != lexical.1 => "the ",
                    Article::Bare => "",
                };
                format!("{prefix}{term}")
            }
        }
    }
    fn japanese(&self) -> String {
        match self {
            Self::Name(value) => value.clone(),
            Self::Pronoun(index) => PRONOUNS[*index as usize].1.into(),
            Self::Noun { key, article, .. } => {
                let (_, _, term) = NOUNS
                    .iter()
                    .find(|(word, _, _)| *word == key)
                    .expect("recorded noun");
                format!(
                    "{}{term}",
                    if *article == Article::Indefinite {
                        "ある"
                    } else {
                        ""
                    }
                )
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Unit {
    Yen,
    Dollar,
    Kg,
    Gram,
    Meter,
    Kilometer,
    Percent,
    Person,
}
const UNITS: &[(Unit, &str, &str, &str)] = &[
    (Unit::Yen, "円", "yen", "yen"),
    (Unit::Dollar, "ドル", "dollar", "dollars"),
    (Unit::Kg, "kg", "kg", "kg"),
    (Unit::Gram, "g", "g", "g"),
    (Unit::Meter, "m", "m", "m"),
    (Unit::Kilometer, "km", "km", "km"),
    (Unit::Percent, "%", "percent", "percent"),
    (Unit::Person, "人", "person", "people"),
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Amount {
    value: String,
    unit: Unit,
}

impl Amount {
    fn parse(text: &str, lang: TranslationLanguage) -> Option<Self> {
        static NUMBER: LazyLock<Regex> = LazyLock::new(|| {
            Regex::new(r"^[+-]?(?:[0-9]{1,3}(?:,[0-9]{3})+|[0-9]+)(?:\.[0-9]+)?$")
                .expect("decimal amount regex")
        });
        for (unit, ja, en_s, en_p) in UNITS {
            let units = if lang == TranslationLanguage::Japanese {
                [*ja, *ja]
            } else {
                [*en_s, *en_p]
            };
            for label in units {
                if let Some(value) = text.strip_suffix(label).map(str::trim_end) {
                    let value = if value.len() <= 28 && NUMBER.is_match(value) {
                        Some(value.to_owned())
                    } else {
                        small_number(value).map(|value| value.to_string())
                    };
                    if let Some(value) = value {
                        return Some(Self { value, unit: *unit });
                    }
                }
            }
        }
        None
    }
    fn english(&self) -> String {
        let (_, _, one, many) = UNITS
            .iter()
            .find(|(key, _, _, _)| *key == self.unit)
            .expect("recorded unit");
        format!(
            "{} {}",
            self.value,
            if self.value == "1" { one } else { many }
        )
    }
    fn japanese(&self) -> String {
        let (_, label, _, _) = UNITS
            .iter()
            .find(|(key, _, _, _)| *key == self.unit)
            .expect("recorded unit");
        format!("{}{label}", self.value)
    }
}

impl Clause {
    fn english(&self, plain: bool) -> String {
        let mut text = self.subject.english(false);
        let predicate = match self.verb {
            Verb::Be => english_copula(self, plain).into(),
            Verb::Rain | Verb::Snow => {
                let base = if self.verb == Verb::Rain {
                    "rain"
                } else {
                    "snow"
                };
                let mut weather = self.clone();
                if weather.tense == Tense::Present
                    && weather
                        .timing
                        .iter()
                        .any(|part| matches!(part, Timing::Relative(1 | 3)))
                {
                    weather.tense = Tense::Future;
                }
                english_action(
                    [
                        base,
                        if self.verb == Verb::Rain {
                            "rains"
                        } else {
                            "snows"
                        },
                        if self.verb == Verb::Rain {
                            "rained"
                        } else {
                            "snowed"
                        },
                    ],
                    &weather,
                    plain,
                )
            }
            _ => {
                let lexeme = VERBS
                    .iter()
                    .find(|lexeme| lexeme.verb == self.verb)
                    .expect("recorded verb");
                english_action(lexeme.en, self, plain)
            }
        };
        text.push_str(&predicate);
        if let Some(object) = &self.object {
            text.push(' ');
            text.push_str(&object.english(true));
        }
        if let Some(amount) = &self.amount {
            text.push(' ');
            text.push_str(&amount.english());
        }
        for timing in &self.timing {
            text.push(' ');
            text.push_str(&timing.english());
        }
        text
    }

    fn japanese(&self, register: TranslationRegister, subject_particle: char) -> String {
        let polite = match register {
            TranslationRegister::Preserve => self.polite,
            TranslationRegister::Plain => false,
            TranslationRegister::Polite => true,
        };
        let mut text = String::new();
        let weather = matches!(self.verb, Verb::Rain | Verb::Snow);
        if !weather && !(self.subject == Nominal::Pronoun(YOU) && self.tense == Tense::Ability) {
            text.push_str(&self.subject.japanese());
            text.push(subject_particle);
        }
        let has_time = self.timing.iter().any(Timing::is_clock);
        for timing in self.timing.iter().filter(|timing| !timing.is_clock()) {
            text.push_str(&timing.japanese(self.verb == Verb::Be || has_time));
        }
        for timing in self.timing.iter().filter(|timing| timing.is_clock()) {
            text.push_str(&timing.japanese(self.verb == Verb::Be));
        }
        if let Some(object) = &self.object {
            text.push_str(&object.japanese());
            text.push(object_particle(self.verb));
        }
        if let Some(amount) = &self.amount {
            text.push_str(&amount.japanese());
        }
        let surface = if self.verb == Verb::Be {
            japanese_copula(self.tense, self.negative, polite).into()
        } else {
            let mut lemma = match self.verb {
                Verb::Rain | Verb::Snow => {
                    text.push_str(if self.verb == Verb::Rain {
                        "雨が"
                    } else {
                        "雪が"
                    });
                    "降る"
                }
                _ => {
                    VERBS
                        .iter()
                        .find(|lexeme| lexeme.verb == self.verb)
                        .expect("recorded verb")
                        .ja[0]
                }
            };
            let key = self.subject.key().unwrap_or_default().to_ascii_lowercase();
            if matches!(self.verb, Verb::Open | Verb::Close) {
                let facility = match key.as_str() {
                    "library" | "museum" | "gallery" => 1,
                    "store" | "shop" => 2,
                    "venue" | "hall" => 3,
                    _ => 0,
                };
                lemma = VERBS
                    .iter()
                    .find(|entry| entry.verb == self.verb)
                    .expect("recorded verb")
                    .ja[facility];
            }
            japanese_verb(lemma, self.tense, self.negative, polite)
        };
        text.push_str(&surface);
        text
    }
}

fn english_copula(clause: &Clause, plain: bool) -> &'static str {
    let subject = &clause.subject;
    let pronoun = matches!(subject, Nominal::Pronoun(_));
    let first_person = subject == &Nominal::Pronoun(I);
    let singular = first_person || subject.third_singular();
    let (formal, contracted) = match (clause.tense, clause.negative, first_person, singular) {
        (Tense::Future, true, _, _) => (" will not be", " won't be"),
        (Tense::Future, false, _, _) if pronoun => (" will be", "'ll be"),
        (Tense::Future, false, _, _) => (" will be", " will be"),
        (Tense::Past, true, _, true) => (" was not", " wasn't"),
        (Tense::Past, true, _, false) => (" were not", " weren't"),
        (Tense::Past, false, _, true) => (" was", " was"),
        (Tense::Past, false, _, false) => (" were", " were"),
        (_, true, true, _) => (" am not", "'m not"),
        (_, true, false, true) => (" is not", " isn't"),
        (_, true, false, false) => (" are not", " aren't"),
        (_, false, true, _) => (" am", "'m"),
        (_, false, false, true) if pronoun => (" is", "'s"),
        (_, false, false, false) if pronoun => (" are", "'re"),
        (_, false, false, true) => (" is", " is"),
        (_, false, false, false) => (" are", " are"),
    };
    if plain { contracted } else { formal }
}

fn english_action(forms: [&str; 3], clause: &Clause, plain: bool) -> String {
    let [base, singular, past] = forms;
    let (auxiliary, lexical_verb) = match (
        clause.tense,
        clause.negative,
        clause.subject.third_singular(),
    ) {
        (Tense::Ability, false, _) => (" can ", base),
        (Tense::Ability, true, _) => (if plain { " can't " } else { " cannot " }, base),
        (Tense::Future, false, _) if plain && matches!(&clause.subject, Nominal::Pronoun(_)) => {
            ("'ll ", base)
        }
        (Tense::Future, false, _) => (" will ", base),
        (Tense::Future, true, _) => (if plain { " won't " } else { " will not " }, base),
        (Tense::Past, false, _) => (" ", past),
        (Tense::Past, true, _) => (if plain { " didn't " } else { " did not " }, base),
        (Tense::Present, true, true) => (if plain { " doesn't " } else { " does not " }, base),
        (Tense::Present, true, false) => (if plain { " don't " } else { " do not " }, base),
        (Tense::Present, false, true) => (" ", singular),
        (Tense::Present, false, false) => (" ", base),
    };
    format!("{auxiliary}{lexical_verb}")
}

fn object_particle(verb: Verb) -> char {
    if verb == Verb::Attend { 'に' } else { 'を' }
}

fn japanese_copula(tense: Tense, negative: bool, polite: bool) -> &'static str {
    match (tense == Tense::Past, negative, polite) {
        (false, false, false) => "だ",
        (false, false, true) => "です",
        (true, false, false) => "だった",
        (true, false, true) => "でした",
        (false, true, false) => "ではない",
        (false, true, true) => "ではありません",
        (true, true, false) => "ではなかった",
        (true, true, true) => "ではありませんでした",
    }
}

const FORMS: [(Tense, bool, bool); 12] = [
    (Tense::Present, false, false),
    (Tense::Present, true, false),
    (Tense::Present, false, true),
    (Tense::Present, true, true),
    (Tense::Past, false, false),
    (Tense::Past, true, false),
    (Tense::Past, false, true),
    (Tense::Past, true, true),
    (Tense::Ability, false, false),
    (Tense::Ability, true, false),
    (Tense::Ability, false, true),
    (Tense::Ability, true, true),
];

fn japanese_clause(text: &str) -> Option<Clause> {
    let mut subjects = text.match_indices(['は', 'が']).filter_map(|(at, marker)| {
        Nominal::parse_japanese(&text[..at], false)
            .map(|noun| (noun, text[at + marker.len()..].trim()))
    });
    for (subject, predicate) in &mut subjects {
        if let Some(clause) = japanese_predicate(subject, predicate, false) {
            return Some(clause);
        }
    }
    if let Some(clause) = japanese_weather(text) {
        return Some(clause);
    }
    japanese_predicate(Nominal::Pronoun(YOU), text, true)
}

fn japanese_predicate(subject: Nominal, predicate: &str, only_ability: bool) -> Option<Clause> {
    for entry in VERBS {
        for lemma in entry.ja {
            for (tense, negative, polite) in FORMS {
                if only_ability && tense != Tense::Ability {
                    continue;
                }
                let form = japanese_verb(lemma, tense, negative, polite);
                let Some(before) = predicate.strip_suffix(&form).map(str::trim) else {
                    continue;
                };
                let mut timing_raw = before;
                let mut object = None;
                if entry.object {
                    let matched = before.match_indices(object_particle(entry.verb)).find_map(
                        |(at, particle)| {
                            let parts = &before[..at];
                            let suffix = &before[at + particle.len()..];
                            if !suffix.is_empty() {
                                let kind = Nominal::parse_japanese(parts.trim(), true)?;
                                parse_timing(suffix, TranslationLanguage::Japanese)?;
                                return Some((kind, suffix));
                            }
                            parts.char_indices().map(|(at, _)| at).find_map(|start| {
                                let kind = Nominal::parse_japanese(parts[start..].trim(), true)?;
                                parse_timing(&parts[..start], TranslationLanguage::Japanese)?;
                                Some((kind, &parts[..start]))
                            })
                        },
                    );
                    let Some((kind, timing)) = matched else {
                        continue;
                    };
                    object = Some(kind);
                    timing_raw = timing;
                }
                let Some(timing) = parse_timing(timing_raw, TranslationLanguage::Japanese) else {
                    continue;
                };
                return Some(Clause {
                    subject,
                    verb: entry.verb,
                    object,
                    timing,
                    amount: None,
                    tense,
                    negative,
                    polite,
                });
            }
        }
    }
    if only_ability {
        return None;
    }
    for (tense, negative, polite) in FORMS {
        if tense == Tense::Ability {
            continue;
        }
        let Some(value) = predicate
            .strip_suffix(japanese_copula(tense, negative, polite))
            .map(str::trim)
        else {
            continue;
        };
        let amount = Amount::parse(value, TranslationLanguage::Japanese);
        let timing = if amount.is_some() {
            Vec::new()
        } else {
            let Some(times) = parse_timing(value, TranslationLanguage::Japanese) else {
                continue;
            };
            if times.is_empty() {
                continue;
            }
            times
        };
        return Some(Clause {
            subject,
            verb: Verb::Be,
            object: None,
            timing,
            amount,
            tense,
            negative,
            polite,
        });
    }
    None
}

fn japanese_weather(text: &str) -> Option<Clause> {
    for (word, verb) in [("雨", Verb::Rain), ("雪", Verb::Snow)] {
        for (tense, negative, polite) in FORMS {
            if tense == Tense::Ability {
                continue;
            }
            let verb_form = japanese_verb("降る", tense, negative, polite);
            let copula = japanese_copula(tense, negative, polite);
            let patterns = [format!("{word}が{verb_form}"), format!("{word}{copula}")];
            for (i, pattern) in patterns.iter().enumerate() {
                if i == 1 && negative {
                    continue;
                }
                let Some(value) = text.strip_suffix(pattern) else {
                    continue;
                };
                let value = value.strip_suffix('は').unwrap_or(value);
                let Some(timing) = parse_timing(value, TranslationLanguage::Japanese) else {
                    continue;
                };
                return Some(Clause {
                    subject: Nominal::Pronoun(IT),
                    verb,
                    object: None,
                    timing,
                    amount: None,
                    tense,
                    negative,
                    polite,
                });
            }
        }
    }
    None
}

fn english_clause(text: &str) -> Option<Clause> {
    let lower = text.to_ascii_lowercase();
    for (at, _) in text.match_indices(' ') {
        let Some(subject) = Nominal::parse_english(&text[..at], false) else {
            continue;
        };
        let Some(predicate) = lower.get(at + 1..) else {
            continue;
        };
        let predicate = predicate.trim_start();
        if let Some(clause) = english_predicate(subject, predicate, text[at + 1..].trim_start()) {
            return Some(clause);
        }
    }
    None
}

fn strip_words<'a>(source: &'a str, prefix: &str) -> Option<&'a str> {
    source
        .strip_prefix(prefix)
        .filter(|rest| rest.is_empty() || rest.starts_with(' '))
        .map(str::trim_start)
}

fn english_timing(text: &str) -> Option<Vec<Timing>> {
    parse_timing(
        &text.replace("a.m.", "am").replace("p.m.", "pm"),
        TranslationLanguage::English,
    )
}

fn english_verb<'a>(
    source: &'a str,
    forms: [&str; 3],
    third: bool,
) -> Option<(Tense, bool, &'a str)> {
    let [base, singular, past] = forms;
    for (prefix, tense, negative) in [
        ("will not ", Tense::Future, true),
        ("won't ", Tense::Future, true),
        ("will ", Tense::Future, false),
        ("cannot ", Tense::Ability, true),
        ("can't ", Tense::Ability, true),
        ("can not ", Tense::Ability, true),
        ("can ", Tense::Ability, false),
        ("did not ", Tense::Past, true),
        ("didn't ", Tense::Past, true),
        (
            if third { "does not " } else { "do not " },
            Tense::Present,
            true,
        ),
        (
            if third { "doesn't " } else { "don't " },
            Tense::Present,
            true,
        ),
    ] {
        if let Some(rest) = source
            .strip_prefix(prefix)
            .and_then(|rest| strip_words(rest, base))
        {
            return Some((tense, negative, rest));
        }
    }
    if let Some(rest) = strip_words(source, if third { singular } else { base }) {
        Some((Tense::Present, false, rest))
    } else {
        strip_words(source, past).map(|rest| (Tense::Past, false, rest))
    }
}

fn english_predicate(subject: Nominal, predicate: &str, original: &str) -> Option<Clause> {
    let third = subject.third_singular();
    for entry in VERBS {
        let Some((tense, negative, rest)) = english_verb(predicate, entry.en, third) else {
            continue;
        };
        let mut object = None;
        let mut timing = english_timing(rest);
        if entry.object {
            if let Some((kind, when)) = english_object_and_time(rest, original) {
                object = Some(kind);
                timing = Some(when);
            }
            if object.is_none() {
                continue;
            }
        }
        if let Some(timing) = timing {
            return Some(Clause {
                subject,
                verb: entry.verb,
                object,
                timing,
                amount: None,
                tense,
                negative,
                polite: true,
            });
        }
    }
    if subject == Nominal::Pronoun(IT) {
        for (verb, forms) in [
            (Verb::Rain, ["rain", "rains", "rained"]),
            (Verb::Snow, ["snow", "snows", "snowed"]),
        ] {
            if let Some((tense, negative, rest)) = english_verb(predicate, forms, true)
                && tense != Tense::Ability
                && let Some(timing) = english_timing(rest)
            {
                return Some(Clause {
                    subject,
                    verb,
                    object: None,
                    timing,
                    amount: None,
                    tense,
                    negative,
                    polite: true,
                });
            }
        }
    }
    let first = subject == Nominal::Pronoun(I);
    let present = if third {
        "is"
    } else if first {
        "am"
    } else {
        "are"
    };
    let past = if third || first { "was" } else { "were" };
    for (be, tense, forced_negative) in [
        ("will not be", Tense::Future, true),
        ("won't be", Tense::Future, true),
        ("will be", Tense::Future, false),
        (present, Tense::Present, false),
        (past, Tense::Past, false),
    ] {
        let Some(value) = strip_words(predicate, be) else {
            continue;
        };
        let (negative, value) =
            strip_words(value, "not").map_or((false, value), |tail| (true, tail));
        if negative && forced_negative {
            return None;
        }
        let amount = Amount::parse(value, TranslationLanguage::English);
        let timing = if amount.is_some() {
            Vec::new()
        } else {
            let Some(times) = english_timing(value) else {
                continue;
            };
            if times.is_empty() {
                continue;
            }
            times
        };
        return Some(Clause {
            subject,
            verb: Verb::Be,
            object: None,
            timing,
            amount,
            tense,
            negative: negative || forced_negative,
            polite: true,
        });
    }
    None
}

fn english_object_and_time(rest: &str, original_predicate: &str) -> Option<(Nominal, Vec<Timing>)> {
    for end in rest
        .match_indices(' ')
        .map(|(at, _)| at)
        .chain(std::iter::once(rest.len()))
    {
        let normalized = &rest[..end];
        // The ASCII case fold preserves offsets and proper name capitalization.
        let at = original_predicate.len().checked_sub(rest.len())?;
        let candidate = original_predicate.get(at..at + end)?;
        if !candidate.eq_ignore_ascii_case(normalized) {
            return None;
        }
        let object = if let Some(index) = ["me", "you", "us", "them", "him", "her", "it"]
            .iter()
            .position(|p| p.eq_ignore_ascii_case(candidate))
        {
            Some(Nominal::Pronoun(index as u8))
        } else {
            Nominal::parse_english(candidate, true)
        };
        if let Some(object) = object
            && let Some(timing) = english_timing(&rest[end..])
        {
            return Some((object, timing));
        }
    }
    None
}

fn parse_condition(text: &str, lang: TranslationLanguage) -> (Option<Condition>, &str) {
    let patterns: &[(&str, bool, bool)] = match lang {
        TranslationLanguage::Japanese => &[
            ("雨なら", false, false),
            ("雨の場合は", false, false),
            ("雨が降る場合は", false, false),
            ("雨が降ったら", false, false),
            ("雨が降らなければ", false, true),
            ("雨でなければ", false, true),
            ("雨が降らない場合は", false, true),
            ("雪なら", true, false),
            ("雪の場合は", true, false),
            ("雪が降る場合は", true, false),
            ("雪が降らなければ", true, true),
            ("雪が降らない場合は", true, true),
        ],
        TranslationLanguage::English => &[
            ("if it rains", false, false),
            ("if it does not rain", false, true),
            ("if it doesn't rain", false, true),
            ("unless it rains", false, true),
            ("if it snows", true, false),
            ("if it does not snow", true, true),
            ("if it doesn't snow", true, true),
            ("unless it snows", true, true),
        ],
    };
    let split = if lang == TranslationLanguage::Japanese {
        text.split_once(['、', ','])
    } else {
        text.split_once(',')
    };
    if let Some((prefix, body)) = split {
        let prefix = prefix.trim();
        let prefix = if lang == TranslationLanguage::Japanese {
            prefix.strip_prefix("もし").unwrap_or(prefix)
        } else {
            prefix
        };
        if let Some((_, snow, negative)) = patterns
            .iter()
            .find(|(form, _, _)| form.eq_ignore_ascii_case(prefix))
        {
            return (
                Some(Condition::Weather(Weather {
                    snow: *snow,
                    negative: *negative,
                })),
                body.trim(),
            );
        }
        let clause = match lang {
            TranslationLanguage::Japanese => prefix
                .strip_suffix("場合は")
                .or_else(|| prefix.strip_suffix("場合"))
                .or_else(|| prefix.strip_suffix("なら"))
                .and_then(japanese_clause),
            TranslationLanguage::English => {
                if prefix
                    .get(..3)
                    .is_some_and(|marker| marker.eq_ignore_ascii_case("if "))
                {
                    english_clause(&prefix[3..])
                } else if prefix
                    .get(..7)
                    .is_some_and(|marker| marker.eq_ignore_ascii_case("unless "))
                {
                    english_clause(&prefix[7..]).map(|mut clause| {
                        clause.negative = !clause.negative;
                        clause
                    })
                } else {
                    None
                }
            }
        };
        if let Some(clause) = clause {
            return (Some(Condition::Clause(clause)), body.trim());
        }
    } else if lang == TranslationLanguage::Japanese {
        for (form, snow, negative) in patterns {
            if let Some(body) = text.strip_prefix(form) {
                return (
                    Some(Condition::Weather(Weather {
                        snow: *snow,
                        negative: *negative,
                    })),
                    body.trim(),
                );
            }
        }
    }
    (None, text)
}

fn parse_sentence(text: &str, lang: TranslationLanguage) -> Option<Sentence> {
    let (condition, text) = parse_condition(text, lang);
    let markers: &[(&str, Connector)] = match lang {
        TranslationLanguage::Japanese => &[
            ("が、", Connector::Contrast),
            ("し、", Connector::Addition),
            ("、そして", Connector::Addition),
        ],
        TranslationLanguage::English => &[
            (", but ", Connector::Contrast),
            (", and ", Connector::Addition),
            (" but ", Connector::Contrast),
        ],
    };
    for (marker, connector) in markers {
        if let Some((first, second)) = text.split_once(marker) {
            // The scope of a leading condition over a coordination is ambiguous here.
            if condition.is_some() {
                return None;
            }
            let (first, second) = (
                timed_clause(first.trim(), lang)?,
                timed_clause(second.trim(), lang)?,
            );
            return Some(Sentence {
                condition,
                main: first,
                joined: Some((*connector, second)),
            });
        }
    }
    Some(Sentence {
        condition,
        main: timed_clause(text, lang)?,
        joined: None,
    })
}

fn timed_clause(text: &str, lang: TranslationLanguage) -> Option<Clause> {
    let (parser, delimiter): (fn(&str) -> Option<Clause>, &[char]) = match lang {
        TranslationLanguage::Japanese => (japanese_clause, &['、', ',']),
        TranslationLanguage::English => (english_clause, &[',']),
    };
    if let Some(clause) = parser(text) {
        return Some(clause);
    }
    let (prefix, tail) = text.split_once(delimiter)?;
    let mut timing = match lang {
        TranslationLanguage::Japanese => parse_timing(prefix.trim(), lang)?,
        TranslationLanguage::English => english_timing(&prefix.trim().to_ascii_lowercase())?,
    };
    let mut clause = parser(tail.trim())?;
    if timing.is_empty()
        || !clause.timing.iter().all(|part| {
            timing
                .iter()
                .all(|prefix| part.is_clock() != prefix.is_clock())
        })
    {
        return None;
    }
    timing.append(&mut clause.timing);
    clause.timing = timing;
    Some(clause)
}

fn capitalize_ascii(text: String) -> String {
    let mut text = text;
    if let Some(first) = text.get_mut(..1) {
        first.make_ascii_uppercase();
    }
    text
}

impl Sentence {
    fn render(&self, lang: TranslationLanguage, register: TranslationRegister) -> String {
        match lang {
            TranslationLanguage::English => {
                let plain = register == TranslationRegister::Plain;
                let mut text = match &self.condition {
                    Some(Condition::Weather(weather)) => weather.english_condition(plain).into(),
                    Some(Condition::Clause(clause)) => format!("If {}, ", clause.english(plain)),
                    None => String::new(),
                };
                text.push_str(&self.main.english(plain));
                if let Some((connector, clause)) = &self.joined {
                    text.push_str(match connector {
                        Connector::Contrast => ", but ",
                        Connector::Addition => ", and ",
                    });
                    text.push_str(&clause.english(plain));
                }
                text.push('.');
                capitalize_ascii(text)
            }
            TranslationLanguage::Japanese => {
                let mut text = match &self.condition {
                    Some(Condition::Weather(weather)) => weather.japanese_condition().into(),
                    Some(Condition::Clause(clause)) => {
                        let condition = clause.japanese(TranslationRegister::Plain, 'が');
                        if let Some(nominal) = condition.strip_suffix('だ') {
                            format!("{nominal}である場合は、")
                        } else {
                            format!("{condition}場合は、")
                        }
                    }
                    None => String::new(),
                };
                text.push_str(&self.main.japanese(register, 'は'));
                if let Some((connector, clause)) = &self.joined {
                    text.push_str(match connector {
                        Connector::Contrast => "が、",
                        Connector::Addition => "し、",
                    });
                    text.push_str(&clause.japanese(register, 'は'));
                }
                text.push('。');
                text
            }
        }
    }
}

fn parse_line(text: &str, lang: TranslationLanguage) -> Option<Line> {
    let (prefix, text) = text
        .strip_prefix("- ")
        .map_or(("", text), |body| ("- ", body));
    let mut sentences = Vec::new();
    let mut start = 0;
    let abbreviations = if lang == TranslationLanguage::English {
        text.to_ascii_lowercase()
    } else {
        String::new()
    };
    for (at, ch) in text.char_indices() {
        let end = at + ch.len_utf8();
        if (lang == TranslationLanguage::Japanese && ch == '。')
            || (lang == TranslationLanguage::English
                && ch == '.'
                && !(text
                    .as_bytes()
                    .get(at.wrapping_sub(1))
                    .is_some_and(u8::is_ascii_digit)
                    && text.as_bytes().get(end).is_some_and(u8::is_ascii_digit))
                && !abbreviations
                    .get(at + 1..)
                    .is_some_and(|tail| tail.starts_with("m."))
                && !abbreviations
                    .get(at.saturating_sub(3)..end)
                    .is_some_and(|word| matches!(word, "a.m." | "p.m.")))
        {
            sentences.push(parse_sentence(text[start..at].trim(), lang)?);
            start = end;
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        sentences.push(parse_sentence(last, lang)?);
    }
    (!sentences.is_empty()).then_some(Line {
        prefix: prefix.into(),
        sentences,
    })
}

impl TranslationMeaningPlan {
    pub(crate) fn from_request(
        request: TranslationRequest,
        source_bundle: String,
        revision: Option<TranslationRevisionProof>,
    ) -> Result<Self> {
        let unavailable = || {
            failure(
                "TRANSLATION_UNSUPPORTED",
                "対応する短い記述として原文の意味を確定できませんでした。",
            )
        };
        if request.source_language == request.target_language
            || request.source.text.len() > 4096
            || request.source.text.trim().is_empty()
        {
            return Err(unavailable());
        }
        let mut lines = Vec::new();
        let mut count = 0;
        for line in request.source.text.lines() {
            if lines.len() >= 16 {
                return Err(unavailable());
            }
            let line = if line.trim().is_empty() {
                Line {
                    prefix: String::new(),
                    sentences: Vec::new(),
                }
            } else {
                parse_line(line.trim(), request.source_language).ok_or_else(unavailable)?
            };
            count += line.sentences.len();
            if count > 16 {
                return Err(unavailable());
            }
            lines.push(line);
        }
        Ok(Self {
            request,
            source_bundle,
            revision,
            lines,
        })
    }

    pub(crate) fn dependencies(&self) -> Vec<String> {
        let mut ids = vec![self.source_bundle.clone()];
        if let Some(proof) = &self.revision {
            ids.extend([
                proof.request_bundle.clone(),
                proof.prior_result.clone(),
                proof.prior_compute.clone(),
                proof.prior_render.clone(),
            ]);
        }
        ids
    }

    pub(crate) fn target_locale(&self) -> &'static str {
        self.request.target_language.locale()
    }

    fn render(&self) -> String {
        let target = self.request.target_language;
        let register = self
            .revision
            .as_ref()
            .map_or(self.request.register, |proof| proof.register);
        let separator = if target == TranslationLanguage::English {
            " "
        } else {
            ""
        };
        self.lines
            .iter()
            .map(|line| {
                let body = line
                    .sentences
                    .iter()
                    .map(|sentence| sentence.render(target, register))
                    .collect::<Vec<_>>()
                    .join(separator);
                format!("{}{body}", line.prefix)
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn grammar(&self, budget: usize) -> Result<MeaningGenerationRequest> {
        let text = self.render();
        let fields = text
            .split_inclusive('\n')
            .map(|value| MeaningGrammarFragment::ExactField {
                value: value.into(),
            })
            .collect();
        Ok(MeaningGenerationRequest {
            purpose: MeaningCompositionPurpose::ShortComposition,
            format: MeaningCompositionFormat::Paragraph,
            style: MeaningCompositionStyle::Neutral,
            units: vec![MeaningGrammarUnit {
                id: "translated-text".into(),
                role: MeaningRole::Body,
                evidence_ids: self.dependencies(),
                dependencies: Vec::new(),
                semantic_sha256: identity(&(ARTIFACT_KIND, self))?,
                fragments: fields,
            }],
            max_output_bytes: budget.min(MAX_EXACT_FIELD_GENERATION_BYTES),
            max_tokens: MAX_EXACT_FIELD_GENERATION_TOKENS,
        })
    }

    pub(crate) fn verify_surface(&self, text: &str) -> Result<()> {
        if text != self.render() {
            return Err(failure(
                "TRANSLATION_SURFACE",
                "訳文が確定した意味と形式に一致しません。",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratedTranslationArtifact {
    pub(crate) kind: String,
    pub(crate) meaning_plan: TranslationMeaningPlan,
    pub(crate) content_generation: MeaningGenerationReport,
}

impl GeneratedTranslationArtifact {
    pub(crate) fn new(
        meaning_plan: TranslationMeaningPlan,
        content_generation: MeaningGenerationReport,
    ) -> Self {
        Self {
            kind: ARTIFACT_KIND.into(),
            meaning_plan,
            content_generation,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use minidora_domain::SourceSpan;

    #[test]
    fn english_register_uses_the_same_source_and_preserves_names_times_and_layout() {
        let source =
            "- 雨が降らない場合は、Alexは午前9時に到着しません。\n- 私は会議に参加しませんでした。";
        let request = TranslationRequest {
            source: SourceSpan {
                start: 0,
                end: source.len(),
                text: source.into(),
            },
            source_language: TranslationLanguage::Japanese,
            target_language: TranslationLanguage::English,
            register: TranslationRegister::Preserve,
        };
        let form = |register| {
            Some(TranslationRevisionProof {
                request_bundle: "revision".into(),
                prior_result: "previous".into(),
                prior_compute: "compute".into(),
                prior_render: "render".into(),
                register,
            })
        };
        let original =
            TranslationMeaningPlan::from_request(request.clone(), "source".into(), None).unwrap();
        let plain = TranslationMeaningPlan::from_request(
            request.clone(),
            "source".into(),
            form(TranslationRegister::Plain),
        )
        .unwrap();
        let polite = TranslationMeaningPlan::from_request(
            request,
            "source".into(),
            form(TranslationRegister::Polite),
        )
        .unwrap();
        let formal =
            "- If it does not rain, Alex does not arrive at 9 AM.\n- I did not attend the meeting.";
        assert_eq!(original.render(), formal);
        assert_eq!(
            plain.render(),
            "- If it doesn't rain, Alex doesn't arrive at 9 AM.\n- I didn't attend the meeting."
        );
        assert_eq!(polite.render(), formal);
        assert_eq!(original.lines, plain.lines);
        assert_eq!(plain.lines, polite.lines);
    }

    #[test]
    fn english_contractions_follow_typed_predicate_and_subject() {
        let mut clause = Clause {
            subject: Nominal::Pronoun(I),
            verb: Verb::Be,
            object: None,
            timing: Vec::new(),
            amount: Some(Amount {
                value: "2".into(),
                unit: Unit::Kilometer,
            }),
            tense: Tense::Present,
            negative: true,
            polite: false,
        };
        assert_eq!(clause.english(true), "I'm not 2 km");
        assert_eq!(clause.english(false), "I am not 2 km");
        clause.verb = Verb::Arrive;
        clause.amount = None;
        for (tense, expected) in [
            (Tense::Present, "I don't arrive"),
            (Tense::Future, "I won't arrive"),
            (Tense::Ability, "I can't arrive"),
        ] {
            clause.tense = tense;
            assert_eq!(clause.english(true), expected);
        }
        clause.tense = Tense::Future;
        clause.negative = false;
        assert_eq!(clause.english(true), "I'll arrive");
        clause.subject = Nominal::Name("Alex".into());
        assert_eq!(clause.english(true), "Alex will arrive");
        clause.tense = Tense::Present;
        assert_eq!(clause.english(true), "Alex arrives");
    }
}
