//! Source-backed message drafting for the ordinary Core route.

pub(crate) mod english_input;
use std::collections::BTreeMap;
use std::sync::LazyLock;

use minidora_domain::{
    ClauseEnding, MessageClause, MessageDraft, MessageFormat, MessagePurpose, Result,
    SentenceLimit, SourceSpan, WritingRegister, failure, identity,
};
use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::language::{
    MAX_EXACT_FIELD_GENERATION_BYTES, MAX_EXACT_FIELD_GENERATION_TOKENS, MeaningCompositionFormat,
    MeaningCompositionPurpose, MeaningCompositionStyle, MeaningGenerationRequest,
    MeaningGrammarFragment,
    MeaningGrammarFragment::{Choice, ExactField, Literal},
    MeaningGrammarUnit, MeaningRole,
};

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v219-draft-message";

pub(crate) struct ParsedMessage {
    pub(crate) draft: MessageDraft,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

fn original_span(raw: &str, part: &str) -> SourceSpan {
    let start = part.as_ptr() as usize - raw.as_ptr() as usize;
    SourceSpan {
        start,
        end: start + part.len(),
        text: part.to_owned(),
    }
}

fn field(raw: &str, part: &str) -> Option<SourceSpan> {
    let part = part.trim();
    (!part.is_empty()
        && part.len() <= 512
        && !part.contains([
            '。', '、', ',', '！', '!', '？', '?', '「', '」', '『', '』', '"', '：', ';', '；',
        ])
        && !part.chars().any(char::is_control))
    .then(|| original_span(raw, part))
}

fn clause(raw: &str, phrase: &str) -> Option<MessageClause> {
    use ClauseEnding::*;
    for (ending, values) in [
        (Cannot, &["できません", "できない"][..]),
        (DoNot, &["しません", "しない"]),
        (IsNot, &["ではありません", "ではない", "じゃない"]),
        (Can, &["できます", "できる"]),
        (WantToDo, &["したいです", "したい"]),
        (Did, &["しました", "した"]),
        (Do, &["します", "する"]),
        (Is, &["である", "です", "だ"]),
    ] {
        for value in values {
            if let Some(stem) = phrase.strip_suffix(value) {
                return Some(MessageClause {
                    stem: field(raw, stem)?,
                    ending,
                });
            }
        }
    }
    None
}

fn message_purpose(raw: &str, text: &str) -> Option<MessagePurpose> {
    for ending in [
        "かどうか尋ねる",
        "かどうか聞く",
        "か確認する",
        "か尋ねる",
        "か聞く",
    ] {
        if let Some(content) = text.strip_suffix(ending) {
            return clause(raw, content).map(|clause| MessagePurpose::Ask { clause });
        }
    }
    for ending in [
        "ことを伝える",
        "と伝える",
        "ことを知らせる",
        "と知らせる",
        "ことを連絡する",
        "旨を伝える",
        "旨を知らせる",
        "旨を",
    ] {
        if let Some(content) = text.strip_suffix(ending) {
            return clause(raw, content).map(|clause| MessagePurpose::Inform { clause });
        }
    }
    for ending in ["をお願いする", "を依頼する", "を頼む"] {
        if let Some(action) = text.strip_suffix(ending) {
            return field(raw, action).map(|action| MessagePurpose::Request { action });
        }
    }
    for ending in ["ことを案内する", "旨を案内する", "ことをお知らせする"] {
        if let Some(content) = text.strip_suffix(ending) {
            return clause(raw, content).map(|clause| MessagePurpose::Announce { clause });
        }
    }
    let invitation = ["お断りする", "断る", "辞退する", "見送る"]
        .iter()
        .find_map(|ending| text.strip_suffix(ending))?;
    let occasion = ["出席", "参加", "お誘い", "招待", "お招き"]
        .iter()
        .find_map(|kind| {
            let invitation = invitation.strip_suffix('を')?.strip_suffix(kind)?;
            invitation
                .strip_suffix("への")
                .or_else(|| invitation.strip_suffix('の'))
        })?;
    Some(MessagePurpose::DeclineInvitation {
        occasion: field(raw, occasion)?,
        gratitude: None,
    })
}

fn sentence_limit(count: &str, approximate: bool) -> Option<SentenceLimit> {
    let count = match count {
        "1" | "１" | "一" => 1,
        "2" | "２" | "二" => 2,
        "3" | "３" | "三" => 3,
        _ => return None,
    };
    Some(if approximate {
        SentenceLimit::AtMost(count)
    } else {
        SentenceLimit::Exact(count)
    })
}

fn omit_reason_directive(text: &str) -> bool {
    matches!(
        text,
        "理由は書かないで"
            | "理由は書かないでください"
            | "理由は不要"
            | "理由は省いて"
            | "理由には触れないで"
            | "理由は一切書かないで"
            | "理由は書かなくていい"
            | "理由は書かなくて大丈夫"
            | "理由は伏せて"
    )
}

fn ends_in_cause_link(text: &str) -> bool {
    ["ので", "ため", "ために", "から"]
        .iter()
        .any(|ending| text.ends_with(ending))
}

fn decline_constraints(text: &str) -> Vec<&str> {
    // A comma after a cause belongs to the following non-attendance clause.
    let mut clauses = Vec::new();
    let mut start = 0;
    for (index, delimiter) in
        text.match_indices(['、', ',', '。', '！', '!', '？', '?', '\r', '\n'])
    {
        let candidate = text[start..index].trim();
        if matches!(delimiter, "、" | ",") && ends_in_cause_link(candidate) {
            continue;
        }
        if !candidate.is_empty() {
            clauses.push(candidate);
        }
        start = index + delimiter.len();
    }
    clauses.push(text[start..].trim());
    clauses
}

fn decline_reason(raw: &str, text: &str) -> Option<SourceSpan> {
    static CAUSE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"\A(?P<cause>(?P<detail>[^、,。]+?)(?:ので|ため(?:に)?|から))[ \t　]*[、,]?[ \t　]*",
            r"(?:(?:参加|出席)(?:が)?(?:できません|できない|難しいです|困難です)|出られ(?:ません|ない)|伺え(?:ません|ない)|行け(?:ません|ない)|欠席(?:します|いたします|する))\z"
        ))
        .expect("decline reason grammar")
    });
    let parts = CAUSE.captures(text)?;
    let detail = parts.name("detail")?.as_str().trim_end();
    if invalid_decline_reason(detail) {
        return None;
    }
    field(raw, parts.name("cause")?.as_str())
}

fn invalid_decline_reason(text: &str) -> bool {
    static ASSERTS_ATTENDANCE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"(?:(?:参加|出席)(?:が)?(?:します|いたします|致します|する|できます|できる)|(?:出られ|伺え|行け)ます)",
            r"(?:が|し|けれど|ので|ため|から|つもり|予定|\z)"
        ))
        .expect("positive attendance clause grammar")
    });
    [
        "ください",
        "下さい",
        "して",
        "してほしい",
        "して欲しい",
        "くれ",
    ]
    .iter()
    .any(|ending| text.ends_with(ending))
        || ASSERTS_ATTENDANCE.is_match(text)
}

fn asks_invitation_gratitude(text: &str) -> bool {
    static GRATITUDE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(concat!(
            r"\A(?:(?:お誘い|お招き|お声がけ|お声掛け|ご招待|招待)(?:への|には?|の)[ \t　]*)?",
            r"(?:お礼|感謝)(?:を|も)(?:伝えたい(?:です)?|伝えて(?:ください)?|添えて(?:ください)?)\z"
        ))
        .expect("invitation gratitude grammar")
    });
    GRATITUDE.is_match(text)
}

pub(crate) fn parse(raw: &str) -> Result<Option<ParsedMessage>> {
    let text = raw
        .trim()
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim();
    if text.len() > 4096
        || text.contains(['「', '」', '『', '』', '"'])
        || !["メール", "メッセージ", "連絡文", "案内文"]
            .iter()
            .any(|word| text.contains(word))
    {
        return Ok(None);
    }
    let pattern = |pattern| {
        Regex::new(pattern).map_err(|error| failure("PARSER_CONFIGURATION", error.to_string()))
    };
    let outer = pattern(
        r"^(?P<purpose>.+?)\s*[、,]?\s*(?P<register>丁寧な|ていねいな|改まった|フォーマルな|くだけた|カジュアルな|親しみやすい)?(?P<form>メール(?:の?本文|文)?|メッセージ(?:の?本文|文)?|連絡文|案内文)(?:の下書き)?\s*(?:を|で)?\s*(?:(?P<count>[123１２３一二三])文(?P<limit>以内|ぴったり)?で?)?\s*(?P<command>書いて(?:ください)?|作って(?:ください)?|作成して(?:ください)?|にして(?:ください)?|下書きして(?:ください)?|考えて(?:ください)?)?(?P<constraints>(?:[、,。！!？?\r\n][\s\S]*)?)$",
    )?;
    let Some(parts) = outer.captures(text) else {
        return Ok(None);
    };
    if parts.name("count").is_none() && parts.name("command").is_none() {
        return Ok(None);
    }
    let format_span = parts.name("form").expect("required form").as_str();
    let format = if format_span.starts_with("メール") {
        MessageFormat::Email
    } else {
        MessageFormat::Message
    };
    let mut body_only = format_span.ends_with("本文");
    let mut omit_subject = false;
    let register = match parts.name("register").map(|part| part.as_str()) {
        Some("改まった" | "フォーマルな") => WritingRegister::Formal,
        Some("くだけた" | "カジュアルな" | "親しみやすい") => {
            WritingRegister::Casual
        }
        _ => WritingRegister::Polite,
    };
    let mut limit = parts.name("count").and_then(|count| {
        sentence_limit(
            count.as_str(),
            parts
                .name("limit")
                .is_some_and(|part| part.as_str() == "以内"),
        )
    });
    let mut roles = BTreeMap::new();
    let mut role = |key, value| {
        roles
            .entry(format!("/DraftMessage/{key}"))
            .or_insert_with(Vec::new)
            .push(original_span(raw, value));
    };
    role("format", format_span);
    if let Some(part) = parts.name("register") {
        role("register", part.as_str());
    }
    if let Some(part) = parts.name("count") {
        role("sentence_count", part.as_str());
    }
    if let Some(part) = parts.name("limit") {
        role("sentence_count", part.as_str());
    }
    let mut body = parts
        .name("purpose")
        .expect("required purpose")
        .as_str()
        .trim();
    let addressee =
        pattern(r"^(?P<name>[^、,。]{1,48}?)(?P<marker>宛てに?|宛に|へ|に)[、,]\s*(?P<rest>.+)$")?;
    let decline_addressee =
        pattern(r"^(?P<name>[^、,。]{1,48}?)(?P<marker>宛てに|宛に)[ \t　]*(?P<rest>.+)$")?;
    let mut recipient = None;
    if let Some(parts) = addressee.captures(body).or_else(|| {
        decline_addressee.captures(body).filter(|parts| {
            matches!(
                message_purpose(raw, parts.name("rest").expect("required purpose").as_str()),
                Some(MessagePurpose::DeclineInvitation { .. })
            )
        })
    }) {
        let name = parts.name("name").expect("required name").as_str();
        let explicit = parts["marker"].starts_with("宛");
        let addressed = [
            "様",
            "さん",
            "殿",
            "御中",
            "先生",
            "部",
            "課",
            "担当者",
            "チーム",
        ]
        .iter()
        .any(|end| name.trim().ends_with(end));
        let name = name.trim();
        let name = ["取引先の", "お客様の", "上司の", "同僚の"]
            .iter()
            .find_map(|prefix| name.strip_prefix(prefix).map(|rest| (*prefix, rest)))
            .map_or(name, |(prefix, rest)| {
                role("recipient_relationship", &name[..prefix.len()]);
                rest
            });
        if (explicit || addressed)
            && let Some(name_span) = field(raw, name)
        {
            recipient = Some(name_span);
            role("recipient", name.trim());
            body = parts
                .name("rest")
                .expect("required remaining purpose")
                .as_str()
                .trim();
        }
    }
    let mut omit_reason = false;
    // A standalone modifier belongs to the draft, not to the recipient or the action.
    if let Some((prefix, rest)) = body.split_once(['、', ','])
        && omit_reason_directive(prefix.trim())
    {
        omit_reason = true;
        role("constraints", prefix.trim());
        body = rest.trim_start();
    }
    let mut circumstance = None;
    if let Some((prefix, rest)) = body.split_once(['、', ',']) {
        let prefix = prefix.trim();
        if [
            "までに",
            "までには",
            "なら",
            "場合は",
            "場合には",
            "のときは",
            "の際は",
        ]
        .iter()
        .any(|end| prefix.ends_with(end))
        {
            circumstance = field(raw, prefix);
            if circumstance.is_none() {
                return Ok(None);
            }
            role("circumstance", prefix);
            body = rest.trim_start();
        }
    }
    let Some(mut purpose) = message_purpose(raw, body) else {
        return Ok(None);
    };
    let declining = matches!(purpose, MessagePurpose::DeclineInvitation { .. });
    if !declining && text.contains(['\r', '\n']) {
        return Ok(None);
    }
    role("purpose", body);
    if let MessagePurpose::DeclineInvitation { occasion, .. } = &purpose {
        role("occasion", &raw[occasion.start..occasion.end]);
    }

    let mut reason = None;
    let sentence_count =
        pattern(r"^(?P<count>[123１２３一二三])文(?P<limit>以内|ぴったり)?(?:で|に)(?:して)?$")?;
    let tail = parts
        .name("constraints")
        .expect("constraints capture")
        .as_str();
    let constraints = if declining {
        decline_constraints(tail)
    } else {
        tail.split(['、', ',', '。']).map(str::trim).collect()
    };
    for candidate in constraints.into_iter().filter(|s| !s.is_empty()) {
        let count_clause = if declining {
            candidate
                .strip_prefix("本文は")
                .map_or(candidate, str::trim_start)
        } else {
            candidate
        };
        match candidate {
            value if omit_reason_directive(value) => omit_reason = true,
            "本文だけ" | "本文のみ" => body_only = true,
            "件名は不要" | "件名なし" => omit_subject = true,
            "送信しないで"
            | "送信はしないで"
            | "送信せず下書きだけ"
            | "送らないで"
            | "日本語で" => {}
            _ => {
                if let Some(value) = candidate.strip_prefix("理由は") {
                    if reason.is_some()
                        || (declining && invalid_decline_reason(value.trim()))
                        || [
                            "書か",
                            "書いて",
                            "触れ",
                            "入れて",
                            "載せ",
                            "省い",
                            "不要",
                            "伏せ",
                            "明か",
                            "送信",
                            "送って",
                            "付け加",
                            "含め",
                            "一切",
                            "なし",
                        ]
                        .iter()
                        .any(|verb| value.contains(verb))
                    {
                        return Ok(None);
                    }
                    reason = field(raw, value);
                    if reason.is_none() {
                        return Ok(None);
                    }
                    role("reason", value.trim());
                } else if let Some(parts) = sentence_count.captures(count_clause) {
                    let next = sentence_limit(
                        &parts["count"],
                        parts
                            .name("limit")
                            .is_some_and(|part| part.as_str() == "以内"),
                    );
                    if limit.is_some() && limit != next {
                        return Ok(None);
                    }
                    limit = next;
                } else if let MessagePurpose::DeclineInvitation { gratitude, .. } = &mut purpose {
                    if asks_invitation_gratitude(candidate) {
                        if gratitude.is_some() {
                            return Ok(None);
                        }
                        *gratitude = Some(original_span(raw, candidate));
                        role("gratitude", candidate);
                    } else if reason.is_none()
                        && let Some(cause) = decline_reason(raw, candidate)
                    {
                        role("reason", &raw[cause.start..cause.end]);
                        reason = Some(cause);
                    } else {
                        return Ok(None);
                    }
                } else {
                    // In particular a second instruction cannot silently be dropped.
                    return Ok(None);
                }
            }
        }
        role("constraints", candidate);
    }
    Ok(Some(ParsedMessage {
        draft: MessageDraft {
            recipient,
            purpose,
            circumstance,
            reason,
            omit_reason,
            format,
            register,
            sentence_limit: limit,
            omit_subject,
            body_only,
        },
        roles,
    }))
}

const ARTIFACT_KIND: &str = "minidora.core-computation.message-draft.v1";
const REVISION_KIND: &str = "minidora.core-computation.message-revision.v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageRevisionProof {
    pub(crate) prior_result: String,
    pub(crate) prior_compute: String,
    pub(crate) prior_render: String,
    pub(crate) compact: bool,
    pub(crate) shorter_than: Option<usize>,
}

pub(crate) fn rendered_body(text: &str) -> &str {
    text.rsplit('\n').next().unwrap_or(text)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MessageMeaningPlan {
    pub(crate) source_bundle: String,
    pub(crate) draft: MessageDraft,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<MessageRevisionProof>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) english_locale: Option<String>,
}

impl MessageMeaningPlan {
    pub(crate) fn dependencies(&self) -> Vec<String> {
        let mut dependencies = vec![self.source_bundle.clone()];
        if let Some(revision) = &self.revision {
            dependencies.extend([
                revision.prior_result.clone(),
                revision.prior_compute.clone(),
                revision.prior_render.clone(),
            ]);
        }
        dependencies
    }

    fn artifact_kind(&self) -> &'static str {
        if self.revision.is_some() {
            REVISION_KIND
        } else {
            ARTIFACT_KIND
        }
    }

    pub(crate) fn output_locale<'a>(&'a self, request_locale: &'a str) -> &'a str {
        if let Some(locale) = &self.english_locale {
            return locale;
        }
        if matches!(request_locale, "en" | "en-US") {
            return "ja";
        }
        request_locale
    }

    fn generation_request(
        &self,
        fragments: Vec<MeaningGrammarFragment>,
        budget: usize,
    ) -> Result<MeaningGenerationRequest> {
        Ok(MeaningGenerationRequest {
            purpose: MeaningCompositionPurpose::ShortComposition,
            format: MeaningCompositionFormat::Paragraph,
            style: MeaningCompositionStyle::Neutral,
            units: vec![MeaningGrammarUnit {
                id: "message-draft".into(),
                role: MeaningRole::Body,
                evidence_ids: self.dependencies(),
                dependencies: Vec::new(),
                semantic_sha256: identity(&(self.artifact_kind(), self))?,
                fragments,
            }],
            max_output_bytes: budget.min(MAX_EXACT_FIELD_GENERATION_BYTES),
            max_tokens: MAX_EXACT_FIELD_GENERATION_TOKENS,
        })
    }

    pub(crate) fn grammar(&self, budget: usize) -> Result<MeaningGenerationRequest> {
        use MessagePurpose::*;
        if self.english_locale.is_some() {
            let (fragments, _) = self.english_fragments()?;
            return self.generation_request(fragments, budget);
        }
        let draft = &self.draft;
        let formal = draft.register == WritingRegister::Formal;
        let polite = draft.register != WritingRegister::Casual;
        let reason = draft.reason.as_ref().filter(|_| !draft.omit_reason);
        let compact = self
            .revision
            .as_ref()
            .is_some_and(|revision| revision.compact);
        let count = match draft.sentence_limit {
            Some(SentenceLimit::Exact(count)) => count,
            _ if compact => 1,
            Some(SentenceLimit::AtMost(limit)) => limit.min(if reason.is_some() { 2 } else { 1 }),
            None => {
                if draft.format == MessageFormat::Email || reason.is_some() {
                    2
                } else {
                    1
                }
            }
        };
        let mut fragments = Vec::new();
        let literal = |value: &str| Literal {
            value: value.into(),
        };
        let exact = |span: &SourceSpan| ExactField {
            value: span.text.clone(),
        };
        let newline = || ExactField { value: "\n".into() };
        let complete = |fragments| self.generation_request(fragments, budget);
        if !draft.body_only {
            if draft.format == MessageFormat::Email && !draft.omit_subject {
                fragments.push(literal("件名："));
                match &draft.purpose {
                    Request { action } => fragments.extend([exact(action), literal("のお願い")]),
                    Ask { clause } => fragments.extend([
                        exact(&clause.stem),
                        literal(clause_surface(clause.ending, false)),
                        literal("かのご確認"),
                    ]),
                    Inform { clause } => fragments.extend([
                        exact(&clause.stem),
                        literal(clause_surface(clause.ending, false)),
                        literal("件"),
                    ]),
                    Announce { clause } => fragments.extend([
                        exact(&clause.stem),
                        literal(clause_surface(clause.ending, false)),
                        literal("件のお知らせ"),
                    ]),
                    DeclineInvitation { occasion, .. } => fragments.extend([
                        exact(occasion),
                        literal(if polite {
                            "への出席辞退のご連絡"
                        } else {
                            "の欠席について"
                        }),
                    ]),
                }
                fragments.push(newline());
            }
            if let Some(recipient) = &draft.recipient {
                fragments.push(exact(recipient));
                if polite
                    && !["様", "さん", "殿", "御中", "先生"]
                        .iter()
                        .any(|end| recipient.text.ends_with(end))
                {
                    fragments.push(literal(" 様"));
                }
                fragments.push(newline());
            }
        }
        if let DeclineInvitation {
            occasion,
            gratitude,
        } = &draft.purpose
        {
            fragments.extend(decline_body(
                draft,
                occasion,
                gratitude.as_ref(),
                reason,
                compact,
            ));
            return complete(fragments);
        }
        if count == 1
            && let Some(reason) = reason
        {
            fragments.extend([
                literal("「"),
                exact(reason),
                literal(if compact {
                    "」が理由で、"
                } else if polite {
                    "」という事情があり、"
                } else {
                    "」という事情があって、"
                }),
            ]);
        }
        if let Some(circumstance) = &draft.circumstance {
            fragments.extend([exact(circumstance), literal("、")]);
        }
        match &draft.purpose {
            Request { action } => {
                fragments.extend([exact(action), literal("を")]);
                fragments.push(if formal {
                    literal(if compact {
                        "お願いいたします"
                    } else {
                        "お願い申し上げます"
                    })
                } else if polite && compact {
                    literal("お願いできますか")
                } else if polite {
                    Choice {
                        slot: "request-ending".into(),
                        alternatives: vec![
                            "お願いできますか".into(),
                            "お願いしてもよろしいでしょうか".into(),
                        ],
                    }
                } else {
                    literal("お願いできる？")
                });
                if polite {
                    fragments.push(literal("。"));
                }
            }
            Ask { clause } => {
                fragments.extend([
                    exact(&clause.stem),
                    literal(clause_surface(clause.ending, polite && !formal)),
                    literal(if formal {
                        if compact {
                            "か、お知らせいただけますか。"
                        } else {
                            "か、お知らせいただけますでしょうか。"
                        }
                    } else if polite {
                        "か。"
                    } else {
                        "？"
                    }),
                ]);
            }
            Inform { clause } => {
                fragments.extend([
                    exact(&clause.stem),
                    literal(
                        if compact && polite && !formal && clause.ending == ClauseEnding::WantToDo {
                            "したいです"
                        } else {
                            clause_surface(clause.ending, polite && !formal)
                        },
                    ),
                    literal(if formal && compact {
                        "旨、ご連絡いたします。"
                    } else if formal {
                        "旨、ご連絡申し上げます。"
                    } else {
                        "。"
                    }),
                ]);
            }
            Announce { clause } => fragments.extend([
                exact(&clause.stem),
                literal(clause_surface(clause.ending, false)),
                literal(if formal {
                    "ことをお知らせ申し上げます。"
                } else if polite {
                    "ことをお知らせします。"
                } else {
                    "ことをお知らせするね。"
                }),
            ]),
            DeclineInvitation { .. } => unreachable!("decline body has already been generated"),
        }
        let mut emitted = 1;
        if count > 1
            && let Some(reason) = reason
        {
            fragments.extend([
                literal("理由は「"),
                exact(reason),
                literal(if polite {
                    "」です。"
                } else {
                    "」だよ。"
                }),
            ]);
            emitted += 1;
        }
        if count - emitted == 2 {
            fragments.push(literal(match (polite, compact) {
                (true, true) => "ありがとうございます。",
                (true, false) => "ご一読いただきありがとうございます。",
                (false, true) => "ありがとう。",
                (false, false) => "読んでくれてありがとう。",
            }));
            emitted += 1;
        }
        if emitted < count {
            fragments.push(literal(match (&draft.purpose, polite, compact) {
                (Ask { .. }, true, true) => "ご返信いただけると幸いです。",
                (Ask { .. }, true, false) => "お返事をいただけると幸いです。",
                (Ask { .. }, false, true) => "わかったら教えて。",
                (Ask { .. }, false, false) => "わかったら教えてね。",
                (Request { .. }, true, true) => "ご検討をお願いいたします。",
                (Request { .. }, true, false) => "ご検討のほどよろしくお願いいたします。",
                (Request { .. }, false, _) => "よろしくね。",
                (_, true, true) => "ご確認をお願いします。",
                (_, true, false) => "ご確認をお願いいたします。",
                (_, false, _) => "確認してね。",
            }));
        }
        complete(fragments)
    }

    fn english_fragments(&self) -> Result<(Vec<MeaningGrammarFragment>, usize)> {
        use MessagePurpose::{DeclineInvitation, Request};
        use WritingRegister::{Casual, Formal, Polite};

        let draft = &self.draft;
        let reason = draft.reason.as_ref().filter(|_| !draft.omit_reason);
        let compact = self
            .revision
            .as_ref()
            .is_some_and(|revision| revision.compact);
        let preferred = match &draft.purpose {
            Request { .. }
                if !compact && (draft.format == MessageFormat::Email || reason.is_some()) =>
            {
                2
            }
            DeclineInvitation {
                gratitude: Some(_), ..
            } if !compact => 2,
            Request { .. } | DeclineInvitation { .. } => 1,
            _ => {
                return Err(failure(
                    "MESSAGE_SURFACE_CONSTRAINT",
                    "This English draft requires a request or an invitation refusal.",
                ));
            }
        };
        let count = match draft.sentence_limit {
            Some(SentenceLimit::Exact(count)) => usize::from(count),
            Some(SentenceLimit::AtMost(limit)) => preferred.min(usize::from(limit)),
            None => preferred,
        };
        if !(1..=3).contains(&count) {
            return Err(failure(
                "MESSAGE_SURFACE_CONSTRAINT",
                "The English message body requires one to three sentences.",
            ));
        }

        let literal = |value: &str| Literal {
            value: value.into(),
        };
        let exact = |source: &SourceSpan| ExactField {
            value: source.text.clone(),
        };
        let newline = || ExactField { value: "\n".into() };
        let mut fragments = Vec::new();
        if !draft.body_only {
            if draft.format == MessageFormat::Email && !draft.omit_subject {
                match &draft.purpose {
                    Request { action } => {
                        fragments.extend([literal("Subject: Request to "), exact(action)])
                    }
                    DeclineInvitation { occasion, .. } => {
                        fragments.extend([literal("Subject: Unable to attend "), exact(occasion)]);
                    }
                    _ => unreachable!("the available purpose was checked above"),
                }
                fragments.push(newline());
            }
            if let Some(recipient) = &draft.recipient {
                let greeting = if draft.register == Formal {
                    "Dear "
                } else {
                    "Hi "
                };
                fragments.extend([literal(greeting), exact(recipient), literal(","), newline()]);
            }
        }

        let mut emitted = 1;
        match &draft.purpose {
            Request { action } => {
                let inline_reason = reason.filter(|_| count == 1);
                if let Some(reason) = inline_reason {
                    fragments.extend([literal("Because "), exact(reason), literal(", ")]);
                }
                if let Some(circumstance) = &draft.circumstance {
                    fragments.extend([exact(circumstance), literal(", ")]);
                }
                let start_lowercase = inline_reason.is_some() || draft.circumstance.is_some();
                let opening = match (compact, draft.register, start_lowercase) {
                    (true, _, false) => "Please ",
                    (true, _, true) => "please ",
                    (false, Formal, false) => "Would you please ",
                    (false, Formal, true) => "would you please ",
                    (false, Polite, false) => "Could you ",
                    (false, Polite, true) => "could you ",
                    (false, Casual, false) => "Can you ",
                    (false, Casual, true) => "can you ",
                };
                fragments.extend([literal(opening), exact(action)]);
                if !compact || !action.text.ends_with('.') {
                    fragments.push(literal(if compact { "." } else { "?" }));
                }
                if let Some(reason) = reason.filter(|_| count > 1) {
                    fragments.extend([literal(" The reason is that "), exact(reason)]);
                    if !reason.text.ends_with('.') {
                        fragments.push(literal("."));
                    }
                    emitted += 1;
                }
            }
            DeclineInvitation {
                occasion,
                gratitude,
            } => {
                if gratitude.is_some() {
                    let thanks = match (draft.register, count > 1) {
                        (Casual, true) => "Thanks for the invitation. ",
                        (Casual, false) => "Thanks for the invitation, but ",
                        (_, true) => "Thank you for inviting me. ",
                        (_, false) => "Thank you for inviting me, but ",
                    };
                    fragments.push(literal(thanks));
                    if count > 1 {
                        emitted += 1;
                    }
                }
                if let Some(circumstance) = &draft.circumstance {
                    fragments.extend([exact(circumstance), literal(", ")]);
                }
                let opening = match (compact, draft.register) {
                    (true, Casual | Polite) => "I can't attend ",
                    (true, Formal) => "I cannot attend ",
                    (false, Formal) => "I regret that I am unable to attend ",
                    (false, Polite) => "I am unable to attend ",
                    (false, Casual) => "I can't make it to ",
                };
                fragments.extend([literal(opening), exact(occasion)]);
                if let Some(reason) = reason {
                    fragments.extend([literal(" because "), exact(reason)]);
                }
                if !reason.unwrap_or(occasion).text.ends_with('.') {
                    fragments.push(literal("."));
                }
            }
            _ => unreachable!("the available purpose was checked above"),
        }

        if emitted < count {
            let closing = match (&draft.purpose, draft.register) {
                (Request { .. }, Formal) => " I appreciate your consideration.",
                (Request { .. }, Polite) => " Please let me know if that works for you.",
                (Request { .. }, Casual) => " Let me know if that works.",
                (DeclineInvitation { .. }, Casual) => " I'm sorry to miss it.",
                (DeclineInvitation { .. }, _) => " I'm sorry that I can't be there.",
                _ => unreachable!("the available purpose was checked above"),
            };
            fragments.push(literal(closing));
            emitted += 1;
        }
        if emitted < count {
            fragments.push(literal(match &draft.purpose {
                Request { .. } => " Please let me know if you have any questions.",
                DeclineInvitation { .. } => " I hope it goes well.",
                _ => unreachable!("the available purpose was checked above"),
            }));
            emitted += 1;
        }
        Ok((fragments, emitted))
    }

    pub(crate) fn verify_surface(&self, text: &str) -> Result<()> {
        let body = rendered_body(text);
        if self
            .revision
            .as_ref()
            .and_then(|revision| revision.shorter_than)
            .is_some_and(|prior_length| body.chars().count() >= prior_length)
        {
            return Err(failure(
                "MESSAGE_CAN_NOT_SHORTEN",
                if self.english_locale.is_some() {
                    "The body cannot be shortened further while preserving its content, style, and sentence limit."
                } else {
                    "文数・元の内容・文体を保って本文をこれ以上短くできません。"
                },
            ));
        }
        let (count, subject_prefix) = if self.english_locale.is_some() {
            let (fragments, declared_count) = self.english_fragments()?;
            // Count the grammatical sentences, including a final abbreviation
            // used as the terminator. Exact fields may contain other periods.
            let expected: String = fragments
                .iter()
                .map(|fragment| match fragment {
                    Literal { value } | ExactField { value } => value.as_str(),
                    Choice { .. } => unreachable!("English message fragments are exact"),
                })
                .collect();
            if text != expected {
                return Err(failure(
                    "MESSAGE_SURFACE_CONSTRAINT",
                    "The draft differs from its planned content or layout.",
                ));
            }
            (declared_count, "Subject: ")
        } else {
            (body.matches(['。', '？']).count(), "件名：")
        };
        let limit_matches = match self.draft.sentence_limit {
            Some(SentenceLimit::Exact(limit)) => count == limit as usize,
            Some(SentenceLimit::AtMost(limit)) => count <= limit as usize,
            None => true,
        };
        if count == 0
            || !limit_matches
            || (self.draft.body_only && body != text)
            || (self.draft.omit_subject && text.starts_with(subject_prefix))
        {
            return Err(failure(
                "MESSAGE_SURFACE_CONSTRAINT",
                if self.english_locale.is_some() {
                    "The draft fails the requested sentence count or body layout."
                } else {
                    "下書きの文数または本文指定に合いません。"
                },
            ));
        }
        Ok(())
    }
}

fn decline_body(
    draft: &MessageDraft,
    occasion: &SourceSpan,
    gratitude: Option<&SourceSpan>,
    reason: Option<&SourceSpan>,
    compact: bool,
) -> Vec<MeaningGrammarFragment> {
    let formal = draft.register == WritingRegister::Formal;
    let polite = draft.register != WritingRegister::Casual;
    let preferred = if gratitude.is_some() && !compact {
        2
    } else {
        1
    };
    let count = match draft.sentence_limit {
        Some(SentenceLimit::Exact(count)) => count,
        Some(SentenceLimit::AtMost(limit)) => limit.min(preferred),
        None => preferred,
    };
    let literal = |value: &str| Literal {
        value: value.into(),
    };
    let exact = |span: &SourceSpan| ExactField {
        value: span.text.clone(),
    };
    let mut fragments = Vec::new();
    let mut emitted = 1;
    if let Some(gratitude) = gratitude {
        let source = &gratitude.text;
        let wording = if source.starts_with("お招き") {
            if polite {
                "お招きいただき"
            } else {
                "招いてくれて"
            }
        } else if source.starts_with("お声がけ") || source.starts_with("お声掛け") {
            if polite {
                "お声がけいただき"
            } else {
                "声をかけてくれて"
            }
        } else if source.starts_with("ご招待") || source.starts_with("招待") {
            if polite {
                "ご招待いただき"
            } else {
                "招待してくれて"
            }
        } else if polite {
            "お誘いいただき"
        } else {
            "誘ってくれて"
        };
        fragments.push(literal(wording));
        fragments.push(literal(match (count > 1, polite, compact) {
            (true, true, _) => "ありがとうございます。",
            (true, false, _) => "ありがとう。",
            (false, true, true) => "感謝しますが、",
            (false, true, false) => "感謝しておりますが、",
            (false, false, _) => "ありがたいけれど、",
        }));
        if count > 1 {
            emitted += 1;
        }
    }
    if let Some(circumstance) = &draft.circumstance {
        fragments.extend([exact(circumstance), literal("、")]);
    }
    if let Some(reason) = reason {
        fragments.push(exact(reason));
        fragments.push(literal(if ends_in_cause_link(&reason.text) {
            "、"
        } else if ["ある", "いる", "ない", "する", "ます", "です"]
            .iter()
            .any(|ending| reason.text.ends_with(ending))
        {
            "ので、"
        } else {
            "のため、"
        }));
    }
    fragments.extend([
        exact(occasion),
        literal(if formal || (polite && compact) {
            "への出席は辞退いたします。"
        } else if polite {
            "への参加は見送らせていただきます。"
        } else {
            "への参加は見送るね。"
        }),
    ]);
    if emitted < count {
        fragments.push(literal(if formal {
            "ご期待に添えず申し訳ございません。"
        } else if polite {
            "参加できず申し訳ありません。"
        } else {
            "参加できなくてごめんね。"
        }));
        emitted += 1;
    }
    if emitted < count {
        fragments.push(literal(if polite {
            "どうぞよろしくお願いいたします。"
        } else {
            "よろしくね。"
        }));
    }
    fragments
}

fn clause_surface(ending: ClauseEnding, polite: bool) -> &'static str {
    use ClauseEnding::*;
    match (ending, polite) {
        (Do, true) => "します",
        (Do, false) => "する",
        (DoNot, true) => "しません",
        (DoNot, false) => "しない",
        (Did, true) => "しました",
        (Did, false) => "した",
        (WantToDo, true) => "したいと考えています",
        (WantToDo, false) => "したい",
        (Can, true) => "できます",
        (Can, false) => "できる",
        (Cannot, true) => "できません",
        (Cannot, false) => "できない",
        (Is, true) => "です",
        (Is, false) => "だ",
        (IsNot, true) => "ではありません",
        (IsNot, false) => "ではない",
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratedMessageArtifact {
    kind: String,
    pub(crate) meaning_plan: MessageMeaningPlan,
    pub(crate) content_generation: crate::language::MeaningGenerationReport,
}

impl GeneratedMessageArtifact {
    pub(crate) fn new(
        meaning_plan: MessageMeaningPlan,
        content_generation: crate::language::MeaningGenerationReport,
    ) -> Self {
        Self {
            kind: meaning_plan.artifact_kind().into(),
            meaning_plan,
            content_generation,
        }
    }
}
