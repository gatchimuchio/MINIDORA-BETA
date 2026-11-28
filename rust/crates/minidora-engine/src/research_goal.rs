//! Store and replay the observations of a bounded multi-source research goal.
use crate::{
    input::verify_web_research_input,
    language, ports,
    scheduler::{GoalNode, GoalNodeKind},
    search::{self, SearchCandidate, SearchObservation, SearchProvider},
    store::Store,
    web_research,
};
use minidora_domain::{
    Calculation, ContentPlan, Error, EvidencePacket, Guarantee, InputBundle, Intent, Permissions,
    RequestEnvelope, Result, ResultRecord, canonical, digest, failure,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(crate) const PREDICATE: &str = "WEB_RESEARCH_COMPARISON_V1";
pub(crate) const READ_NODE: &str = "research-observations";
pub(crate) const REPORT_NODE: &str = "research-comparison";
pub(crate) const CALC_NODE: &str = "research-independent-calculation";
const MAX_OUTPUT_RESERVE: usize = 16 * 1024;

pub(crate) fn nodes(independent_calculation: bool) -> Vec<GoalNode> {
    let mut nodes = vec![
        GoalNode {
            id: READ_NODE.into(),
            kind: GoalNodeKind::Evidence,
            requires: vec![],
        },
        GoalNode {
            id: REPORT_NODE.into(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![READ_NODE.into()],
        },
    ];
    if independent_calculation {
        nodes.push(GoalNode {
            id: CALC_NODE.into(),
            kind: GoalNodeKind::ComputedValue,
            requires: vec![],
        });
    }
    nodes
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Observation {
    listing: Option<String>,
    bodies: Vec<String>,
    stop: Option<String>,
}

pub(crate) struct Outcome {
    pub observation: String,
    pub comparison: String,
    pub calculation: Option<String>,
    pub units: Vec<String>,
    pub evidence: Vec<EvidencePacket>,
    pub complete: bool,
}

impl Outcome {
    pub fn dependencies(&self) -> Vec<String> {
        self.calculation
            .iter()
            .chain([&self.observation, &self.comparison])
            .cloned()
            .collect()
    }
}

fn mismatch() -> Error {
    failure(
        "WEB_RESEARCH_MISMATCH",
        "同じ会話に保存した調査の依頼、検索候補、本文を照合できません。",
    )
}

fn candidates<'a>(
    listing: &'a SearchObservation,
    permissions: &Permissions,
) -> Vec<&'a SearchCandidate> {
    listing
        .candidates
        .iter()
        .filter(|item| {
            search::candidate_body_uri(item)
                .and_then(|uri| ports::approved_url(&uri, permissions))
                .is_ok()
        })
        .take(web_research::MAX_RESEARCH_SOURCES)
        .collect()
}

fn observe_failure(error: Error, observation: &mut Observation) -> Result<()> {
    match error {
        Error::Contract { ref code, .. } if !crate::core::read_case_changed(&error) => {
            observation.stop = Some(code.clone());
            Ok(())
        }
        other => Err(other),
    }
}

#[derive(Serialize)]
#[serde(tag = "status", content = "value", rename_all = "SCREAMING_SNAKE_CASE")]
enum LocalCalculation {
    Confirmed(Calculation),
    Unresolved(String),
}

fn local_calculation(expression: Option<&str>) -> Result<Option<LocalCalculation>> {
    expression
        .map(|expression| match language::calculate(expression) {
            Ok(calculation) => Ok(LocalCalculation::Confirmed(calculation)),
            Err(Error::Contract { code, .. }) => Ok(LocalCalculation::Unresolved(code)),
            Err(error) => Err(error),
        })
        .transpose()
}

fn read(
    store: &Store,
    request: &RequestEnvelope,
    topic: &str,
    budget: Result<ports::RetrievalBudget>,
    admit: &dyn Fn() -> Result<()>,
) -> Result<Observation> {
    let mut observation = Observation {
        listing: None,
        bodies: Vec::new(),
        stop: None,
    };
    let actor = &request.actor_id;
    let case = &request.case_id;
    let permissions = &request.permissions;
    if !permissions
        .read_hosts
        .contains(SearchProvider::Wikipedia.host())
    {
        observation.stop = Some(ports::READ_PERMISSION_REQUIRED.into());
        return Ok(observation);
    }
    let mut budget = match budget {
        Ok(value) => value,
        Err(error) => {
            observe_failure(error, &mut observation)?;
            return Ok(observation);
        }
    };
    budget.reserve_output(
        Duration::from_millis((request.budget.deadline_ms / 4).min(5000)),
        (request.budget.max_evidence_bytes / 4).min(MAX_OUTPUT_RESERVE),
    );
    let listing = match search::search_with_budget(
        SearchProvider::Wikipedia,
        topic,
        permissions,
        &request.budget,
        &mut budget,
        admit,
    ) {
        Ok(value) => value,
        Err(error) => {
            observe_failure(error, &mut observation)?;
            return Ok(observation);
        }
    };
    let wait = listing.redirect_wait.is_some();
    let listing = listing.observation;
    observation.listing = Some(store.put_artifact(actor, case, &listing)?);
    if wait {
        observation.stop = Some(ports::RETRY_DEFERRED.into());
        return Ok(observation);
    }
    if let Some(code) = &listing.failure {
        observation.stop = Some(code.clone());
        return Ok(observation);
    }
    let candidates = candidates(&listing, permissions);
    if candidates.is_empty() {
        observation.stop = Some("SEARCH_CANDIDATE_UNAVAILABLE".into());
        return Ok(observation);
    }
    for candidate in candidates {
        match search::fetch_candidate_with_budget(candidate, permissions, &mut budget, admit) {
            Ok(read) => {
                let packet = read.observation;
                observation
                    .bodies
                    .push(store.put_artifact(actor, case, &packet)?);
                if read.redirect_wait.is_some() {
                    observation.stop = Some(ports::RETRY_DEFERRED.into());
                    break;
                }
                if let Some(reason) = terminal_body_failure(&packet) {
                    observation.stop = Some(reason.into());
                    break;
                }
            }
            Err(error) => {
                observe_failure(error, &mut observation)?;
                break;
            }
        }
    }
    Ok(observation)
}

fn terminal_body_failure(packet: &EvidencePacket) -> Option<&str> {
    packet.failure.as_deref().filter(|code| {
        packet.retry_after.is_some()
            || matches!(
                *code,
                "PRIVATE_READ_TARGET" | ports::DEADLINE_EXCEEDED | ports::REDIRECT_RETRY_DEFERRED
            )
    })
}

fn stop_label(code: &str) -> &'static str {
    match code {
        ports::READ_PERMISSION_REQUIRED => "Wikipedia日本語版への通信が許可されていません。",
        ports::RETRY_DEFERRED => "接続先が指定した再試行時点を待っています。",
        "SEARCH_CANDIDATE_UNAVAILABLE" => "検索で本文の候補を確認できませんでした。",
        ports::DEADLINE_EXCEEDED => "取得の締切に達しました。",
        ports::HTTP_START_BUDGET_EXHAUSTED => "通信回数の上限に達しました。",
        "PRIVATE_READ_TARGET" => "安全に接続できる公開アドレスを確認できませんでした。",
        _ => "検索一覧または本文の取得が完了しませんでした。",
    }
}

fn valid_chain(packet: &EvidencePacket) -> bool {
    let chain = &packet.redirect_chain;
    chain.len() <= ports::MAX_HTTP_STARTS_PER_SOURCE
        && chain
            .iter()
            .enumerate()
            .all(|(i, uri)| !chain[..i].contains(uri))
}

fn materialize(
    store: &Store,
    bundle: &InputBundle,
    observation: &Observation,
    calculation: &Option<LocalCalculation>,
) -> Result<(Vec<u8>, Vec<String>, Vec<EvidencePacket>, bool)> {
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let permissions = &request.permissions;
    let Intent::CompareWebResearch { topic, expression } = &bundle.meaning.intent else {
        return Err(mismatch());
    };
    let reserved_non_read = nodes(expression.is_some()).len() - 1
        + 3 // Compute, content and render.
        + usize::from(request.output_path.is_some());
    let available_starts = (request.budget.max_actions as usize)
        .checked_sub(reserved_non_read)
        .ok_or_else(mismatch)?;
    let listing = observation
        .listing
        .as_ref()
        .map(|reference| -> Result<SearchObservation> {
            let bytes = store.get_bytes(actor, case, reference)?;
            let listing: SearchObservation = serde_json::from_slice(&bytes)?;
            if canonical(&listing)? != bytes
                || listing.query != *topic
                || listing.provider != SearchProvider::Wikipedia
                || !valid_chain(&listing.listing)
            {
                return Err(mismatch());
            }
            if listing.failure.is_none() {
                search::validate_stored_observation(&listing, Some(permissions))?;
            } else {
                search::validate_failed_observation(&listing, Some(permissions))?;
            }
            Ok(listing)
        })
        .transpose()?;
    let candidates = listing
        .as_ref()
        .filter(|listing| listing.failure.is_none())
        .map_or_else(Vec::new, |listing| candidates(listing, permissions));
    if observation.bodies.len() > candidates.len() {
        return Err(mismatch());
    }
    let mut packets = Vec::new();
    let mut retry_evidence = Vec::new();
    let mut raw_bytes = 0usize;
    let mut starts = 0usize;
    if let Some(list) = &listing {
        raw_bytes = list.listing.raw_body.len();
        starts = list.listing.redirect_chain.len();
        if list.listing.retry_after.is_some() {
            retry_evidence.push(list.listing.clone());
        }
    }
    for (index, (reference, candidate)) in observation.bodies.iter().zip(&candidates).enumerate() {
        let bytes = store.get_bytes(actor, case, reference)?;
        let packet: EvidencePacket = serde_json::from_slice(&bytes)?;
        if packet.mode != "LIVE"
            || packet.provider_id != "https-direct-url-v1"
            || !valid_chain(&packet)
            || packet.id != ports::fetch_packet_identity(&packet)?
            || packet.request_uri != search::candidate_body_uri(candidate)?
            || packet.raw_sha256 != digest(&packet.raw_body)
            || canonical(&packet)? != bytes
            || listing
                .as_ref()
                .is_none_or(|list| packet.observed_at_unix_ms < list.listing.observed_at_unix_ms)
        {
            return Err(mismatch());
        }
        ports::validate_observed_route(&packet, permissions)?;
        raw_bytes = raw_bytes.saturating_add(packet.raw_body.len());
        starts = starts.saturating_add(packet.redirect_chain.len());
        if let Some(reason) = terminal_body_failure(&packet) {
            let stop_matches = observation.stop.as_deref() == Some(reason)
                || (reason == ports::REDIRECT_RETRY_DEFERRED
                    && observation.stop.as_deref() == Some(ports::RETRY_DEFERRED));
            if index + 1 != observation.bodies.len() || !stop_matches {
                return Err(mismatch());
            }
        }
        if packet.retry_after.is_some() {
            retry_evidence.push(packet.clone());
        }
        if packet.failure.is_none() {
            packets.push((*candidate, packet));
        }
    }
    if starts > available_starts || raw_bytes > request.budget.max_evidence_bytes {
        return Err(mismatch());
    }
    let sources: Vec<_> = packets
        .iter()
        .map(|(candidate, packet)| (*candidate, packet))
        .collect();
    let report = web_research::compare(topic, &sources, permissions)?;
    let mut units = Vec::new();
    match calculation {
        Some(LocalCalculation::Confirmed(calculation)) => {
            units.push(format!("別件の計算は {} です。", calculation.exact_value));
        }
        Some(LocalCalculation::Unresolved(code)) => {
            units.push(format!("別件の計算を確定できませんでした（{code}）。"));
        }
        None => {}
    }
    units.extend(report.units());
    if let Some(reason) = &observation.stop {
        units.push(stop_label(reason).into());
    }
    let report_bytes = canonical(&report)?;
    let proof_bytes = report_bytes
        .len()
        .saturating_add(canonical(observation)?.len())
        .saturating_add(canonical(calculation)?.len());
    if raw_bytes
        .saturating_add(proof_bytes)
        .saturating_add(units.join("\n").len())
        > request.budget.max_evidence_bytes
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "調査結果が出力の証拠予算を超えました。",
        ));
    }
    let complete =
        report.complete() && !matches!(calculation, Some(LocalCalculation::Unresolved(_)));
    Ok((report_bytes, units, retry_evidence, complete))
}

pub(crate) fn produce(
    store: &Store,
    bundle: &InputBundle,
    retrieval: Result<ports::RetrievalBudget>,
    admit: &dyn Fn() -> Result<()>,
) -> Result<Outcome> {
    verify_web_research_input(bundle)?;
    let request = &bundle.request;
    let Intent::CompareWebResearch { topic, expression } = &bundle.meaning.intent else {
        return Err(mismatch());
    };
    let calculation = local_calculation(expression.as_deref())?;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let calculation_ref = calculation
        .as_ref()
        .map(|calc| store.put_artifact(actor, case, calc))
        .transpose()?;
    let observation = read(store, request, topic, retrieval, admit)?;
    let (comparison, units, evidence, complete) =
        materialize(store, bundle, &observation, &calculation)?;
    Ok(Outcome {
        calculation: calculation_ref,
        observation: store.put_artifact(actor, case, &observation)?,
        comparison: store.put_bytes(actor, case, &comparison)?,
        units,
        evidence,
        complete,
    })
}

pub(crate) fn verify_delivery(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<Outcome> {
    verify_web_research_input(bundle)?;
    let Intent::CompareWebResearch { expression, .. } = &bundle.meaning.intent else {
        return Err(mismatch());
    };
    let (comparison, refs) = result.dependencies.split_last().ok_or_else(mismatch)?;
    let (observation, refs) = refs.split_last().ok_or_else(mismatch)?;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let calculation = local_calculation(expression.as_deref())?;
    let bytes = store.get_bytes(actor, case, observation)?;
    let observed: Observation = serde_json::from_slice(&bytes)?;
    if bytes != canonical(&observed)? {
        return Err(mismatch());
    }
    let calc_ref = match (refs, calculation.as_ref()) {
        ([reference], Some(calc))
            if store.get_bytes(actor, case, reference)? == canonical(calc)? =>
        {
            Some(reference.clone())
        }
        ([], None) => None,
        _ => return Err(mismatch()),
    };
    let (expected, units, evidence, complete) =
        materialize(store, bundle, &observed, &calculation)?;
    if !complete
        || result.guarantee != Guarantee::SourceReport
        || plan.guarantee != result.guarantee
        || result.calculation.is_some()
        || result.evidence != evidence
        || result.text != plan.text
        || plan.required_units != units
        || result.text != units.join("\n")
        || store.get_bytes(actor, case, comparison)? != expected
    {
        return Err(mismatch());
    }
    Ok(Outcome {
        observation: observation.clone(),
        comparison: comparison.clone(),
        calculation: calc_ref,
        units,
        evidence,
        complete,
    })
}

pub(crate) fn verify_retry_source(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    packet: &EvidencePacket,
) -> Result<()> {
    verify_web_research_input(bundle)?;
    if !result.evidence.contains(packet) || packet.retry_after.is_none() {
        return Err(mismatch());
    }
    let observation = result
        .dependencies
        .iter()
        .rev()
        .nth(1)
        .ok_or_else(mismatch)?;
    let bytes = store.get_bytes(
        &bundle.request.actor_id,
        &bundle.request.case_id,
        observation,
    )?;
    let observed: Observation = serde_json::from_slice(&bytes)?;
    let Intent::CompareWebResearch { expression, .. } = &bundle.meaning.intent else {
        return Err(mismatch());
    };
    let calculation = local_calculation(expression.as_deref())?;
    let (_, _, evidence, _) = materialize(store, bundle, &observed, &calculation)?;
    if canonical(&observed)? != bytes || evidence != result.evidence {
        return Err(mismatch());
    }
    Ok(())
}
