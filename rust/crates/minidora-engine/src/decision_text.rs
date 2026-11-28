//! Realization of the Core's evaluated option and condition evidence.
use minidora_domain::{
    DecisionChange, DecisionDirection, DecisionRelation, DecisionRequest, DecisionValue, Result,
    identity,
};
use serde::{Deserialize, Serialize};

use crate::decision::{
    DecisionEvaluation, DecisionOutcome, DecisionReuse, PreferenceCheck, PreferenceFinding,
    RuleStatus, property_key,
};
use crate::language::{
    MAX_EXACT_FIELD_GENERATION_BYTES, MAX_EXACT_FIELD_GENERATION_TOKENS, MeaningCompositionFormat,
    MeaningCompositionPurpose, MeaningCompositionStyle, MeaningGenerationRequest,
    MeaningGrammarFragment, MeaningGrammarUnit, MeaningRole,
};

const ARTIFACT_KIND: &str = "minidora.core-computation.option-decision.v1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionPlan {
    pub(crate) source_bundle: String,
    pub(crate) decision: DecisionRequest,
    pub(crate) evaluation: DecisionEvaluation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) revision: Option<DecisionRevisionProof>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionRevisionProof {
    pub(crate) prior_result: String,
    pub(crate) prior_bundle: String,
    pub(crate) prior_compute: String,
    pub(crate) prior_render: String,
    pub(crate) change: DecisionChange,
    pub(crate) reuse: DecisionReuse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) attachment: Option<crate::attached_document::DecisionFieldSource>,
}

impl DecisionPlan {
    pub(crate) fn dependencies(&self) -> Vec<String> {
        let mut refs = vec![self.source_bundle.clone()];
        if let Some(revision) = &self.revision {
            refs.extend([
                revision.prior_bundle.clone(),
                revision.prior_result.clone(),
                revision.prior_compute.clone(),
                revision.prior_render.clone(),
            ]);
        }
        refs
    }

    pub(crate) fn is_conditional(&self) -> bool {
        matches!(&self.evaluation.outcome, DecisionOutcome::Undetermined(_))
    }

    pub(crate) fn grammar(&self, budget: usize) -> Result<MeaningGenerationRequest> {
        let mut units: Vec<MeaningGrammarUnit> = Vec::new();
        let evidence_ids = self.dependencies();
        let mut emit = |role, fragments: Vec<MeaningGrammarFragment>| -> Result<()> {
            let id = format!("decision-{}", units.len());
            let semantic_sha256 = identity(&(ARTIFACT_KIND, self, &id))?;
            let dependencies = units
                .last()
                .map(|unit| unit.id.clone())
                .into_iter()
                .collect();
            units.push(MeaningGrammarUnit {
                id,
                role,
                semantic_sha256,
                evidence_ids: evidence_ids.clone(),
                dependencies,
                fragments,
            });
            Ok(())
        };

        let title = match &self.evaluation.outcome {
            DecisionOutcome::Recommend(index) => vec![
                literal("おすすめは「"),
                exact(self.name(*index)),
                literal("」です。"),
            ],
            DecisionOutcome::Tie(indices) => {
                let mut fragments = vec![literal("同順位の候補は")];
                for (position, index) in indices.iter().enumerate() {
                    if position > 0 {
                        fragments.push(literal("、"));
                    }
                    fragments.extend([literal("「"), exact(self.name(*index)), literal("」")]);
                }
                fragments.push(literal("です。"));
                fragments
            }
            DecisionOutcome::NoEligible => vec![literal("必須条件を満たす候補はありません。")],
            DecisionOutcome::Undetermined(_) => vec![literal("候補を確定できません。")],
        };
        emit(MeaningRole::Opening, title)?;

        let mut policy = vec![literal(if self.revision.is_some() {
            "前の比較に今回の変更を反映しました。"
        } else {
            "提示された情報を前提に比較しました。"
        })];
        if !self.decision.preferences.is_empty() {
            policy.push(literal("優先順は"));
            for (index, preference) in self.decision.preferences.iter().enumerate() {
                if index > 0 {
                    policy.push(literal("、次に"));
                }
                policy.extend([
                    exact(&preference.property.text),
                    literal(match preference.direction {
                        DecisionDirection::Minimize => "が小さいこと",
                        DecisionDirection::Maximize => "が大きいこと",
                    }),
                ]);
            }
            policy.push(literal("です。"));
        }
        emit(MeaningRole::Body, policy)?;

        if let Some(revision) = &self.revision
            && let Some(source) = &revision.attachment
            && let Some(evidence) = source.matches.first()
            && let DecisionChange::Fact { candidate, fact } = &revision.change
        {
            let location = if let Some(page) = evidence.field.page {
                format!("{page}ページ{}行目", evidence.field.line)
            } else {
                format!("{}行目", evidence.field.line)
            };
            emit(
                MeaningRole::Detail,
                vec![
                    literal("添付「"),
                    exact(&source.filename),
                    literal("」の"),
                    exact(&location),
                    literal("にある「"),
                    exact(&candidate.text),
                    literal("」の「"),
                    exact(&fact.property.text),
                    literal("」の記載「"),
                    exact(&evidence.field.value),
                    literal("」を反映しました。"),
                ],
            )?;
        }

        for (index, statuses) in self.evaluation.assessments.iter().enumerate() {
            if !statuses.is_empty() {
                emit(MeaningRole::Detail, self.requirements(index, statuses))?;
            }
        }

        // The full comparison proof remains available. Show the comparisons
        // that explain the top result instead of repeating every pair.
        for finding in self
            .evaluation
            .comparisons
            .iter()
            .filter(|finding| match &self.evaluation.outcome {
                DecisionOutcome::Recommend(index) => {
                    finding.left == *index || finding.right == *index
                }
                DecisionOutcome::Tie(indices) => {
                    indices.contains(&finding.left) && indices.contains(&finding.right)
                }
                DecisionOutcome::Undetermined(indices) => {
                    indices.contains(&finding.left)
                        && indices.contains(&finding.right)
                        && matches!(
                            finding.checks.last(),
                            Some(PreferenceCheck::Unresolved { .. })
                        )
                }
                DecisionOutcome::NoEligible => false,
            })
            .take(3)
        {
            if !finding.checks.is_empty() {
                emit(MeaningRole::Detail, self.comparison(finding))?;
            }
        }
        if self.is_conditional() {
            emit(
                MeaningRole::Limitation,
                vec![literal(
                    "結論に影響する未確認・矛盾・単位の違いを確認できれば、候補を絞れます。",
                )],
            )?;
        }
        Ok(MeaningGenerationRequest {
            purpose: MeaningCompositionPurpose::ShortComposition,
            format: MeaningCompositionFormat::Bullets,
            style: MeaningCompositionStyle::Neutral,
            units,
            max_output_bytes: budget.min(MAX_EXACT_FIELD_GENERATION_BYTES),
            max_tokens: MAX_EXACT_FIELD_GENERATION_TOKENS,
        })
    }

    fn name(&self, index: usize) -> &str {
        &self.decision.candidates[index].name.text
    }

    fn facts<'a>(
        &'a self,
        index: usize,
        property: &str,
    ) -> impl Iterator<Item = &'a DecisionValue> {
        let key = property_key(property);
        self.decision.candidates[index]
            .facts
            .iter()
            .filter(move |fact| property_key(&fact.property.text) == key)
            .filter(|fact| !matches!(&fact.value, DecisionValue::Unknown { .. }))
            .map(|fact| &fact.value)
    }

    fn requirements(&self, index: usize, statuses: &[RuleStatus]) -> Vec<MeaningGrammarFragment> {
        let mut fragments = vec![literal("「"), exact(self.name(index)), literal("」は")];
        if statuses.contains(&RuleStatus::Unmet) {
            fragments.push(literal("必須条件に合わないため候補から外れます。"));
        } else if statuses.iter().all(|status| *status == RuleStatus::Met) {
            fragments.push(literal("必須条件を満たします。"));
        } else {
            fragments.push(literal("必須条件の一部を確認できません。"));
        }
        for (condition, status) in self.decision.requirements.iter().zip(statuses) {
            let relation = match condition.relation {
                DecisionRelation::AtLeast => "以上",
                DecisionRelation::AtMost => "以下",
                DecisionRelation::Greater => "超",
                DecisionRelation::Less => "未満",
                DecisionRelation::Equal => "",
            };
            let expected = format!("{}{relation}", value_text(&condition.expected));
            fragments.extend([
                exact(&condition.property.text),
                literal("（条件 "),
                exact(&expected),
                literal("）は"),
            ]);
            if matches!(status, RuleStatus::Met | RuleStatus::Unmet)
                && let Some(value) = self.facts(index, &condition.property.text).next()
            {
                fragments.extend([exact(&value_text(value)), literal("で")]);
            }
            fragments.push(literal(match status {
                RuleStatus::Met => "条件に合います。",
                RuleStatus::Unmet => "条件に合いません。",
                RuleStatus::Missing => "情報を確認できません。",
                RuleStatus::Conflict => "入力の値が食い違っています。",
                RuleStatus::Incompatible => "同じ尺度で比較できません。",
            }));
        }
        fragments
    }

    fn comparison(&self, finding: &PreferenceFinding) -> Vec<MeaningGrammarFragment> {
        let mut fragments = vec![
            literal("「"),
            exact(self.name(finding.left)),
            literal("」と「"),
            exact(self.name(finding.right)),
            literal("」の比較では、"),
        ];
        for (index, check) in finding.checks.iter().enumerate() {
            let preference = &self.decision.preferences[index];
            fragments.extend([exact(&preference.property.text), literal("は")]);
            match check {
                PreferenceCheck::Unresolved { left, right } => {
                    for (candidate, status) in [(finding.left, left), (finding.right, right)] {
                        if let Some(status) = status {
                            fragments.extend([
                                literal("「"),
                                exact(self.name(candidate)),
                                literal("」で"),
                                literal(match status {
                                    RuleStatus::Missing => "情報がなく、",
                                    RuleStatus::Conflict => "値が食い違い、",
                                    _ => "同じ尺度で比較できず、",
                                }),
                            ]);
                        }
                    }
                    fragments.push(literal("優先順を確定できません。"));
                }
                _ => {
                    let left = self.facts(finding.left, &preference.property.text).next();
                    let right = self.facts(finding.right, &preference.property.text).next();
                    if let (Some(left), Some(right)) = (left, right) {
                        fragments.extend([
                            exact(&value_text(left)),
                            literal("と"),
                            exact(&value_text(right)),
                            literal("で"),
                        ]);
                    }
                    if matches!(check, PreferenceCheck::Equal) {
                        fragments.push(literal("同じです。"));
                    } else {
                        let better = if matches!(check, PreferenceCheck::LeftBetter) {
                            finding.left
                        } else {
                            finding.right
                        };
                        fragments.extend([
                            literal("「"),
                            exact(self.name(better)),
                            literal("」を優先します。"),
                        ]);
                    }
                }
            }
        }
        fragments
    }
}

fn value_text(value: &DecisionValue) -> String {
    match value {
        DecisionValue::Quantity { amount, unit } => format!(
            "{}{}",
            amount.text,
            unit.as_ref().map_or("", |span| span.text.as_str())
        ),
        DecisionValue::Flag { present, .. } => if *present { "あり" } else { "なし" }.into(),
        DecisionValue::Unknown { .. } => "未確認".into(),
    }
}

fn literal(value: &str) -> MeaningGrammarFragment {
    MeaningGrammarFragment::Literal {
        value: value.into(),
    }
}

fn exact(value: &str) -> MeaningGrammarFragment {
    MeaningGrammarFragment::ExactField {
        value: value.into(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GeneratedDecisionArtifact {
    kind: String,
    decision_plan: DecisionPlan,
    pub(crate) content_generation: crate::language::MeaningGenerationReport,
}

impl GeneratedDecisionArtifact {
    pub(crate) fn new(
        decision_plan: DecisionPlan,
        content_generation: crate::language::MeaningGenerationReport,
    ) -> Self {
        Self {
            kind: ARTIFACT_KIND.into(),
            decision_plan,
            content_generation,
        }
    }
}
