use crate::math;
use minidora_domain::{
    DecisionCandidate, DecisionDirection, DecisionFact, DecisionRelation, DecisionRequest,
    DecisionValue, Result, failure,
};
use num_rational::BigRational;
use serde::{Deserialize, Serialize};
use std::{cmp::Ordering, collections::BTreeMap, time::Instant};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionEvaluation {
    pub outcome: DecisionOutcome,
    pub assessments: Vec<Vec<RuleStatus>>,
    pub comparisons: Vec<PreferenceFinding>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionReuse {
    pub assessments: Vec<[usize; 2]>,
    pub comparisons: Vec<[usize; 2]>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum DecisionOutcome {
    Recommend(usize),
    Tie(Vec<usize>),
    NoEligible,
    Undetermined(Vec<usize>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum RuleStatus {
    Met,
    Unmet,
    Missing,
    Conflict,
    Incompatible,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PreferenceFinding {
    pub left: usize,
    pub right: usize,
    pub checks: Vec<PreferenceCheck>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum PreferenceCheck {
    Equal,
    LeftBetter,
    RightBetter,
    Unresolved {
        left: Option<RuleStatus>,
        right: Option<RuleStatus>,
    },
}

enum ExactValue<'a> {
    Quantity {
        amount: BigRational,
        unit: Option<&'a str>,
    },
    Flag(bool),
}

impl<'a> ExactValue<'a> {
    fn parse(value: &'a DecisionValue) -> Result<Self> {
        match value {
            DecisionValue::Quantity { amount, unit } => Ok(Self::Quantity {
                amount: math::scalar(&amount.text)?,
                unit: unit.as_ref().map(|span| span.text.as_str()),
            }),
            DecisionValue::Flag { present, .. } => Ok(Self::Flag(*present)),
            DecisionValue::Unknown { .. } => Err(failure(
                "DECISION_INPUT_INVALID",
                "未確認の値を必須条件の基準値にできません。",
            )),
        }
    }

    fn compare(&self, right: &Self) -> Option<Ordering> {
        match (self, right) {
            (Self::Flag(left), Self::Flag(right)) => Some(left.cmp(right)),
            (
                Self::Quantity {
                    amount: left,
                    unit: left_unit,
                },
                Self::Quantity {
                    amount: right,
                    unit: right_unit,
                },
            ) if left_unit == right_unit => Some(left.cmp(right)),
            (
                Self::Quantity {
                    amount: left,
                    unit: Some(left_unit),
                },
                Self::Quantity {
                    amount: right,
                    unit: Some(right_unit),
                },
            ) => {
                let (left_scale, right_scale) = compatible_scales(left_unit, right_unit)?;
                Some((left * left_scale).cmp(&(right * right_scale)))
            }
            _ => None,
        }
    }
}

pub(crate) fn compare_known_values(
    left: &DecisionValue,
    right: &DecisionValue,
) -> Result<Option<Ordering>> {
    Ok(ExactValue::parse(left)?.compare(&ExactValue::parse(right)?))
}

fn compatible_scales(left: &str, right: &str) -> Option<(BigRational, BigRational)> {
    fn seconds(unit: &str) -> Option<BigRational> {
        let (numerator, denominator) = match unit {
            "ms" | "ミリ秒" => (1_i64, 1000_i64),
            "s" | "秒" => (1, 1),
            "min" | "分" => (60, 1),
            "h" | "時間" => (3600, 1),
            _ => return None,
        };
        Some(BigRational::new(numerator.into(), denominator.into()))
    }

    fn length(unit: &str) -> Option<BigRational> {
        math::exact_length_scale_to_meters(match unit {
            "キロメートル" => "km",
            "メートル" => "m",
            "センチメートル" => "cm",
            "ミリメートル" => "mm",
            _ => unit,
        })
    }

    let left = property_key(left);
    let right = property_key(right);
    if left == right {
        let one = BigRational::from_integer(1.into());
        Some((one.clone(), one))
    } else if let (Some(left), Some(right)) = (length(&left), length(&right)) {
        Some((left, right))
    } else {
        Some((seconds(&left)?, seconds(&right)?))
    }
}

enum Observation<'a> {
    Value(ExactValue<'a>),
    Conflict,
}

impl<'a> Observation<'a> {
    fn include(&mut self, next: ExactValue<'a>) {
        if matches!(self, Self::Value(prior) if prior.compare(&next) != Some(Ordering::Equal)) {
            *self = Self::Conflict;
        }
    }
}

type Facts<'a> = BTreeMap<String, Observation<'a>>;

pub(crate) fn property_key(text: &str) -> String {
    text.nfkc().collect::<String>().trim().to_owned()
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "DEADLINE",
            "候補の比較中に元の依頼の締切へ達しました。",
        ));
    }
    Ok(())
}

fn candidate_facts(candidate: &DecisionCandidate, deadline: Instant) -> Result<Facts<'_>> {
    use std::collections::btree_map::Entry;

    let mut facts = BTreeMap::new();
    for fact in &candidate.facts {
        check_deadline(deadline)?;
        if matches!(fact.value, DecisionValue::Unknown { .. }) {
            continue;
        }
        let value = ExactValue::parse(&fact.value)?;
        match facts.entry(property_key(&fact.property.text)) {
            Entry::Vacant(entry) => {
                entry.insert(Observation::Value(value));
            }
            Entry::Occupied(mut entry) => entry.get_mut().include(value),
        }
    }
    Ok(facts)
}

fn assess(
    observation: Option<&Observation<'_>>,
    relation: &DecisionRelation,
    expected: &ExactValue<'_>,
) -> RuleStatus {
    let value = match observation {
        None => return RuleStatus::Missing,
        Some(Observation::Conflict) => return RuleStatus::Conflict,
        Some(Observation::Value(value)) => value,
    };
    if matches!(value, ExactValue::Flag(_)) && !matches!(relation, DecisionRelation::Equal) {
        return RuleStatus::Incompatible;
    }
    let Some(ordering) = value.compare(expected) else {
        return RuleStatus::Incompatible;
    };
    let met = match relation {
        DecisionRelation::AtLeast => ordering != Ordering::Less,
        DecisionRelation::AtMost => ordering != Ordering::Greater,
        DecisionRelation::Greater => ordering == Ordering::Greater,
        DecisionRelation::Less => ordering == Ordering::Less,
        DecisionRelation::Equal => ordering == Ordering::Equal,
    };
    if met {
        RuleStatus::Met
    } else {
        RuleStatus::Unmet
    }
}

fn ranking_value<'a, 'source>(
    observation: Option<&'a Observation<'source>>,
) -> std::result::Result<&'a ExactValue<'source>, RuleStatus> {
    match observation {
        Some(Observation::Value(value @ ExactValue::Quantity { .. })) => Ok(value),
        Some(Observation::Value(ExactValue::Flag(_))) => Err(RuleStatus::Incompatible),
        Some(Observation::Conflict) => Err(RuleStatus::Conflict),
        None => Err(RuleStatus::Missing),
    }
}

fn compare_preference(
    left: Option<&Observation<'_>>,
    right: Option<&Observation<'_>>,
    direction: &DecisionDirection,
) -> PreferenceCheck {
    let (left, right) = match (ranking_value(left), ranking_value(right)) {
        (Ok(left), Ok(right)) => (left, right),
        (left, right) => {
            return PreferenceCheck::Unresolved {
                left: left.err(),
                right: right.err(),
            };
        }
    };
    match (left.compare(right), direction) {
        (Some(Ordering::Equal), _) => PreferenceCheck::Equal,
        (Some(Ordering::Less), DecisionDirection::Minimize)
        | (Some(Ordering::Greater), DecisionDirection::Maximize) => PreferenceCheck::LeftBetter,
        (Some(_), _) => PreferenceCheck::RightBetter,
        (None, _) => PreferenceCheck::Unresolved {
            left: Some(RuleStatus::Incompatible),
            right: Some(RuleStatus::Incompatible),
        },
    }
}

fn validate_bounds(request: &DecisionRequest, deadline: Instant) -> Result<()> {
    check_deadline(deadline)?;
    if request.candidates.is_empty()
        || request.candidates.len() > 8
        || request
            .candidates
            .iter()
            .any(|candidate| candidate.facts.len() > 12)
        || request.requirements.len() > 6
        || request.preferences.len() > 4
    {
        return Err(failure(
            "DECISION_INPUT_LIMIT",
            "候補は1〜8件、各候補の情報は12件、必須条件は6件、優先条件は4件まで扱えます。",
        ));
    }
    Ok(())
}

fn preference_keys(request: &DecisionRequest) -> Vec<(String, &DecisionDirection)> {
    request
        .preferences
        .iter()
        .map(|preference| {
            (
                property_key(&preference.property.text),
                &preference.direction,
            )
        })
        .collect()
}

fn comparison_checks(
    preferences: &[(String, &DecisionDirection)],
    deadline: Instant,
    mut compare: impl FnMut(&str, &DecisionDirection) -> Result<PreferenceCheck>,
) -> Result<Vec<PreferenceCheck>> {
    check_deadline(deadline)?;
    let mut checks = Vec::new();
    for (property, direction) in preferences {
        check_deadline(deadline)?;
        let check = compare(property, direction)?;
        let decisive = check != PreferenceCheck::Equal;
        checks.push(check);
        if decisive {
            break;
        }
    }
    Ok(checks)
}

fn possible_candidates(assessments: &[Vec<RuleStatus>]) -> Vec<usize> {
    assessments
        .iter()
        .enumerate()
        .filter_map(|(index, statuses)| (!statuses.contains(&RuleStatus::Unmet)).then_some(index))
        .collect()
}

fn candidate_pairs(possible: &[usize]) -> impl Iterator<Item = [usize; 2]> + '_ {
    possible.iter().enumerate().flat_map(|(index, &left)| {
        possible[index + 1..]
            .iter()
            .map(move |&right| [left, right])
    })
}

fn assemble_evaluation(
    assessments: Vec<Vec<RuleStatus>>,
    possible: Vec<usize>,
    comparisons: Vec<PreferenceFinding>,
    deadline: Instant,
) -> Result<DecisionEvaluation> {
    let eligible = assessments
        .iter()
        .map(|statuses| statuses.iter().all(|status| *status == RuleStatus::Met))
        .collect::<Vec<_>>();
    let mut worse = vec![false; assessments.len()];
    for finding in &comparisons {
        match finding.checks.last() {
            Some(PreferenceCheck::LeftBetter) if eligible[finding.left] => {
                worse[finding.right] = true
            }
            Some(PreferenceCheck::RightBetter) if eligible[finding.right] => {
                worse[finding.left] = true
            }
            _ => {}
        }
    }
    let remaining = possible
        .into_iter()
        .filter(|&candidate| !worse[candidate])
        .collect::<Vec<_>>();
    let unresolved = remaining.iter().any(|&candidate| !eligible[candidate])
        || comparisons.iter().any(|finding| {
            !worse[finding.left]
                && !worse[finding.right]
                && matches!(
                    finding.checks.last(),
                    Some(PreferenceCheck::Unresolved { .. })
                )
        });
    let outcome = match remaining.as_slice() {
        [] => DecisionOutcome::NoEligible,
        _ if unresolved => DecisionOutcome::Undetermined(remaining),
        [candidate] => DecisionOutcome::Recommend(*candidate),
        _ => DecisionOutcome::Tie(remaining),
    };
    check_deadline(deadline)?;
    Ok(DecisionEvaluation {
        outcome,
        assessments,
        comparisons,
    })
}

pub(crate) fn evaluate(request: &DecisionRequest, deadline: Instant) -> Result<DecisionEvaluation> {
    validate_bounds(request, deadline)?;
    let facts = request
        .candidates
        .iter()
        .map(|candidate| candidate_facts(candidate, deadline))
        .collect::<Result<Vec<_>>>()?;
    let requirements = request
        .requirements
        .iter()
        .map(|requirement| {
            check_deadline(deadline)?;
            Ok((
                property_key(&requirement.property.text),
                &requirement.relation,
                ExactValue::parse(&requirement.expected)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let preferences = preference_keys(request);

    let mut assessments = Vec::with_capacity(facts.len());
    for candidate in &facts {
        let mut statuses = Vec::with_capacity(requirements.len());
        for (property, relation, expected) in &requirements {
            check_deadline(deadline)?;
            statuses.push(assess(candidate.get(property), relation, expected));
        }
        assessments.push(statuses);
    }
    let possible = possible_candidates(&assessments);
    let comparisons = candidate_pairs(&possible)
        .map(|[left, right]| {
            let checks = comparison_checks(&preferences, deadline, |property, direction| {
                Ok(compare_preference(
                    facts[left].get(property),
                    facts[right].get(property),
                    direction,
                ))
            })?;
            Ok(PreferenceFinding {
                left,
                right,
                checks,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    assemble_evaluation(assessments, possible, comparisons, deadline)
}

type RawFacts<'a> = BTreeMap<String, Vec<&'a DecisionFact>>;

fn index_facts(request: &DecisionRequest, deadline: Instant) -> Result<Vec<RawFacts<'_>>> {
    request
        .candidates
        .iter()
        .map(|candidate| {
            let mut indexed = RawFacts::new();
            for fact in &candidate.facts {
                check_deadline(deadline)?;
                indexed
                    .entry(property_key(&fact.property.text))
                    .or_default()
                    .push(fact);
            }
            Ok(indexed)
        })
        .collect()
}

struct RevisionObservations<'a> {
    raw: Vec<RawFacts<'a>>,
    prepared: Vec<BTreeMap<String, Option<Observation<'a>>>>,
}

impl<'a> RevisionObservations<'a> {
    fn prepare(&mut self, candidate: usize, property: &str, deadline: Instant) -> Result<()> {
        if self.prepared[candidate].contains_key(property) {
            return Ok(());
        }
        let mut observation: Option<Observation<'a>> = None;
        for fact in self.raw[candidate].get(property).into_iter().flatten() {
            check_deadline(deadline)?;
            if matches!(fact.value, DecisionValue::Unknown { .. }) {
                continue;
            }
            let value = ExactValue::parse(&fact.value)?;
            if let Some(previous) = observation.as_mut() {
                previous.include(value);
            } else {
                observation = Some(Observation::Value(value));
            }
        }
        self.prepared[candidate].insert(property.into(), observation);
        Ok(())
    }

    fn get(&self, candidate: usize, property: &str) -> Option<&Observation<'a>> {
        self.prepared[candidate]
            .get(property)
            .and_then(Option::as_ref)
    }
}

fn previous_decision_shape() -> minidora_domain::Error {
    failure(
        "DECISION_REVISION_PROOF_INVALID",
        "保存済みの比較結果と候補・条件の対応が一致しません。",
    )
}

fn prior_pair_findings<'a>(
    prior: &DecisionRequest,
    saved: &'a DecisionEvaluation,
) -> Result<BTreeMap<[usize; 2], &'a PreferenceFinding>> {
    let possible = possible_candidates(&saved.assessments);
    if saved.comparisons.len() != possible.len() * possible.len().saturating_sub(1) / 2 {
        return Err(previous_decision_shape());
    }
    candidate_pairs(&possible)
        .zip(&saved.comparisons)
        .map(|(pair, proof)| {
            let mut checks = proof.checks.iter();
            let stop = checks.next_back();
            if pair != [proof.left, proof.right]
                || proof.checks.len() > prior.preferences.len()
                || checks.any(|check| *check != PreferenceCheck::Equal)
                || (matches!(stop, None | Some(PreferenceCheck::Equal))
                    && proof.checks.len() != prior.preferences.len())
            {
                return Err(previous_decision_shape());
            }
            Ok((pair, proof))
        })
        .collect()
}

pub(crate) fn evaluate_revision(
    prior: &DecisionRequest,
    saved: &DecisionEvaluation,
    next: &DecisionRequest,
    deadline: Instant,
) -> Result<(DecisionEvaluation, DecisionReuse)> {
    validate_bounds(prior, deadline)?;
    validate_bounds(next, deadline)?;
    if prior.candidates.len() != next.candidates.len()
        || prior
            .candidates
            .iter()
            .zip(&next.candidates)
            .any(|(prior, next)| prior.name != next.name)
        || saved.assessments.len() != prior.candidates.len()
        || saved
            .assessments
            .iter()
            .any(|row| row.len() != prior.requirements.len())
    {
        return Err(previous_decision_shape());
    }
    let saved_pairs = prior_pair_findings(prior, saved)?;
    let original = index_facts(prior, deadline)?;
    let mut observations = RevisionObservations {
        raw: index_facts(next, deadline)?,
        prepared: (0..next.candidates.len())
            .map(|_| BTreeMap::new())
            .collect(),
    };
    let mut reuse = DecisionReuse {
        assessments: Vec::new(),
        comparisons: Vec::new(),
    };
    let mut assessments = vec![Vec::with_capacity(next.requirements.len()); next.candidates.len()];
    for (criterion, rule) in next.requirements.iter().enumerate() {
        let property = property_key(&rule.property.text);
        let unchanged_rule = prior.requirements.get(criterion) == Some(rule);
        let mut expected = None;
        for (candidate, row) in assessments.iter_mut().enumerate() {
            check_deadline(deadline)?;
            if unchanged_rule
                && original[candidate].get(&property) == observations.raw[candidate].get(&property)
            {
                row.push(saved.assessments[candidate][criterion]);
                reuse.assessments.push([candidate, criterion]);
                continue;
            }
            if expected.is_none() {
                expected = Some(ExactValue::parse(&rule.expected)?);
            }
            observations.prepare(candidate, &property, deadline)?;
            row.push(assess(
                observations.get(candidate, &property),
                &rule.relation,
                expected.as_ref().expect("expected value is prepared"),
            ));
        }
    }
    reuse.assessments.sort_unstable();

    let possible = possible_candidates(&assessments);
    let preferences = preference_keys(next);
    let mut comparisons = Vec::new();
    for [left, right] in candidate_pairs(&possible) {
        check_deadline(deadline)?;
        if let Some(proof) = saved_pairs.get(&[left, right])
            && proof.checks.len() <= next.preferences.len()
            && (proof.checks.len() == next.preferences.len()
                || proof
                    .checks
                    .last()
                    .is_some_and(|last| *last != PreferenceCheck::Equal))
            && (0..proof.checks.len()).all(|index| {
                let property = &preferences[index].0;
                prior.preferences[index] == next.preferences[index]
                    && original[left].get(property) == observations.raw[left].get(property)
                    && original[right].get(property) == observations.raw[right].get(property)
            })
        {
            comparisons.push((**proof).clone());
            reuse.comparisons.push([left, right]);
            continue;
        }
        let checks = comparison_checks(&preferences, deadline, |property, direction| {
            observations.prepare(left, property, deadline)?;
            observations.prepare(right, property, deadline)?;
            Ok(compare_preference(
                observations.get(left, property),
                observations.get(right, property),
                direction,
            ))
        })?;
        comparisons.push(PreferenceFinding {
            left,
            right,
            checks,
        });
    }
    // Also admit a changed supplied value that this decision did not need.
    // A later revision may add a rule depending on it.
    for (candidate, original_facts) in original.iter().enumerate() {
        let changed = observations.raw[candidate]
            .iter()
            .filter(|(property, facts)| original_facts.get(*property) != Some(*facts))
            .map(|(property, _)| property.clone())
            .collect::<Vec<_>>();
        for property in changed {
            observations.prepare(candidate, &property, deadline)?;
        }
    }
    Ok((
        assemble_evaluation(assessments, possible, comparisons, deadline)?,
        reuse,
    ))
}
