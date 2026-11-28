//! Explicit, source-preserving Japanese input for a finite work plan.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use minidora_domain::{
    Intent, Result, SourceSpan, WorkPlanChange, WorkPlanDependency, WorkPlanTask,
    WorkPlanWithoutPersonnel,
};
use regex::Regex;

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v247-work-plan";
pub(crate) const REVISION_PARSER_VERSION: &str = "declarative-ja-v248-work-plan-revision";
const CONSULTATION_PARSER_VERSION: &str = "declarative-ja-v1-work-plan-consultation";
const CONSULTATION_REVISION_PARSER_VERSION: &str =
    "declarative-ja-v1-work-plan-consultation-revision";
pub(crate) const HISTORY_REVISION_PARSER_VERSION: &str =
    "declarative-ja-v1-work-plan-history-revision";
pub(crate) const HEARING_HISTORY_REVISION_PARSER_VERSION: &str =
    "declarative-ja-v2-work-plan-history-revision";
pub(crate) const PERSONNEL_HEARING_PARSER_VERSION: &str =
    "declarative-ja-v1-work-plan-personnel-hearing";

pub(crate) struct ParsedWorkPlan {
    pub(crate) version: &'static str,
    pub(crate) intent: Intent,
    pub(crate) roles: BTreeMap<String, Vec<SourceSpan>>,
}

type Roles = BTreeMap<String, Vec<SourceSpan>>;

const PLAN_HEADERS: &[&str] = &[
    "作業計画を立てて",
    "作業計画を作って",
    "作業計画を組んで",
    "段取りを組んで",
    "段取りを考えて",
    "計画を立てて",
    "計画を作って",
];

pub(crate) fn version_matches(version: &str, intent: &Intent) -> bool {
    match intent {
        Intent::PlanWork { .. } => matches!(version, PARSER_VERSION | CONSULTATION_PARSER_VERSION),
        Intent::ClarifyWorkPlanPersonnel { .. } => version == PERSONNEL_HEARING_PARSER_VERSION,
        Intent::ReviseWorkPlan { .. } => {
            matches!(
                version,
                REVISION_PARSER_VERSION | CONSULTATION_REVISION_PARSER_VERSION
            ) || is_historical_revision_version(version)
        }
        _ => {
            !is_historical_revision_version(version)
                && !matches!(
                    version,
                    PARSER_VERSION
                        | CONSULTATION_PARSER_VERSION
                        | REVISION_PARSER_VERSION
                        | CONSULTATION_REVISION_PARSER_VERSION
                        | PERSONNEL_HEARING_PARSER_VERSION
                )
        }
    }
}

pub(crate) fn is_historical_revision_version(version: &str) -> bool {
    matches!(
        version,
        HISTORY_REVISION_PARSER_VERSION | HEARING_HISTORY_REVISION_PARSER_VERSION
    )
}

pub(crate) fn has_historical_revision_anchor(intent: &Intent, roles: &Roles) -> bool {
    matches!(intent, Intent::ReviseWorkPlan { .. })
        && roles.get("/ReviseWorkPlan/anchor").is_some_and(|anchors| {
            matches!(anchors.as_slice(), [anchor] if ["前回の", "先ほどの", "さっきの", "前の"]
                .iter()
                .any(|prefix| anchor.text.starts_with(prefix)))
        })
}

struct Grammar {
    task: Regex,
    deadline: Regex,
    personnel: Regex,
    personnel_answer: Regex,
    remaining: Regex,
    duration_edit: Regex,
    deadline_edit: Regex,
    personnel_edit: Regex,
}

static GRAMMAR: LazyLock<Grammar> = LazyLock::new(|| {
    let numeral = r"[0-9０-９〇零一二三四五六七八九十百千]{1,8}";
    let minutes =
        format!(r"(?:{numeral}[ \t　]*時間(?:[ \t　]*{numeral}[ \t　]*分)?|{numeral}[ \t　]*分)");
    let make = |expression: String| Regex::new(&expression).expect("work plan grammar");
    Grammar {
        task: make(format!(
            r"\A(?P<name>.+?)(?:に|は|[:：])[ \t　]*(?P<duration>{minutes})(?:[ \t　]*(?:かかります|かかる|です))?(?:[ \t　]*(?:[（(](?P<workers>{numeral}人)[）)]|(?P<needed>{numeral}人)(?:が)?必要))?\z"
        )),
        deadline: make(format!(
            concat!(
                r"\A(?:(?:期限|締切|制限時間|全体の期限)は[ \t　]*(?P<labelled>{minutes})(?:以内|まで)?(?:です)?",
                r"|(?:残り)?(?P<finishing>{minutes})(?:以内(?:に|で)|で)(?:終えたい|終わらせたい|終わらせて|終える|終わるように)(?:ください)?)\z"
            ),
            minutes = minutes
        )),
        personnel: make(format!(
            concat!(
                r"\A(?:(?:全員で|人数は|人手は|作業者は|作業人数は|人数の上限は)[ \t　]*(?P<labelled>{numeral}人)(?:です)?",
                r"|(?P<workers>{numeral}人)[ \t　]*で(?:作業する|進めたい|作業したい)?(?P<rest>.*))\z"
            ),
            numeral = numeral
        )),
        personnel_answer: make(format!(
            concat!(
                r"\A(?:はい[、，,][ 　]*)?(?:(?:人数|作業人数|作業者数|作業者|人手|全員)は[ 　]*)?",
                r"(?P<crew>{numeral}[ 　]*[人名]|ひとり|ふたり)",
                r"(?:[ 　]*(?:です|でお願いします|で大丈夫(?:です)?|でいいです|で構いません|で作業します))?\z"
            ),
            numeral = numeral
        )),
        remaining: make(format!(
            r"\A(?:もう[ 　]*)?(?:あと|残り|使える時間は)[ 　]*(?P<limit>{minutes})(?:しかない|しかありません|しかなくなった|に減った|です|になった|以内)?\z"
        )),
        duration_edit: make(format!(
            r"\A(?P<task>.+?)(?:の所要時間(?:だけ|を)|の時間(?:だけ|を)|を)[ \t　]*(?P<duration>{minutes})(?:に変えて|に変更して|にして)(?:ください)?\z"
        )),
        deadline_edit: make(format!(
            r"\A(?:締切|期限|制限時間)(?:だけ|を)[ \t　]*(?P<deadline>{minutes})(?:以内)?(?:に変えて|に変更して|にして)(?:ください)?\z"
        )),
        personnel_edit: make(format!(
            r"\A(?:人数|作業人数|人手|作業者数)(?:だけ|を)[ \t　]*(?P<personnel>{numeral}人)(?:に変えて|に変更して|にして)(?:ください)?\z"
        )),
    }
});

fn terminal_trim(text: &str) -> &str {
    text.trim()
        .trim_end_matches(['。', '.', '！', '!', '？', '?'])
        .trim_end()
}

fn original_span(raw: &str, part: &str) -> SourceSpan {
    let start = part.as_ptr() as usize - raw.as_ptr() as usize;
    SourceSpan {
        start,
        end: start + part.len(),
        text: part.into(),
    }
}

fn role(roles: &mut Roles, key: &str, span: SourceSpan) {
    roles.entry(key.into()).or_default().push(span);
}

fn label(raw: &str, text: &str) -> Option<SourceSpan> {
    let text = text.trim();
    let text = if let Some(inner) = text.strip_prefix('「') {
        inner.strip_suffix('」')?.trim()
    } else {
        text
    };
    if text.is_empty()
        || text.len() > 192
        || text.chars().any(char::is_control)
        || text.contains([
            '「', '」', '『', '』', '"', '。', '！', '？', '!', '?', ':', '：', '→',
        ])
        || [
            "してください",
            "して下さい",
            "実行して",
            "送信して",
            "保存して",
            "公開して",
            "削除して",
        ]
        .into_iter()
        .any(|action| text.contains(action))
        || [
            "して",
            "しろ",
            "せよ",
            "送って",
            "調べて",
            "教えて",
            "読んで",
        ]
        .into_iter()
        .any(|ending| text.ends_with(ending))
    {
        return None;
    }
    Some(original_span(raw, text))
}

fn number(text: &str) -> Option<u16> {
    if text
        .chars()
        .all(|character| character.is_ascii_digit() || ('０'..='９').contains(&character))
    {
        return text.chars().try_fold(0u16, |value, character| {
            let digit = if character.is_ascii_digit() {
                u16::try_from(u32::from(character) - u32::from('0')).ok()?
            } else {
                u16::try_from(u32::from(character) - u32::from('０')).ok()?
            };
            value.checked_mul(10)?.checked_add(digit)
        });
    }
    if matches!(text, "零" | "〇") {
        return Some(0);
    }
    let mut value = 0u16;
    let mut last_unit = 10_000;
    let mut digit = None;
    for character in text.chars() {
        if let Some(next) = "一二三四五六七八九"
            .chars()
            .position(|number| number == character)
        {
            if digit.replace(u16::try_from(next).ok()? + 1).is_some() {
                return None;
            }
            continue;
        }
        let unit = match character {
            '十' => 10,
            '百' => 100,
            '千' => 1_000,
            _ => return None,
        };
        if unit >= last_unit {
            return None;
        }
        value += digit.take().unwrap_or(1) * unit;
        last_unit = unit;
    }
    Some(value + digit.unwrap_or(0))
}

fn minutes(text: &str) -> Option<u16> {
    let value = if let Some((hours, remaining)) = text.split_once("時間") {
        let base = number(hours.trim())?.checked_mul(60)?;
        let remaining = remaining.trim();
        if remaining.is_empty() {
            base
        } else {
            base.checked_add(number(remaining.strip_suffix('分')?.trim())?)?
        }
    } else {
        number(text.strip_suffix('分')?.trim())?
    };
    (value > 0).then_some(value)
}

fn people(text: &str) -> Option<u8> {
    crew_count(text.strip_suffix('人')?)
}

fn crew_count(numeral: &str) -> Option<u8> {
    let count = u8::try_from(number(numeral.trim())?).ok()?;
    (1..=8).contains(&count).then_some(count)
}

pub(crate) fn personnel_answer(text: &str) -> Option<(SourceSpan, u8)> {
    if text.len() > 128 || text.chars().any(char::is_control) {
        return None;
    }
    let body = text.trim();
    let body = body.strip_suffix(['。', '.', '！', '!']).unwrap_or(body);
    let crew = GRAMMAR
        .personnel_answer
        .captures(body.trim_end())?
        .name("crew")?;
    let count = match crew.as_str() {
        "ひとり" => 1,
        "ふたり" => 2,
        number => crew_count(number.strip_suffix(['人', '名'])?)?,
    };
    let offset = text.len() - text.trim_start().len();
    Some((
        SourceSpan {
            start: offset + crew.start(),
            end: offset + crew.end(),
            text: crew.as_str().into(),
        },
        count,
    ))
}

fn set_once<T: Copy + Eq>(slot: &mut Option<T>, current: T) -> Option<()> {
    if slot.is_some_and(|prior| prior != current) {
        return None;
    }
    *slot = Some(current);
    Some(())
}

fn purpose(raw: &str, head: &str) -> Option<Option<SourceSpan>> {
    let head = head.strip_suffix("ください").unwrap_or(head).trim_end();
    for command in PLAN_HEADERS {
        let Some(prefix) = head.strip_suffix(command) else {
            continue;
        };
        if prefix.is_empty() {
            return Some(None);
        }
        if let Some(goal) = prefix.trim_end().strip_suffix('の') {
            return label(raw, goal).map(Some);
        }
    }
    None
}

#[derive(Default)]
struct Parts<'a> {
    tasks: Vec<WorkPlanTask>,
    references: Vec<(&'a str, &'a str)>,
    personnel: Option<u8>,
    deadline: Option<u16>,
    parallel: Option<bool>,
    roles: Roles,
}

fn task_label(raw: &str, value: &str, tasks: &[WorkPlanTask]) -> Option<SourceSpan> {
    let exact = label(raw, value)?;
    if tasks.iter().any(|task| task.name.text == exact.text) {
        return Some(exact);
    }
    let common_suffix = ["を始めて", "を行って", "を始める", "を行う", "を開始する"]
        .into_iter()
        .find_map(|suffix| value.strip_suffix(suffix))?;
    let stripped = label(raw, common_suffix)?;
    tasks
        .iter()
        .any(|task| task.name.text == stripped.text)
        .then_some(stripped)
}

fn dependency_labels(raw: &str, value: &str, tasks: &[WorkPlanTask]) -> Option<Vec<SourceSpan>> {
    if let Some(name) = task_label(raw, value, tasks) {
        return Some(vec![name]);
    }
    value
        .split('と')
        .map(|part| task_label(raw, part, tasks))
        .collect()
}

impl<'a> Parts<'a> {
    fn task(&mut self, raw: &str, clause: &str) -> Option<()> {
        let group = GRAMMAR.task.captures(clause)?;
        let name = label(raw, group.name("name")?.as_str())?;
        let duration = group.name("duration")?.as_str();
        let workers = group.name("workers").or_else(|| group.name("needed"));
        let workers_count = match workers {
            Some(value) => people(value.as_str())?,
            None => 1,
        };
        if self.tasks.len() >= 10 || self.tasks.iter().any(|prior| prior.name.text == name.text) {
            return None;
        }
        let index = self.tasks.len();
        let duration_minutes = minutes(duration)?;
        role(
            &mut self.roles,
            &format!("/PlanWork/tasks/{index}/name"),
            name.clone(),
        );
        role(
            &mut self.roles,
            &format!("/PlanWork/tasks/{index}/duration_minutes"),
            original_span(raw, duration),
        );
        if let Some(workers) = workers {
            role(
                &mut self.roles,
                &format!("/PlanWork/tasks/{index}/workers"),
                original_span(raw, workers.as_str()),
            );
        }
        self.tasks.push(WorkPlanTask {
            name,
            duration_minutes,
            workers: workers_count,
        });
        Some(())
    }

    fn deadline(&mut self, raw: &str, clause: &str) -> Option<()> {
        let group = GRAMMAR.deadline.captures(clause)?;
        let value = group
            .name("labelled")
            .or_else(|| group.name("finishing"))?
            .as_str();
        set_once(&mut self.deadline, minutes(value)?)?;
        role(
            &mut self.roles,
            "/PlanWork/deadline_minutes",
            original_span(raw, value),
        );
        Some(())
    }

    fn constraint(&mut self, raw: &str, clause: &'a str) -> Option<()> {
        if let Some(group) = GRAMMAR.personnel.captures(clause) {
            let value = group
                .name("labelled")
                .or_else(|| group.name("workers"))?
                .as_str();
            set_once(&mut self.personnel, people(value)?)?;
            role(
                &mut self.roles,
                "/PlanWork/personnel",
                original_span(raw, value),
            );
            let rest = group
                .name("rest")
                .map_or("", |capture| capture.as_str().trim());
            return if rest.is_empty() {
                Some(())
            } else {
                self.deadline(raw, rest)
            };
        }
        if GRAMMAR.deadline.is_match(clause) {
            return self.deadline(raw, clause);
        }
        let parallel = match clause {
            "並行可"
            | "同時作業可"
            | "並行してよい"
            | "並行してもよい"
            | "並行できる作業は並行して"
            | "独立した作業は並行して" => Some(true),
            "順番に"
            | "順番に進めて"
            | "並行不可"
            | "同時作業なし"
            | "一つずつ"
            | "並行せず"
            | "並行せずに"
            | "並行せず順番に"
            | "並行せず順番に進めて" => Some(false),
            _ => None,
        };
        if let Some(value) = parallel {
            set_once(&mut self.parallel, value)?;
            role(
                &mut self.roles,
                "/PlanWork/allow_parallel",
                original_span(raw, clause),
            );
            return Some(());
        }
        if let Some((before, after)) = ["が終わってから", "の後に", "の後で"]
            .into_iter()
            .find_map(|marker| clause.split_once(marker))
        {
            self.references.push((before, after));
            return Some(());
        }
        if let Some((after, before)) = ["の前に", "より先に"]
            .into_iter()
            .find_map(|marker| clause.split_once(marker))
        {
            self.references.push((before, after));
            return Some(());
        }
        if clause.contains('→') {
            let names: Vec<_> = clause.split('→').collect();
            self.references
                .extend(names.windows(2).map(|pair| (pair[0], pair[1])));
            return Some(());
        }
        self.task(raw, clause)
    }

    fn dependencies(&mut self, raw: &str) -> Option<Vec<WorkPlanDependency>> {
        let mut dependencies: Vec<WorkPlanDependency> = Vec::new();
        for (before, after) in &self.references {
            let before = dependency_labels(raw, before, &self.tasks)?;
            let after = dependency_labels(raw, after, &self.tasks)?;
            for left in &before {
                for right in &after {
                    if dependencies
                        .iter()
                        .any(|edge| edge.before.text == left.text && edge.after.text == right.text)
                    {
                        continue;
                    }
                    let index = dependencies.len();
                    role(
                        &mut self.roles,
                        &format!("/PlanWork/dependencies/{index}/before"),
                        left.clone(),
                    );
                    role(
                        &mut self.roles,
                        &format!("/PlanWork/dependencies/{index}/after"),
                        right.clone(),
                    );
                    dependencies.push(WorkPlanDependency {
                        before: left.clone(),
                        after: right.clone(),
                    });
                }
            }
        }
        Some(dependencies)
    }
}

fn revision(raw: &str, text: &str) -> Option<ParsedWorkPlan> {
    let body = ["前回の", "直前の", "先ほどの", "さっきの", "前の", "その"]
        .into_iter()
        .find_map(|marker| text.strip_prefix(marker))?;
    let clause = ["作業計画", "計画", "段取り"]
        .into_iter()
        .find_map(|name| body.strip_prefix(name))?
        .strip_prefix(['で', 'の', 'は', 'を'])?;
    let anchor = &text[..text.len() - clause.len()];
    let clause = clause
        .trim_start()
        .trim_start_matches(['、', ',', '，'])
        .trim_start();
    let mut roles = BTreeMap::from([(
        "/ReviseWorkPlan/anchor".into(),
        vec![original_span(raw, anchor)],
    )]);
    let (change, key, evidence) = if let Some(group) = GRAMMAR.deadline_edit.captures(clause) {
        let value = group.name("deadline")?.as_str();
        (
            WorkPlanChange::Deadline {
                deadline_minutes: minutes(value)?,
            },
            "/ReviseWorkPlan/deadline_minutes",
            value,
        )
    } else if let Some(group) = GRAMMAR.personnel_edit.captures(clause) {
        let value = group.name("personnel")?.as_str();
        (
            WorkPlanChange::Personnel {
                personnel: people(value)?,
            },
            "/ReviseWorkPlan/personnel",
            value,
        )
    } else if let Some(group) = GRAMMAR.duration_edit.captures(clause) {
        let name = label(raw, group.name("task")?.as_str())?;
        let value = group.name("duration")?.as_str();
        role(&mut roles, "/ReviseWorkPlan/task", name.clone());
        (
            WorkPlanChange::TaskDuration {
                task: name,
                duration_minutes: minutes(value)?,
            },
            "/ReviseWorkPlan/duration_minutes",
            value,
        )
    } else {
        return None;
    };
    role(&mut roles, key, original_span(raw, evidence));
    Some(ParsedWorkPlan {
        version: REVISION_PARSER_VERSION,
        intent: Intent::ReviseWorkPlan { change },
        roles,
    })
}

fn initial(raw: &str, text: &str) -> Option<ParsedWorkPlan> {
    let mut clauses = plan_clauses(text);
    let goal = purpose(raw, clauses.next()?)?;
    let mut parts = Parts::default();
    for clause in clauses {
        parts.constraint(raw, clause)?;
    }
    finish_plan(raw, parts, goal, PARSER_VERSION)
}

fn plan_clauses(text: &str) -> impl DoubleEndedIterator<Item = &str> {
    text.split([
        '。', '、', ',', '，', '；', ';', '！', '!', '？', '?', '\n', '\r',
    ])
    .map(str::trim)
    .filter(|clause| !clause.is_empty())
}

fn finish_plan(
    raw: &str,
    mut parts: Parts<'_>,
    purpose: Option<SourceSpan>,
    version: &'static str,
) -> Option<ParsedWorkPlan> {
    if parts.tasks.is_empty() {
        return None;
    }
    if let Some(purpose) = &purpose {
        role(&mut parts.roles, "/PlanWork/purpose", purpose.clone());
    }
    let dependencies = parts.dependencies(raw)?;
    let draft = WorkPlanWithoutPersonnel {
        purpose,
        tasks: parts.tasks,
        dependencies,
        deadline_minutes: parts.deadline?,
        allow_parallel: parts.parallel.unwrap_or(true),
    };
    if let Some(personnel) = parts.personnel {
        return Some(ParsedWorkPlan {
            version,
            intent: Intent::PlanWork {
                request: draft.with_personnel(personnel),
            },
            roles: parts.roles,
        });
    }
    if draft.tasks.iter().any(|task| {
        matches!(
            task.name.text.as_str(),
            "人数" | "人手" | "作業者" | "作業人数" | "作業者数" | "人数の上限" | "全員"
        )
    }) {
        // A crew clause with the wrong unit can otherwise look like a timed task.
        return None;
    }
    Some(ParsedWorkPlan {
        version: PERSONNEL_HEARING_PARSER_VERSION,
        intent: Intent::ClarifyWorkPlanPersonnel { draft },
        roles: parts
            .roles
            .into_iter()
            .map(|(key, spans)| {
                let field = key.strip_prefix("/PlanWork/").expect("initial plan role");
                (format!("/ClarifyWorkPlanPersonnel/draft/{field}"), spans)
            })
            .collect(),
    })
}

fn consultation_question(raw: &str, clause: &str) -> bool {
    (clause == "間に合う" && raw.trim_end().ends_with(['？', '?']))
        || matches!(
            clause,
            "何から手をつければいい"
                | "何から手をつければいいですか"
                | "何から始めればいい"
                | "何から始めればいいですか"
                | "どの順番で進めればいい"
                | "どの順番で進めればいいですか"
                | "どう進めればいい"
                | "どう進めればいいですか"
                | "間に合いますか"
        )
}

fn consultation_constraint<'a>(parts: &mut Parts<'a>, raw: &str, clause: &'a str) -> Option<()> {
    let mut remaining = clause;
    let mut personnel = None;
    if let Some(rest) = GRAMMAR
        .personnel
        .captures(clause)
        .and_then(|group| group.name("rest"))
    {
        personnel = Some(clause[..rest.start()].trim_end());
        remaining = rest.as_str().trim_start();
    }
    if let Some(time) = GRAMMAR
        .remaining
        .captures(remaining)
        .and_then(|group| group.name("limit"))
    {
        let time = time.as_str();
        if let Some(personnel) = personnel {
            parts.constraint(raw, personnel)?;
        }
        set_once(&mut parts.deadline, minutes(time)?)?;
        role(
            &mut parts.roles,
            "/PlanWork/deadline_minutes",
            original_span(raw, time),
        );
        return Some(());
    }

    let sequence = clause
        .split_once("が終わったら")
        .map(|(before, after)| (before, after.trim()))
        .or_else(|| {
            let (after, before) = clause.split_once('は')?;
            let before = ["が終わってから", "をしてから", "してから"]
                .into_iter()
                .find_map(|ending| {
                    let before = before.strip_suffix("です").unwrap_or(before).trim_end();
                    before.strip_suffix(ending)
                })?;
            Some((before, after))
        });
    if let Some((before, after)) = sequence {
        let after = [
            "をします",
            "をする",
            "を行います",
            "を始めます",
            "に進みます",
        ]
        .into_iter()
        .find_map(|ending| after.strip_suffix(ending))
        .unwrap_or(after);
        parts.references.push((before, after));
        return Some(());
    }

    let clause = ["かかりそうです", "かかりそう", "かかりますね"]
        .into_iter()
        .find_map(|ending| clause.strip_suffix(ending))
        .map_or(clause, str::trim_end);
    parts.constraint(raw, clause)
}

fn consultation(raw: &str, text: &str) -> Option<ParsedWorkPlan> {
    let mut clauses = plan_clauses(text);
    let last = clauses.next_back()?;
    let (command, tail_constraint) = if consultation_question(raw, last) {
        (last, None)
    } else {
        let last = last.strip_suffix("ください").unwrap_or(last).trim_end();
        let (command, prefix) = PLAN_HEADERS.iter().find_map(|command| {
            last.strip_suffix(command)
                .map(|prefix| (*command, prefix.trim_end()))
        })?;
        let authored_command = &last[last.len() - command.len()..];
        let tail = (!matches!(prefix, "" | "この条件で" | "これで")).then_some(prefix);
        (authored_command, tail)
    };
    let mut parts = Parts::default();
    let mut goal = None;
    for (index, clause) in clauses.chain(tail_constraint).enumerate() {
        if index == 0
            && let Some(purpose) = clause.strip_suffix("です")
            && ["準備", "相談", "作業"]
                .iter()
                .any(|suffix| purpose.ends_with(suffix))
            && !purpose.contains(['は', 'が', 'を'])
            && ![
                "必要", "禁止", "以内", "締切", "制限", "人数", "時間", "並行", "同時",
            ]
            .iter()
            .any(|condition| purpose.contains(condition))
            && !purpose
                .chars()
                .any(|ch| ch.is_ascii_digit() || ('０'..='９').contains(&ch))
        {
            goal = Some(label(raw, purpose)?);
            continue;
        }
        consultation_constraint(&mut parts, raw, clause)?;
    }
    role(
        &mut parts.roles,
        "/PlanWork/request",
        original_span(raw, command),
    );
    finish_plan(raw, parts, goal, CONSULTATION_PARSER_VERSION)
}

fn consultation_revision(raw: &str, text: &str) -> Option<ParsedWorkPlan> {
    let mut clauses = plan_clauses(text);
    let first = clauses.next()?;
    let anchor = ["前回の", "直前の", "先ほどの", "さっきの", "前の", "その"]
        .into_iter()
        .find_map(|prefix| first.strip_prefix(prefix))
        .is_some_and(|name| matches!(name, "作業計画" | "計画" | "段取り" | "予定"));
    let clause = if anchor { clauses.next()? } else { first };
    let question = clauses.next();
    if clauses.next().is_some()
        || question.is_some_and(|question| !consultation_question(raw, question))
        || (!anchor && question.is_none())
    {
        return None;
    }

    let mut parts = Parts::default();
    consultation_constraint(&mut parts, raw, clause)?;
    let (change, mut roles) = if let Some(limit) = parts.deadline
        && parts.personnel.is_none()
        && parts.tasks.is_empty()
    {
        let evidence = parts.roles.remove("/PlanWork/deadline_minutes")?;
        (
            WorkPlanChange::Deadline {
                deadline_minutes: limit,
            },
            BTreeMap::from([("/ReviseWorkPlan/deadline_minutes".into(), evidence)]),
        )
    } else if anchor
        && let [task] = parts.tasks.as_slice()
        && !parts.roles.contains_key("/PlanWork/tasks/0/workers")
    {
        let evidence = parts.roles.remove("/PlanWork/tasks/0/duration_minutes")?;
        (
            WorkPlanChange::TaskDuration {
                task: task.name.clone(),
                duration_minutes: task.duration_minutes,
            },
            BTreeMap::from([
                ("/ReviseWorkPlan/duration_minutes".into(), evidence),
                ("/ReviseWorkPlan/task".into(), vec![task.name.clone()]),
            ]),
        )
    } else {
        return None;
    };
    if anchor {
        role(
            &mut roles,
            "/ReviseWorkPlan/anchor",
            original_span(raw, first),
        );
    }
    if let Some(question) = question {
        role(
            &mut roles,
            "/ReviseWorkPlan/request",
            original_span(raw, question),
        );
    }
    Some(ParsedWorkPlan {
        version: CONSULTATION_REVISION_PARSER_VERSION,
        intent: Intent::ReviseWorkPlan { change },
        roles,
    })
}

pub(crate) fn parse(raw: &str) -> Result<Option<ParsedWorkPlan>> {
    let text = terminal_trim(raw);
    if text.len() > 8192 {
        return Ok(None);
    }
    let structured = if text.contains("計画") || text.contains("段取り") {
        revision(raw, text).or_else(|| initial(raw, text))
    } else {
        None
    };
    Ok(structured
        .or_else(|| consultation_revision(raw, text))
        .or_else(|| consultation(raw, text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONSULTATION: &str = "発送準備です。梱包は30分、宛名確認は15分、出荷点検は10分。梱包と宛名確認が終わったら出荷点検をします。50分以内に終える段取りを考えて。";
    const EXPLICIT: &str = "発送準備の作業計画を立てて。梱包は30分。宛名確認は15分。出荷点検は10分。梱包と宛名確認が終わってから出荷点検。期限は50分。";

    #[test]
    fn work_plan_personnel_gap_preserves_exact_known_constraints() {
        for (raw, complete, old_version) in [
            (EXPLICIT, format!("{EXPLICIT}人数は二人。"), PARSER_VERSION),
            (
                CONSULTATION,
                CONSULTATION.replace("50分以内", "2人で50分以内"),
                CONSULTATION_PARSER_VERSION,
            ),
        ] {
            let pending = parse(raw).unwrap().unwrap();
            assert_eq!(pending.version, PERSONNEL_HEARING_PARSER_VERSION);
            assert!(version_matches(pending.version, &pending.intent));
            assert!(!version_matches(old_version, &pending.intent));
            let Intent::ClarifyWorkPlanPersonnel { draft } = pending.intent else {
                panic!("expected only the crew size to be missing")
            };
            assert_eq!(draft.deadline_minutes, 50);
            assert_eq!(draft.tasks.len(), 3);
            assert_eq!(draft.dependencies.len(), 2);
            assert!(draft.allow_parallel);
            assert!(
                pending
                    .roles
                    .contains_key("/ClarifyWorkPlanPersonnel/draft/deadline_minutes")
            );
            for (key, spans) in pending.roles {
                assert!(key.starts_with("/ClarifyWorkPlanPersonnel/draft/"));
                assert!(!key.ends_with("/personnel"));
                for span in spans {
                    assert_eq!(&raw[span.start..span.end], span.text);
                }
            }
            let complete = parse(&complete).unwrap().unwrap();
            assert_eq!(complete.version, old_version);
            assert!(!version_matches(
                PERSONNEL_HEARING_PARSER_VERSION,
                &complete.intent
            ));
            assert!(
                complete
                    .roles
                    .keys()
                    .all(|key| key.starts_with("/PlanWork/"))
            );
            let Intent::PlanWork { request } = complete.intent else {
                panic!("a supplied crew size must use the ordinary completed plan parser")
            };
            assert_eq!(request.personnel, 2);
            assert_eq!(request.deadline_minutes, draft.deadline_minutes);
        }
    }

    #[test]
    fn personnel_gap_does_not_mask_other_missing_or_malformed_constraints() {
        for raw in [
            CONSULTATION.replace("50分以内に終える", ""),
            CONSULTATION.replace("梱包は30分、", ""),
            CONSULTATION.replace("梱包と宛名確認が終わったら", "見積が終わったら"),
            CONSULTATION.replace("50分以内", "9人で50分以内"),
            CONSULTATION.replace("50分以内", "人数は2人。人数は3人。50分以内"),
            CONSULTATION.replace("50分以内", "人数は2分。50分以内"),
        ] {
            assert!(parse(&raw).unwrap().is_none(), "{raw}");
        }
    }

    #[test]
    fn personnel_answers_are_whole_utterances_with_exact_numeral_spans() {
        for (raw, text, count) in [
            ("2人", "2人", 2),
            ("　はい、２人です。　", "２人", 2),
            ("作業者は八名です", "八名", 8),
            ("人数は二人です", "二人", 2),
            ("ふたりでお願いします", "ふたり", 2),
            ("１ 人で大丈夫です", "１ 人", 1),
        ] {
            let (span, actual) = personnel_answer(raw).unwrap_or_else(|| panic!("{raw}"));
            assert_eq!(span.text, text);
            assert_eq!(&raw[span.start..span.end], text);
            assert_eq!(actual, count);
        }
        for raw in [
            "0人",
            "9人",
            "二人か三人",
            "２人ですか？",
            "2人？",
            "2人で。期限は30分。",
            "はいいいえ",
        ] {
            assert!(personnel_answer(raw).is_none(), "{raw}");
        }
    }
}
