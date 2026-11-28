//! Store and replay the observations of a bounded multi-source research goal.
use crate::{
    input::{
        self, WEB_RESEARCH_DIVERSE_PARSER_VERSION, WEB_RESEARCH_V2_PARSER_VERSION,
        verify_web_research_input,
    },
    language, ports,
    scheduler::{GoalNode, GoalNodeKind},
    search::{self, SearchCandidate, SearchObservation, SearchProvider},
    store::Store,
    web_research,
};
use minidora_domain::{
    BranchOutcome, BranchState, Calculation, ContentPlan, Error, EvidencePacket, Guarantee,
    IndependentRequest, InputBundle, Intent, Permissions, RequestEnvelope, Result, ResultRecord,
    canonical, digest, failure,
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub(crate) const PREDICATE: &str = "WEB_RESEARCH_COMPARISON_V1";
pub(crate) const READ_NODE: &str = "research-observations";
pub(crate) const REPORT_NODE: &str = "research-comparison";
pub(crate) const CALC_NODE: &str = "research-independent-calculation";
const MAX_OUTPUT_RESERVE: usize = 16 * 1024;
const PRIMARY_DIVERSE_BODIES: usize = 1;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scope: Option<ResearchScope>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    additional_publisher: Option<ProviderObservation>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProviderObservation {
    listing: Option<String>,
    bodies: Vec<String>,
    stop: Option<String>,
}

impl Observation {
    fn stop(&self) -> Option<&str> {
        self.stop
            .as_deref()
            .or_else(|| self.additional_publisher.as_ref()?.stop.as_deref())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
struct ResearchScope {
    input_fingerprint: String,
    source_range: (usize, usize),
}

#[derive(Clone, Copy)]
pub(crate) struct ResearchAction<'a> {
    topic: &'a str,
    expression: Option<&'a str>,
    current: bool,
    diverse: bool,
}

impl<'a> ResearchAction<'a> {
    fn bind(intent: &'a Intent, current: bool) -> Option<Self> {
        let Intent::CompareWebResearch { topic, expression } = intent else {
            return None;
        };
        Some(Self {
            topic,
            expression: expression.as_deref(),
            current,
            diverse: false,
        })
    }

    fn for_bundle(bundle: &'a InputBundle) -> Result<Self> {
        let version = bundle.meaning.parser_version.as_str();
        let diverse = version == WEB_RESEARCH_DIVERSE_PARSER_VERSION;
        let mut action = Self::bind(
            &bundle.meaning.intent,
            diverse || version == WEB_RESEARCH_V2_PARSER_VERSION,
        )
        .ok_or_else(mismatch)?;
        action.diverse = diverse;
        Ok(action)
    }

    pub(crate) fn for_item(bundle: &InputBundle, item: &'a IndependentRequest) -> Option<Self> {
        let version = bundle.meaning.parser_version.as_str();
        if !matches!(
            version,
            crate::composition::RESEARCH_PARSER_VERSION
                | crate::composition::DIVERSE_RESEARCH_PARSER_VERSION
        ) {
            return None;
        }
        let mut action = Self::bind(&item.intent, true)?;
        action.diverse = version == crate::composition::DIVERSE_RESEARCH_PARSER_VERSION
            && input::is_diverse_research_item(item);
        Some(action)
    }

    fn primary_limit(self) -> usize {
        if self.diverse {
            PRIMARY_DIVERSE_BODIES
        } else {
            web_research::MAX_RESEARCH_SOURCES
        }
    }

    pub(crate) fn incomplete_reason(self) -> &'static str {
        if self.diverse {
            "WEB_RESEARCH_INSUFFICIENT_PUBLISHERS"
        } else {
            "WEB_RESEARCH_INSUFFICIENT_SOURCES"
        }
    }

    fn secondary_limit(self) -> usize {
        web_research::MAX_RESEARCH_SOURCES - self.primary_limit()
    }

    fn provider(self) -> SearchProvider {
        let japanese = self
            .topic
            .chars()
            .any(|c| matches!(c, 'ぁ'..='ヿ' | '㐀'..='鿿'));
        if self.current && !japanese {
            SearchProvider::WikipediaEnglish
        } else {
            SearchProvider::Wikipedia
        }
    }

    pub(crate) fn is_admitted(self, permissions: &Permissions) -> bool {
        permissions.read_hosts.contains(self.provider().host())
            || (self.diverse
                && permissions
                    .read_hosts
                    .contains(SearchProvider::EuropePmcOpenAccess.host()))
    }

    fn scoped_permissions(
        self,
        provider: SearchProvider,
        permissions: &Permissions,
    ) -> Option<Permissions> {
        self.current.then(|| Permissions {
            read_hosts: permissions
                .read_hosts
                .get(provider.host())
                .cloned()
                .into_iter()
                .collect(),
            ..Permissions::default()
        })
    }
}

pub(crate) struct Outcome {
    pub observation: String,
    pub comparison: String,
    pub calculation: Option<String>,
    pub units: Vec<String>,
    pub evidence: Vec<EvidencePacket>,
    pub complete: bool,
    pub branches: Vec<BranchOutcome>,
    pub has_citations: bool,
    pub read_starts: usize,
    pub raw_bytes: usize,
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
    limit: usize,
) -> Vec<&'a SearchCandidate> {
    listing
        .candidates
        .iter()
        .filter(|item| {
            search::candidate_body_uri(item)
                .and_then(|uri| ports::approved_url(&uri, permissions))
                .is_ok()
        })
        .take(limit)
        .collect()
}

fn observe_failure(error: Error, observation: &mut ProviderObservation) -> Result<()> {
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

fn branch_outcomes(
    calculation: Option<(&LocalCalculation, &str)>,
    observation: &Observation,
    observation_ref: &str,
    comparison_complete: bool,
    comparison_ref: &str,
) -> Vec<BranchOutcome> {
    let mut branches = Vec::with_capacity(1 + usize::from(calculation.is_some()));
    if let Some((calculation, reference)) = calculation {
        let (state, reason_code) = match calculation {
            LocalCalculation::Confirmed(_) => (BranchState::Confirmed, None),
            LocalCalculation::Unresolved(code) => (BranchState::Failed, Some(code.clone())),
        };
        branches.push(BranchOutcome {
            node_id: CALC_NODE.into(),
            state,
            reason_code,
            artifact_ref: reference.into(),
        });
    }
    let (state, reason_code, artifact_ref) = if comparison_complete {
        (BranchState::Confirmed, None, comparison_ref)
    } else if let Some(reason) = observation.stop() {
        (
            BranchState::Blocked,
            Some(reason.to_owned()),
            observation_ref,
        )
    } else {
        (
            BranchState::Blocked,
            Some("WEB_RESEARCH_INSUFFICIENT_SOURCES".into()),
            comparison_ref,
        )
    };
    branches.push(BranchOutcome {
        node_id: REPORT_NODE.into(),
        state,
        reason_code,
        artifact_ref: artifact_ref.into(),
    });
    branches
}

fn read(
    store: &Store,
    request: &RequestEnvelope,
    action: ResearchAction<'_>,
    budget: Result<&mut ports::RetrievalBudget>,
    admit: &dyn Fn() -> Result<()>,
) -> Result<Observation> {
    let mut missing_budget = ProviderObservation::default();
    let mut budget = match budget {
        Ok(budget) => Some(budget),
        Err(error) => {
            observe_failure(error, &mut missing_budget)?;
            None
        }
    };
    let provider = action.provider();
    let primary = read_provider(
        store,
        request,
        action,
        provider,
        action.primary_limit(),
        budget.as_deref_mut(),
        missing_budget.stop.as_deref(),
        admit,
        None,
    )?;
    let additional_publisher = if action.diverse {
        let verified_primary = recover_provider(
            store,
            request,
            action,
            provider,
            action.primary_limit(),
            primary.listing.as_deref(),
            &primary.bodies,
            primary.stop.as_deref(),
        )?;
        let comparison_source = verified_primary
            .packets
            .first()
            .map(|(candidate, body)| (candidate, body));
        Some(read_provider(
            store,
            request,
            action,
            SearchProvider::EuropePmcOpenAccess,
            action.secondary_limit(),
            budget,
            missing_budget.stop.as_deref(),
            admit,
            comparison_source,
        )?)
    } else {
        None
    };
    Ok(Observation {
        listing: primary.listing,
        bodies: primary.bodies,
        stop: primary.stop,
        scope: None,
        additional_publisher,
    })
}

#[allow(clippy::too_many_arguments)]
fn read_provider(
    store: &Store,
    request: &RequestEnvelope,
    action: ResearchAction<'_>,
    provider: SearchProvider,
    limit: usize,
    budget: Option<&mut ports::RetrievalBudget>,
    unavailable: Option<&str>,
    admit: &dyn Fn() -> Result<()>,
    comparison_source: Option<(&SearchCandidate, &EvidencePacket)>,
) -> Result<ProviderObservation> {
    let mut observation = ProviderObservation::default();
    let actor = &request.actor_id;
    let case = &request.case_id;
    let scoped = action.scoped_permissions(provider, &request.permissions);
    let permissions = scoped.as_ref().unwrap_or(&request.permissions);
    if !permissions.read_hosts.contains(provider.host()) {
        observation.stop = Some(ports::READ_PERMISSION_REQUIRED.into());
        return Ok(observation);
    }
    let Some(budget) = budget else {
        observation.stop = unavailable.map(str::to_owned);
        return Ok(observation);
    };
    let listing = match search::search_with_budget(
        provider,
        action.topic,
        permissions,
        &request.budget,
        budget,
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
    let candidates = candidates(&listing, permissions, limit);
    if candidates.is_empty() {
        observation.stop = Some("SEARCH_CANDIDATE_UNAVAILABLE".into());
        return Ok(observation);
    }
    for candidate in candidates {
        match search::fetch_candidate_with_budget(candidate, permissions, budget, admit) {
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
                if let Some(primary) = comparison_source
                    && packet.failure.is_none()
                    && web_research::compare_diverse(
                        action.topic,
                        &[primary, (candidate, &packet)],
                        &request.permissions,
                    )?
                    .complete()
                {
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

struct Materialized {
    report_bytes: Vec<u8>,
    units: Vec<String>,
    retry_evidence: Vec<EvidencePacket>,
    comparison_complete: bool,
    has_citations: bool,
    read_starts: usize,
    raw_bytes: usize,
}

fn standalone_starts(bundle: &InputBundle, action: ResearchAction<'_>) -> Result<usize> {
    let reserved = nodes(action.expression.is_some()).len() - 1
        + 3 // Compute, content and render.
        + usize::from(bundle.request.output_path.is_some());
    (bundle.request.budget.max_actions as usize)
        .checked_sub(reserved)
        .ok_or_else(mismatch)
}

fn materialize(
    store: &Store,
    bundle: &InputBundle,
    action: ResearchAction<'_>,
    observation: &Observation,
    calculation: &Option<LocalCalculation>,
    available_starts: usize,
) -> Result<Materialized> {
    let request = &bundle.request;
    let permissions = &request.permissions;
    if action.diverse != observation.additional_publisher.is_some() {
        return Err(mismatch());
    }
    let primary = recover_provider(
        store,
        request,
        action,
        action.provider(),
        action.primary_limit(),
        observation.listing.as_deref(),
        &observation.bodies,
        observation.stop.as_deref(),
    )?;
    let mut retained = primary;
    if let Some(extra) = &observation.additional_publisher {
        let additional = recover_provider(
            store,
            request,
            action,
            SearchProvider::EuropePmcOpenAccess,
            action.secondary_limit(),
            extra.listing.as_deref(),
            &extra.bodies,
            extra.stop.as_deref(),
        )?;
        retained.packets.extend(additional.packets);
        retained.retry_evidence.extend(additional.retry_evidence);
        retained.raw_bytes = retained.raw_bytes.saturating_add(additional.raw_bytes);
        retained.starts = retained.starts.saturating_add(additional.starts);
    }
    let ProviderEvidence {
        packets,
        retry_evidence,
        raw_bytes,
        starts,
    } = retained;
    if starts > available_starts || raw_bytes > request.budget.max_evidence_bytes {
        return Err(mismatch());
    }
    let sources: Vec<_> = packets
        .iter()
        .map(|(candidate, packet)| (candidate, packet))
        .collect();
    let english = action.current && crate::core::supports_english_output_locale(&request.locale);
    let report = if action.diverse {
        web_research::compare_diverse(action.topic, &sources, permissions)?
    } else if action.current {
        web_research::compare_current(action.topic, &sources, permissions)?
    } else {
        web_research::compare(action.topic, &sources, permissions)?
    };
    let mut units = Vec::new();
    match calculation {
        Some(LocalCalculation::Confirmed(calculation)) => {
            let exact = &calculation.exact_value;
            units.push(if english {
                format!("The separate calculation is {exact}.")
            } else {
                format!("別件の計算は {exact} です。")
            });
        }
        Some(LocalCalculation::Unresolved(code)) => {
            units.push(if english {
                format!("The separate calculation did not finish ({code}).")
            } else {
                format!("別件の計算を確定できませんでした（{code}）。")
            });
        }
        None => {}
    }
    units.extend(if action.current {
        report.localized_units(english)
    } else {
        report.units()
    });
    if let Some(reason) = &observation.stop {
        units.push(if action.current {
            current_stop_label(reason, action.provider(), english)
        } else {
            stop_label(reason).into()
        });
    }
    if let Some(reason) = observation
        .additional_publisher
        .as_ref()
        .and_then(|value| value.stop.as_deref())
    {
        units.push(current_stop_label(
            reason,
            SearchProvider::EuropePmcOpenAccess,
            english,
        ));
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
        if action.current {
            return Ok(Materialized {
                report_bytes: canonical(&(
                    "minidora.web-research.preview-budget.v1",
                    digest(&report_bytes),
                ))?,
                units: vec![current_stop_label(
                    "RESOURCE_LIMIT",
                    action.provider(),
                    english,
                )],
                retry_evidence,
                comparison_complete: false,
                has_citations: false,
                read_starts: starts,
                raw_bytes,
            });
        }
        return Err(failure(
            "RESOURCE_LIMIT",
            "調査結果が出力の証拠予算を超えました。",
        ));
    }
    Ok(Materialized {
        report_bytes,
        units,
        retry_evidence,
        comparison_complete: report.complete(),
        has_citations: report.has_citations(),
        read_starts: starts,
        raw_bytes,
    })
}

#[derive(Default)]
struct ProviderEvidence {
    packets: Vec<(SearchCandidate, EvidencePacket)>,
    retry_evidence: Vec<EvidencePacket>,
    raw_bytes: usize,
    starts: usize,
}

#[allow(clippy::too_many_arguments)]
fn recover_provider(
    store: &Store,
    request: &RequestEnvelope,
    action: ResearchAction<'_>,
    provider: SearchProvider,
    limit: usize,
    listing_ref: Option<&str>,
    body_refs: &[String],
    stop: Option<&str>,
) -> Result<ProviderEvidence> {
    let actor = &request.actor_id;
    let case = &request.case_id;
    let narrowed = action.scoped_permissions(provider, &request.permissions);
    let permissions = narrowed.as_ref().unwrap_or(&request.permissions);
    let listing = listing_ref
        .map(|reference| -> Result<SearchObservation> {
            let bytes = store.get_bytes(actor, case, reference)?;
            let listing: SearchObservation = serde_json::from_slice(&bytes)?;
            if canonical(&listing)? != bytes
                || listing.query != action.topic
                || listing.provider != provider
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
        .map_or_else(Vec::new, |listing| candidates(listing, permissions, limit));
    if body_refs.len() > candidates.len() {
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
    for (index, (reference, candidate)) in body_refs.iter().zip(&candidates).enumerate() {
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
            let stop_matches = stop == Some(reason)
                || (reason == ports::REDIRECT_RETRY_DEFERRED
                    && stop == Some(ports::RETRY_DEFERRED));
            if index + 1 != body_refs.len() || !stop_matches {
                return Err(mismatch());
            }
        }
        if packet.retry_after.is_some() {
            retry_evidence.push(packet.clone());
        }
        if packet.failure.is_none() {
            if action.diverse {
                search::validate_research_body(candidate, &packet, permissions)?;
            }
            packets.push(((*candidate).clone(), packet));
        }
    }
    Ok(ProviderEvidence {
        packets,
        retry_evidence,
        raw_bytes,
        starts,
    })
}

fn current_stop_label(code: &str, provider: SearchProvider, english: bool) -> String {
    if code == ports::READ_PERMISSION_REQUIRED {
        let host = provider.host();
        return if english {
            format!("Read access to {host} has not been authorized.")
        } else {
            format!("{host} への閲覧が許可されていません。")
        };
    }
    if code == "RESOURCE_LIMIT" {
        return if english {
            "The observed article evidence is too large to display within the request budget."
                .into()
        } else {
            "取得した引用と根拠が出力予算に収まらなかったため、比較の表示を保留しました。".into()
        };
    }
    if !english {
        return stop_label(code).into();
    }
    match code {
        ports::RETRY_DEFERRED => "The provider asked us to wait before retrying.",
        "SEARCH_CANDIDATE_UNAVAILABLE" => "The search found no candidate article to read.",
        ports::DEADLINE_EXCEEDED => "The deadline for obtaining sources was reached.",
        ports::HTTP_START_BUDGET_EXHAUSTED => "The request limit for reading sources was reached.",
        "PRIVATE_READ_TARGET" => "A public destination could not be verified for this source.",
        _ => "I could not finish reading the search results or article bodies.",
    }
    .into()
}

pub(crate) fn output_reserve_time(deadline_ms: u64) -> Duration {
    Duration::from_millis((deadline_ms / 4).min(5000))
}

pub(crate) fn produce(
    store: &Store,
    bundle: &InputBundle,
    mut retrieval: Result<ports::RetrievalBudget>,
    admit: &dyn Fn() -> Result<()>,
) -> Result<Outcome> {
    verify_web_research_input(bundle)?;
    let action = ResearchAction::for_bundle(bundle)?;
    if let Ok(budget) = &mut retrieval {
        let request_budget = &bundle.request.budget;
        budget.reserve_output(
            output_reserve_time(request_budget.deadline_ms),
            (request_budget.max_evidence_bytes / 4).min(MAX_OUTPUT_RESERVE),
        );
    }
    let calculation = local_calculation(action.expression)?;
    let observation = match retrieval {
        Ok(mut budget) => read(store, &bundle.request, action, Ok(&mut budget), admit)?,
        Err(error) => read(store, &bundle.request, action, Err(error), admit)?,
    };
    retain_outcome(
        store,
        bundle,
        action,
        observation,
        calculation,
        standalone_starts(bundle, action)?,
    )
}

fn retain_outcome(
    store: &Store,
    bundle: &InputBundle,
    action: ResearchAction<'_>,
    observation: Observation,
    calculation: Option<LocalCalculation>,
    available_starts: usize,
) -> Result<Outcome> {
    let request = &bundle.request;
    let (actor, case) = (&request.actor_id, &request.case_id);
    let calculation_ref = calculation
        .as_ref()
        .map(|calc| store.put_artifact(actor, case, calc))
        .transpose()?;
    let materialized = materialize(
        store,
        bundle,
        action,
        &observation,
        &calculation,
        available_starts,
    )?;
    let observation_ref = store.put_artifact(actor, case, &observation)?;
    let comparison_ref = store.put_bytes(actor, case, &materialized.report_bytes)?;
    Ok(evaluated_outcome(
        &observation,
        observation_ref,
        comparison_ref,
        calculation.as_ref(),
        calculation_ref,
        materialized,
    ))
}

fn evaluated_outcome(
    observation: &Observation,
    observation_ref: String,
    comparison_ref: String,
    calculation: Option<&LocalCalculation>,
    calculation_ref: Option<String>,
    materialized: Materialized,
) -> Outcome {
    let branches = branch_outcomes(
        calculation.zip(calculation_ref.as_deref()),
        observation,
        &observation_ref,
        materialized.comparison_complete,
        &comparison_ref,
    );
    Outcome {
        calculation: calculation_ref,
        observation: observation_ref,
        comparison: comparison_ref,
        units: materialized.units,
        evidence: materialized.retry_evidence,
        has_citations: materialized.has_citations,
        read_starts: materialized.read_starts,
        raw_bytes: materialized.raw_bytes,
        complete: branches
            .iter()
            .all(|branch| branch.state == BranchState::Confirmed),
        branches,
    }
}

fn compound_scope(bundle: &InputBundle, item: &IndependentRequest) -> ResearchScope {
    ResearchScope {
        input_fingerprint: bundle.fingerprint.clone(),
        source_range: (item.source.start, item.source.end),
    }
}

pub(crate) fn observe_composed(
    store: &Store,
    bundle: &InputBundle,
    item: &IndependentRequest,
    action: ResearchAction<'_>,
    budget: Result<&mut ports::RetrievalBudget>,
    admit: &dyn Fn() -> Result<()>,
) -> Result<Outcome> {
    let mut observation = read(store, &bundle.request, action, budget, admit)?;
    observation.scope = Some(compound_scope(bundle, item));
    retain_outcome(
        store,
        bundle,
        action,
        observation,
        None,
        bundle.request.budget.max_actions as usize,
    )
}

pub(crate) fn replay_composed(
    store: &Store,
    bundle: &InputBundle,
    item: &IndependentRequest,
    action: ResearchAction<'_>,
    reference: &str,
) -> Result<Outcome> {
    let observation =
        load_observation(store, bundle, reference, Some(compound_scope(bundle, item)))?;
    let materialized = materialize(
        store,
        bundle,
        action,
        &observation,
        &None,
        bundle.request.budget.max_actions as usize,
    )?;
    let comparison = digest(&materialized.report_bytes);
    if store.get_bytes(
        &bundle.request.actor_id,
        &bundle.request.case_id,
        &comparison,
    )? != materialized.report_bytes
    {
        return Err(mismatch());
    }
    Ok(evaluated_outcome(
        &observation,
        reference.into(),
        comparison,
        None,
        None,
        materialized,
    ))
}

fn load_observation(
    store: &Store,
    bundle: &InputBundle,
    reference: &str,
    scope: Option<ResearchScope>,
) -> Result<Observation> {
    let request = &bundle.request;
    let bytes = store.get_bytes(&request.actor_id, &request.case_id, reference)?;
    let observation: Observation = serde_json::from_slice(&bytes)?;
    if canonical(&observation)? != bytes || observation.scope != scope {
        return Err(mismatch());
    }
    Ok(observation)
}

pub(crate) fn verify_delivery(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<Outcome> {
    verify_web_research_input(bundle)?;
    let action = ResearchAction::for_bundle(bundle)?;
    let (comparison, refs) = result.dependencies.split_last().ok_or_else(mismatch)?;
    let (observation, refs) = refs.split_last().ok_or_else(mismatch)?;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let calculation = local_calculation(action.expression)?;
    let observed = load_observation(store, bundle, observation, None)?;
    let calc_ref = match (refs, calculation.as_ref()) {
        ([reference], Some(calc))
            if store.get_bytes(actor, case, reference)? == canonical(calc)? =>
        {
            Some(reference.clone())
        }
        ([], None) => None,
        _ => return Err(mismatch()),
    };
    let materialized = materialize(
        store,
        bundle,
        action,
        &observed,
        &calculation,
        standalone_starts(bundle, action)?,
    )?;
    if store.get_bytes(actor, case, comparison)? != materialized.report_bytes {
        return Err(mismatch());
    }
    let outcome = evaluated_outcome(
        &observed,
        observation.clone(),
        comparison.clone(),
        calculation.as_ref(),
        calc_ref,
        materialized,
    );
    if !outcome.complete
        || (!result.branch_outcomes.is_empty() && result.branch_outcomes != outcome.branches)
        || result.guarantee != Guarantee::SourceReport
        || plan.guarantee != result.guarantee
        || result.calculation.is_some()
        || result.evidence != outcome.evidence
        || result.text != plan.text
        || plan.required_units != outcome.units
        || result.text != outcome.units.join("\n")
    {
        return Err(mismatch());
    }
    Ok(outcome)
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
    let observed = load_observation(store, bundle, observation, None)?;
    let action = ResearchAction::for_bundle(bundle)?;
    let calculation = local_calculation(action.expression)?;
    let materialized = materialize(
        store,
        bundle,
        action,
        &observed,
        &calculation,
        standalone_starts(bundle, action)?,
    )?;
    if materialized.retry_evidence != result.evidence {
        return Err(mismatch());
    }
    Ok(())
}
