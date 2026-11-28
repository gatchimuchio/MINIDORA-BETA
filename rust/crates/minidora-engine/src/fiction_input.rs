//! Source-preserving interpretation for explicitly requested short Japanese fiction.

use std::collections::BTreeMap;

use minidora_domain::{
    FictionAudience, FictionGenre, FictionRegister, FictionRequest, FictionTone, Result, SourceSpan,
};

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v243-fiction";

pub(crate) struct ParsedFiction {
    pub(crate) draft: FictionRequest,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

const GENRES: &[(&str, FictionGenre)] = &[
    ("短い物語", FictionGenre::Story),
    ("短いお話", FictionGenre::Story),
    ("短い童話", FictionGenre::FairyTale),
    ("短い小説", FictionGenre::ShortStory),
    ("短編小説", FictionGenre::ShortStory),
    ("短編", FictionGenre::ShortStory),
    ("物語", FictionGenre::Story),
    ("お話", FictionGenre::Story),
    ("童話", FictionGenre::FairyTale),
];
const COMMANDS: &[&str] = &[
    "書いてください",
    "作ってください",
    "創作してください",
    "考えてください",
    "書いて",
    "作って",
    "創作して",
    "考えて",
];
const AUDIENCES: &[(&str, FictionAudience)] = &[
    ("小さな子ども", FictionAudience::YoungChildren),
    ("小さな子供", FictionAudience::YoungChildren),
    ("未就学児", FictionAudience::YoungChildren),
    ("幼児", FictionAudience::YoungChildren),
    ("小学生", FictionAudience::Elementary),
    ("子ども", FictionAudience::Elementary),
    ("子供", FictionAudience::Elementary),
    ("こども", FictionAudience::Elementary),
    ("大人", FictionAudience::Adult),
    ("成人", FictionAudience::Adult),
    ("一般", FictionAudience::General),
];
const REGISTERS: &[(&str, FictionRegister)] = &[
    ("です・ます調", FictionRegister::Polite),
    ("ですます調", FictionRegister::Polite),
    ("丁寧な文体", FictionRegister::Polite),
    ("ていねいな文体", FictionRegister::Polite),
    ("敬体", FictionRegister::Polite),
    ("だ・である調", FictionRegister::Plain),
    ("である調", FictionRegister::Plain),
    ("常体", FictionRegister::Plain),
];
const TONES: &[(&str, FictionTone)] = &[
    ("心温まる", FictionTone::Warm),
    ("心あたたまる", FictionTone::Warm),
    ("温かい", FictionTone::Warm),
    ("あたたかい", FictionTone::Warm),
    ("ほのぼのとした", FictionTone::Warm),
    ("ユーモラスな", FictionTone::Playful),
    ("おもしろおかしい", FictionTone::Playful),
    ("楽しい", FictionTone::Playful),
    ("不思議な", FictionTone::Mysterious),
    ("神秘的な", FictionTone::Mysterious),
];

#[derive(Default)]
struct Parts {
    protagonist: Option<SourceSpan>,
    setting: Option<SourceSpan>,
    required_elements: Vec<SourceSpan>,
    audience: Option<FictionAudience>,
    register: Option<FictionRegister>,
    tone: Option<FictionTone>,
    genre: Option<FictionGenre>,
    sentence_count: Option<u8>,
    roles: BTreeMap<String, Vec<SourceSpan>>,
}

fn original_span(raw: &str, part: &str) -> SourceSpan {
    let start = part.as_ptr() as usize - raw.as_ptr() as usize;
    SourceSpan {
        start,
        end: start + part.len(),
        text: part.to_owned(),
    }
}

fn insert_role(roles: &mut BTreeMap<String, Vec<SourceSpan>>, role: &str, evidence: SourceSpan) {
    roles
        .entry(format!("/DraftFiction/{role}"))
        .or_default()
        .push(evidence);
}

fn typed<T: Copy + PartialEq>(
    raw: &str,
    roles: &mut BTreeMap<String, Vec<SourceSpan>>,
    role: &str,
    slot: &mut Option<T>,
    value: T,
    evidence: &str,
) -> bool {
    if slot.is_some_and(|prior| prior != value) {
        return false;
    }
    *slot = Some(value);
    insert_role(roles, role, original_span(raw, evidence));
    true
}

fn nominal(raw: &str, value: &str) -> Option<SourceSpan> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 192
        || value.chars().any(char::is_control)
        || value.contains([
            '。', '、', '，', ',', '：', ':', '！', '!', '？', '?', ';', '；',
        ])
        || [
            "してください",
            "して下さい",
            "してほしい",
            "して欲しい",
            "を送って",
            "を送信して",
            "を公開して",
            "を検索して",
            "を実行して",
            "を保存して",
        ]
        .into_iter()
        .any(|command| value.contains(command))
        || COMMANDS
            .iter()
            .copied()
            .chain([
                "して",
                "しないで",
                "させて",
                "と言って",
                "を入れて",
                "調べて",
                "教えて",
                "答えて",
                "読んで",
                "知らせて",
                "送って",
                "事実として",
                "実話として",
            ])
            .any(|ending| value.ends_with(ending))
    {
        return None;
    }
    Some(original_span(raw, value))
}

fn framed_field(
    raw: &str,
    roles: &mut BTreeMap<String, Vec<SourceSpan>>,
    role: &str,
    slot: &mut Option<SourceSpan>,
    value: &str,
) -> bool {
    let Some(evidence) = nominal(raw, value) else {
        return false;
    };
    if slot
        .as_ref()
        .is_some_and(|prior| prior.text != evidence.text)
    {
        return false;
    }
    *slot = Some(evidence.clone());
    insert_role(roles, role, evidence);
    true
}

fn prefix<T: Copy>(value: &str, choices: &[(&str, T)]) -> Option<(T, usize)> {
    choices
        .iter()
        .find(|(word, _)| value.starts_with(*word))
        .map(|(word, choice)| (*choice, word.len()))
}

fn literal_prefix<'a>(text: &'a str, phrases: &[&str]) -> Option<&'a str> {
    phrases.iter().find_map(|phrase| text.strip_prefix(*phrase))
}

fn literal_suffix<'a>(text: &'a str, phrases: &[&str]) -> Option<&'a str> {
    phrases.iter().find_map(|phrase| text.strip_suffix(*phrase))
}

fn exact_count(text: &str) -> Option<(u8, &str)> {
    let first = text.chars().next()?;
    let value = "12345678"
        .find(first)
        .or_else(|| "１２３４５６７８".chars().position(|digit| digit == first))
        .or_else(|| "一二三四五六七八".chars().position(|digit| digit == first))?;
    let rest = text[first.len_utf8()..].strip_prefix('文')?;
    let remainder = if rest.is_empty() {
        rest
    } else {
        literal_prefix(rest, &["ぴったりで", "の", "で", "に"])?
    };
    Some((u8::try_from(value).ok()? + 1, remainder))
}

fn count_after_genre(text: &str) -> Option<(&str, u8, &str)> {
    let (without_suffix, _) = text.rsplit_once('文')?;
    let (start, _) = without_suffix.char_indices().next_back()?;
    let evidence = &text[start..];
    let (count, remaining) = exact_count(evidence)?;
    (remaining.is_empty() && !evidence.ends_with('の')).then_some((&text[..start], count, evidence))
}

fn remove_copula(text: &str) -> &str {
    let text = text.trim();
    text.strip_suffix("です").unwrap_or(text).trim_end()
}

enum PremiseField {
    Protagonist,
    Setting,
}

fn leading_premise(text: &str) -> Option<(PremiseField, &str, &str)> {
    use PremiseField::{Protagonist, Setting};
    [
        ("が主人公の", Protagonist),
        ("を主人公にした", Protagonist),
        ("を主人公として", Protagonist),
        ("を舞台にした", Setting),
        ("を舞台として", Setting),
    ]
    .into_iter()
    .filter_map(|(marker, role)| {
        let (value, remainder) = text.split_once(marker)?;
        Some((role, value, remainder))
    })
    .min_by_key(|(_, value, remainder)| (value.len(), remainder.len()))
}

struct Command<'a> {
    prefix: &'a str,
    genre: FictionGenre,
    word: &'a str,
    count: Option<(u8, &'a str)>,
}

fn writing_command(text: &str) -> Option<Command<'_>> {
    let before_command = literal_suffix(text, COMMANDS)?.trim_end();
    let (head, count) = match count_after_genre(before_command) {
        Some((head, count, evidence)) => (head, Some((count, evidence))),
        None => (before_command, None),
    };
    let head = head.trim_end();
    let head = head.strip_suffix('を').unwrap_or(head).trim_end();
    GENRES.iter().find_map(|(word, genre)| {
        let prefix = head.strip_suffix(*word)?;
        Some(Command {
            prefix,
            genre: *genre,
            word: &head[prefix.len()..],
            count,
        })
    })
}

impl Parts {
    fn protagonist(&mut self, raw: &str, value: &str) -> bool {
        framed_field(
            raw,
            &mut self.roles,
            "protagonist",
            &mut self.protagonist,
            value,
        )
    }

    fn setting(&mut self, raw: &str, value: &str) -> bool {
        framed_field(raw, &mut self.roles, "setting", &mut self.setting, value)
    }

    fn count(&mut self, raw: &str, count: u8, evidence: &str) -> bool {
        typed(
            raw,
            &mut self.roles,
            "sentence_count",
            &mut self.sentence_count,
            count,
            evidence,
        )
    }

    fn leading_modifiers(&mut self, raw: &str, mut rest: &str) -> bool {
        while !rest.trim().is_empty() {
            rest = rest.trim_start();
            let consumed = if let Some((audience, end)) = prefix(rest, AUDIENCES)
                && let Some(remainder) =
                    literal_prefix(&rest[end..], &["向けの", "向けに", "向けで", "向け"])
            {
                if !typed(
                    raw,
                    &mut self.roles,
                    "audience",
                    &mut self.audience,
                    audience,
                    &rest[..rest.len() - remainder.len()],
                ) {
                    return false;
                }
                remainder
            } else if let Some((register, end)) = prefix(rest, REGISTERS)
                && let Some(remainder) = literal_prefix(&rest[end..], &["で", "の"])
            {
                if !typed(
                    raw,
                    &mut self.roles,
                    "register",
                    &mut self.register,
                    register,
                    &rest[..rest.len() - remainder.len()],
                ) {
                    return false;
                }
                remainder
            } else if let Some((count, remainder)) = exact_count(rest) {
                if !self.count(raw, count, &rest[..rest.len() - remainder.len()]) {
                    return false;
                }
                remainder
            } else if let Some((field, value, remainder)) = leading_premise(rest) {
                let applied = match field {
                    PremiseField::Protagonist => self.protagonist(raw, value),
                    PremiseField::Setting => self.setting(raw, value),
                };
                if !applied {
                    return false;
                }
                remainder
            } else if let Some((tone, end)) = prefix(rest, TONES) {
                if !typed(
                    raw,
                    &mut self.roles,
                    "tone",
                    &mut self.tone,
                    tone,
                    &rest[..end],
                ) {
                    return false;
                }
                literal_prefix(&rest[end..], &["雰囲気で", "雰囲気にして", "感じで"])
                    .unwrap_or(&rest[end..])
            } else {
                return false;
            };
            rest = consumed;
        }
        true
    }

    fn command(&mut self, raw: &str, command: Command<'_>) -> bool {
        self.genre.is_none()
            && typed(
                raw,
                &mut self.roles,
                "genre",
                &mut self.genre,
                command.genre,
                command.word,
            )
            && command
                .count
                .is_none_or(|(count, evidence)| self.count(raw, count, evidence))
            && self.leading_modifiers(raw, command.prefix)
    }

    fn required(&mut self, raw: &str, text: &str) -> bool {
        let mut start = 0;
        for (end, separator) in text.match_indices(['と', '・']) {
            let suffix = &text[end + separator.len()..];
            if separator == "と" {
                let hiragana = |character: char| ('\u{3041}'..='\u{309f}').contains(&character);
                if literal_prefix(suffix, &["の", "いう"]).is_some()
                    || (text[..end].chars().next_back().is_some_and(hiragana)
                        && suffix.chars().next().is_some_and(hiragana))
                {
                    continue;
                }
            }
            if !self.element(raw, &text[start..end]) {
                return false;
            }
            start = end + separator.len();
        }
        self.element(raw, &text[start..])
    }

    fn element(&mut self, raw: &str, element: &str) -> bool {
        let Some(evidence) = nominal(raw, element) else {
            return false;
        };
        if self.required_elements.len() >= 3
            || self
                .required_elements
                .iter()
                .any(|prior| prior.text == evidence.text)
        {
            return false;
        }
        self.required_elements.push(evidence.clone());
        insert_role(&mut self.roles, "required_elements", evidence);
        true
    }

    fn constraint(&mut self, raw: &str, text: &str) -> bool {
        if let Some(value) = text
            .strip_prefix("主人公は")
            .map(remove_copula)
            .or_else(|| literal_suffix(text, &["を主人公にして", "を主人公に", "が主人公"]))
        {
            return self.protagonist(raw, value);
        }
        if let Some(value) = text
            .strip_prefix("舞台は")
            .map(remove_copula)
            .or_else(|| literal_suffix(text, &["を舞台にして", "を舞台に", "を舞台として"]))
        {
            return self.setting(raw, value);
        }
        if let Some(value) = literal_prefix(text, &["読者は", "対象読者は", "対象は"]) {
            let value = remove_copula(value);
            return prefix(value, AUDIENCES).is_some_and(|(audience, end)| {
                matches!(&value[end..], "" | "向け")
                    && typed(
                        raw,
                        &mut self.roles,
                        "audience",
                        &mut self.audience,
                        audience,
                        value,
                    )
            });
        }
        if let Some(value) = literal_prefix(text, &["本文は", "文数は"]) {
            let value = value.trim_start();
            return exact_count(value).is_some_and(|(count, tail)| {
                matches!(tail, "" | "して" | "してください") && self.count(raw, count, value)
            });
        }
        if let Some(value) = literal_suffix(
            text,
            &[
                "を必ず登場させてください",
                "を必ず登場させて",
                "を登場させてください",
                "を登場させて",
                "を物語に入れてください",
                "を物語に入れて",
                "を必ず入れてください",
                "を必ず入れて",
                "を入れてください",
                "を入れて",
                "を必ず出してください",
                "を必ず出して",
                "を出してください",
                "を出して",
            ],
        )
        .or_else(|| {
            literal_prefix(
                text,
                &[
                    "必ず登場させるものは",
                    "登場させるものは",
                    "必須の要素は",
                    "小道具は",
                ],
            )
            .map(remove_copula)
        }) {
            return self.required(raw, value);
        }
        self.leading_modifiers(raw, text)
    }
}

pub(crate) fn parse(raw: &str) -> Result<Option<ParsedFiction>> {
    let text = raw.trim();
    if text.len() > 4096
        || text.contains(['「', '」', '『', '』', '"', '“', '”', '\x60'])
        || !GENRES.iter().any(|(genre, _)| text.contains(*genre))
        || !COMMANDS.iter().any(|command| text.contains(*command))
    {
        return Ok(None);
    }
    let mut parts = Parts::default();
    for clause in text
        .split(['。', '、', '，', ',', '！', '!', '？', '?', '\r', '\n'])
        .map(str::trim)
        .filter(|clause| !clause.is_empty())
    {
        let accepted = if let Some(command) = writing_command(clause) {
            parts.command(raw, command)
        } else {
            parts.constraint(raw, clause)
        };
        if !accepted {
            return Ok(None);
        }
    }
    let Some(genre) = parts.genre else {
        return Ok(None);
    };
    Ok(Some(ParsedFiction {
        draft: FictionRequest {
            protagonist: parts.protagonist,
            setting: parts.setting,
            required_elements: parts.required_elements,
            audience: parts.audience.unwrap_or_default(),
            register: parts.register.unwrap_or_default(),
            tone: parts.tone.unwrap_or_default(),
            genre,
            sentence_count: parts.sentence_count.unwrap_or(5),
        },
        roles: parts.roles,
    }))
}
