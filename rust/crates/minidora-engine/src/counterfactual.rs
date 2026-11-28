//! Read-only comparison of ordinary and hypothetical relation proofs.
use crate::core::{self, CounterfactualRelationReport, CounterfactualRelationStatus};
use crate::relations::{self, QueryOutcome};
use crate::scheduler::{GoalNode, GoalNodeKind};
use crate::store::Store;
use minidora_domain::{
    CaseRevision, ConditionStatus, GoalContract, Guarantee, InputBundle, Intent, Lifecycle,
    MemoryState, Proposition, RelationRule, Result, ResultRecord, canonical, digest, failure,
    identity,
};
use serde::Serialize;
use std::collections::BTreeSet;

pub(crate) const PREDICATE: &str = "COUNTERFACTUAL_RELATION_PROOF_REQUIRED";
const MAX_ACTIONS: u32 = 4096;
const MAX_VISIBLE_RULES: usize = 4;
const KIND: &str = "minidora.core.counterfactual-relation.v1";
const RECOVERY_CODE: &str = "COUNTERFACTUAL_RECOVERY_MISMATCH";

#[derive(Debug, Serialize)]
pub(crate) struct CounterfactualProof {
    kind: &'static str,
    source_bundle: String,
    memory_sha256: String,
    assumptions: Vec<Proposition>,
    replace_opposites: bool,
    actual: QueryOutcome,
    pub(crate) scenario: CounterfactualRelationReport,
    rules: Vec<RelationRule>,
}

pub(crate) fn available_actions(max_actions: u32, required_actions: usize) -> u32 {
    max_actions
        .saturating_sub(u32::try_from(required_actions).unwrap_or(u32::MAX))
        .min(MAX_ACTIONS)
}

fn budget_exhausted() -> minidora_domain::Error {
    failure(
        "COUNTERFACTUAL_BUDGET",
        "現在の実行予算では、保存された前提と仮定の両方を照合できません。",
    )
}

pub(crate) fn compute(
    bundle: &InputBundle,
    memory: MemoryState,
    available_actions: u32,
) -> Result<CounterfactualProof> {
    let Intent::AskCounterfactualRelation {
        assumptions,
        proposition,
        replace_opposites,
    } = &bundle.meaning.intent
    else {
        return Err(failure(
            "COUNTERFACTUAL_INPUT_REQUIRED",
            "照合する仮定と関係がありません。",
        ));
    };

    // Both evaluators inspect the given snapshot. Reject oversized snapshots
    // before canonicalizing or iterating records, even if no rule can fire.
    let record_actions = u32::try_from(
        memory
            .facts
            .len()
            .saturating_add(memory.rules.len())
            .saturating_add(memory.conditions.len()),
    )
    .map_err(|_| budget_exhausted())?;
    let override_scan_actions = if *replace_opposites {
        u32::try_from(memory.facts.len()).map_err(|_| budget_exhausted())?
    } else {
        0
    };
    let minimum_scenario_actions = record_actions
        .saturating_add(u32::try_from(assumptions.len()).unwrap_or(u32::MAX))
        .saturating_add(1);
    let remaining = available_actions
        .min(MAX_ACTIONS)
        .checked_sub(record_actions.saturating_add(override_scan_actions))
        .filter(|remaining| *remaining >= minimum_scenario_actions)
        .ok_or_else(budget_exhausted)?;

    let superseded_fact_ids = if *replace_opposites {
        exact_direct_opposites(&memory, assumptions)
    } else {
        Vec::new()
    };
    let minimum_scenario_actions = minimum_scenario_actions
        .saturating_add(u32::try_from(superseded_fact_ids.len()).unwrap_or(u32::MAX));
    let actual_actions = remaining
        .checked_sub(minimum_scenario_actions)
        .ok_or_else(budget_exhausted)?;
    // query requires a positive limit even if the actual relation can be
    // answered without any inference steps.
    let actual = relations::query(&memory, proposition, actual_actions.max(1))?;
    let scenario_actions = remaining
        .checked_sub(actual.steps)
        .filter(|actions| *actions >= minimum_scenario_actions)
        .ok_or_else(budget_exhausted)?;
    let memory_sha256 = digest(&canonical(&memory)?);
    let mut rules = memory.rules.clone();
    let request = &bundle.request;
    let scenario = core::evaluate_counterfactual_in_memory(
        &request.actor_id,
        &request.case_id,
        || Ok(memory),
        assumptions,
        &superseded_fact_ids,
        proposition,
        scenario_actions,
    )?;
    let selected_rules: BTreeSet<_> = actual
        .used_rules
        .iter()
        .chain(&scenario.dependencies)
        .map(String::as_str)
        .collect();
    rules.retain(|rule| selected_rules.contains(rule.id.as_str()));
    Ok(CounterfactualProof {
        kind: KIND,
        source_bundle: bundle.fingerprint.clone(),
        memory_sha256,
        assumptions: assumptions.clone(),
        replace_opposites: *replace_opposites,
        actual,
        scenario,
        rules,
    })
}

fn exact_direct_opposites(memory: &MemoryState, assumptions: &[Proposition]) -> Vec<String> {
    let opposites: BTreeSet<_> = assumptions.iter().map(relations::opposition).collect();
    memory
        .facts
        .iter()
        .filter(|fact| {
            fact.valid
                && fact.guarantee == Guarantee::UserPremise
                && fact.dependencies.is_empty()
                && !fact.source_input.is_empty()
                && opposites.contains(&fact.proposition)
        })
        .map(|fact| fact.id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

impl CounterfactualProof {
    pub(crate) fn units(&self) -> Vec<String> {
        use CounterfactualRelationStatus::{Conflict, Contradicted, Supported, Unknown};
        let actual = match self.actual.status.as_str() {
            "SUPPORTED" => Supported,
            "CONTRADICTED" => Contradicted,
            "CONFLICT" => Conflict,
            _ => Unknown,
        };
        let target = &self.scenario.target;
        let assumptions = self
            .assumptions
            .iter()
            .map(|assumption| format!("「{}」", core::describe(assumption)))
            .collect::<Vec<_>>()
            .join("、");
        let mut units = vec![
            format!(
                "検討する関係は「{}」です。適用範囲は「{}」です。",
                core::describe(target),
                target.scope
            ),
            format!(
                "保存済みの前提では、{}",
                finding(actual, self.actual.condition_status, target)
            ),
            format!(
                "この検討だけで{assumptions}と仮定すると、{}",
                finding(self.scenario.status, self.scenario.condition_status, target)
            ),
        ];
        let changed = if actual == self.scenario.status {
            "結論の判定は同じです。"
        } else {
            "仮定を加えた場合、結論の判定が変わります。"
        };
        units.push(changed.into());
        if self.replace_opposites {
            let removed = self.scenario.superseded_fact_ids.len();
            let replacement = if removed == 0 {
                "仮定に置き換えられる登録済みの反対前提はありませんでした。".into()
            } else {
                format!("仮定の中で、反対の登録済み前提{removed}件を置き換えました。")
            };
            units.push(replacement);
        }
        if !self.rules.is_empty() {
            let rules = self
                .rules
                .iter()
                .take(MAX_VISIBLE_RULES)
                .map(|rule| {
                    let premises = std::iter::once(&rule.premise)
                        .chain(&rule.additional_premises)
                        .map(core::describe)
                        .collect::<Vec<_>>()
                        .join("、かつ");
                    format!("「{premises}」なら「{}」", core::describe(&rule.conclusion))
                })
                .collect::<Vec<_>>()
                .join("。");
            let additional = self.rules.len().saturating_sub(MAX_VISIBLE_RULES);
            let suffix = if additional > 0 {
                format!("。ほか{additional}件")
            } else {
                String::new()
            };
            units.push(format!("判定に使った規則は、{rules}{suffix}です。"));
        }
        units.push("仮定による変更は会話の記憶に登録されません。".into());
        units
    }
}

fn finding(
    status: CounterfactualRelationStatus,
    condition_status: Option<ConditionStatus>,
    target: &Proposition,
) -> String {
    use CounterfactualRelationStatus::{Conflict, Contradicted, Supported, Unknown};
    match status {
        Supported => format!("「{}」を支持します。", core::describe(target)),
        Contradicted => format!(
            "反対の「{}」を支持します。",
            core::describe(&relations::opposition(target))
        ),
        Conflict => "相反する根拠があり、結論を確定できません。".into(),
        Unknown => match condition_status {
            Some(ConditionStatus::False) => "必要な条件が成立せず、判定の対象になりません。".into(),
            Some(ConditionStatus::Conflict) => {
                "必要な条件に相反する証拠があり、判定できません。".into()
            }
            Some(ConditionStatus::Unknown) => "必要な条件を確認できず、判定できません。".into(),
            _ => "この関係を判定できる根拠が足りません。".into(),
        },
    }
}

pub(crate) fn verify_committed(
    store: &Store,
    state: &CaseRevision,
    result: &ResultRecord,
) -> Result<()> {
    let Some(bundle) = state.bundle.as_ref().filter(|bundle| {
        matches!(
            bundle.meaning.intent,
            Intent::AskCounterfactualRelation { .. }
        )
    }) else {
        return Ok(());
    };
    let mismatch = || {
        failure(
            RECOVERY_CODE,
            "保存した仮定の回答を元の依頼と照合できません。",
        )
    };
    let actor = &state.actor_id;
    let case = &state.case_id;
    let request = &bundle.request;
    let [proof_ref] = result.dependencies.as_slice() else {
        return Err(mismatch());
    };
    if request.actor_id != *actor
        || request.case_id != *case
        || result.id != identity(request)?
        || result.input_id != request.input_id
        || result.input_version != request.input_version
        || !crate::input::verified_counterfactual_relation_input(bundle)?
        || state.input_fingerprint.as_deref() != Some(result.id.as_str())
        || result.lifecycle != Lifecycle::Commit
        || result.reason.is_some()
        || result.calculation.is_some()
        || !result.receipts.is_empty()
        || !result.evidence.is_empty()
        || store
            .get_input_bundle(actor, case, &result.input_id, result.input_version)?
            .is_none_or(|(_, registered)| registered != *bundle)
        || !store.input_consumed_memory(actor, case, &result.input_id, result.input_version)?
    {
        return Err(mismatch());
    }
    let goal_ref = state.goal_sha256.as_deref().ok_or_else(mismatch)?;
    let goal: GoalContract = serde_json::from_slice(&store.get_bytes(actor, case, goal_ref)?)?;
    if goal.input_fingerprint != result.id
        || goal.intent != bundle.meaning.intent
        || goal.output_path.is_some()
        || request.output_path.is_some()
        || !request.attachments.is_empty()
        || goal.permissions != request.permissions
        || goal.budget != request.budget
        || goal.required_predicates != core::goal_predicates(&bundle.meaning, false, None)
    {
        return Err(mismatch());
    }
    let nodes: Vec<GoalNode> =
        serde_json::from_slice(&store.get_bytes(actor, case, &goal.required_graph_ref)?)?;
    if nodes.as_slice()
        != [
            GoalNode {
                id: "compute".into(),
                kind: GoalNodeKind::CoreResult,
                requires: Vec::new(),
            },
            GoalNode {
                id: "content".into(),
                kind: GoalNodeKind::ContentPlan,
                requires: vec!["compute".into()],
            },
            GoalNode {
                id: "render".into(),
                kind: GoalNodeKind::RenderedArtifact,
                requires: vec!["content".into()],
            },
        ]
    {
        return Err(mismatch());
    }
    store.inherit_memory_context(actor, case, result)?;
    let revision = result.actor_memory_revision.ok_or_else(mismatch)?;
    let memory = match store.load_memory_revision(actor, revision)? {
        Some(memory) => memory,
        None if revision == 0 => MemoryState::default(),
        None => return Err(mismatch()),
    };
    let proof = compute(
        bundle,
        memory,
        available_actions(goal.budget.max_actions, nodes.len()),
    )?;
    let proof_bytes = canonical(&proof)?;
    let units = proof.units();
    let text = units.join("\n");
    let plan = result.content_plan.as_ref().ok_or_else(mismatch)?;
    if store.get_bytes(actor, case, proof_ref)? != proof_bytes
        || proof_bytes.len().saturating_add(text.len()) > goal.budget.max_evidence_bytes
        || proof.scenario.guarantee != result.guarantee
        || proof.scenario.base_memory_revision != revision
        || plan.required_units != units
        || plan.locale != request.locale
        || plan.text != text
        || result
            .render
            .as_ref()
            .is_none_or(|render| !core::validate_goal_render(&goal, plan, render))
    {
        return Err(mismatch());
    }
    crate::document::revalidate_previous_document_render(
        store,
        actor,
        case,
        result,
        Some(proof_ref),
        RECOVERY_CODE,
        "counterfactual relation",
    )?;
    Ok(())
}
