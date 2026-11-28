//! Finite Japanese event reports. Spans always point into the submitted turn.

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
