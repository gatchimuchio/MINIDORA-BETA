//! Explicit, source-preserving Japanese input for a finite work plan.
use std::collections::BTreeMap;
use std::sync::LazyLock;

use minidora_domain::{
    Intent, Result, SourceSpan, WorkPlanChange, WorkPlanDependency, WorkPlanRequest, WorkPlanTask,
};
use regex::Regex;

pub(crate) const PARSER_VERSION: &str = "declarative-ja-v247-work-plan";
pub(crate) const REVISION_PARSER_VERSION: &str = "declarative-ja-v248-work-plan-revision";

pub(crate) struct ParsedWorkPlan {
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

pub(crate) fn parser_version(intent: &Intent) -> Option<&'static str> {
    match intent {
        Intent::PlanWork { .. } => Some(PARSER_VERSION),
        Intent::ReviseWorkPlan { .. } => Some(REVISION_PARSER_VERSION),
        _ => None,
    }
}

struct Grammar {
    task: Regex,
    deadline: Regex,
    personnel: Regex,
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
    let count = u8::try_from(number(text.strip_suffix('人')?.trim())?).ok()?;
    (1..=8).contains(&count).then_some(count)
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
        intent: Intent::ReviseWorkPlan { change },
        roles,
    })
}

fn initial(raw: &str, text: &str) -> Option<ParsedWorkPlan> {
    let mut clauses = text
        .split([
            '。', '、', ',', '，', '；', ';', '！', '!', '？', '?', '\n', '\r',
        ])
        .map(str::trim)
        .filter(|clause| !clause.is_empty());
    let goal = purpose(raw, clauses.next()?)?;
    let mut parts = Parts::default();
    if let Some(goal) = &goal {
        role(&mut parts.roles, "/PlanWork/purpose", goal.clone());
    }
    for clause in clauses {
        parts.constraint(raw, clause)?;
    }
    if parts.tasks.is_empty() {
        return None;
    }
    let dependencies = parts.dependencies(raw)?;
    Some(ParsedWorkPlan {
        intent: Intent::PlanWork {
            request: WorkPlanRequest {
                purpose: goal,
                tasks: parts.tasks,
                dependencies,
                personnel: parts.personnel?,
                deadline_minutes: parts.deadline?,
                allow_parallel: parts.parallel.unwrap_or(true),
            },
        },
        roles: parts.roles,
    })
}

pub(crate) fn parse(raw: &str) -> Result<Option<ParsedWorkPlan>> {
    let text = terminal_trim(raw);
    if text.len() > 8192 || (!text.contains("計画") && !text.contains("段取り")) {
        return Ok(None);
    }
    Ok(revision(raw, text).or_else(|| initial(raw, text)))
}
