//! Planning and observation for bounded lists of requests within one Goal.
use crate::{
    attachment_analysis::AnalysisAction,
    composition_input,
    composition_source::{self, SourceObservation, SourceRequest},
    core::{
        exact_calculation_unit, observe_initial_text_generation, read_case_changed,
        supports_english_output_locale,
    },
    input, language, math,
    news::{self, NewsAction},
    ports,
    research_goal::{self, ResearchAction},
    scheduler::{GoalNode, GoalNodeKind, Scheduler},
    store::Store,
};
use minidora_domain::{
    BranchOutcome, BranchState, Calculation, ContentPlan, Error, EvidencePacket,
    ExplanationAudience, ExplanationRequest, Guarantee, IndependentRequest, InputBundle, Intent,
    Lifecycle, Result, ResultRecord, SourceSpan, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

pub(crate) const PREDICATE: &str = "INDEPENDENT_REQUESTS_V1";
pub(crate) const PARSER_VERSION: &str = "independent-request-list-v1";
pub(crate) const GENERATED_PARSER_VERSION: &str = "generated-independent-request-list-v1";
pub(crate) const BILINGUAL_STATISTICS_PARSER_VERSION: &str =
    "bilingual-statistics-independent-request-list-v1";
pub(crate) const ATTACHED_PARSER_VERSION: &str = "attached-independent-request-list-v1";
pub(crate) const NEWS_PARSER_VERSION: &str = "news-independent-request-list-v1";
pub(crate) const RESEARCH_PARSER_VERSION: &str = "research-independent-request-list-v1";
pub(crate) const DIVERSE_RESEARCH_PARSER_VERSION: &str =
    "distinct-publishers-independent-request-list-v1";
pub(crate) const EXPLANATION_PREDICATE: &str = "PREVIOUS_COMPOUND_EXPLANATION_V1";
pub(crate) const EXPLANATION_SOURCE_NODE: &str = "previous-compound-observations";
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
    if let Some(action) = language::CalculationAction::bind(&item.intent) {
        return action.requires();
    }
    match item.intent.as_ref() {
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

#[derive(Serialize, Deserialize)]
struct Value {
    guarantee: Guarantee,
    units: Vec<String>,
    bytes: Vec<u8>,
    #[serde(skip)]
    quantities: BTreeMap<&'static str, String>,
}

#[derive(Serialize, Deserialize)]
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

fn initial_text_generation_item(item: &IndependentRequest) -> bool {
    matches!(
        item.intent.as_ref(),
        Intent::DraftMessage { .. } | Intent::TranslateText { .. }
    )
}

fn observe_generated_item(
    bundle: &InputBundle,
    item: &IndependentRequest,
    deadline: Instant,
) -> Result<Option<(InputBundle, Value)>> {
    let Some(leaf) = input::generated_independent_leaf(&bundle.request, item)? else {
        return Ok(None);
    };
    Ok(
        observe_initial_text_generation(&leaf, deadline)?.map(|(text, bytes)| {
            (
                leaf,
                Value {
                    guarantee: Guarantee::Computed,
                    units: vec![text],
                    bytes,
                    quantities: BTreeMap::new(),
                },
            )
        }),
    )
}

fn evaluate(
    item: &IndependentRequest,
    bundle: &InputBundle,
    english: bool,
    deadline: Option<Instant>,
    quantities: &BTreeMap<String, language::SequenceValue>,
    prerequisites: &BTreeMap<String, String>,
) -> Result<Branch> {
    if generated_items_enabled(bundle) && initial_text_generation_item(item) {
        let Some(end) = deadline else {
            return Ok(Branch::Blocked("BRANCH_TIME_BUDGET"));
        };
        return match observe_generated_item(bundle, item, end) {
            Ok(Some((_, value))) => Ok(Branch::Value(value)),
            Ok(None) => Ok(Branch::Blocked("BRANCH_INPUT_UNVERIFIED")),
            Err(Error::Contract { code, .. }) if code == "MODEL_DEADLINE" => {
                Ok(Branch::Blocked("BRANCH_TIME_BUDGET"))
            }
            Err(Error::Contract { .. }) => Ok(Branch::Blocked("BRANCH_GENERATION_UNAVAILABLE")),
            Err(error) => Err(error),
        };
    }
    if let Some(action) = language::CalculationAction::bind(&item.intent) {
        let observed = match action.observe(quantities, prerequisites) {
            Ok(value) => value,
            Err(Error::Contract { code, .. }) if code == "CALCULATION_REFERENCE_UNAVAILABLE" => {
                return Ok(Branch::Blocked("BRANCH_REFERENCE_VALUE_UNAVAILABLE"));
            }
            Err(error) => return Err(error),
        };
        let calculation = observed.calculation();
        return Ok(Branch::Value(Value {
            units: vec![exact_calculation_unit(calculation)],
            bytes: observed.proof_bytes()?,
            quantities: BTreeMap::from([("", calculation.exact_value.clone())]),
            guarantee: Guarantee::Computed,
        }));
    }
    if let Some(action) = math::ProbabilityAction::bind(&item.intent) {
        let observed = action.observe()?;
        return Ok(Branch::Value(Value {
            units: vec![observed.unit],
            bytes: observed.bytes,
            quantities: observed
                .quantity
                .map_or_else(BTreeMap::new, |value| BTreeMap::from([("", value)])),
            guarantee: Guarantee::Computed,
        }));
    }
    let mut partial_reason = None;
    let value = match item.intent.as_ref() {
        Intent::Statistics { .. }
        | Intent::FiniteProbabilityStatistics { .. }
        | Intent::DescribeStatistics { .. } => {
            let english_statistics = bilingual_statistics_enabled(bundle)
                && matches!(&*item.intent, Intent::DescribeStatistics { .. });
            if english && !english_statistics {
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
            let report = math::conversational_math_report(&item.intent, true, english)?;
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
    let source_count = live_read_count(bundle, items);
    // Keep time for evaluating independent items after a slow external provider.
    let output_reserve = if bundle.meaning.parser_version == DIVERSE_RESEARCH_PARSER_VERSION {
        research_goal::output_reserve_time(request.budget.deadline_ms)
    } else {
        Duration::from_millis((request.budget.deadline_ms / 4).min(1000))
    };
    let mut reads = composition_source::SourceReads::new(
        retrieval,
        &request.budget,
        source_count,
        output_reserve,
        admit,
    )?;
    evaluate_items(
        store,
        bundle,
        items,
        graph,
        Some(started + compute_limit),
        |_, source| reads.observe(store, bundle, source),
        |_, bytes| store.put_bytes(&request.actor_id, &request.case_id, bytes),
        output,
    )
}

pub(crate) fn live_read_count(bundle: &InputBundle, items: &[IndependentRequest]) -> usize {
    let allow_news = news_items_enabled(bundle)
        && !news::admitted_publishers(&bundle.request.permissions).is_empty();
    items
        .iter()
        .filter(|item| {
            composition_source::read_url(item).is_some()
                || (allow_news && NewsAction::bind(&item.intent).is_some())
                || (research_items_enabled(bundle)
                    && ResearchAction::for_item(bundle, item)
                        .is_some_and(|action| action.is_admitted(&bundle.request.permissions)))
        })
        .count()
}

fn generated_items_enabled(bundle: &InputBundle) -> bool {
    bundle.meaning.parser_version == GENERATED_PARSER_VERSION
}

fn bilingual_statistics_enabled(bundle: &InputBundle) -> bool {
    matches!(
        bundle.meaning.parser_version.as_str(),
        BILINGUAL_STATISTICS_PARSER_VERSION | GENERATED_PARSER_VERSION
    )
}

fn news_items_enabled(bundle: &InputBundle) -> bool {
    matches!(
        bundle.meaning.parser_version.as_str(),
        NEWS_PARSER_VERSION | RESEARCH_PARSER_VERSION | DIVERSE_RESEARCH_PARSER_VERSION
    )
}

fn research_items_enabled(bundle: &InputBundle) -> bool {
    matches!(
        bundle.meaning.parser_version.as_str(),
        RESEARCH_PARSER_VERSION | DIVERSE_RESEARCH_PARSER_VERSION
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
    if request.context_ref.is_some() || request.speech.is_some() || request.purpose.is_some() {
        return Err(failure(
            "COMPOUND_SCOPE_UNAVAILABLE",
            if english {
                "Independent items accept text with optional attachments and a file destination."
            } else {
                "複数の依頼は入力欄に書いてください。添付と保存先を指定できます。参照先、音声、用途指定は外してください。"
            },
        ));
    }
    let matches_input = match bundle.meaning.parser_version.as_str() {
        PARSER_VERSION if request.attachments.is_empty() => {
            bundle.meaning.unresolved.is_empty()
                && input::parse_independent_requests(&request.text)?.as_ref()
                    == Some(&bundle.meaning.intent)
        }
        BILINGUAL_STATISTICS_PARSER_VERSION => {
            input::bilingual_statistics_independent_input(request)?.as_ref() == Some(bundle)
        }
        GENERATED_PARSER_VERSION => {
            input::generated_independent_input(request)?.as_ref() == Some(bundle)
        }
        ATTACHED_PARSER_VERSION => {
            input::validate_request_attachments(request)?;
            input::attached_independent_input(request)?.as_ref() == Some(bundle)
        }
        NEWS_PARSER_VERSION => {
            input::validate_request_attachments(request)?;
            input::news_independent_input(request)?.as_ref() == Some(bundle)
        }
        RESEARCH_PARSER_VERSION => {
            input::validate_request_attachments(request)?;
            input::research_independent_input(request)?.as_ref() == Some(bundle)
        }
        DIVERSE_RESEARCH_PARSER_VERSION => {
            input::validate_request_attachments(request)?;
            input::diverse_research_independent_input(request)?.as_ref() == Some(bundle)
        }
        _ => false,
    };
    if !matches_input {
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
    deadline: Option<Instant>,
    mut read_source: impl FnMut(usize, SourceRequest<'_>) -> Result<SourceObservation>,
    mut retain_observation: impl FnMut(usize, &[u8]) -> Result<String>,
    output: &mut Outcome,
) -> Result<()> {
    let request = &bundle.request;
    let english = supports_english_output_locale(&request.locale);
    // The remaining half covers the shared plan/render and their retained bytes.
    let mut evidence_remaining = request.budget.max_evidence_bytes / 2;
    let mut sources = BTreeMap::<String, String>::new();
    let mut quantities = BTreeMap::new();
    let attached = !request.attachments.is_empty();
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
        let mut retry_evidence = Vec::new();
        let available = evidence_remaining / (items.len() - index);
        let evaluated = if deadline.is_some_and(|end| Instant::now() >= end) {
            Ok(Branch::Blocked("BRANCH_TIME_BUDGET"))
        } else if blocked_predecessor {
            Ok(Branch::Blocked("BRANCH_REFERENCE_PREREQUISITE_UNCONFIRMED"))
        } else if news_items_enabled(bundle)
            && let Some(action) = NewsAction::bind(&item.intent)
        {
            if news::admitted_publishers(&request.permissions).is_empty() {
                Ok(Branch::Blocked(ports::READ_PERMISSION_REQUIRED))
            } else if available < 512 {
                Ok(Branch::Blocked("BRANCH_EVIDENCE_BUDGET"))
            } else {
                read_source(index, SourceRequest::News { item, action }).and_then(|read| {
                    let SourceObservation::News(read) = read else {
                        return Err(source_kind_mismatch());
                    };
                    evidence_ref = Some(read.reference);
                    retry_evidence = read.retry_evidence;
                    let value = Value {
                        guarantee: if read.confirmed {
                            Guarantee::SourceReport
                        } else {
                            Guarantee::Unknown
                        },
                        units: read.units,
                        bytes: read.report_bytes,
                        quantities: BTreeMap::new(),
                    };
                    if read.complete {
                        Ok(Branch::Value(value))
                    } else {
                        Ok(Branch::Partial(
                            value,
                            if read.confirmed {
                                "NEWS_PARTIALLY_CONFIRMED"
                            } else {
                                "NEWS_ARTICLES_UNAVAILABLE"
                            },
                        ))
                    }
                })
            }
        } else if research_items_enabled(bundle)
            && let Some(action) = ResearchAction::for_item(bundle, item)
        {
            if !action.is_admitted(&request.permissions) {
                Ok(Branch::Blocked(ports::READ_PERMISSION_REQUIRED))
            } else if available < 512 {
                Ok(Branch::Blocked("BRANCH_EVIDENCE_BUDGET"))
            } else {
                read_source(index, SourceRequest::Research { item, action }).and_then(|read| {
                    let SourceObservation::Research(read) = read else {
                        return Err(source_kind_mismatch());
                    };
                    evidence_ref = Some(read.observation);
                    retry_evidence = read.evidence;
                    let value = Value {
                        guarantee: if read.has_citations {
                            Guarantee::SourceReport
                        } else {
                            Guarantee::Unknown
                        },
                        units: read.units,
                        bytes: canonical(&read.comparison)?,
                        quantities: BTreeMap::new(),
                    };
                    if read.complete {
                        Ok(Branch::Value(value))
                    } else {
                        Ok(Branch::Partial(value, action.incomplete_reason()))
                    }
                })
            }
        } else if let Some(uri) = composition_source::read_url(item) {
            // The failed branch record must fit even if the read has no value.
            if available < 512 {
                Ok(Branch::Blocked("BRANCH_EVIDENCE_BUDGET"))
            } else {
                read_source(index, SourceRequest::Url(uri)).and_then(|read| {
                    let SourceObservation::Url(reference, packet) = read else {
                        return Err(source_kind_mismatch());
                    };
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
                        retry_evidence.push(*packet);
                    }
                    value
                })
            }
        } else if attached
            && let Some(action) = AnalysisAction::bind(&item.intent)
            && let Some(end) = deadline
        {
            evaluate_attached_item(&action, bundle, english, end, available)
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
            evaluate(item, bundle, english, deadline, &quantities, &prerequisites)
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
            && composition_source::read_url(item).is_some()
            && let Some(evidence) = observed.evidence_ref
        {
            sources.insert(id.clone(), evidence.to_owned());
        }
        if let Some(value) = observed.value {
            if output.guarantee == Guarantee::Unknown
                || value.guarantee == Guarantee::Hypothesis
                || (output.guarantee == Guarantee::Computed
                    && value.guarantee == Guarantee::SourceReport)
                || (value.guarantee == Guarantee::Derived
                    && matches!(
                        output.guarantee,
                        Guarantee::Computed | Guarantee::SourceReport
                    ))
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
        output.retry_evidence.extend(retry_evidence);
    }
    Ok(())
}

fn source_kind_mismatch() -> Error {
    failure(
        "COMPOUND_SOURCE_MISMATCH",
        "The read observation does not match the authored operation.",
    )
}

fn evaluate_attached_item(
    action: &AnalysisAction<'_>,
    bundle: &InputBundle,
    english: bool,
    deadline: Instant,
    available: usize,
) -> Result<Branch> {
    let observed = action.observe(&bundle.request.attachments, english, deadline, available)?;
    let bytes = observed.proof_bytes()?;
    let value = Value {
        guarantee: Guarantee::Derived,
        units: observed.units,
        bytes,
        quantities: observed
            .exact_value
            .map(|exact| ("", exact))
            .into_iter()
            .collect(),
    };
    if !observed.condition_confirmed {
        Ok(Branch::Partial(
            value,
            "BRANCH_ATTACHMENT_CONDITION_UNCONFIRMED",
        ))
    } else if value.quantities.is_empty() {
        Ok(Branch::Partial(
            value,
            "BRANCH_ATTACHMENT_VALUE_UNAVAILABLE",
        ))
    } else {
        Ok(Branch::Value(value))
    }
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
    let status = branch_status(english, observation.state, observation.reason_code);
    vec![format!("{heading}　{status}")]
}

fn branch_status(english: bool, state: BranchState, reason: Option<&str>) -> &'static str {
    match (english, state, reason) {
        (true, _, Some("BRANCH_LOCALE_UNAVAILABLE")) => {
            "English output is unavailable for this item."
        }
        (true, _, Some("BRANCH_GENERATION_UNAVAILABLE")) => {
            "The item could not generate a verified text result."
        }
        (false, _, Some("BRANCH_GENERATION_UNAVAILABLE")) => {
            "この項目では照合済みの文面を作成できませんでした。"
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
        (true, _, Some("BRANCH_ATTACHMENT_CONDITION_UNCONFIRMED")) => {
            "The attached document did not establish the filter for this item."
        }
        (false, _, Some("BRANCH_ATTACHMENT_CONDITION_UNCONFIRMED")) => {
            "添付資料で絞り込み条件を確定できませんでした。"
        }
        (true, _, Some("BRANCH_ATTACHMENT_VALUE_UNAVAILABLE")) => {
            "The filtered rows did not provide a total."
        }
        (false, _, Some("BRANCH_ATTACHMENT_VALUE_UNAVAILABLE")) => {
            "絞り込んだ行から合計値を得られませんでした。"
        }
        (true, _, Some("WEB_RESEARCH_INSUFFICIENT_SOURCES")) => {
            "Fewer than two article bodies could be verified for this topic."
        }
        (false, _, Some("WEB_RESEARCH_INSUFFICIENT_SOURCES")) => {
            "この話題を確認できた記事本文が2件に達しませんでした。"
        }
        (true, _, Some("WEB_RESEARCH_INSUFFICIENT_PUBLISHERS")) => {
            "The available article bodies did not establish a comparison across publishers."
        }
        (false, _, Some("WEB_RESEARCH_INSUFFICIENT_PUBLISHERS")) => {
            "取得できた記事本文から、別々の発信元での比較を確認できませんでした。"
        }
        (true, _, Some("NEWS_FILE_DELIVERY_UNAVAILABLE")) => {
            "News can be shown in chat. Remove the file destination to retrieve it."
        }
        (false, _, Some("NEWS_FILE_DELIVERY_UNAVAILABLE")) => {
            "ニュースはチャット表示に対応しています。取得するには保存先の指定を外してください。"
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
    }
}

pub(crate) struct PreviousExplanation {
    pub(crate) units: Vec<String>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) source_ref: String,
}

type ObservationRefs = BTreeMap<String, String>;
type RetainedGraph = (
    Vec<GoalNode>,
    ObservationRefs,
    Option<ObservationRefs>,
    Option<ObservationRefs>,
    Option<ObservationRefs>,
    BTreeMap<String, Option<ObservationRefs>>,
);

fn explanation_mismatch(english: bool) -> Error {
    failure(
        "COMPOUND_EXPLANATION_SOURCE_INVALID",
        if english {
            "The saved item results could not be matched to the original request."
        } else {
            "保存された項目の記録を元の依頼と照合できませんでした。"
        },
    )
}

fn compound_items_for_selection<'a>(
    parent: &'a InputBundle,
    prior: &ResultRecord,
    selection: Option<usize>,
    english: bool,
) -> Result<&'a [IndependentRequest]> {
    let items = delivery_items(parent)?;
    let request = &parent.request;
    let invalid = || explanation_mismatch(english);
    if !matches!(prior.lifecycle, Lifecycle::Commit | Lifecycle::Suspend)
        || identity(request)? != prior.id
        || request.input_id != prior.input_id
        || request.input_version != prior.input_version
        || prior.branch_outcomes.len() != items.len()
    {
        return Err(invalid());
    }
    if selection.is_some_and(|number| !(1..=items.len()).contains(&number)) {
        let count = items.len();
        return Err(failure(
            "COMPOUND_ITEM_UNAVAILABLE",
            if english {
                format!(
                    "The previous request has {count} items. Choose a number from 1 to {count}."
                )
            } else {
                format!(
                    "前の依頼には {count} 項目あります。1 から {count} の番号を指定してください。"
                )
            },
        ));
    }
    Ok(items)
}

struct RetainedCompound<'a> {
    parent: &'a InputBundle,
    prior: &'a ResultRecord,
    items: &'a [IndependentRequest],
    expected_nodes: Vec<GoalNode>,
    observed: ObservationRefs,
    consumed: BTreeMap<String, Option<ObservationRefs>>,
    plan: &'a ContentPlan,
    source_ref: &'a str,
    render_ref: String,
    evidence_bytes: usize,
}

fn check_revision_evidence(bytes: usize, limit: usize, english: bool) -> Result<()> {
    if bytes > limit {
        return Err(failure(
            "RESOURCE_LIMIT",
            if english {
                "The saved compound item exceeds the evidence budget."
            } else {
                "保存済みの項目を照合するための証拠予算が不足しています。"
            },
        ));
    }
    Ok(())
}

fn read_retained_compound<'a>(
    store: &Store,
    parent: &'a InputBundle,
    prior: &'a ResultRecord,
    items: &'a [IndependentRequest],
    evidence_limit: Option<usize>,
    english: bool,
) -> Result<RetainedCompound<'a>> {
    let request = &parent.request;
    let invalid = || explanation_mismatch(english);
    let read = |reference| store.get_bytes(&request.actor_id, &request.case_id, reference);
    let source_ref = prior.execution_graph_ref.as_deref().ok_or_else(invalid)?;
    let graph_bytes = read(source_ref)?;
    let plan = prior.content_plan.as_ref().ok_or_else(invalid)?;
    let evidence_bytes = if let Some(limit) = evidence_limit {
        check_revision_evidence(graph_bytes.len(), limit, english)?;
        let evidence_bytes = graph_bytes
            .len()
            .saturating_add(canonical(plan)?.len())
            .saturating_add(canonical(prior.render.as_ref().ok_or_else(invalid)?)?.len());
        check_revision_evidence(evidence_bytes, limit, english)?;
        evidence_bytes
    } else {
        0
    };
    let (recorded_nodes, observed, _, _, _, consumed): RetainedGraph =
        serde_json::from_slice(&graph_bytes)?;
    let expected_nodes: Vec<_> = nodes(items).collect();
    let render_ref = crate::document::revalidate_previous_document_render(
        store,
        &request.actor_id,
        &request.case_id,
        prior,
        None,
        "COMPOUND_EXPLANATION_SOURCE_INVALID",
        "compound request",
    )
    .map_err(|error| match error {
        Error::Contract { .. } => invalid(),
        error => error,
    })?;
    let compute = digest(&canonical(&(
        &prior.calculation,
        &plan.required_units,
        &prior.evidence,
        &prior.dependencies,
    ))?);
    if !recorded_nodes.starts_with(&expected_nodes)
        || observed.get("content") != Some(&digest(&canonical(plan)?))
        || observed.get("render") != Some(&render_ref)
        || observed.get("compute") != Some(&compute)
        || plan.locale != request.locale
        || prior.calculation.is_some()
        || plan.dependencies.len() != items.len() + 1
        || !plan.dependencies.starts_with(&prior.dependencies)
        || plan.dependencies.last() != Some(&compute)
        || prior.dependencies.len() != items.len()
        || consumed.get("compute").and_then(Option::as_ref)
            != Some(
                &prior
                    .branch_outcomes
                    .iter()
                    .map(|branch| (branch.node_id.clone(), branch.artifact_ref.clone()))
                    .collect(),
            )
    {
        return Err(invalid());
    }
    Ok(RetainedCompound {
        parent,
        prior,
        items,
        expected_nodes,
        observed,
        consumed,
        plan,
        source_ref,
        render_ref,
        evidence_bytes,
    })
}

impl RetainedCompound<'_> {
    fn observation<'a>(
        &self,
        index: usize,
        bytes: &'a [u8],
        english: bool,
    ) -> Result<Observation<'a>> {
        let invalid = || explanation_mismatch(english);
        let node = &self.expected_nodes[index];
        let item = &self.items[index];
        let branch = &self.prior.branch_outcomes[index];
        let saved: Observation<'_> = serde_json::from_slice(bytes)?;
        let actual_inputs = node
            .requires
            .iter()
            .map(|id| {
                Ok((
                    id.clone(),
                    self.observed.get(id).ok_or_else(invalid)?.clone(),
                ))
            })
            .collect::<Result<ObservationRefs>>()?;
        if saved.node_id != node.id
            || branch.node_id != node.id
            || saved.input_fingerprint != self.parent.fingerprint
            || saved.source_range != (item.source.start, item.source.end)
            || saved.state != branch.state
            || saved.reason_code != branch.reason_code.as_deref()
            || (saved.state == BranchState::Confirmed) != saved.reason_code.is_none()
            || (saved.state == BranchState::Confirmed && saved.value.is_none())
            || (saved.state == BranchState::Failed && saved.value.is_some())
            || self.prior.dependencies[index] != branch.artifact_ref
            || self.observed.get(&node.id) != Some(&branch.artifact_ref)
            || self.consumed.get(&node.id).and_then(Option::as_ref) != Some(&actual_inputs)
            || canonical(&saved)? != bytes
        {
            return Err(invalid());
        }
        Ok(saved)
    }
}

fn compound_item_time_budget(english: bool) -> Error {
    failure(
        "BRANCH_TIME_BUDGET",
        if english {
            "The time budget expired while verifying the saved item."
        } else {
            "項目の記録を照合するための時間が足りませんでした。"
        },
    )
}

fn verified_generated_observation(
    parent: &InputBundle,
    item: &IndependentRequest,
    saved: &Observation<'_>,
    deadline: Instant,
    english: bool,
) -> Result<(InputBundle, String)> {
    let invalid = || explanation_mismatch(english);
    if Instant::now() >= deadline {
        return Err(compound_item_time_budget(english));
    }
    let value = saved.value.as_ref().ok_or_else(invalid)?;
    if saved.state != BranchState::Confirmed || saved.evidence_ref.is_some() {
        return Err(invalid());
    }
    let (leaf, reproduced) = observe_generated_item(parent, item, deadline)
        .map_err(|error| match error {
            Error::Contract { code, .. } if code == "MODEL_DEADLINE" => {
                compound_item_time_budget(english)
            }
            error => error,
        })?
        .ok_or_else(invalid)?;
    if value.guarantee != reproduced.guarantee
        || value.units != reproduced.units
        || value.bytes != reproduced.bytes
    {
        return Err(invalid());
    }
    let [text] = reproduced.units.as_slice() else {
        return Err(invalid());
    };
    Ok((leaf, text.clone()))
}

pub(crate) struct GeneratedRevisionBase {
    pub(crate) leaf: InputBundle,
    pub(crate) text: String,
    pub(crate) observation_ref: String,
    pub(crate) parent_render_ref: String,
    pub(crate) evidence_bytes: usize,
}

/// Verify the original generated result of a single numbered item.
pub(crate) fn verified_generated_revision_base(
    store: &Store,
    parent: &InputBundle,
    prior: &ResultRecord,
    one_based_item: usize,
    current_deadline: Instant,
    max_evidence_bytes: usize,
    english: bool,
) -> Result<GeneratedRevisionBase> {
    let unavailable = || {
        failure(
            "COMPOUND_ITEM_REVISION_UNAVAILABLE",
            if english {
                "The selected item has no confirmed draft or translation to revise."
            } else {
                "指定した項目には、修正できる確定済みの文案や訳文がありません。"
            },
        )
    };
    let now = Instant::now();
    let parent_deadline = now
        .checked_add(Duration::from_millis(parent.request.budget.deadline_ms))
        .ok_or_else(|| compound_item_time_budget(english))?;
    let deadline = current_deadline.min(parent_deadline);
    if now >= deadline {
        return Err(compound_item_time_budget(english));
    }
    let items = compound_items_for_selection(parent, prior, Some(one_based_item), english)?;
    let index = one_based_item - 1;
    let item = &items[index];
    let branch = &prior.branch_outcomes[index];
    if !generated_items_enabled(parent)
        || !initial_text_generation_item(item)
        || branch.state != BranchState::Confirmed
    {
        return Err(unavailable());
    }
    let evidence_limit = max_evidence_bytes.min(parent.request.budget.max_evidence_bytes);
    let retained =
        read_retained_compound(store, parent, prior, items, Some(evidence_limit), english)?;
    let request = &parent.request;
    let bytes = store.get_bytes(&request.actor_id, &request.case_id, &branch.artifact_ref)?;
    let evidence_bytes = retained.evidence_bytes.saturating_add(bytes.len());
    check_revision_evidence(evidence_bytes, evidence_limit, english)?;
    let saved = retained.observation(index, &bytes, english)?;
    let (leaf, text) = verified_generated_observation(parent, item, &saved, deadline, english)?;
    if Instant::now() >= deadline {
        return Err(compound_item_time_budget(english));
    }
    Ok(GeneratedRevisionBase {
        leaf,
        text,
        observation_ref: branch.artifact_ref.clone(),
        parent_render_ref: retained.render_ref,
        evidence_bytes,
    })
}

/// Explain saved observations without resuming the parent Goal or using its grants.
pub(crate) fn explain_previous(
    store: &Store,
    parent: &InputBundle,
    prior: &ResultRecord,
    selection: Option<usize>,
    english: bool,
    current_deadline: Instant,
) -> Result<PreviousExplanation> {
    let items = compound_items_for_selection(parent, prior, selection, english)?;
    let retained = read_retained_compound(store, parent, prior, items, None, english)?;
    let request = &parent.request;
    let invalid = || explanation_mismatch(english);
    let read = |reference| store.get_bytes(&request.actor_id, &request.case_id, reference);
    let source_ref = retained.source_ref;
    let plan = retained.plan;
    let observed = &retained.observed;
    let consumed = &retained.consumed;
    let branch_bytes = prior
        .branch_outcomes
        .iter()
        .map(|branch| read(&branch.artifact_ref))
        .collect::<Result<Vec<_>>>()?;
    let mut branches = Vec::with_capacity(items.len());
    let mut visible = Vec::new();
    let parent_english = supports_english_output_locale(&request.locale);
    let attached = !request.attachments.is_empty();
    let deadline = Instant::now() + Duration::from_millis(request.budget.deadline_ms);
    let mut attached_quantities = BTreeMap::new();
    for (index, (item, bytes)) in items.iter().zip(&branch_bytes).enumerate() {
        let saved = retained.observation(index, bytes, english)?;
        if generated_items_enabled(parent)
            && initial_text_generation_item(item)
            && selection.is_none_or(|number| number == index + 1)
            && saved.value.is_some()
        {
            verified_generated_observation(
                parent,
                item,
                &saved,
                deadline.min(current_deadline),
                english,
            )?;
        }
        if bilingual_statistics_enabled(parent)
            && matches!(&*item.intent, Intent::DescribeStatistics { .. })
            && let Some(value) = &saved.value
        {
            let expected = evaluate(
                item,
                parent,
                parent_english,
                Some(deadline),
                &BTreeMap::new(),
                &BTreeMap::new(),
            )?;
            let (actual, reason) = match expected {
                Branch::Value(value) => (value, None),
                Branch::Partial(value, reason) => (value, Some(reason)),
                Branch::Blocked(_) => return Err(invalid()),
            };
            if saved.reason_code != reason
                || value.guarantee != actual.guarantee
                || value.units != actual.units
                || value.bytes != actual.bytes
            {
                return Err(invalid());
            }
        }
        if attached
            && let Some(action) = AnalysisAction::bind(&item.intent)
            && let Some(value) = &saved.value
        {
            let actual = action.verify_retained(
                &request.attachments,
                parent_english,
                deadline,
                request.budget.max_evidence_bytes / 2,
                &value.bytes,
            )?;
            if actual.units != value.units
                || value.guarantee != Guarantee::Derived
                || actual.exact_value.is_some() != (saved.state == BranchState::Confirmed)
            {
                return Err(invalid());
            }
            attached_quantities.insert(saved.node_id, actual.exact_value);
        }
        if news_items_enabled(parent)
            && let Some(action) = NewsAction::bind(&item.intent)
        {
            if let Some(reference) = saved.evidence_ref {
                let actual =
                    composition_source::replay_news(store, parent, item, &action, reference)?;
                if let Some(value) = &saved.value {
                    let guarantee = if actual.confirmed {
                        Guarantee::SourceReport
                    } else {
                        Guarantee::Unknown
                    };
                    let reason = if actual.complete {
                        None
                    } else if actual.confirmed {
                        Some("NEWS_PARTIALLY_CONFIRMED")
                    } else {
                        Some("NEWS_ARTICLES_UNAVAILABLE")
                    };
                    if actual.report_bytes != value.bytes
                        || value.units != actual.units
                        || value.guarantee != guarantee
                        || saved.reason_code != reason
                        || (saved.state == BranchState::Confirmed) != actual.complete
                    {
                        return Err(invalid());
                    }
                } else if saved.reason_code != Some("BRANCH_EVIDENCE_BUDGET") {
                    return Err(invalid());
                }
            } else if saved.value.is_some() {
                return Err(invalid());
            }
        }
        if research_items_enabled(parent)
            && let Some(action) = ResearchAction::for_item(parent, item)
        {
            if let Some(reference) = saved.evidence_ref {
                let actual =
                    research_goal::replay_composed(store, parent, item, action, reference)?;
                if let Some(value) = &saved.value {
                    let guarantee = if actual.has_citations {
                        Guarantee::SourceReport
                    } else {
                        Guarantee::Unknown
                    };
                    let reason = (!actual.complete).then_some(action.incomplete_reason());
                    if value.bytes != canonical(&actual.comparison)?
                        || value.units != actual.units
                        || value.guarantee != guarantee
                        || saved.reason_code != reason
                        || (saved.state == BranchState::Confirmed) != actual.complete
                    {
                        return Err(invalid());
                    }
                } else if saved.reason_code != Some("BRANCH_EVIDENCE_BUDGET") {
                    return Err(invalid());
                }
            } else if saved.value.is_some() {
                return Err(invalid());
            }
        }
        visible.extend(display_units(&saved, index + 1, parent_english));
        branches.push(saved);
    }
    if visible != plan.required_units || plan.text != visible.join("\n") {
        return Err(invalid());
    }

    let count = branches
        .iter()
        .filter(|b| b.state == BranchState::Confirmed)
        .count();
    let total = branches.len();
    let mut units = vec![if english {
        format!("In the previous request, {count} of {total} items were confirmed.")
    } else {
        format!("前の依頼は {total} 項目のうち {count} 項目が確定しました。")
    }];
    if prior.lifecycle == Lifecycle::Suspend && count == total {
        units.push(if english {
            "The overall request still has an unfinished delivery or finalization.".into()
        } else {
            "依頼全体では保存または確定処理が保留されています。".into()
        });
    }
    for (index, (item, branch)) in items.iter().zip(&branches).enumerate() {
        if selection.is_some_and(|number| number != index + 1) {
            continue;
        }
        let number = index + 1;
        units.push(if english {
            format!("Request {number}")
        } else {
            format!("依頼 {number}")
        });
        if branch.state != BranchState::Confirmed {
            let status = match (english, branch.reason_code) {
                (true, Some("BRANCH_CAPABILITY_NOT_CONNECTED")) => {
                    "This operation is unavailable inside a multi-item request."
                }
                (false, Some("BRANCH_CAPABILITY_NOT_CONNECTED")) => {
                    "この操作は複数項目の依頼の中では利用できません。"
                }
                (true, Some("BRANCH_EVIDENCE_BUDGET" | "BRANCH_TIME_BUDGET")) => {
                    "The item ran out of time or space to retain the result."
                }
                (false, Some("BRANCH_EVIDENCE_BUDGET" | "BRANCH_TIME_BUDGET")) => {
                    "時間または結果を保持するための予算が足りませんでした。"
                }
                _ => branch_status(english, branch.state, branch.reason_code),
            };
            units.push(status.into());
            for id in prerequisite_ids(item) {
                let predecessor = branches
                    .iter()
                    .position(|saved| saved.node_id == id)
                    .ok_or_else(invalid)?;
                if branches[predecessor].state != BranchState::Confirmed {
                    let number = predecessor + 1;
                    units.push(if english {
                        format!("Request {number} had not been confirmed.")
                    } else {
                        format!("参照先の依頼 {number} が未確定でした。")
                    });
                }
            }
            if let Some(value) = &branch.value {
                units.extend(value.units.iter().cloned());
            }
            continue;
        }
        let value = branch.value.as_ref().ok_or_else(invalid)?;
        if let Some(action) = language::CalculationAction::bind(&item.intent) {
            let kept = action.decode_observation(&value.bytes)?;
            let mut quantities = BTreeMap::new();
            for used in kept.consumed() {
                let key = action.reference_source(&used.step_id).ok_or_else(invalid)?;
                let predecessor = branches
                    .iter()
                    .position(|saved| saved.node_id == key)
                    .ok_or_else(invalid)?;
                let field = used.step_id.strip_prefix(key).ok_or_else(invalid)?;
                let field = field.strip_prefix('-').unwrap_or(field);
                if branches[predecessor].state != BranchState::Confirmed
                    || observed.get(key) != Some(&used.artifact_sha256)
                    || !proves_consumed_value(
                        &items[predecessor].intent,
                        &branches[predecessor],
                        attached_quantities.get(key).and_then(Option::as_deref),
                        field,
                        &used.exact_value,
                    )?
                {
                    return Err(invalid());
                }
                if !quantities.contains_key(&used.step_id) {
                    let number = predecessor + 1;
                    let amount = &used.exact_value;
                    units.push(if english {
                        format!("Uses {amount} from request {number}.")
                    } else {
                        format!("依頼 {number} の値 {amount} を使いました。")
                    });
                    quantities.insert(
                        used.step_id.clone(),
                        language::SequenceValue {
                            exact_value: amount.clone(),
                            artifact_sha256: used.artifact_sha256.clone(),
                        },
                    );
                }
            }
            let prerequisites = consumed
                .get(branch.node_id)
                .and_then(Option::as_ref)
                .ok_or_else(invalid)?;
            let actual = action.observe(&quantities, prerequisites)?;
            if actual.proof_bytes()? != value.bytes {
                return Err(invalid());
            }
            append_calculation_explanation(actual.calculation(), english, &mut units);
            continue;
        }
        if let Some(action) = math::ProbabilityAction::bind(&item.intent) {
            let actual = action.verify_retained(&value.bytes)?;
            if value.units.as_slice() != std::slice::from_ref(&actual.unit) {
                return Err(invalid());
            }
            units.extend(actual.explain(english));
            continue;
        }
        match item.intent.as_ref() {
            Intent::DescribeStatistics { .. } if bilingual_statistics_enabled(parent) => {
                let report = math::conversational_math_report(&item.intent, true, english)?;
                units.extend(report.explanation_units);
            }
            Intent::AnalyzeAttachments { .. } if attached => {
                units.extend(value.units.iter().cloned());
            }
            Intent::CompareWebResearch { .. } if research_items_enabled(parent) => {
                units.push(if english {
                    "This comparison uses the article bodies saved with the previous request."
                        .into()
                } else {
                    "前の依頼で保存した記事本文を使って照合しました。".into()
                });
                units.extend(value.units.iter().cloned());
            }
            Intent::AskCurrentNews { .. } if news_items_enabled(parent) => {
                units.push(if english {
                    "Publication time comes from the publisher. This explanation uses the saved articles.".into()
                } else {
                    "掲載日時は配信元の表示です。今回は保存した記事を参照しました。".into()
                });
                units.extend(value.units.iter().cloned());
            }
            Intent::SelectRequestJsonNumber { source, pointer } => {
                let predecessor = branches
                    .iter()
                    .position(|saved| saved.node_id == source)
                    .ok_or_else(invalid)?;
                let evidence = branches[predecessor].evidence_ref.ok_or_else(invalid)?;
                let selected =
                    composition_source::select_json_number(store, parent, evidence, pointer)?;
                if canonical(&selected)? != value.bytes {
                    return Err(invalid());
                }
                let number = predecessor + 1;
                let amount = selected.exact_value;
                units.push(if english {
                    format!("Reads the exact number {amount} at {pointer} in the saved JSON from request {number}.")
                } else {
                    format!("依頼 {number} で取得したJSONの {pointer} から、数値 {amount} を読み取りました。")
                });
            }
            _ if let Some(uri) = composition_source::read_url(item) => {
                let evidence = branch.evidence_ref.ok_or_else(invalid)?;
                let packet = retained_source_packet(store, parent, uri, evidence)?;
                if canonical(&(&packet.id, &packet.raw_sha256, evidence))? != value.bytes {
                    return Err(invalid());
                }
                let final_uri = &packet.final_uri;
                let size = packet.raw_body.len();
                units.push(if english {
                    format!("The saved source at {final_uri} supplied {size} bytes.")
                } else {
                    format!("出典 {final_uri} から取得した {size} バイトの本文が記録されています。")
                });
            }
            _ => {
                units.push(if english {
                    "The saved visible result was:".into()
                } else {
                    "保存されている表示結果です。".into()
                });
                units.extend(value.units.iter().cloned());
            }
        }
    }
    let dependencies = std::iter::once(prior.id.clone())
        .chain(std::iter::once(source_ref.to_owned()))
        .chain(prior.dependencies.iter().cloned())
        .collect();
    Ok(PreviousExplanation {
        units,
        dependencies,
        source_ref: source_ref.to_owned(),
    })
}

fn proves_consumed_value(
    intent: &Intent,
    observation: &Observation<'_>,
    attached_quantity: Option<&str>,
    field: &str,
    expected: &str,
) -> Result<bool> {
    let Some(value) = &observation.value else {
        return Ok(false);
    };
    let bytes = &value.bytes;
    if field.is_empty() {
        if let Some(action) = language::CalculationAction::bind(intent) {
            return Ok(action.decode_observation(bytes)?.calculation().exact_value == expected);
        }
        if let Some(action) = math::ProbabilityAction::bind(intent) {
            let produced = action.verify_retained(bytes)?;
            return Ok(produced.quantity.as_deref() == Some(expected));
        }
    }
    Ok(match intent {
        Intent::AnalyzeAttachments { .. } if field.is_empty() => {
            attached_quantity == Some(expected)
        }
        Intent::SelectRequestJsonNumber { .. } if field.is_empty() => {
            serde_json::from_slice::<serde_json::Value>(bytes)?
                .get("exact_value")
                .and_then(serde_json::Value::as_str)
                == Some(expected)
        }
        Intent::Statistics { .. }
        | Intent::DescribeStatistics { .. }
        | Intent::FiniteProbabilityStatistics { .. } => {
            math::conversational_exact_quantities(intent, bytes)?
                .get(field)
                .is_some_and(|exact| exact == expected)
        }
        _ => false,
    })
}

fn append_calculation_explanation(
    calculation: &Calculation,
    english: bool,
    units: &mut Vec<String>,
) {
    let expression = &calculation.expression;
    let value = &calculation.exact_value;
    units.push(if english {
        format!("Calculated expression: {expression}")
    } else {
        format!("計算した式は {expression} です。")
    });
    units.extend(calculation.steps.iter().cloned());
    units.push(if english {
        format!("Exact result: {value}")
    } else {
        format!("厳密な結果は {value} です。")
    });
}

#[derive(Deserialize)]
struct RetainedObservation {
    node_id: String,
    input_fingerprint: String,
    source_range: (usize, usize),
    evidence_ref: Option<String>,
}

fn retained_source_packet(
    store: &Store,
    bundle: &InputBundle,
    uri: &str,
    reference: &str,
) -> Result<EvidencePacket> {
    let request = &bundle.request;
    let bytes = store.get_bytes(&request.actor_id, &request.case_id, reference)?;
    let packet: EvidencePacket = serde_json::from_slice(&bytes)?;
    if packet.request_uri != uri || canonical(&packet)? != bytes {
        return Err(failure(
            "COMPOUND_SOURCE_MISMATCH",
            "The saved source packet differs from the original request.",
        ));
    }
    ports::validate_successful_packet(&packet, &request.permissions)?;
    Ok(packet)
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
        || plan.locale != request.locale
    {
        return Err(invalid());
    }
    let mut read_usage = if research_items_enabled(bundle) {
        let reserved = items.len() + 3 + usize::from(request.output_path.is_some())
            - live_read_count(bundle, items);
        let starts = (request.budget.max_actions as usize)
            .checked_sub(reserved)
            .ok_or_else(invalid)?;
        Some((starts, request.budget.max_evidence_bytes / 2))
    } else {
        None
    };
    let mut graph = Scheduler::new(nodes(items).collect(), request.budget.max_actions)?;
    let mut observed = Outcome::new();
    evaluate_items(
        store,
        bundle,
        items,
        &mut graph,
        (!request.attachments.is_empty() || generated_items_enabled(bundle))
            .then(|| Instant::now() + Duration::from_millis(request.budget.deadline_ms)),
        |index, source| {
            let branch = &result.branch_outcomes[index];
            let bytes =
                store.get_bytes(&request.actor_id, &request.case_id, &branch.artifact_ref)?;
            let retained: RetainedObservation = serde_json::from_slice(&bytes)?;
            let reference = retained.evidence_ref.ok_or_else(invalid)?;
            let observed_source = match source {
                SourceRequest::Url(uri) => retained_source_packet(store, bundle, uri, &reference)
                    .map(|packet| SourceObservation::Url(reference, Box::new(packet))),
                SourceRequest::News { item, action } => {
                    composition_source::replay_news(store, bundle, item, &action, &reference)
                        .map(SourceObservation::News)
                }
                SourceRequest::Research { item, action } => {
                    research_goal::replay_composed(store, bundle, item, action, &reference)
                        .map(SourceObservation::Research)
                }
            }
            .map_err(|error| match error {
                Error::Contract { code, .. }
                    if matches!(
                        code.as_str(),
                        "COMPOUND_SOURCE_MISMATCH"
                            | "COMPOUND_NEWS_PROOF_INVALID"
                            | "WEB_RESEARCH_MISMATCH"
                    ) =>
                {
                    invalid()
                }
                error => error,
            })?;
            if let Some((starts_remaining, bytes_remaining)) = &mut read_usage {
                let (starts, bytes) = match &observed_source {
                    SourceObservation::Url(_, packet) => {
                        (packet.redirect_chain.len(), packet.raw_body.len())
                    }
                    SourceObservation::News(news) => (news.read_starts, news.raw_bytes),
                    SourceObservation::Research(research) => {
                        (research.read_starts, research.raw_bytes)
                    }
                };
                *starts_remaining = starts_remaining.checked_sub(starts).ok_or_else(invalid)?;
                *bytes_remaining = bytes_remaining.checked_sub(bytes).ok_or_else(invalid)?;
            }
            Ok(observed_source)
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
        || observed.retry_evidence != result.evidence
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
    let items = delivery_items(bundle)?;
    if identity(&bundle.request)? != result.id
        || !result.evidence.contains(packet)
        || packet.mode != "LIVE"
        || !matches!(packet.packet_kind.as_str(), "FETCH_FAILURE" | "SEARCH_LIST")
        || packet.failure.is_none()
        || packet.retry_after.is_none()
        || packet.raw_sha256 != digest(&packet.raw_body)
        || (packet.packet_kind == "FETCH_FAILURE"
            && packet.id != ports::fetch_packet_identity(packet)?)
    {
        return Err(invalid());
    }
    if (ports::is_news_packet(packet) || research_items_enabled(bundle))
        && verify_scoped_retry(store, bundle, result, items, packet)?
    {
        return Ok(());
    }
    if packet.provider_id != "https-direct-url-v1" || packet.packet_kind != "FETCH_FAILURE" {
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

fn verify_scoped_retry(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    items: &[IndependentRequest],
    packet: &EvidencePacket,
) -> Result<bool> {
    let request = &bundle.request;
    for (index, item) in items.iter().enumerate() {
        let news = (news_items_enabled(bundle) && ports::is_news_packet(packet))
            .then(|| NewsAction::bind(&item.intent))
            .flatten();
        let research = (research_items_enabled(bundle) && !ports::is_news_packet(packet))
            .then(|| ResearchAction::for_item(bundle, item))
            .flatten();
        if news.is_none() && research.is_none() {
            continue;
        }
        let Some(branch) = result.branch_outcomes.get(index) else {
            continue;
        };
        let id = node_id(index);
        if branch.node_id != id || !result.dependencies.contains(&branch.artifact_ref) {
            continue;
        }
        let bytes = store.get_bytes(&request.actor_id, &request.case_id, &branch.artifact_ref)?;
        let observed: RetainedObservation = serde_json::from_slice(&bytes)?;
        if observed.node_id == id
            && observed.input_fingerprint == bundle.fingerprint
            && observed.source_range == (item.source.start, item.source.end)
            && let Some(reference) = observed.evidence_ref
        {
            let is_retained = if let Some(action) = news {
                composition_source::replay_news(store, bundle, item, &action, &reference)?
                    .retry_evidence
                    .contains(packet)
            } else if let Some(action) = research {
                research_goal::replay_composed(store, bundle, item, action, &reference)?
                    .evidence
                    .contains(packet)
            } else {
                false
            };
            if is_retained {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

#[cfg(test)]
#[path = "composition/news_tests.rs"]
mod news_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Engine;
    use minidora_domain::RequestEnvelope;

    #[test]
    fn previous_compound_explanation_rejects_changed_branch_proof() {
        let temporary = tempfile::tempdir().unwrap();
        let mut engine = Engine::open(temporary.path()).unwrap();
        let mut request = RequestEnvelope::new(
            "case",
            "original",
            "owner",
            "- Calculate 27 * 4\n- Calculate @{request-1} + 12",
        );
        request.locale = "en-US".into();
        let original = engine.execute("owner", request).unwrap();
        assert_eq!(original.lifecycle, Lifecycle::Commit, "{original:?}");
        let parent = engine
            .latest("owner", "case")
            .unwrap()
            .unwrap()
            .bundle
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        let explanation =
            explain_previous(&engine.store, &parent, &original, Some(2), true, deadline).unwrap();
        assert!(
            explanation
                .units
                .iter()
                .any(|unit| unit == "Uses 108 from request 1.")
        );
        assert_eq!(explanation.dependencies[0], original.id);
        assert_eq!(
            explanation.source_ref,
            *original.execution_graph_ref.as_ref().unwrap()
        );

        let mut substituted = original.clone();
        substituted.branch_outcomes[1].artifact_ref =
            original.branch_outcomes[0].artifact_ref.clone();
        assert!(
            explain_previous(
                &engine.store,
                &parent,
                &substituted,
                Some(2),
                true,
                deadline
            )
            .is_err()
        );

        let mut changed = original;
        changed.text.push('!');
        assert!(explain_previous(&engine.store, &parent, &changed, None, false, deadline).is_err());
    }
}
