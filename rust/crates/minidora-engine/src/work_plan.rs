//! A local, advisory schedule for explicitly supplied work and constraints.
use std::collections::BTreeMap;
use std::time::Instant;

use minidora_domain::{Result, WorkPlanChange, WorkPlanRequest, failure, identity};
use serde::{Deserialize, Serialize};

use crate::language::{
    MAX_EXACT_FIELD_GENERATION_BYTES, MAX_EXACT_FIELD_GENERATION_TOKENS, MeaningCompositionFormat,
    MeaningCompositionPurpose, MeaningCompositionStyle, MeaningGenerationReport,
    MeaningGenerationRequest, MeaningGrammarFragment, MeaningGrammarUnit, MeaningRole,
};
use crate::scheduler::{GoalNode, GoalNodeKind, Scheduler};

pub(crate) const ARTIFACT_KIND: &str = "minidora.core-computation.work-plan.v1";
const MAX_SEARCH_STATES: usize = 40_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkPlanRevisionProof {
    pub(crate) request_bundle: String,
    pub(crate) prior_result: String,
    pub(crate) prior_compute: String,
    pub(crate) prior_render: String,
    pub(crate) change: WorkPlanChange,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WorkPlanMeaningPlan {
    pub(crate) request: WorkPlanRequest,
    pub(crate) source_bundle: String,
    pub(crate) revision: Option<WorkPlanRevisionProof>,
    evaluation: WorkPlanEvaluation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct WorkPlanEvaluation {
    status: DeadlineStatus,
    schedule: Option<Schedule>,
    // A proposal under relaxed constraints, separate from the schedule under current constraints.
    alternative: Option<Alternative>,
    affected_steps: Vec<usize>,
    reused_steps: Vec<usize>,
    changed_steps: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum DeadlineStatus {
    WithinDeadline,
    DeadlineInfeasible,
    DeadlineUnresolved,
    CrewInfeasible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Schedule {
    slots: Vec<Slot>,
    makespan_minutes: u32,
    lower_bound_minutes: u32,
    optimal: bool,
    inspected_states: usize,
    fixed_steps: Vec<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Slot {
    start_minute: u32,
    end_minute: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE", deny_unknown_fields)]
enum Alternative {
    AdditionalCrew { personnel: u8, schedule: Schedule },
    MoveFixedSteps { schedule: Schedule },
}

struct Graph<'a> {
    request: &'a WorkPlanRequest,
    prerequisites: Vec<Vec<usize>>,
    index: BTreeMap<&'a str, usize>,
}

fn label_ok(text: &str) -> bool {
    !text.is_empty()
        && text == text.trim()
        && text.len() <= 240
        && !text.chars().any(char::is_control)
        && !text.contains(['「', '」', '。', '！', '？'])
}

fn invalid_plan() -> minidora_domain::Error {
    failure(
        "WORK_PLAN_INVALID",
        "作業名、所要時間、人員、期限と前後関係を確認してください。",
    )
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "WORK_PLAN_DEADLINE",
            "制限された計算時間内に計画の照合を終えられませんでした。",
        ));
    }
    Ok(())
}

impl<'a> Graph<'a> {
    fn new(request: &'a WorkPlanRequest) -> Result<Self> {
        if !(1..=10).contains(&request.tasks.len())
            || !(1..=8).contains(&request.personnel)
            || request.deadline_minutes == 0
            || request
                .purpose
                .as_ref()
                .is_some_and(|purpose| !label_ok(&purpose.text))
            || request.tasks.iter().any(|task| {
                !label_ok(&task.name.text)
                    || task.duration_minutes == 0
                    || !(1..=8).contains(&task.workers)
            })
        {
            return Err(invalid_plan());
        }
        let index: BTreeMap<_, _> = request
            .tasks
            .iter()
            .enumerate()
            .map(|(i, task)| (task.name.text.as_str(), i))
            .collect();
        if index.len() != request.tasks.len() {
            return Err(failure(
                "WORK_PLAN_DUPLICATE_TASK",
                "同じ作業名が複数あります。",
            ));
        }
        let mut prerequisites = vec![Vec::new(); request.tasks.len()];
        for edge in &request.dependencies {
            let before = *index
                .get(edge.before.text.as_str())
                .ok_or_else(invalid_plan)?;
            let after = *index
                .get(edge.after.text.as_str())
                .ok_or_else(invalid_plan)?;
            prerequisites[after].push(before);
        }
        for dependencies in &mut prerequisites {
            dependencies.sort_unstable();
        }
        // Use the shared validator for unique, complete, acyclic prerequisite graphs.
        // It is never advanced with the proposed schedule: no real work has taken place.
        let nodes = request
            .tasks
            .iter()
            .enumerate()
            .map(|(i, task)| GoalNode {
                id: task.name.text.clone(),
                kind: GoalNodeKind::ComputedValue,
                requires: prerequisites[i]
                    .iter()
                    .map(|p| request.tasks[*p].name.text.clone())
                    .collect(),
            })
            .collect();
        Scheduler::new(nodes, 0).map_err(|_| {
            failure(
                "WORK_PLAN_DEPENDENCIES",
                "作業の前後関係に重複または循環があります。",
            )
        })?;
        Ok(Self {
            request,
            prerequisites,
            index,
        })
    }

    fn affected(&self, root: usize) -> Vec<usize> {
        let mut affected = vec![false; self.request.tasks.len()];
        let mut pending = vec![root];
        while let Some(next) = pending.pop() {
            if std::mem::replace(&mut affected[next], true) {
                continue;
            }
            pending.extend(
                self.prerequisites
                    .iter()
                    .enumerate()
                    .filter_map(|(index, edges)| edges.contains(&next).then_some(index)),
            );
        }
        (0..affected.len())
            .filter(|index| affected[*index])
            .collect()
    }

    fn duration(&self, index: usize) -> u32 {
        u32::from(self.request.tasks[index].duration_minutes)
    }

    fn earliest_bound(&self, scheduled: &[Option<Slot>]) -> u32 {
        let mut ends: Vec<_> = scheduled
            .iter()
            .map(|slot| slot.map(|slot| slot.end_minute))
            .collect();
        while ends.iter().any(Option::is_none) {
            for index in 0..ends.len() {
                if ends[index].is_some() {
                    continue;
                }
                if let Some(release) = self.prerequisites[index]
                    .iter()
                    .try_fold(0, |latest, before| ends[*before].map(|end| latest.max(end)))
                {
                    ends[index] = Some(release + self.duration(index));
                }
            }
        }
        let critical = ends.into_iter().flatten().max().unwrap_or_default();
        let labor: u32 = if self.request.allow_parallel {
            self.request
                .tasks
                .iter()
                .map(|task| u32::from(task.workers) * u32::from(task.duration_minutes))
                .sum::<u32>()
                .div_ceil(u32::from(self.request.personnel))
        } else {
            self.request
                .tasks
                .iter()
                .map(|task| u32::from(task.duration_minutes))
                .sum()
        };
        critical.max(labor)
    }

    fn next_slot(&self, index: usize, scheduled: &[Option<Slot>]) -> Option<Slot> {
        let release = self.prerequisites[index]
            .iter()
            .try_fold(0, |latest, prior| {
                scheduled[*prior].map(|slot| latest.max(slot.end_minute))
            })?;
        let mut starts = vec![release];
        starts.extend(
            scheduled
                .iter()
                .flatten()
                .map(|slot| slot.end_minute)
                .filter(|end| *end > release),
        );
        starts.sort_unstable();
        starts.dedup();
        starts.into_iter().find_map(|start| {
            let end = start + self.duration(index);
            let admissible = std::iter::once(start)
                .chain(
                    scheduled
                        .iter()
                        .flatten()
                        .map(|slot| slot.start_minute)
                        .filter(|at| *at > start && *at < end),
                )
                .all(|at| {
                    let concurrent = scheduled.iter().enumerate().filter_map(|(task, slot)| {
                        slot.filter(|slot| slot.start_minute <= at && at < slot.end_minute)
                            .map(|_| self.request.tasks[task].workers)
                    });
                    if self.request.allow_parallel {
                        let used: u16 = concurrent.map(u16::from).sum();
                        used + u16::from(self.request.tasks[index].workers)
                            <= u16::from(self.request.personnel)
                    } else {
                        concurrent.count() == 0
                    }
                });
            admissible.then_some(Slot {
                start_minute: start,
                end_minute: end,
            })
        })
    }

    fn validate_schedule(&self, schedule: &Schedule, pins: &[Option<Slot>]) -> Result<()> {
        let slots = &schedule.slots;
        if slots.len() != self.request.tasks.len() || pins.len() != slots.len() {
            return Err(failure(
                "WORK_PLAN_SCHEDULE_INVALID",
                "計画案に必要な作業の集合が一致しません。",
            ));
        }
        let prerequisites = slots.iter().enumerate().all(|(i, slot)| {
            slot.end_minute.checked_sub(slot.start_minute) == Some(self.duration(i))
                && pins[i].is_none_or(|fixed| fixed == *slot)
                && self.prerequisites[i]
                    .iter()
                    .all(|before| slots[*before].end_minute <= slot.start_minute)
        });
        let resources = slots.iter().all(|current| {
            let simultaneous = slots.iter().enumerate().filter(|(_, slot)| {
                slot.start_minute <= current.start_minute && current.start_minute < slot.end_minute
            });
            if self.request.allow_parallel {
                simultaneous
                    .map(|(task, _)| u16::from(self.request.tasks[task].workers))
                    .sum::<u16>()
                    <= u16::from(self.request.personnel)
            } else {
                simultaneous.count() <= 1
            }
        });
        if !prerequisites
            || !resources
            || slots.iter().map(|slot| slot.end_minute).max() != Some(schedule.makespan_minutes)
            || schedule.lower_bound_minutes > schedule.makespan_minutes
        {
            return Err(failure(
                "WORK_PLAN_SCHEDULE_INVALID",
                "計画案の作業順序または人員を照合できません。",
            ));
        }
        Ok(())
    }

    fn schedule(&self, pins: &[Option<Slot>], deadline: Instant) -> Result<Schedule> {
        check_deadline(deadline)?;
        let mut greedy = pins.to_owned();
        while greedy.iter().any(Option::is_none) {
            let (index, slot) = (0..greedy.len())
                .filter(|i| greedy[*i].is_none())
                .find_map(|i| self.next_slot(i, &greedy).map(|slot| (i, slot)))
                .ok_or_else(invalid_plan)?;
            greedy[index] = Some(slot);
        }
        let lower_bound = self.earliest_bound(pins);
        let best = greedy.into_iter().flatten().collect::<Vec<_>>();
        let mut search = Search {
            graph: self,
            deadline,
            lower_bound,
            finish: best
                .iter()
                .map(|slot| slot.end_minute)
                .max()
                .ok_or_else(invalid_plan)?,
            best,
            visited: 0,
            truncated: false,
        };
        search.next(&mut pins.to_owned())?;
        let optimal = !search.truncated || search.finish == lower_bound;
        let schedule = Schedule {
            slots: search.best,
            makespan_minutes: search.finish,
            lower_bound_minutes: if optimal { search.finish } else { lower_bound },
            optimal,
            inspected_states: search.visited,
            fixed_steps: pins
                .iter()
                .enumerate()
                .filter_map(|(i, slot)| slot.is_some().then_some(i))
                .collect(),
        };
        self.validate_schedule(&schedule, pins)?;
        Ok(schedule)
    }
}

struct Search<'g, 'r> {
    graph: &'g Graph<'r>,
    deadline: Instant,
    lower_bound: u32,
    finish: u32,
    best: Vec<Slot>,
    visited: usize,
    truncated: bool,
}

impl Search<'_, '_> {
    fn next(&mut self, scheduled: &mut [Option<Slot>]) -> Result<()> {
        if self.finish == self.lower_bound {
            return Ok(());
        }
        if self.visited == MAX_SEARCH_STATES {
            self.truncated = true;
            return Ok(());
        }
        if self.visited.is_multiple_of(128) {
            check_deadline(self.deadline)?;
        }
        self.visited += 1;
        if scheduled.iter().all(Option::is_some) {
            let finish = scheduled
                .iter()
                .flatten()
                .map(|slot| slot.end_minute)
                .max()
                .unwrap_or_default();
            if finish < self.finish {
                self.finish = finish;
                self.best = scheduled.iter().flatten().copied().collect();
            }
            return Ok(());
        }
        for index in 0..scheduled.len() {
            if scheduled[index].is_some() {
                continue;
            }
            if let Some(slot) = self.graph.next_slot(index, scheduled)
                && slot.end_minute < self.finish
            {
                scheduled[index] = Some(slot);
                self.next(scheduled)?;
                scheduled[index] = None;
                if self.finish == self.lower_bound || self.truncated {
                    break;
                }
            }
        }
        Ok(())
    }
}

impl WorkPlanMeaningPlan {
    pub(crate) fn new(
        request: WorkPlanRequest,
        source_bundle: String,
        deadline: Instant,
    ) -> Result<Self> {
        let evaluation = Self::evaluate(&request, None, None, deadline)?;
        Ok(Self {
            request,
            source_bundle,
            revision: None,
            evaluation,
        })
    }

    pub(crate) fn revise(
        prior: &Self,
        proof: WorkPlanRevisionProof,
        deadline: Instant,
    ) -> Result<Self> {
        let mut request = prior.request.clone();
        match &proof.change {
            WorkPlanChange::TaskDuration {
                task,
                duration_minutes,
            } => {
                let changed = request
                    .tasks
                    .iter_mut()
                    .find(|step| step.name.text == task.text)
                    .ok_or_else(|| {
                        failure(
                            "WORK_PLAN_TASK_NOT_FOUND",
                            "時間を変える作業名が元の計画に見つかりません。",
                        )
                    })?;
                changed.duration_minutes = *duration_minutes;
            }
            WorkPlanChange::Personnel { personnel } => request.personnel = *personnel,
            WorkPlanChange::Deadline { deadline_minutes } => {
                request.deadline_minutes = *deadline_minutes
            }
        }
        let evaluation = Self::evaluate(&request, Some(prior), Some(&proof.change), deadline)?;
        Ok(Self {
            request,
            source_bundle: prior.source_bundle.clone(),
            revision: Some(proof),
            evaluation,
        })
    }

    fn evaluate(
        request: &WorkPlanRequest,
        prior: Option<&Self>,
        change: Option<&WorkPlanChange>,
        deadline: Instant,
    ) -> Result<WorkPlanEvaluation> {
        let graph = Graph::new(request)?;
        let prior_schedule = prior.and_then(|plan| plan.evaluation.schedule.as_ref());
        let empty = vec![None; request.tasks.len()];
        let affected_steps: Vec<_> = match change {
            Some(WorkPlanChange::TaskDuration { task, .. }) => {
                graph.affected(graph.index[task.text.as_str()])
            }
            Some(WorkPlanChange::Personnel { .. }) => (0..request.tasks.len()).collect(),
            _ => Vec::new(),
        };
        let required_crew = request
            .tasks
            .iter()
            .map(|step| step.workers)
            .max()
            .ok_or_else(invalid_plan)?;
        let mut alternative = None;
        let schedule = if required_crew > request.personnel {
            let mut more_people = request.clone();
            more_people.personnel = required_crew;
            alternative = Some(Alternative::AdditionalCrew {
                personnel: required_crew,
                schedule: Graph::new(&more_people)?.schedule(&empty, deadline)?,
            });
            None
        } else if let (Some(prior), Some(WorkPlanChange::Deadline { .. })) =
            (prior_schedule, change)
        {
            graph.validate_schedule(prior, &empty)?;
            Some(prior.clone())
        } else {
            let mut pins = empty.clone();
            if let Some(prior) = prior_schedule
                && matches!(change, Some(WorkPlanChange::TaskDuration { .. }))
            {
                for (index, slot) in prior.slots.iter().enumerate() {
                    if !affected_steps.contains(&index) {
                        pins[index] = Some(*slot);
                    }
                }
            }
            Some(graph.schedule(&pins, deadline)?)
        };
        let due = u32::from(request.deadline_minutes);
        if let Some(schedule) = &schedule
            && schedule.makespan_minutes > due
            && !schedule.fixed_steps.is_empty()
        {
            let flexible = graph.schedule(&empty, deadline)?;
            if flexible.makespan_minutes < schedule.makespan_minutes {
                alternative = Some(Alternative::MoveFixedSteps { schedule: flexible });
            }
        }
        let status = match &schedule {
            None => DeadlineStatus::CrewInfeasible,
            Some(schedule) if schedule.makespan_minutes <= due => DeadlineStatus::WithinDeadline,
            Some(schedule) if schedule.lower_bound_minutes > due => {
                DeadlineStatus::DeadlineInfeasible
            }
            _ => DeadlineStatus::DeadlineUnresolved,
        };
        let mut reused_steps = Vec::new();
        let mut changed_steps = Vec::new();
        if let (Some(prior), Some(next)) = (prior_schedule, schedule.as_ref()) {
            for index in 0..next.slots.len() {
                if prior.slots[index] == next.slots[index] {
                    reused_steps.push(index);
                } else {
                    changed_steps.push(index);
                }
            }
        }
        Ok(WorkPlanEvaluation {
            status,
            schedule,
            alternative,
            affected_steps,
            reused_steps,
            changed_steps,
        })
    }

    pub(crate) fn dependencies(&self) -> Vec<String> {
        let mut references = vec![self.source_bundle.clone()];
        if let Some(revision) = &self.revision {
            references.extend([
                revision.request_bundle.clone(),
                revision.prior_result.clone(),
                revision.prior_compute.clone(),
                revision.prior_render.clone(),
            ]);
        }
        references
    }

    fn list(&self, indexes: &[usize]) -> String {
        if indexes.is_empty() {
            return "なし".into();
        }
        indexes
            .iter()
            .map(|i| self.request.tasks[*i].name.text.as_str())
            .collect::<Vec<_>>()
            .join("、")
    }

    fn schedule_lines(&self, schedule: &Schedule) -> Vec<String> {
        let mut order: Vec<_> = (0..schedule.slots.len()).collect();
        order.sort_by_key(|i| (schedule.slots[*i].start_minute, *i));
        order
            .into_iter()
            .map(|i| {
                let task = &self.request.tasks[i];
                let slot = schedule.slots[i];
                let after = self
                    .request
                    .dependencies
                    .iter()
                    .filter(|edge| edge.after.text == task.name.text)
                    .map(|edge| edge.before.text.as_str())
                    .collect::<Vec<_>>();
                let prerequisite = if after.is_empty() {
                    String::new()
                } else {
                    format!("。先行工程は{}", after.join("、"))
                };
                format!(
                    "・{}　{}〜{}分（{}人）{prerequisite}",
                    task.name.text, slot.start_minute, slot.end_minute, task.workers
                )
            })
            .collect()
    }

    fn units(&self) -> Vec<String> {
        let title = self
            .request
            .purpose
            .as_ref()
            .map_or("作業", |purpose| purpose.text.as_str());
        let mut units = vec![
            format!("「{title}」の計画案"),
            format!(
                "開始から{}分以内、同時に{}人まで。{}。作業は計画段階です。",
                self.request.deadline_minutes,
                self.request.personnel,
                if self.request.allow_parallel {
                    "人員の範囲で並行作業可"
                } else {
                    "1工程ずつ実施"
                }
            ),
        ];
        if let Some(revision) = &self.revision {
            units.push(match &revision.change {
                WorkPlanChange::TaskDuration {
                    task,
                    duration_minutes,
                } => format!(
                    "見直した条件は「{}」の所要時間（{}分）です。",
                    task.text, duration_minutes
                ),
                WorkPlanChange::Personnel { personnel } => format!(
                    "同時に作業できる人数を{}人に変更した場合の計画です。",
                    personnel
                ),
                WorkPlanChange::Deadline { deadline_minutes } => {
                    format!("期限を{}分に変更した場合の計画です。", deadline_minutes)
                }
            });
        }
        if let Some(schedule) = &self.evaluation.schedule {
            let finish = schedule.makespan_minutes;
            let due = u32::from(self.request.deadline_minutes);
            units.push(match self.evaluation.status {
                DeadlineStatus::WithinDeadline if finish == due => {
                    format!("この案は{finish}分で終わり、期限ちょうどです。")
                }
                DeadlineStatus::WithinDeadline => format!(
                    "この案は{finish}分で終わり、期限まで{}分あります。",
                    due - finish
                ),
                DeadlineStatus::DeadlineInfeasible => format!(
                    "{}の所要時間は{finish}分です。下限は{}分で、期限の{due}分には収まりません。",
                    if schedule.fixed_steps.is_empty() {
                        "確認した案"
                    } else {
                        "工程を固定した案"
                    },
                    schedule.lower_bound_minutes,
                ),
                DeadlineStatus::DeadlineUnresolved => format!(
                    "見つかった案は{finish}分です。{due}分以内に収まる案の有無は確定していません。"
                ),
                DeadlineStatus::CrewInfeasible => {
                    unreachable!("crew infeasible plan has no primary schedule")
                }
            });
            units.extend(self.schedule_lines(schedule));
            units.push(if schedule.optimal {
                if schedule.fixed_steps.is_empty() {
                    format!("作業順と人員の条件を満たす最短所要時間は{finish}分です。")
                } else {
                    format!(
                        "{}を元の時間に固定した場合の最短所要時間は{finish}分です。",
                        self.list(&schedule.fixed_steps)
                    )
                }
            } else {
                format!(
                    "所要時間の下限は{}分です。探索上限に達したため、最短かどうかは未確定です。",
                    schedule.lower_bound_minutes
                )
            });
        } else {
            let blocked = self
                .request
                .tasks
                .iter()
                .filter(|task| task.workers > self.request.personnel)
                .map(|task| format!("{}（{}人必要）", task.name.text, task.workers))
                .collect::<Vec<_>>();
            units.push(format!(
                "指定の人員では「{}」を含む計画は成立しません。",
                blocked.join("、")
            ));
        }
        if self.revision.is_some() {
            units.push(format!(
                "再検討した工程は{}、時刻を維持した工程は{}、時刻が変わった工程は{}です。",
                self.list(&self.evaluation.affected_steps),
                self.list(&self.evaluation.reused_steps),
                self.list(&self.evaluation.changed_steps),
            ));
        }
        if let Some(alternative) = &self.evaluation.alternative {
            let (label, schedule) = match alternative {
                Alternative::AdditionalCrew {
                    personnel,
                    schedule,
                } => (
                    format!("対案として、{}人を確保した場合", personnel),
                    schedule,
                ),
                Alternative::MoveFixedSteps { schedule } => {
                    ("対案として、固定した工程も動かせる場合".into(), schedule)
                }
            };
            units.push(format!(
                "{label}、{}分の案があります。",
                schedule.makespan_minutes
            ));
            units.extend(self.schedule_lines(schedule));
        }
        units
    }

    pub(crate) fn grammar(&self, budget: usize) -> Result<MeaningGenerationRequest> {
        let fragments = self
            .units()
            .into_iter()
            .enumerate()
            .map(|(i, value)| MeaningGrammarFragment::ExactField {
                value: if i == 0 { value } else { format!("\n{value}") },
            })
            .collect();
        Ok(MeaningGenerationRequest {
            purpose: MeaningCompositionPurpose::ShortComposition,
            format: MeaningCompositionFormat::Paragraph,
            style: MeaningCompositionStyle::Neutral,
            units: vec![MeaningGrammarUnit {
                id: "work-plan".into(),
                role: MeaningRole::Body,
                evidence_ids: self.dependencies(),
                dependencies: Vec::new(),
                semantic_sha256: identity(&(ARTIFACT_KIND, self))?,
                fragments,
            }],
            max_output_bytes: budget.min(MAX_EXACT_FIELD_GENERATION_BYTES),
            max_tokens: MAX_EXACT_FIELD_GENERATION_TOKENS,
        })
    }

    pub(crate) fn verify_surface(&self, text: &str) -> Result<()> {
        if self.units().join("\n") != text {
            return Err(failure(
                "WORK_PLAN_SURFACE",
                "表示された計画案と計算した時刻・制約が一致しません。",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratedWorkPlanArtifact {
    pub(crate) kind: String,
    pub(crate) meaning_plan: WorkPlanMeaningPlan,
    pub(crate) content_generation: MeaningGenerationReport,
}

impl GeneratedWorkPlanArtifact {
    pub(crate) fn new(
        meaning_plan: WorkPlanMeaningPlan,
        content_generation: MeaningGenerationReport,
    ) -> Self {
        Self {
            kind: ARTIFACT_KIND.into(),
            meaning_plan,
            content_generation,
        }
    }
}
