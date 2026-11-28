//! Spanish morphology for the shared, bounded translation meaning plan.

use super::{
    Amount, Article, Clause, Condition, Connector, I, IT, Nominal, Sentence, THEY, Tense, Unit,
    VERBS, Verb, WE, Weather, YOU, strip_words,
};
use crate::translation_time::{Timing, parse_timing};
use minidora_domain::{TranslationLanguage, TranslationRegister};

const NOUNS: &[(&str, &str, &str, bool)] = &[
    ("library", "biblioteca", "bibliotecas", true),
    ("museum", "museo", "museos", false),
    ("gallery", "galería", "galerías", true),
    ("station", "estación", "estaciones", true),
    ("store", "tienda", "tiendas", true),
    ("shop", "quiosco", "quioscos", false),
    ("office", "oficina", "oficinas", true),
    ("school", "escuela", "escuelas", true),
    ("room", "sala", "salas", true),
    ("venue", "recinto", "recintos", false),
    ("hall", "auditorio", "auditorios", false),
    ("gate", "portal", "portales", false),
    ("door", "puerta", "puertas", true),
    ("meeting", "reunión", "reuniones", true),
    ("event", "evento", "eventos", false),
    ("class", "clase", "clases", true),
    ("lesson", "lección", "lecciones", true),
    ("workshop", "taller", "talleres", false),
    ("exhibition", "exposición", "exposiciones", true),
    ("lecture", "conferencia", "conferencias", true),
    ("session", "sesión", "sesiones", true),
    ("concert", "concierto", "conciertos", false),
    ("train", "tren", "trenes", false),
    ("bus", "autobús", "autobuses", false),
    ("flight", "vuelo", "vuelos", false),
    ("registration", "inscripción", "inscripciones", true),
    ("service", "servicio", "servicios", false),
    ("ticket", "entrada", "entradas", true),
    ("book", "libro", "libros", false),
    ("key", "llave", "llaves", true),
    ("form", "formulario", "formularios", false),
    ("document", "documento", "documentos", false),
    ("bag", "bolsa", "bolsas", true),
    ("item", "artículo", "artículos", false),
    ("equipment", "equipo", "equipos", false),
    ("application", "solicitud", "solicitudes", true),
    (
        "staff member",
        "miembro del personal",
        "miembros del personal",
        false,
    ),
    ("price", "precio", "precios", false),
    ("total", "total", "totales", false),
    ("fee", "tarifa", "tarifas", true),
    ("weight", "peso", "pesos", false),
    ("distance", "distancia", "distancias", true),
];

const PRONOUNS: [&str; 7] = ["yo", "tú", "nosotros", "ellos", "él", "ella", "ello"];

const UNITS: &[(Unit, &str, &str)] = &[
    (Unit::Yen, "yen", "yenes"),
    (Unit::Dollar, "dólar", "dólares"),
    (Unit::Kg, "kg", "kg"),
    (Unit::Gram, "g", "g"),
    (Unit::Meter, "m", "m"),
    (Unit::Kilometer, "km", "km"),
    (Unit::Percent, "%", "%"),
    (Unit::Person, "persona", "personas"),
];

fn article_word(article: Article, plural: bool, feminine: bool) -> &'static str {
    match (article, plural, feminine) {
        (Article::Definite, false, false) => "el",
        (Article::Definite, false, true) => "la",
        (Article::Definite, true, false) => "los",
        (Article::Definite, true, true) => "las",
        (Article::Indefinite, false, false) => "un",
        (Article::Indefinite, false, true) => "una",
        (Article::Indefinite, true, false) => "unos",
        (Article::Indefinite, true, true) => "unas",
        (Article::Bare, _, _) => "",
    }
}

fn name(text: &str) -> Option<Nominal> {
    let words: Vec<_> = text.split_whitespace().collect();
    (text.len() <= 80
        && (1..=5).contains(&words.len())
        && text.starts_with(char::is_uppercase)
        && words.iter().all(|word| {
            matches!(*word, "de" | "del" | "la" | "las" | "los" | "y")
                || (word.starts_with(char::is_uppercase)
                    && word
                        .chars()
                        .all(|ch| ch.is_alphabetic() || matches!(ch, '-' | '\'' | '’')))
        })
        && !matches!(
            words[0].to_lowercase().as_str(),
            "el" | "la" | "los" | "las" | "un" | "una" | "unos" | "unas" | "si" | "no"
        ))
    .then(|| Nominal::Name(text.into()))
}

fn nominal(text: &str, object: bool) -> Option<(Nominal, bool)> {
    let lower = text.to_lowercase();
    if !object {
        if lower == "usted" {
            return Some((Nominal::Pronoun(YOU), true));
        }
        if let Some(index) = PRONOUNS.iter().position(|word| *word == lower) {
            return Some((Nominal::Pronoun(index as u8), index as u8 != YOU));
        }
    }
    for (key, one, many, feminine) in NOUNS {
        for (word, plural) in [(*one, false), (*many, true)] {
            for article in [Article::Definite, Article::Indefinite, Article::Bare] {
                let term = if article == Article::Bare {
                    if !plural && *key != "equipment" {
                        continue;
                    }
                    lower.as_str()
                } else {
                    let Some(term) = strip_words(&lower, article_word(article, plural, *feminine))
                    else {
                        continue;
                    };
                    term
                };
                if term == word {
                    return Some((
                        Nominal::Noun {
                            key: (*key).into(),
                            plural,
                            article,
                        },
                        true,
                    ));
                }
            }
        }
    }
    if NOUNS
        .iter()
        .any(|(_, one, many, _)| *one == lower || *many == lower)
    {
        return None;
    }
    if timing(text).is_some() {
        return None;
    }
    name(text).map(|name| (name, true))
}

fn render_nominal(nominal: &Nominal, polite: bool) -> String {
    match nominal {
        Nominal::Name(name) => name.clone(),
        Nominal::Pronoun(YOU) if polite => "usted".into(),
        Nominal::Pronoun(index) => PRONOUNS[*index as usize].into(),
        Nominal::Noun {
            key,
            plural,
            article,
        } => {
            let (_, one, many, feminine) = NOUNS
                .iter()
                .find(|(word, _, _, _)| *word == key)
                .expect("Spanish surface for the assigned noun");
            let word = if *plural { many } else { one };
            let article = if *article == Article::Bare && !plural && key != "equipment" {
                Article::Definite
            } else {
                *article
            };
            let prefix = article_word(article, *plural, *feminine);
            if prefix.is_empty() {
                (*word).into()
            } else {
                format!("{prefix} {word}")
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Person {
    First,
    Second,
    Third,
    FirstPlural,
    ThirdPlural,
}

impl Person {
    fn of(subject: &Nominal, polite: bool) -> Self {
        match subject {
            Nominal::Pronoun(I) => Self::First,
            Nominal::Pronoun(YOU) if !polite => Self::Second,
            Nominal::Pronoun(WE) => Self::FirstPlural,
            Nominal::Pronoun(THEY) | Nominal::Noun { plural: true, .. } => Self::ThirdPlural,
            _ => Self::Third,
        }
    }

    fn form(self, forms: [&str; 5]) -> &str {
        forms[self as usize]
    }
}

fn infinitive(verb: Verb) -> &'static str {
    match verb {
        Verb::Start => "empezar",
        Verb::Finish => "terminar",
        Verb::Open => "abrir",
        Verb::Close => "cerrar",
        Verb::Arrive => "llegar",
        Verb::Depart => "salir",
        Verb::Resume => "reanudar",
        Verb::Continue => "continuar",
        Verb::Return => "devolver",
        Verb::Bring => "traer",
        Verb::Accept => "aceptar",
        Verb::Cancel => "cancelar",
        Verb::Postpone => "aplazar",
        Verb::Attend => "asistir",
        Verb::Rain => "llover",
        Verb::Snow => "nevar",
        Verb::Be => "ser",
    }
}

fn present_stem<'a>(lemma: &str, person: Person, stem: &'a str) -> &'a str {
    match (lemma, person) {
        ("salir", Person::First) => "salg",
        ("traer", Person::First) => "traig",
        (_, Person::FirstPlural) => stem,
        ("empezar", _) => "empiez",
        ("cerrar", _) => "cierr",
        ("continuar", _) => "continú",
        ("devolver", _) => "devuelv",
        ("llover", _) => "lluev",
        ("nevar", _) => "niev",
        (_, _) => stem,
    }
}

fn finite(verb: Verb, tense: Tense, person: Person) -> String {
    let lemma = infinitive(verb);
    let (base, ending) = lemma.split_at(lemma.len() - 2);
    match tense {
        Tense::Present if verb == Verb::Be => {
            person.form(["soy", "eres", "es", "somos", "son"]).into()
        }
        Tense::Present => {
            let endings = match ending {
                "ar" => ["o", "as", "a", "amos", "an"],
                "er" => ["o", "es", "e", "emos", "en"],
                _ => ["o", "es", "e", "imos", "en"],
            };
            format!(
                "{}{}",
                present_stem(lemma, person, base),
                person.form(endings)
            )
        }
        Tense::Past if verb == Verb::Be => person
            .form(["fui", "fuiste", "fue", "fuimos", "fueron"])
            .into(),
        Tense::Past if verb == Verb::Bring => person
            .form(["traje", "trajiste", "trajo", "trajimos", "trajeron"])
            .into(),
        Tense::Past => {
            let endings = if ending == "ar" {
                ["é", "aste", "ó", "amos", "aron"]
            } else {
                ["í", "iste", "ió", "imos", "ieron"]
            };
            let stem = if person == Person::First {
                match lemma {
                    "empezar" => "empec",
                    "llegar" => "llegu",
                    "aplazar" => "aplac",
                    _ => base,
                }
            } else {
                base
            };
            format!("{stem}{}", person.form(endings))
        }
        Tense::Future => {
            let stem = if verb == Verb::Depart { "saldr" } else { lemma };
            format!("{stem}{}", person.form(["é", "ás", "á", "emos", "án"]))
        }
        Tense::Ability => {
            let auxiliary = person.form(["puedo", "puedes", "puede", "podemos", "pueden"]);
            format!("{auxiliary} {lemma}")
        }
    }
}

fn predicate(verb: Verb, tense: Tense, person: Person) -> String {
    let conjugation = finite(verb, tense, person);
    if verb == Verb::Resume {
        let reflexive = person.form(["me", "te", "se", "nos", "se"]);
        format!("{reflexive} {conjugation}")
    } else {
        conjugation
    }
}

fn timing(text: &str) -> Option<Vec<Timing>> {
    parse_timing(&text.to_lowercase(), TranslationLanguage::Spanish)
}

fn decimal(text: &str) -> Option<String> {
    let value = text.trim();
    if value.len() > 28 {
        return None;
    }
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|digit| digit.is_ascii_digit());
    let valid = unsigned.split_once(',').map_or_else(
        || digits(unsigned),
        |(whole, fractional)| digits(whole) && digits(fractional),
    );
    valid.then(|| value.replace(',', "."))
}

fn amount(text: &str) -> Option<Amount> {
    let lower = text.to_lowercase();
    UNITS.iter().find_map(|(unit, one, many)| {
        [*one, *many].iter().find_map(|label| {
            let raw = lower.strip_suffix(*label)?;
            // Symbols can touch the number. Word units are independent tokens.
            if !matches!(
                *unit,
                Unit::Percent | Unit::Kg | Unit::Gram | Unit::Meter | Unit::Kilometer
            ) && !raw.ends_with(' ')
            {
                return None;
            }
            let value = decimal(raw)?;
            let expected_label = if value == "1" { *one } else { *many };
            (*label == expected_label).then_some(Amount { value, unit: *unit })
        })
    })
}

fn render_amount(amount: &Amount) -> String {
    let (_, one, many) = UNITS
        .iter()
        .find(|(unit, _, _)| *unit == amount.unit)
        .expect("Spanish label for assigned unit");
    let value = amount.value.replace(',', "").replace('.', ",");
    let unit = if value == "1" { one } else { many };
    format!("{value} {unit}")
}

fn object_and_time(text: &str, verb: Verb) -> Option<(Nominal, Vec<Timing>)> {
    let contracted;
    let text = if verb == Verb::Attend {
        if let Some(tail) = strip_words(&text.to_lowercase(), "al") {
            let tail_start = text.len().checked_sub(tail.len())?;
            contracted = format!("el {}", text.get(tail_start..)?);
            contracted.as_str()
        } else {
            let (prep, tail) = text.split_once(' ')?;
            if !prep.eq_ignore_ascii_case("a") {
                return None;
            }
            tail.trim_start()
        }
    } else {
        text
    };
    for end in text
        .match_indices(' ')
        .map(|(at, _)| at)
        .chain([text.len()])
    {
        if let Some((object, _)) = nominal(&text[..end], true)
            && let Some(when) = timing(&text[end..])
        {
            return Some((object, when));
        }
    }
    None
}

fn split_form<'a>(text: &'a str, form: &str) -> Option<&'a str> {
    text.get(..form.len())
        .filter(|prefix| prefix.to_lowercase() == form)?;
    let tail = &text[form.len()..];
    (tail.is_empty() || tail.starts_with(' ')).then(|| tail.trim_start())
}

fn tense_matches_time(tense: Tense, timing: &[Timing]) -> bool {
    timing.iter().all(|part| match part {
        Timing::Relative(2) => tense == Tense::Past,
        Timing::Relative(1 | 3) => tense != Tense::Past,
        _ => true,
    })
}

struct Complements {
    object: Option<Nominal>,
    timing: Vec<Timing>,
    amount: Option<Amount>,
}

fn complements(text: &str, verb: Verb, needs_object: bool) -> Option<Complements> {
    if needs_object {
        let (object, timing) = object_and_time(text, verb)?;
        return Some(Complements {
            object: Some(object),
            timing,
            amount: None,
        });
    }
    if verb == Verb::Be {
        let value = split_form(text, "de").unwrap_or(text);
        if let Some(amount) = amount(value) {
            return Some(Complements {
                object: None,
                timing: Vec::new(),
                amount: Some(amount),
            });
        }
    }
    let timing = timing(text)?;
    (verb != Verb::Be || !timing.is_empty()).then_some(Complements {
        object: None,
        timing,
        amount: None,
    })
}

fn parse_predicate(
    text: &str,
    subject: &Nominal,
    polite: bool,
    verb: Verb,
    needs_object: bool,
) -> Option<Clause> {
    let person = Person::of(subject, polite);
    let (negative, text) = split_form(text, "no").map_or((false, text), |rest| (true, rest));
    let mut matched = None;
    for tense in [Tense::Present, Tense::Past, Tense::Future, Tense::Ability] {
        if tense == Tense::Ability && matches!(verb, Verb::Be | Verb::Rain | Verb::Snow) {
            continue;
        }
        let form = predicate(verb, tense, person);
        let Some(parts) =
            split_form(text, &form).and_then(|rest| complements(rest, verb, needs_object))
        else {
            continue;
        };
        if !tense_matches_time(tense, &parts.timing) {
            continue;
        }
        if matched.is_some() {
            // Some first-person plural forms are identical in the present and preterite.
            return None;
        }
        matched = Some(Clause {
            subject: subject.clone(),
            verb,
            object: parts.object,
            timing: parts.timing,
            amount: parts.amount,
            tense,
            negative,
            polite,
        });
    }
    matched
}

pub(super) fn clause(text: &str) -> Option<Clause> {
    for verb in [Verb::Rain, Verb::Snow] {
        if let Some(clause) = parse_predicate(text, &Nominal::Pronoun(IT), true, verb, false) {
            return Some(clause);
        }
    }
    for (at, _) in text.match_indices(' ') {
        let Some((subject, polite)) = nominal(&text[..at], false) else {
            continue;
        };
        let rest = text[at + 1..].trim_start();
        for (verb, needs_object) in VERBS
            .iter()
            .map(|entry| (entry.verb, entry.object))
            .chain([(Verb::Be, false)])
        {
            if let Some(clause) = parse_predicate(rest, &subject, polite, verb, needs_object) {
                return Some(clause);
            }
        }
    }
    None
}

fn render_clause(clause: &Clause, register: TranslationRegister, condition: bool) -> String {
    let polite = match register {
        TranslationRegister::Preserve => clause.polite,
        TranslationRegister::Plain => false,
        TranslationRegister::Polite => true,
    };
    let mut words = Vec::new();
    let weather = matches!(clause.verb, Verb::Rain | Verb::Snow);
    if !weather {
        words.push(render_nominal(&clause.subject, polite));
    }
    if clause.negative {
        words.push("no".into());
    }
    let tense = if weather
        && clause.tense == Tense::Present
        && !condition
        && clause
            .timing
            .iter()
            .any(|when| matches!(when, Timing::Relative(1 | 3)))
    {
        Tense::Future
    } else {
        clause.tense
    };
    words.push(predicate(
        clause.verb,
        tense,
        Person::of(&clause.subject, polite),
    ));
    if let Some(object) = &clause.object {
        let phrase = render_nominal(object, polite);
        if clause.verb == Verb::Attend {
            words.push(match phrase.strip_prefix("el ") {
                Some(tail) => format!("al {tail}"),
                None => format!("a {phrase}"),
            });
        } else {
            words.push(phrase);
        }
    }
    if let Some(amount) = &clause.amount {
        words.push("de".into());
        words.push(render_amount(amount));
    }
    words.extend(clause.timing.iter().map(Timing::spanish));
    words.join(" ")
}

pub(super) fn supported(sentence: &Sentence) -> bool {
    let mut clauses = std::iter::once(&sentence.main)
        .chain(sentence.joined.as_ref().map(|(_, clause)| clause))
        .chain(
            sentence
                .condition
                .as_ref()
                .and_then(|condition| match condition {
                    Condition::Clause(clause) => Some(clause),
                    Condition::Weather(_) => None,
                }),
        );
    clauses.all(|clause| {
        !matches!(clause.object, Some(Nominal::Pronoun(_)))
            && !(clause.verb == Verb::Be && clause.tense == Tense::Ability)
            && tense_matches_time(clause.tense, &clause.timing)
    }) && !matches!(
        sentence.condition,
        Some(Condition::Clause(Clause {
            tense: Tense::Future,
            ..
        }))
    )
}

fn weather_condition(weather: Weather) -> &'static str {
    match (weather.snow, weather.negative) {
        (false, false) => "si llueve, ",
        (false, true) => "si no llueve, ",
        (true, false) => "si nieva, ",
        (true, true) => "si no nieva, ",
    }
}

pub(super) fn sentence(sentence: &Sentence, register: TranslationRegister) -> String {
    let mut text = match &sentence.condition {
        Some(Condition::Weather(weather)) => weather_condition(*weather).into(),
        Some(Condition::Clause(clause)) => {
            format!("si {}, ", render_clause(clause, register, true))
        }
        None => String::new(),
    };
    text.push_str(&render_clause(&sentence.main, register, false));
    if let Some((connector, clause)) = &sentence.joined {
        let tail = render_clause(clause, register, false);
        let lower = tail.to_lowercase();
        let join = match connector {
            Connector::Contrast => ", pero ",
            Connector::Addition
                if lower.starts_with(['i', 'í'])
                    || ((lower.starts_with("hi") || lower.starts_with("hí"))
                        && !lower.starts_with("hie")) =>
            {
                " e "
            }
            Connector::Addition => " y ",
        };
        text.push_str(join);
        text.push_str(&tail);
    }
    text.push('.');
    let mut chars = text.chars();
    let initial = chars.next().expect("translated Spanish sentence");
    format!("{}{}", initial.to_uppercase(), chars.as_str())
}
