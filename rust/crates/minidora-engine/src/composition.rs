//! Planning and observation for bounded lists of requests within one Goal.
use crate::{
    composition_input, composition_source,
    core::{exact_calculation_unit, read_case_changed, supports_english_output_locale},
    input, language, math, ports,
    scheduler::{GoalNode, GoalNodeKind, Scheduler},
    store::Store,
};
use minidora_domain::{
    BranchOutcome, BranchState, ContentPlan, Error, EvidencePacket, ExplanationAudience,
    ExplanationRequest, Guarantee, IndependentRequest, InputBundle, Intent, Result, ResultRecord,
    SourceSpan, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

pub(crate) const PREDICATE: &str = "INDEPENDENT_REQUESTS_V1";
pub(crate) const PARSER_VERSION: &str = "independent-request-list-v1";
const MAX_ITEMS: usize = 8;
const MAX_ITEM_BYTES: usize = 8192;

pub(crate) fn parse(text: &str, lower: impl Fn(&str) -> Result<Intent>) -> Result<Option<Intent>> {
    let mut offset = 0;
    let mut lines = text.split_inclusive('\n').filter_map(|raw| {
        let start = offset;
        offset += raw.len();
        let line = raw.trim_end_matches(['\r', '\n']);
        (!line.trim().is_empty()).then_some((start, line))
    });
    let Some((start, first)) = lines.next() else {
        return Ok(None);
    };
    let header = matches!(first.trim_end(), "別々の依頼:" | "別々の依頼：")
        || first
            .trim_end()
            .eq_ignore_ascii_case("Independent requests:");
    if !header
        && (!first.starts_with("- ")
            || !text[start + first.len()..]
                .lines()
                .any(|line| line.starts_with("- ")))
    {
        return Ok(None);
    }
    let invalid = || {
        Some(Intent::Unknown {
            reason: "An independent request list needs two to eight top-level items; indent any continuation by at least two spaces."
                .into(),
        })
    };
    let first_item = (!header).then_some((start, first));
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for (line_start, line) in first_item.into_iter().chain(lines) {
        if line.chars().any(char::is_control) {
            return Ok(invalid());
        }
        if let Some(body) = line.strip_prefix("- ") {
            let item = body.trim();
            if ranges.len() == MAX_ITEMS || item.is_empty() {
                return Ok(invalid());
            }
            let start = line_start + 2 + body.len() - body.trim_start().len();
            ranges.push((start, start + item.len()));
        } else if line.starts_with("  ")
            && let Some((_, end)) = ranges.last_mut()
        {
            *end = line_start + line.trim_end().len();
        } else {
            return Ok(invalid());
        }
    }
    if ranges.len() < 2
        || ranges
            .iter()
            .any(|(start, end)| end - start > MAX_ITEM_BYTES)
    {
        return Ok(invalid());
    }
    let items = ranges
        .into_iter()
        .enumerate()
        .map(|(index, (start, end))| {
            let source = SourceSpan {
                start,
                end,
                text: text[start..end].to_owned(),
            };
            let parsed = composition_input::parse(&source.text, index)
                .and_then(|intent| intent.map_or_else(|| lower(&source.text), Ok));
            let intent = match parsed {
                Ok(intent) => intent,
                Err(Error::Contract { code, .. }) if is_reference_input_stop(&code) => {
                    Intent::Unknown { reason: code }
                }
                Err(Error::Contract { code, message }) => Intent::Unknown {
                    reason: format!("{code}: {message}"),
                },
                Err(error) => return Err(error),
            };
            Ok(IndependentRequest {
                source,
                intent: Box::new(intent),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some(Intent::IndependentRequests { items }))
}

fn node_id(index: usize) -> String {
    format!("request-{}", index + 1)
}

fn is_reference_input_stop(code: &str) -> bool {
    matches!(
        code,
        "BRANCH_REFERENCE_NOT_PRIOR" | "BRANCH_REFERENCE_INVALID"
    )
}

fn prerequisite_ids(item: &IndependentRequest) -> &[String] {
    match item.intent.as_ref() {
        Intent::CalculateFromRequestItems { requires, .. } => requires,
        Intent::SelectRequestJsonNumber { source, .. } => std::slice::from_ref(source),
        _ => &[],
    }
}

pub(crate) fn nodes(items: &[IndependentRequest]) -> impl Iterator<Item = GoalNode> {
    items.iter().enumerate().map(|(index, item)| GoalNode {
        id: node_id(index),
        kind: if composition_source::read_url(item).is_some() {
            GoalNodeKind::Evidence
        } else {
            GoalNodeKind::CoreResult
        },
        requires: prerequisite_ids(item).to_vec(),
    })
}

#[derive(Serialize)]
struct Value {
    guarantee: Guarantee,
    units: Vec<String>,
    bytes: Vec<u8>,
    #[serde(skip)]
    quantities: BTreeMap<&'static str, String>,
}

#[derive(Serialize)]
struct Observation<'a> {
    node_id: &'a str,
    input_fingerprint: &'a str,
    source_range: (usize, usize),
    state: BranchState,
    reason_code: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    evidence_ref: Option<&'a str>,
    value: Option<Value>,
}

enum Branch {
    Value(Value),
    Partial(Value, &'static str),
    Blocked(&'static str),
}

fn evaluate(
    item: &IndependentRequest,
    bundle: &InputBundle,
    english: bool,
    quantities: &BTreeMap<String, language::SequenceValue>,
    prerequisites: &BTreeMap<String, String>,
) -> Result<Branch> {
    let mut partial_reason = None;
    let value = match item.intent.as_ref() {
        Intent::Calculate { expression } => {
            let calculation = language::calculate(expression)?;
            Value {
                units: vec![exact_calculation_unit(&calculation)],
                bytes: canonical(&calculation)?,
                quantities: BTreeMap::from([("", calculation.exact_value)]),
                guarantee: Guarantee::Computed,
            }
        }
        Intent::CalculateFromRequestItems { expression, .. } => {
            let resolved = match language::calculate_sequence_expression(expression, quantities) {
                Ok(resolved) => resolved,
                Err(Error::Contract { code, .. })
                    if code == "CALCULATION_REFERENCE_UNAVAILABLE" =>
                {
                    return Ok(Branch::Blocked("BRANCH_REFERENCE_VALUE_UNAVAILABLE"));
                }
                Err(error) => return Err(error),
            };
            let used: BTreeSet<_> = resolved
                .consumed
                .iter()
                .map(|usage| &usage.artifact_sha256)
                .collect();
            if used != prerequisites.values().collect() {
                return Err(failure(
                    "BRANCH_REFERENCE_MISMATCH",
                    "Consumed values do not match the observed prerequisite artifacts.",
                ));
            }
            Value {
                units: vec![exact_calculation_unit(&resolved.calculation)],
                bytes: canonical(&resolved)?,
                quantities: BTreeMap::from([("", resolved.calculation.exact_value)]),
                guarantee: Guarantee::Computed,
            }
        }
        Intent::Statistics { .. }
        | Intent::FiniteProbabilityStatistics { .. }
        | Intent::DescribeStatistics { .. } => {
            if english {
                return Ok(Branch::Blocked("BRANCH_LOCALE_UNAVAILABLE"));
            }
            if let Intent::DescribeStatistics {
                show_values,
                explanation:
                    Some(ExplanationRequest {
                        audience: ExplanationAudience::Unresolved(_),
                        ..
                    }),
                ..
            } = item.intent.as_ref()
            {
                const AUDIENCE_UNRESOLVED: &str = "BRANCH_AUDIENCE_UNRESOLVED";
                if !show_values {
                    return Ok(Branch::Blocked(AUDIENCE_UNRESOLVED));
                }
                partial_reason = Some(AUDIENCE_UNRESOLVED);
            }
            let report = math::conversational_math_report(&item.intent, true)?;
            let quantities = math::conversational_exact_quantities(&item.intent, &report.bytes)?;
            Value {
                units: report.units,
                bytes: report.bytes,
                quantities,
                guarantee: Guarantee::Computed,
            }
        }
        Intent::TextSummaryCandidates { .. } | Intent::SummarizePastedText { .. } => {
            if english {
                return Ok(Branch::Blocked("BRANCH_LOCALE_UNAVAILABLE"));
            }
            let mut request = bundle.request.clone();
            request.text.clone_from(&item.source.text);
            request.permissions = Default::default();
            let leaf = input::seal(&request, None)?;
            if leaf.meaning.intent != *item.intent || !leaf.meaning.unresolved.is_empty() {
                return Ok(Branch::Blocked("BRANCH_INPUT_UNVERIFIED"));
            }
            let summary = crate::text_summary::direct_summary_candidates(&leaf)?;
            let bytes = canonical(&summary)?;
            Value {
                units: vec![summary.text],
                bytes,
                quantities: BTreeMap::new(),
                guarantee: Guarantee::Hypothesis,
            }
        }
        Intent::Unknown { reason } if is_reference_input_stop(reason) => {
            return Ok(Branch::Blocked(if reason == "BRANCH_REFERENCE_NOT_PRIOR" {
                "BRANCH_REFERENCE_NOT_PRIOR"
            } else {
                "BRANCH_REFERENCE_INVALID"
            }));
        }
        Intent::Unknown { .. } => return Ok(Branch::Blocked("BRANCH_CAPABILITY_UNAVAILABLE")),
        _ => return Ok(Branch::Blocked("BRANCH_CAPABILITY_NOT_CONNECTED")),
    };
    if value.units.is_empty() {
        return Ok(Branch::Blocked("BRANCH_VISIBLE_RESULT_UNAVAILABLE"));
    }
    if let Some(reason) = partial_reason {
        return Ok(Branch::Partial(value, reason));
    }
    Ok(Branch::Value(value))
}

pub(crate) struct Outcome {
    pub(crate) branches: Vec<BranchOutcome>,
    pub(crate) retry_evidence: Vec<EvidencePacket>,
    pub(crate) units: Vec<String>,
    pub(crate) guarantee: Guarantee,
}

impl Outcome {
    pub(crate) fn new() -> Self {
        Self {
            branches: Vec::new(),
            retry_evidence: Vec::new(),
            units: Vec::new(),
            guarantee: Guarantee::Unknown,
        }
    }

    pub(crate) fn complete(&self) -> bool {
        self.branches
            .iter()
            .all(|branch| branch.state == BranchState::Confirmed)
    }
}

pub(crate) fn produce(
    store: &Store,
    bundle: &InputBundle,
    graph: &mut Scheduler,
    started: Instant,
    retrieval: Option<Result<ports::RetrievalBudget>>,
    admit: &dyn Fn() -> Result<()>,
    output: &mut Outcome,
) -> Result<()> {
    let request = &bundle.request;
    let items = delivery_items(bundle)?;
    let render_reserve = (request.budget.deadline_ms / 4).min(250);
    let compute_limit = Duration::from_millis(request.budget.deadline_ms - render_reserve);
    let source_count = items
        .iter()
        .filter(|item| composition_source::read_url(item).is_some())
        .count();
    let mut reads =
        composition_source::SourceReads::new(retrieval, &request.budget, source_count, admit)?;
    evaluate_items(
        store,
        bundle,
        items,
        graph,
        || started.elapsed() >= compute_limit,
        |_, uri| reads.observe(store, bundle, uri),
        |_, bytes| store.put_bytes(&request.actor_id, &request.case_id, bytes),
        output,
    )
}

fn delivery_items(bundle: &InputBundle) -> Result<&[IndependentRequest]> {
    let request = &bundle.request;
    let Intent::IndependentRequests { items } = &bundle.meaning.intent else {
        return Err(failure(
            "COMPOUND_INPUT_MISMATCH",
            "An independent request list is required.",
        ));
    };
    let english = supports_english_output_locale(&request.locale);
    if !request.attachments.is_empty()
        || request.context_ref.is_some()
        || request.speech.is_some()
        || request.purpose.is_some()
    {
        return Err(failure(
            "COMPOUND_SCOPE_UNAVAILABLE",
            if english {
                "Independent items accept standalone text with an optional file destination."
            } else {
                "複数の依頼は入力欄に書いてください。保存先は指定できます。添付、参照先、音声、用途指定は外してください。"
            },
        ));
    }
    if bundle.meaning.parser_version != PARSER_VERSION
        || !bundle.meaning.unresolved.is_empty()
        || input::parse_independent_requests(&request.text)?.as_ref()
            != Some(&bundle.meaning.intent)
    {
        return Err(failure(
            "COMPOUND_INPUT_MISMATCH",
            if english {
                "The items could not be matched to the original text."
            } else {
                "個別の依頼を入力原文と照合できませんでした。"
            },
        ));
    }
    Ok(items)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn evaluate_items(
    store: &Store,
    bundle: &InputBundle,
    items: &[IndependentRequest],
    graph: &mut Scheduler,
    time_exhausted: impl Fn() -> bool,
    mut source_packet: impl FnMut(usize, &str) -> Result<(String, EvidencePacket)>,
    mut retain_observation: impl FnMut(usize, &[u8]) -> Result<String>,
    output: &mut Outcome,
) -> Result<()> {
    let request = &bundle.request;
    let english = supports_english_output_locale(&request.locale);
    // The remaining half covers the shared plan/render and their retained bytes.
    let mut evidence_remaining = request.budget.max_evidence_bytes / 2;
    let mut sources = BTreeMap::<String, String>::new();
    let mut quantities = BTreeMap::new();
    for (index, item) in items.iter().enumerate() {
        let id = node_id(index);
        let prerequisites = prerequisite_ids(item)
            .iter()
            .map(|id| {
                let observed = graph.observed().get(id).ok_or_else(|| {
                    failure(
                        "UNOBSERVED_PREREQUISITE",
                        "A composed item requires an observation in this Goal.",
                    )
                })?;
                Ok((id.clone(), observed.clone()))
            })
            .collect::<Result<BTreeMap<_, _>>>()?;
        let blocked_predecessor = output.branches.iter().any(|outcome| {
            prerequisites.contains_key(&outcome.node_id) && outcome.state != BranchState::Confirmed
        });
        let mut evidence_ref = None;
        let mut retry_evidence = None;
        let available = evidence_remaining / (items.len() - index);
        let evaluated = if time_exhausted() {
            Ok(Branch::Blocked("BRANCH_TIME_BUDGET"))
        } else if blocked_predecessor {
            Ok(Branch::Blocked("BRANCH_REFERENCE_PREREQUISITE_UNCONFIRMED"))
        } else if let Some(uri) = composition_source::read_url(item) {
            // The failed branch record must fit even if the read has no value.
            if available < 512 {
                Ok(Branch::Blocked("BRANCH_EVIDENCE_BUDGET"))
            } else {
                source_packet(index, uri).and_then(|(reference, packet)| {
                    let value = if let Some(code) = &packet.failure {
                        Err(failure(code, "The requested source was not obtained."))
                    } else {
                        let unit = if english {
                            format!("Source retained: {}", packet.final_uri)
                        } else {
                            format!("本文を取得しました。出典は {} です。", packet.final_uri)
                        };
                        Ok(Branch::Value(Value {
                            units: vec![unit],
                            bytes: canonical(&(&packet.id, &packet.raw_sha256, &reference))?,
                            quantities: BTreeMap::new(),
                            guarantee: Guarantee::SourceReport,
                        }))
                    };
                    evidence_ref = Some(reference);
                    if packet.retry_after.is_some() {
                        retry_evidence = Some(packet);
                    }
                    value
                })
            }
        } else if let Intent::SelectRequestJsonNumber { source, pointer } = item.intent.as_ref() {
            match sources.get(source) {
                Some(reference) => {
                    composition_source::select_json_number(store, bundle, reference, pointer)
                        .and_then(|selection| {
                            let unit = if english {
                                format!("JSON number at {pointer}: {}", selection.exact_value)
                            } else {
                                format!(
                                    "JSONの {pointer} の数値は {} です。",
                                    selection.exact_value
                                )
                            };
                            Ok(Branch::Value(Value {
                                units: vec![unit],
                                bytes: canonical(&selection)?,
                                quantities: BTreeMap::from([("", selection.exact_value)]),
                                guarantee: Guarantee::SourceReport,
                            }))
                        })
                }
                None => Ok(Branch::Blocked("BRANCH_REFERENCE_SOURCE_UNAVAILABLE")),
            }
        } else {
            evaluate(item, bundle, english, &quantities, &prerequisites)
        };
        let (state, reason, value) = match evaluated {
            Ok(Branch::Value(value)) => (BranchState::Confirmed, None, Some(value)),
            Ok(Branch::Partial(value, code)) => {
                (BranchState::Blocked, Some(code.into()), Some(value))
            }
            Ok(Branch::Blocked(code)) => (BranchState::Blocked, Some(code.into()), None),
            Err(error) if read_case_changed(&error) => return Err(error),
            Err(Error::Contract { code, .. }) if code == ports::READ_PERMISSION_REQUIRED => {
                (BranchState::Blocked, Some(code), None)
            }
            Err(Error::Contract { code, .. }) => (BranchState::Failed, Some(code), None),
            Err(error) => return Err(error),
        };
        let mut observed = Observation {
            node_id: &id,
            input_fingerprint: &bundle.fingerprint,
            source_range: (item.source.start, item.source.end),
            state,
            reason_code: reason.as_deref(),
            evidence_ref: evidence_ref.as_deref(),
            value,
        };
        let mut bytes = canonical(&observed)?;
        let mut units = display_units(&observed, index + 1, english);
        // Carry unused bytes forward while reserving room for later outcomes.
        if observed.value.is_some() && observation_size(&bytes, &units) > available {
            observed.state = BranchState::Blocked;
            observed.reason_code = Some("BRANCH_EVIDENCE_BUDGET");
            observed.value = None;
            bytes = canonical(&observed)?;
            units = display_units(&observed, index + 1, english);
        }
        evidence_remaining = evidence_remaining
            .checked_sub(observation_size(&bytes, &units))
            .ok_or_else(|| {
                failure(
                    "RESOURCE_LIMIT",
                    if english {
                        "The shared evidence budget cannot retain each item status."
                    } else {
                        "各項目の状態を保持するための証拠予算が足りません。"
                    },
                )
            })?;
        let reference = retain_observation(index, &bytes)?;
        graph.observe(&id, &reference, &prerequisites)?;
        if observed.state == BranchState::Confirmed
            && let Some(evidence) = observed.evidence_ref
        {
            sources.insert(id.clone(), evidence.to_owned());
        }
        if let Some(value) = observed.value {
            if output.guarantee == Guarantee::Unknown
                || value.guarantee == Guarantee::Hypothesis
                || (output.guarantee == Guarantee::Computed
                    && value.guarantee == Guarantee::SourceReport)
            {
                output.guarantee = value.guarantee;
            }
            if observed.state == BranchState::Confirmed {
                quantities.extend(value.quantities.into_iter().map(|(field, exact_value)| {
                    let key = if field.is_empty() {
                        id.clone()
                    } else {
                        format!("{id}-{field}")
                    };
                    (
                        key,
                        language::SequenceValue {
                            exact_value,
                            artifact_sha256: reference.clone(),
                        },
                    )
                }));
            }
        }
        let state = observed.state;
        let reason_code = observed.reason_code.map(str::to_owned);
        output.units.extend(units);
        output.branches.push(BranchOutcome {
            node_id: id,
            state,
            reason_code,
            artifact_ref: reference,
        });
        if let Some(packet) = retry_evidence {
            output.retry_evidence.push(packet);
        }
    }
    Ok(())
}

fn observation_size(bytes: &[u8], units: &[String]) -> usize {
    bytes
        .len()
        .saturating_add(units.iter().map(|unit| unit.len() + 1).sum::<usize>())
}

fn display_units(observation: &Observation<'_>, number: usize, english: bool) -> Vec<String> {
    let heading = if english {
        format!("Request {number}")
    } else {
        format!("依頼 {number}")
    };
    if let Some(value) = &observation.value {
        let heading = if observation.state == BranchState::Blocked {
            if english {
                format!("{heading} (partially complete)")
            } else {
                format!("{heading}（一部保留）")
            }
        } else {
            heading
        };
        return std::iter::once(heading)
            .chain(value.units.iter().cloned())
            .collect();
    }
    let status = match (english, &observation.state, observation.reason_code) {
        (true, _, Some("BRANCH_LOCALE_UNAVAILABLE")) => {
            "English output is unavailable for this item."
        }
        (false, _, Some("BRANCH_AUDIENCE_UNRESOLVED")) => {
            "指定された読者向けの説明は保留しました。"
        }
        (true, _, Some("BRANCH_REFERENCE_NOT_PRIOR")) => "Choose an earlier item in this request.",
        (false, _, Some("BRANCH_REFERENCE_NOT_PRIOR")) => "同じ依頼の前の項目を指定してください。",
        (true, _, Some("BRANCH_REFERENCE_INVALID")) => "The item reference could not be read.",
        (false, _, Some("BRANCH_REFERENCE_INVALID")) => "参照先の指定を読み取れませんでした。",
        (true, _, Some("BRANCH_REFERENCE_PREREQUISITE_UNCONFIRMED")) => {
            "The referenced item is unfinished."
        }
        (false, _, Some("BRANCH_REFERENCE_PREREQUISITE_UNCONFIRMED")) => {
            "参照先の項目が未確定です。"
        }
        (true, _, Some("BRANCH_REFERENCE_VALUE_UNAVAILABLE")) => {
            "The referenced item has no usable value for this field."
        }
        (false, _, Some("BRANCH_REFERENCE_VALUE_UNAVAILABLE")) => {
            "参照先で指定の数値を利用できません。"
        }
        (true, _, Some(ports::READ_PERMISSION_REQUIRED)) => {
            "This source hostname is not authorized."
        }
        (false, _, Some(ports::READ_PERMISSION_REQUIRED)) => {
            "この資料の接続先は許可されていません。"
        }
        (true, _, Some("BRANCH_REFERENCE_SOURCE_UNAVAILABLE")) => {
            "The referenced item has no verified source body."
        }
        (false, _, Some("BRANCH_REFERENCE_SOURCE_UNAVAILABLE")) => {
            "参照先に照合済みの本文がありません。"
        }
        (true, BranchState::Failed, _) => "The operation failed.",
        (false, BranchState::Failed, _) => "処理に失敗しました。",
        (true, _, _) => "This item could not be completed.",
        (false, _, _) => "この項目は保留しました。",
    };
    vec![format!("{heading}　{status}")]
}

#[derive(Deserialize)]
struct RetainedObservation {
    node_id: String,
    input_fingerprint: String,
    source_range: (usize, usize),
    evidence_ref: Option<String>,
}

/// Verify completed items against the sealed input and previously retained bytes.
pub(crate) fn verify_delivery(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<()> {
    let items = delivery_items(bundle)?;
    let request = &bundle.request;
    let invalid = || {
        failure(
            "RECOVERY_COMPOUND_MISMATCH",
            "Saved compound observations do not match the original file request.",
        )
    };
    if identity(request)? != result.id
        || result.branch_outcomes.len() != items.len()
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || plan.locale != request.locale
    {
        return Err(invalid());
    }
    let mut graph = Scheduler::new(nodes(items).collect(), request.budget.max_actions)?;
    let mut observed = Outcome::new();
    evaluate_items(
        store,
        bundle,
        items,
        &mut graph,
        || false,
        |index, uri| {
            let branch = &result.branch_outcomes[index];
            let bytes =
                store.get_bytes(&request.actor_id, &request.case_id, &branch.artifact_ref)?;
            let retained: RetainedObservation = serde_json::from_slice(&bytes)?;
            let reference = retained.evidence_ref.ok_or_else(invalid)?;
            let bytes = store.get_bytes(&request.actor_id, &request.case_id, &reference)?;
            let packet: EvidencePacket = serde_json::from_slice(&bytes)?;
            if packet.request_uri != uri || canonical(&packet)? != bytes {
                return Err(invalid());
            }
            ports::validate_successful_packet(&packet, &request.permissions)?;
            Ok((reference, packet))
        },
        |index, bytes| {
            let reference = &result.branch_outcomes[index].artifact_ref;
            if store.get_bytes(&request.actor_id, &request.case_id, reference)? != bytes {
                return Err(invalid());
            }
            Ok(reference.clone())
        },
        &mut observed,
    )?;
    let dependencies: Vec<_> = observed
        .branches
        .iter()
        .map(|branch| branch.artifact_ref.clone())
        .collect();
    if !observed.complete()
        || observed.branches != result.branch_outcomes
        || result.dependencies != dependencies
        || plan.dependencies.len() != dependencies.len() + 1
        || !plan.dependencies.starts_with(&dependencies)
        || observed.guarantee != result.guarantee
        || observed.guarantee != plan.guarantee
        || observed.units != plan.required_units
        || plan.text != observed.units.join("\n")
        || result.text != plan.text
    {
        return Err(invalid());
    }
    Ok(())
}

/// Confirm that a Retry-After arose from an authored fetch item and its
/// retained observation before the Core restores the case's transport gate.
pub(crate) fn verify_retry_source(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    packet: &EvidencePacket,
) -> Result<()> {
    let invalid = || {
        failure(
            "RETRY_AFTER_SOURCE_INVALID",
            "Saved Retry-After does not match an authored fetch item and its observation.",
        )
    };
    let Intent::IndependentRequests { items } = &bundle.meaning.intent else {
        return Err(invalid());
    };
    if identity(&bundle.request)? != result.id
        || !result.evidence.contains(packet)
        || packet.mode != "LIVE"
        || packet.packet_kind != "FETCH_FAILURE"
        || packet.provider_id != "https-direct-url-v1"
        || packet.failure.is_none()
        || packet.retry_after.is_none()
        || packet.raw_sha256 != digest(&packet.raw_body)
        || packet.id != ports::fetch_packet_identity(packet)?
    {
        return Err(invalid());
    }
    let request = &bundle.request;
    let bytes = canonical(packet)?;
    let reference = digest(&bytes);
    for (index, item) in items
        .iter()
        .enumerate()
        .filter(|(_, item)| composition_source::read_url(item) == Some(packet.request_uri.as_str()))
    {
        let Some(branch) = result.branch_outcomes.get(index) else {
            continue;
        };
        let id = node_id(index);
        if branch.node_id != id || !result.dependencies.contains(&branch.artifact_ref) {
            continue;
        }
        let retained =
            store.get_bytes(&request.actor_id, &request.case_id, &branch.artifact_ref)?;
        let observed: RetainedObservation = serde_json::from_slice(&retained)?;
        if observed.node_id == id
            && observed.input_fingerprint == bundle.fingerprint
            && observed.source_range == (item.source.start, item.source.end)
            && observed.evidence_ref.as_deref() == Some(reference.as_str())
            && store.get_bytes(&request.actor_id, &request.case_id, &reference)? == bytes
        {
            return ports::validate_observed_route(packet, &request.permissions);
        }
    }
    Err(invalid())
}
