//! Shared lexical concepts and inflection for the local translation grammar.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Verb {
    Start,
    Finish,
    Open,
    Close,
    Arrive,
    Depart,
    Resume,
    Continue,
    Return,
    Bring,
    Accept,
    Cancel,
    Postpone,
    Attend,
    Rain,
    Snow,
    Be,
}

#[derive(Clone, Copy)]
pub(super) struct VerbLexeme {
    pub verb: Verb,
    pub en: [&'static str; 3],
    pub ja: &'static [&'static str],
    pub object: bool,
}

pub(super) const VERBS: &[VerbLexeme] = &[
    VerbLexeme {
        verb: Verb::Start,
        en: ["start", "starts", "started"],
        ja: &["始まる", "開始する"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Finish,
        en: ["end", "ends", "ended"],
        ja: &["終わる", "終了する"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Open,
        en: ["open", "opens", "opened"],
        ja: &["開く", "開館する", "開店する", "開場する"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Close,
        en: ["close", "closes", "closed"],
        ja: &["閉まる", "閉館する", "閉店する", "閉場する"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Arrive,
        en: ["arrive", "arrives", "arrived"],
        ja: &["到着する", "着く"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Depart,
        en: ["leave", "leaves", "left"],
        ja: &["出発する", "発車する", "出る"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Resume,
        en: ["resume", "resumes", "resumed"],
        ja: &["再開する"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Continue,
        en: ["continue", "continues", "continued"],
        ja: &["続く"],
        object: false,
    },
    VerbLexeme {
        verb: Verb::Return,
        en: ["return", "returns", "returned"],
        ja: &["返却する", "返す"],
        object: true,
    },
    VerbLexeme {
        verb: Verb::Bring,
        en: ["bring", "brings", "brought"],
        ja: &["持参する"],
        object: true,
    },
    VerbLexeme {
        verb: Verb::Accept,
        en: ["accept", "accepts", "accepted"],
        ja: &["受け付ける", "受け取る"],
        object: true,
    },
    VerbLexeme {
        verb: Verb::Cancel,
        en: ["cancel", "cancels", "canceled"],
        ja: &["中止する", "取り消す"],
        object: true,
    },
    VerbLexeme {
        verb: Verb::Postpone,
        en: ["postpone", "postpones", "postponed"],
        ja: &["延期する"],
        object: true,
    },
    VerbLexeme {
        verb: Verb::Attend,
        en: ["attend", "attends", "attended"],
        ja: &["出席する", "参加する"],
        object: true,
    },
];

pub(super) const NOUNS: &[(&str, &str, &str)] = &[
    ("library", "libraries", "図書館"),
    ("museum", "museums", "博物館"),
    ("gallery", "galleries", "美術館"),
    ("station", "stations", "駅"),
    ("store", "stores", "店"),
    ("shop", "shops", "売店"),
    ("office", "offices", "事務所"),
    ("school", "schools", "学校"),
    ("room", "rooms", "部屋"),
    ("venue", "venues", "会場"),
    ("hall", "halls", "ホール"),
    ("gate", "gates", "ゲート"),
    ("door", "doors", "ドア"),
    ("meeting", "meetings", "会議"),
    ("event", "events", "イベント"),
    ("class", "classes", "授業"),
    ("lesson", "lessons", "レッスン"),
    ("workshop", "workshops", "講習会"),
    ("exhibition", "exhibitions", "展示会"),
    ("lecture", "lectures", "講演"),
    ("session", "sessions", "セッション"),
    ("concert", "concerts", "コンサート"),
    ("train", "trains", "電車"),
    ("bus", "buses", "バス"),
    ("flight", "flights", "便"),
    ("registration", "registrations", "受付"),
    ("service", "services", "サービス"),
    ("ticket", "tickets", "チケット"),
    ("book", "books", "本"),
    ("key", "keys", "鍵"),
    ("form", "forms", "用紙"),
    ("document", "documents", "書類"),
    ("bag", "bags", "かばん"),
    ("item", "items", "品物"),
    ("equipment", "equipment", "機材"),
    ("application", "applications", "申請"),
    ("staff member", "staff members", "職員"),
    ("price", "prices", "価格"),
    ("total", "totals", "合計"),
    ("fee", "fees", "料金"),
    ("weight", "weights", "重さ"),
    ("distance", "distances", "距離"),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Tense {
    Present,
    Past,
    Future,
    Ability,
}

/// Japanese plain and polite inflections share the same lexical stem.
pub(super) fn japanese_verb(lemma: &str, tense: Tense, negative: bool, polite: bool) -> String {
    if tense == Tense::Ability {
        let potential = if let Some(stem) = lemma.strip_suffix("する") {
            format!("{stem}できる")
        } else if matches!(lemma, "受け付ける" | "出る") {
            format!("{}られる", lemma.strip_suffix('る').expect("ichidan lemma"))
        } else {
            let (stem, ending) = stem_and_ending(lemma);
            let able = match ending {
                'く' => 'け',
                'す' => 'せ',
                'る' => 'れ',
                _ => unreachable!("approved verb ending"),
            };
            format!("{stem}{able}る")
        };
        let root = potential
            .strip_suffix('る')
            .expect("potential dictionary form");
        return ichidan(root, &potential, Tense::Present, negative, polite);
    }
    if let Some(stem) = lemma.strip_suffix("する") {
        let tail = match (tense == Tense::Past, negative, polite) {
            (false, false, false) => "する",
            (false, true, false) => "しない",
            (true, false, false) => "した",
            (true, true, false) => "しなかった",
            (false, false, true) => "します",
            (false, true, true) => "しません",
            (true, false, true) => "しました",
            (true, true, true) => "しませんでした",
        };
        return format!("{stem}{tail}");
    }
    let (stem, ending) = stem_and_ending(lemma);
    if matches!(lemma, "受け付ける" | "出る") {
        return ichidan(stem, lemma, tense, negative, polite);
    }
    let (a, i, past) = match ending {
        'く' => ("か", "き", "いた"),
        'す' => ("さ", "し", "した"),
        'る' => ("ら", "り", "った"),
        _ => unreachable!("approved verb ending"),
    };
    match (tense == Tense::Past, negative, polite) {
        (false, false, false) => lemma.into(),
        (false, true, false) => format!("{stem}{a}ない"),
        (true, false, false) => format!("{stem}{past}"),
        (true, true, false) => format!("{stem}{a}なかった"),
        (false, false, true) => format!("{stem}{i}ます"),
        (false, true, true) => format!("{stem}{i}ません"),
        (true, false, true) => format!("{stem}{i}ました"),
        (true, true, true) => format!("{stem}{i}ませんでした"),
    }
}

fn stem_and_ending(lemma: &str) -> (&str, char) {
    let (index, tail) = lemma.char_indices().next_back().expect("nonempty verb");
    (&lemma[..index], tail)
}

fn ichidan(root: &str, lemma: &str, tense: Tense, negative: bool, polite: bool) -> String {
    let ending = match (tense == Tense::Past, negative, polite) {
        (false, false, false) => return lemma.into(),
        (false, true, false) => "ない",
        (true, false, false) => "た",
        (true, true, false) => "なかった",
        (false, false, true) => "ます",
        (false, true, true) => "ません",
        (true, false, true) => "ました",
        (true, true, true) => "ませんでした",
    };
    format!("{root}{ending}")
}
