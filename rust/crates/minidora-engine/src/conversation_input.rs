//! Finite conversation event reports. Spans always point into the submitted turn.

use minidora_domain::{ConversationEvent, ConversationEventKind, Intent, Proposition, SourceSpan};
use regex::Regex;
use std::sync::LazyLock;

use ConversationEventKind::{Activity, Difficulty, Discovery, Feeling, Movement, Observation};

const SALUTATIONS: &[&str] = &["おはようございます", "こんにちは", "こんばんは", "おはよう"];
const POLITE_PAST_ENDINGS: &[&str] = &["ませんでした", "ました"];

#[derive(Clone, Copy)]
enum Inflection {
    Godan,
    Ichidan,
    Suru,
    Kuru,
}

use Inflection::{Godan, Ichidan, Kuru, Suru};

struct Predicate {
    surface: String,
    kind: ConversationEventKind,
    negative: bool,
}

static REPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\p{Han}\p{Hiragana}\p{Katakana}\p{Latin}\p{Nd}・ー、，, 　]+$")
        .expect("event report character classes")
});

static STEM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^[\p{Han}\p{Hiragana}\p{Katakana}ー]{1,24}$")
        .expect("Japanese verb stem character classes")
});

static PREDICATES: LazyLock<Vec<Predicate>> = LazyLock::new(|| {
    let verbs: &[(ConversationEventKind, Inflection, &[&str])] = &[
        (
            Observation,
            Ichidan,
            &[
                "見る",
                "見える",
                "見かける",
                "見掛ける",
                "聞こえる",
                "眺める",
            ],
        ),
        (Observation, Godan, &["聞く", "聴く", "触る"]),
        (Observation, Suru, &["観察する"]),
        (Discovery, Ichidan, &["見つける", "見付ける"]),
        (
            Discovery,
            Godan,
            &["気づく", "気付く", "見つかる", "知る", "分かる", "わかる"],
        ),
        (Discovery, Suru, &["発見する"]),
        (
            Movement,
            Godan,
            &[
                "歩く", "行く", "いく", "帰る", "走る", "泳ぐ", "渡る", "登る", "乗る", "通る",
                "着く", "寄る", "戻る", "飛ぶ",
            ],
        ),
        (Movement, Ichidan, &["降りる", "出る", "出かける"]),
        (
            Movement,
            Suru,
            &[
                "移動する",
                "出発する",
                "到着する",
                "遠回りする",
                "散歩する",
                "帰宅する",
            ],
        ),
        (Movement, Kuru, &["来る", "くる"]),
        (Difficulty, Godan, &["困る", "迷う", "つまずく", "躓く"]),
        (Difficulty, Ichidan, &["遅れる", "間違える"]),
        (Difficulty, Suru, &["苦労する", "失敗する"]),
        (
            Feeling,
            Godan,
            &["喜ぶ", "笑う", "驚く", "楽しむ", "気に入る"],
        ),
        (Feeling, Ichidan, &["疲れる", "飽きる", "感じる"]),
        (Feeling, Suru, &["安心する", "感動する", "緊張する"]),
        (
            Activity,
            Godan,
            &[
                "拾う",
                "見守る",
                "作る",
                "つくる",
                "読む",
                "書く",
                "描く",
                "編む",
                "焼く",
                "磨く",
                "咲く",
                "開く",
                "閉まる",
                "休む",
                "直す",
                "待つ",
                "遊ぶ",
                "続く",
                "折る",
                "飲む",
                "始まる",
                "動く",
                "撮る",
                "まく",
                "飛ばす",
            ],
        ),
        (
            Activity,
            Ichidan,
            &[
                "調べる",
                "食べる",
                "起きる",
                "寝る",
                "片付ける",
                "詰める",
                "受ける",
                "始める",
                "終える",
                "集める",
                "比べる",
                "組み立てる",
                "覚える",
            ],
        ),
        (
            Activity,
            Suru,
            &[
                "用意する",
                "掃除する",
                "整理する",
                "準備する",
                "修理する",
                "観戦する",
                "練習する",
                "撮影する",
                "連絡する",
                "予約する",
                "完成する",
                "工夫する",
            ],
        ),
    ];
    let mut forms = Vec::new();
    for &(kind, class, lemmas) in verbs {
        for lemma in lemmas {
            let (polite, negative, past) = verb_stems(lemma, class);
            let (past_stem, connective) = if let Some(stem) = past.strip_suffix('だ') {
                (stem, 'で')
            } else {
                (past.strip_suffix('た').expect("verb past ending"), 'て')
            };
            for (surface, negative) in [
                (format!("{polite}ました"), false),
                (format!("{polite}ませんでした"), true),
                (past.clone(), false),
                (format!("{negative}なかった"), true),
                (format!("{past_stem}{connective}いました"), false),
                (format!("{past_stem}{connective}いませんでした"), true),
                (format!("{past_stem}{connective}いた"), false),
                (format!("{past_stem}{connective}いなかった"), true),
            ] {
                forms.push(Predicate {
                    surface,
                    kind,
                    negative,
                });
            }
        }
    }
    for (kind, stems) in [
        (
            Feeling,
            &[
                "楽し",
                "嬉し",
                "うれし",
                "悲し",
                "面白",
                "おもしろ",
                "怖",
                "寂し",
                "さびし",
                "寒",
                "暑",
            ][..],
        ),
        (Difficulty, &["難し", "むずかし", "辛", "つら", "苦し"][..]),
    ] {
        for stem in stems {
            for (ending, negative) in [
                ("かった", false),
                ("かったです", false),
                ("くなかった", true),
                ("くなかったです", true),
            ] {
                forms.push(Predicate {
                    surface: format!("{stem}{ending}"),
                    kind,
                    negative,
                });
            }
        }
    }
    // Prefer the full lexical predicate of compound verbs (e.g. 見つける).
    forms.sort_by_key(|form| std::cmp::Reverse(form.surface.len()));
    forms
});

fn verb_stems(lemma: &str, class: Inflection) -> (String, String, String) {
    match class {
        Ichidan => {
            let stem = lemma.strip_suffix('る').expect("ichidan ending");
            (stem.into(), stem.into(), format!("{stem}た"))
        }
        Suru => {
            let stem = lemma.strip_suffix("する").expect("suru ending");
            (
                format!("{stem}し"),
                format!("{stem}し"),
                format!("{stem}した"),
            )
        }
        Kuru => match lemma {
            "来る" => ("来".into(), "来".into(), "来た".into()),
            "くる" => ("き".into(), "こ".into(), "きた".into()),
            _ => unreachable!("supported kuru forms"),
        },
        Godan => {
            let last = lemma.chars().next_back().expect("godan ending");
            let stem = &lemma[..lemma.len() - last.len_utf8()];
            let (polite, negative, mut past) = match last {
                'う' => ("い", "わ", "った"),
                'く' => ("き", "か", "いた"),
                'ぐ' => ("ぎ", "が", "いだ"),
                'す' => ("し", "さ", "した"),
                'つ' => ("ち", "た", "った"),
                'ぬ' => ("に", "な", "んだ"),
                'ぶ' => ("び", "ば", "んだ"),
                'む' => ("み", "ま", "んだ"),
                'る' => ("り", "ら", "った"),
                _ => unreachable!("supported godan ending"),
            };
            if matches!(lemma, "行く" | "いく") {
                past = "った";
            }
            (
                format!("{stem}{polite}"),
                format!("{stem}{negative}"),
                format!("{stem}{past}"),
            )
        }
    }
}

fn source_span(text: &str, start: usize, part: &str) -> SourceSpan {
    let end = start + part.len();
    debug_assert_eq!(text.get(start..end), Some(part));
    SourceSpan {
        start,
        end,
        text: part.into(),
    }
}

fn finite_predicate(report: &str) -> Option<(&str, ConversationEventKind, bool)> {
    if let Some(form) = PREDICATES
        .iter()
        .find(|form| report.ends_with(&form.surface))
    {
        return Some((
            &report[report.len() - form.surface.len()..],
            form.kind,
            form.negative,
        ));
    }
    // An unfamiliar polite verb can be understood as an activity when a
    // separate argument marks its boundary and its stem has a continuative form.
    for suffix in POLITE_PAST_ENDINGS {
        let Some(body) = report.strip_suffix(suffix) else {
            continue;
        };
        let stem = ['を', 'に', 'へ', 'で', 'は', 'が']
            .into_iter()
            .filter_map(|particle| body.rsplit_once(particle))
            .filter(|(argument, _)| !argument.trim().is_empty())
            .map(|(_, stem)| stem)
            .find(|stem| {
                stem.chars().count() >= 2
                    && stem.chars().last().is_some_and(is_continuative)
                    && STEM.is_match(stem)
            });
        if let Some(stem) = stem {
            let start = report.len() - suffix.len() - stem.len();
            return Some((&report[start..], Activity, *suffix == "ませんでした"));
        }
    }
    None
}

fn target_before_predicate(head: &str) -> Option<(usize, &str)> {
    let clause = head.rsplit(['、', '，', ',']).next().unwrap_or(head).trim();
    let trailing = head.len() - head.trim_end().len();
    let offset = head.len() - trailing - clause.len();
    let argument = ["まで", "から", "を", "へ", "に", "で", "が", "は", "と"]
        .into_iter()
        .find_map(|particle| clause.strip_suffix(particle))
        .or_else(|| {
            // A short adverb may follow a named actor in an observed event.
            let (subject, modifier) = clause.split_once(['は', 'が'])?;
            (modifier.chars().count() <= 8 && modifier.ends_with('く')).then_some(subject)
        })?;
    // Do not split inside kana nouns such as はがき. A new argument after
    // an earlier particle must begin with a visibly distinct token.
    let value = argument
        .match_indices(['は', 'が', 'で', 'に', 'を'])
        .filter(|(index, _)| *index != 0)
        .map(|(index, particle)| argument[index + particle.len()..].trim())
        .filter(|tail| {
            tail.chars()
                .next()
                .is_some_and(|first| !is_hiragana(first) && first.is_alphanumeric())
        })
        .last()
        .unwrap_or(argument)
        .trim();
    if value.is_empty() || value.chars().count() > 32 {
        return None;
    }
    Some((offset + argument.rfind(value)?, value))
}

fn is_hiragana(ch: char) -> bool {
    ('ぁ'..='ゖ').contains(&ch)
}

fn admits_leading_clause(clause: &str) -> bool {
    let clause = clause.trim();
    // A temporal subordinate clause can contain its own past verb and arguments.
    let prior_event = ["ときに", "時に", "とき", "時"]
        .into_iter()
        .find_map(|ending| clause.strip_suffix(ending))
        .or_else(|| {
            clause
                .strip_suffix('ら')
                .filter(|event| event.ends_with(['た', 'だ']))
        });
    if let Some(event) = prior_event {
        return finite_predicate(event).is_some();
    }
    if let Some(stem) = clause.strip_suffix("ながら") {
        return finite_predicate(&format!("{stem}ました")).is_some();
    }
    // The remaining lead must be a frame such as 今朝 or 交差点で, without
    // a separate action object or a terminal imperative or hypothetical.
    !clause.is_empty()
        && !clause.contains('を')
        && ![
            "なら",
            "ならば",
            "れば",
            "ないで",
            "するな",
            "しろ",
            "せよ",
            "来い",
            "こい",
        ]
        .iter()
        .any(|ending| clause.ends_with(ending))
        && (!clause.ends_with(['け', 'げ', 'せ', 'て', 'ね', 'べ', 'め', 'れ', 'ろ'])
            || ["ごろ", "ところ", "ていて", "でいて"]
                .iter()
                .any(|ending| clause.ends_with(ending)))
}

pub(crate) fn parse(text: &str) -> Option<ConversationEvent> {
    if text.chars().any(char::is_control) {
        return None;
    }
    let sentence = text.trim().trim_end_matches(['。', '！', '!']).trim_end();
    let start = text.len() - text.trim_start().len();
    let greeting_and_body = SALUTATIONS.iter().find_map(|greeting| {
        let body = sentence
            .strip_prefix(greeting)?
            .strip_prefix(['。', '！', '!', '、', '，', ','])?;
        Some((*greeting, body.trim_start()))
    });
    let (greeting, report) = match greeting_and_body {
        Some((greeting, report)) => (Some(source_span(text, start, greeting)), report),
        None => (None, sentence),
    };
    if report.chars().count() > 160
        || !REPORT.is_match(report)
        || ["もし", "仮に", "訂正", "正しくは", "前の回答"]
            .iter()
            .any(|prefix| report.starts_with(prefix))
        || [
            "してください",
            "してくれ",
            "しなさい",
            "実行せよ",
            "覚えて、",
            "記憶して、",
        ]
        .iter()
        .any(|marker| report.contains(marker))
    {
        return None;
    }
    if let Some((lead, _)) = report.rsplit_once(['、', '，', ','])
        && !lead.split(['、', '，', ',']).all(admits_leading_clause)
    {
        return None;
    }
    let statement = ["よね", "よ", "ね"]
        .into_iter()
        .find_map(|ending| report.strip_suffix(ending))
        .unwrap_or(report);
    let (predicate, kind, negative) = finite_predicate(statement)?;
    let report_start = start + sentence.len() - report.len();
    let head = &statement[..statement.len() - predicate.len()];
    let target = target_before_predicate(head)
        .map(|(offset, target)| source_span(text, report_start + offset, target));
    Some(ConversationEvent {
        report: source_span(text, report_start, report),
        predicate: source_span(text, report_start + head.len(), predicate),
        target,
        greeting,
        kind,
        negative,
    })
}

const ENGLISH_TOPIC_SWITCH: &str = r"by +the +way|on +another +note|changing +the +subject";
const ENGLISH_TIME: &str = concat!(
    r"yesterday(?: +(?:morning|afternoon|evening))?|earlier +today|",
    r"(?:last|this) +(?:morning|afternoon|evening|night|week|weekend|month|year|",
    r"monday|tuesday|wednesday|thursday|friday|saturday|sunday)|",
    r"on +(?:monday|tuesday|wednesday|thursday|friday|saturday|sunday)|",
    r"(?:[0-9]{1,3}|one|two|three|four|five|six|seven|eight|nine|ten|a) +",
    r"(?:minutes?|hours?|days?|weeks?|months?|years?) +ago",
);

static ENGLISH_FRAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        concat!(
            r"(?i)\A(?:(?P<greeting>good +(?:morning|afternoon|evening)|hello|hi|hey)[,.!] +)?",
            r"(?P<report>(?:(?:{topic_switch}), +)?(?:(?:{time}),? +)?(?:i|we) +(?P<clause>.+))\z",
        ),
        topic_switch = ENGLISH_TOPIC_SWITCH,
        time = ENGLISH_TIME,
    ))
    .expect("English event frame")
});

static ENGLISH_SWITCH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)\A(?:{ENGLISH_TOPIC_SWITCH}), +")).expect("English topic switch")
});

static ENGLISH_TRAILING_TIME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?i)(?:\A| +)(?:{ENGLISH_TIME})\z")).expect("English event time")
});

static ENGLISH_NEGATIVE_AUXILIARY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\A(?:did +not|didn['’]t|could +not|couldn['’]t) +")
        .expect("English negative event")
});

static ENGLISH_NOMINAL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\A[\p{L}\p{Nd}][\p{L}\p{M}\p{Nd}]*(?:(?:['’\-]| +)[\p{L}\p{Nd}][\p{L}\p{M}\p{Nd}]*)*\z",
    )
    .expect("English event nominal")
});

const ENGLISH_PREDICATES: &[(ConversationEventKind, &[(&str, &str)])] = &[
    (
        Observation,
        &[
            ("see", "saw"),
            ("spot", "spotted"),
            ("watch", "watched"),
            ("hear", "heard"),
            ("listen", "listened"),
            ("observe", "observed"),
            ("look", "looked"),
        ],
    ),
    (
        Discovery,
        &[
            ("discover", "discovered"),
            ("find", "found"),
            ("realize", "realized"),
            ("realise", "realised"),
            ("recognize", "recognized"),
            ("recognise", "recognised"),
            ("learn", "learned"),
            ("learn", "learnt"),
            ("identify", "identified"),
            ("notice", "noticed"),
        ],
    ),
    (
        Activity,
        &[
            ("do", "did"),
            ("make", "made"),
            ("finish", "finished"),
            ("complete", "completed"),
            ("start", "started"),
            ("try", "tried"),
            ("build", "built"),
            ("write", "wrote"),
            ("draw", "drew"),
            ("paint", "painted"),
            ("carve", "carved"),
            ("cook", "cooked"),
            ("bake", "baked"),
            ("work", "worked"),
            ("practice", "practiced"),
            ("practise", "practised"),
            ("prepare", "prepared"),
            ("play", "played"),
            ("repair", "repaired"),
            ("plant", "planted"),
        ],
    ),
    (
        Movement,
        &[
            ("go", "went"),
            ("walk", "walked"),
            ("visit", "visited"),
            ("arrive", "arrived"),
            ("leave", "left"),
            ("return", "returned"),
            ("come", "came"),
            ("move", "moved"),
            ("drive", "drove"),
            ("travel", "traveled"),
            ("travel", "travelled"),
            ("run", "ran"),
            ("cycle", "cycled"),
            ("reach", "reached"),
            ("hike", "hiked"),
        ],
    ),
    (
        Difficulty,
        &[
            ("struggle", "struggled"),
            ("have trouble", "had trouble"),
            ("have difficulty", "had difficulty"),
            ("have a hard time", "had a hard time"),
        ],
    ),
    (
        Feeling,
        &[
            ("feel", "felt"),
            ("enjoy", "enjoyed"),
            ("like", "liked"),
            ("love", "loved"),
            ("miss", "missed"),
            ("worry", "worried"),
            ("have fun", "had fun"),
        ],
    ),
];

fn english_word_prefix<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    let rest = &text[prefix.len()..];
    (head.eq_ignore_ascii_case(prefix) && (rest.is_empty() || rest.starts_with(' ')))
        .then_some(rest)
}

fn english_predicate(clause: &str) -> Option<(usize, ConversationEventKind, bool)> {
    let auxiliary = ENGLISH_NEGATIVE_AUXILIARY.find(clause);
    let never = english_word_prefix(clause, "never");
    let verb = auxiliary.as_ref().map_or_else(
        || never.unwrap_or(clause).trim_start(),
        |boundary| &clause[boundary.end()..],
    );
    let (form, kind) = ENGLISH_PREDICATES
        .iter()
        .flat_map(|(kind, forms)| forms.iter().map(move |(base, past)| (kind, base, past)))
        .filter_map(|(kind, base, past)| {
            let form = if auxiliary.is_some() { base } else { past };
            english_word_prefix(verb, form).map(|_| (*form, *kind))
        })
        .max_by_key(|(form, _)| form.len())?;
    Some((
        clause.len() - verb.len() + form.len(),
        kind,
        auxiliary.is_some() || never.is_some(),
    ))
}

const ENGLISH_PREPOSITIONS: &[&str] = &[
    "to", "with", "at", "in", "from", "on", "about", "near", "around", "through", "into", "for",
    "along", "across", "past", "beside", "by", "under", "over", "outside", "inside", "during",
];
const ENGLISH_CLAUSE_WORDS: &[&str] = &[
    "and",
    "or",
    "but",
    "then",
    "if",
    "unless",
    "because",
    "though",
    "although",
    "where",
    "when",
    "while",
    "whether",
    "which",
    "who",
    "whom",
    "whose",
    "however",
    "so",
    "please",
    "can",
    "could",
    "would",
    "should",
    "will",
    "must",
    "may",
    "might",
    "do",
    "does",
    "did",
    "not",
    "never",
    "am",
    "is",
    "are",
    "was",
    "were",
    "be",
    "been",
    "being",
    "has",
    "have",
    "had",
    "say",
    "said",
    "tell",
    "told",
    "stop",
    "stopped",
    "change",
    "changed",
    "open",
    "opened",
    "close",
    "closed",
    "fall",
    "fell",
    "glow",
    "glowed",
    "melt",
    "melted",
    "bloom",
    "bloomed",
    "sing",
    "sang",
    "laugh",
    "laughed",
    "take",
    "took",
    "talk",
    "talked",
    "send",
    "sent",
    "delete",
    "deleted",
    "save",
    "saved",
    "execute",
    "publish",
    "download",
    "upload",
    "remove",
    "replace",
    "ignore",
    "assuming",
    "suppose",
    "supposing",
    "tomorrow",
    "singing",
    "bend",
    "bent",
];
const ENGLISH_DETERMINERS: &[&str] = &[
    "a", "an", "the", "this", "that", "these", "those", "my", "our", "your", "their", "his", "her",
    "its", "some",
];

fn english_target(tail: &str) -> Option<Option<(usize, &str)>> {
    let tail = ENGLISH_TRAILING_TIME
        .find(tail)
        .map_or(tail, |time| tail[..time.start()].trim_end());
    if tail.is_empty() {
        return Some(None);
    }
    if !ENGLISH_NOMINAL.is_match(tail) {
        return None;
    }
    let lower = tail.to_ascii_lowercase();
    let words = lower
        .split(' ')
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let start = usize::from(ENGLISH_PREPOSITIONS.contains(&words[0]));
    let target = *words.get(start)?;
    for (index, word) in words.iter().copied().enumerate().skip(start) {
        if ENGLISH_CLAUSE_WORDS.contains(&word)
            || word == "to"
            || (word == "that" && index != start)
            || word.ends_with("n't")
            || word.ends_with("n’t")
            || ["i", "we", "he", "she", "they"].contains(&word)
            || (["you", "it", "him", "them", "us", "me"].contains(&word)
                && (index != start || words.len() != start + 1))
        {
            return None;
        }
        if let Some(&(base, past)) = ENGLISH_PREDICATES
            .iter()
            .flat_map(|(_, forms)| forms.iter())
            .find(|(base, past)| word == *base || word == *past)
        {
            // Directly introduced modifiers and nouns can have the same form
            // as a verb, as in `the finished bowl` or `a visit`.
            let nominal = index == start + 1
                && ENGLISH_DETERMINERS.contains(&target)
                && ((word == past && index + 1 < words.len())
                    || (word == base && index + 1 == words.len()));
            if !nominal {
                return None;
            }
        }
    }
    let nominal = if start == 0 {
        tail
    } else {
        tail[words[0].len()..].trim_start()
    };
    let end = nominal.match_indices(' ').find_map(|(offset, _)| {
        let next = nominal[offset..].trim_start().split(' ').next()?;
        ENGLISH_PREPOSITIONS
            .iter()
            .any(|preposition| next.eq_ignore_ascii_case(preposition))
            .then_some(offset)
    });
    Some(Some((
        tail.len() - nominal.len(),
        end.map_or(nominal, |end| &nominal[..end]),
    )))
}

pub(crate) fn parse_english(text: &str) -> Option<ConversationEvent> {
    if text.len() > 640 || text.chars().any(char::is_control) {
        return None;
    }
    let sentence = text.trim();
    let sentence = sentence.strip_suffix(['.', '!']).unwrap_or(sentence);
    let fields = ENGLISH_FRAME.captures(sentence)?;
    let report = fields.name("report")?;
    let clause = fields.name("clause")?;
    let (end, kind, negative) = english_predicate(clause.as_str())?;
    let tail = clause.as_str()[end..].trim_start();
    let target = english_target(tail)?;
    let start = text.len() - text.trim_start().len();
    let clause_start = start + clause.start();
    let tail_start = clause_start + clause.as_str().len() - tail.len();
    Some(ConversationEvent {
        report: source_span(text, start + report.start(), report.as_str()),
        predicate: source_span(text, clause_start, &clause.as_str()[..end]),
        target: target.map(|(offset, part)| source_span(text, tail_start + offset, part)),
        greeting: fields
            .name("greeting")
            .map(|value| source_span(text, start + value.start(), value.as_str())),
        kind,
        negative,
    })
}

fn is_event_in_copular_slot(proposition: &Proposition) -> bool {
    proposition.predicate == "is"
        && !proposition.negative
        && (finite_predicate(&proposition.object).is_some()
            || ["ませんでした", "ました", "でした", "だった"]
                .iter()
                .any(|ending| proposition.object.ends_with(ending))
            || ends_in_polite_nonpast(&proposition.object))
}

fn ends_in_polite_nonpast(text: &str) -> bool {
    let Some(stem) = text
        .strip_suffix("ます")
        .or_else(|| text.strip_suffix("ません"))
    else {
        return false;
    };
    PREDICATES
        .iter()
        .any(|form| form.surface.strip_suffix("ました") == Some(stem))
        || stem.chars().count() >= 2
            && stem.chars().any(|ch| ('一'..='龯').contains(&ch))
            && stem.chars().last().is_some_and(is_continuative)
}

fn is_continuative(ch: char) -> bool {
    "いえきぎけげしじせぜちてでにねひびぴへべぺみめりれ".contains(ch)
}

pub(crate) fn changes_topic_in_english(event: &ConversationEvent) -> bool {
    ENGLISH_SWITCH.is_match(&event.report.text)
}

pub(crate) fn can_continue_topic(event: &ConversationEvent) -> bool {
    event.greeting.is_none()
        && !event.report.text.starts_with("ところで")
        && !(event
            .predicate
            .text
            .starts_with(|ch: char| ch.is_ascii_alphabetic())
            && changes_topic_in_english(event))
}

pub(crate) fn admits_event(intent: &Intent, text: &str, event: &ConversationEvent) -> bool {
    match intent {
        Intent::Unknown { .. } => true,
        Intent::Assert { proposition } | Intent::AssertPreviousRelation { proposition, .. } => {
            proposition.predicate == "is"
                && !proposition.negative
                && proposition.temporal_scope.is_none()
                && proposition.condition_refs.is_empty()
                && (pseudo_copula(intent, text)
                    || finite_predicate(&event.predicate.text).is_some())
        }
        _ => false,
    }
}

/// The permissive legacy proposition grammar historically read a polite
/// event predicate as a copular attribute. This guard applies to fresh turns.
pub(crate) fn pseudo_copula(intent: &Intent, text: &str) -> bool {
    let proposition = match intent {
        Intent::Assert { proposition }
        | Intent::AssertPreviousRelation { proposition, .. }
        | Intent::Ask { proposition }
        | Intent::AskPreviousRelation { proposition, .. } => proposition,
        _ => return false,
    };
    if proposition.predicate != "is" || proposition.negative {
        return false;
    }
    let terminal = text
        .trim()
        .trim_end_matches(['。', '？', '?', '！', '!'])
        .trim_end();
    let terminal = terminal.strip_suffix('か').unwrap_or(terminal);
    finite_predicate(terminal).is_some()
        || is_event_in_copular_slot(proposition)
            && !["である", "です", "だ"]
                .iter()
                .any(|ending| terminal.ends_with(ending))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_events_keep_original_anchors_and_distinct_meanings() {
        let source = "  Hi! Yesterday I spotted a kestrel at the river.  ";
        let event = parse_english(source).unwrap();
        assert_eq!(
            event.report.text,
            "Yesterday I spotted a kestrel at the river"
        );
        assert_eq!(event.kind, Observation);
        assert!(!event.negative);
        assert_eq!(event.greeting.as_ref().unwrap().text, "Hi");
        assert!(!can_continue_topic(&event));
        let movement = parse_english("I went.").unwrap();
        assert_eq!(movement.kind, Movement);
        assert_eq!(movement.predicate.text, "went");
        assert!(movement.target.is_none());

        for (source, predicate, target, kind, negative) in [
            (source, "spotted", "a kestrel", Observation, false),
            (
                "I noticed an unfamiliar sign last night.",
                "noticed",
                "an unfamiliar sign",
                Discovery,
                false,
            ),
            (
                "I did not finish the clay bowl.",
                "did not finish",
                "the clay bowl",
                Activity,
                true,
            ),
            (
                "I couldn't repair the loose hinge.",
                "couldn't repair",
                "the loose hinge",
                Activity,
                true,
            ),
            (
                "We went to a lantern exhibition.",
                "went",
                "a lantern exhibition",
                Movement,
                false,
            ),
            (
                "I struggled with the new bus route.",
                "struggled",
                "the new bus route",
                Difficulty,
                false,
            ),
            (
                "On Friday, I felt proud of my model.",
                "felt",
                "proud of my model",
                Feeling,
                false,
            ),
            (
                "I didn’t see the 桜 display.",
                "didn’t see",
                "the 桜 display",
                Observation,
                true,
            ),
        ] {
            let event = parse_english(source).unwrap_or_else(|| panic!("not parsed: {source}"));
            assert_eq!(event.predicate.text, predicate);
            assert_eq!(event.target.as_ref().unwrap().text, target);
            assert_eq!(event.kind, kind);
            assert_eq!(event.negative, negative);
            for span in [
                Some(&event.report),
                Some(&event.predicate),
                event.target.as_ref(),
                event.greeting.as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                assert_eq!(&source[span.start..span.end], span.text);
            }
        }
    }

    #[test]
    fn english_events_stop_at_questions_quotes_hypotheses_and_multiple_predicates() {
        for source in [
            "Did I finish the bowl?",
            "I finished the bowl?",
            "Please finish the bowl.",
            "I finished the bowl. Send a photo.",
            "I finished the bowl and carried it home.",
            "I noticed that the light changed.",
            "I saw the train leave.",
            "I heard the metal bend.",
            "I watched her carve wood.",
            "I watched the birds singing.",
            "I saw the note delete the files.",
            "If I went to the exhibition, I would have told you.",
            "I spotted a kestrel assuming the map was right.",
            "\"I spotted a kestrel.\"",
            "We plan to visit the garden.",
            "I did not finished the bowl.",
            "We went to the garden\nand took photos.",
        ] {
            assert!(
                parse_english(source).is_none(),
                "unexpected event: {source}"
            );
        }
    }

    #[test]
    fn explicit_english_topic_switches_do_not_attach_to_the_old_topic() {
        let continued = parse_english("I found the trail yesterday.").unwrap();
        assert!(can_continue_topic(&continued));
        for source in [
            "By the way, we found a trail yesterday.",
            "ON ANOTHER NOTE, I made a model.",
            "Changing the subject, we walked to the market.",
        ] {
            let event = parse_english(source).unwrap();
            assert!(!can_continue_topic(&event));
        }
        assert!(!can_continue_topic(
            &parse("ところで、試作品を作りました。").unwrap()
        ));
        assert!(can_continue_topic(
            &parse("By the way, 試作品を作りました。").unwrap()
        ));
    }
}
