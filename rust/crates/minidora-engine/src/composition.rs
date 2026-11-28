//! Planning and observation for explicitly independent requests.
use crate::{
    core::{exact_calculation_unit, supports_english_output_locale},
    input, language, math,
    scheduler::{GoalNode, GoalNodeKind, Scheduler},
    store::Store,
};
use minidora_domain::{
    BranchOutcome, BranchState, Error, ExplanationAudience, ExplanationRequest, Guarantee,
    IndependentRequest, InputBundle, Intent, Result, SourceSpan, canonical, failure,
};
use serde::Serialize;
use std::{
    collections::BTreeMap,
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
        .map(|(start, end)| {
            let source = SourceSpan {
                start,
                end,
                text: text[start..end].to_owned(),
            };
            let intent = match lower(&source.text) {
                Ok(intent) => intent,
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

pub(crate) fn nodes(items: &[IndependentRequest]) -> impl Iterator<Item = GoalNode> {
    (0..items.len()).map(|index| GoalNode {
        id: node_id(index),
        kind: GoalNodeKind::CoreResult,
        requires: Vec::new(),
    })
}

#[derive(Serialize)]
struct Value {
    guarantee: Guarantee,
    units: Vec<String>,
    bytes: Vec<u8>,
}

#[derive(Serialize)]
struct Observation<'a> {
    node_id: &'a str,
    input_fingerprint: &'a str,
    source_range: (usize, usize),
    state: BranchState,
    reason_code: Option<&'a str>,
    value: Option<Value>,
}

enum Branch {
    Value(Value),
    Partial(Value, &'static str),
    Blocked(&'static str),
}

fn evaluate(item: &IndependentRequest, bundle: &InputBundle, english: bool) -> Result<Branch> {
    let mut partial_reason = None;
    let value = match item.intent.as_ref() {
        Intent::Calculate { expression } => {
            let calculation = language::calculate(expression)?;
            Value {
                units: vec![exact_calculation_unit(&calculation)],
                bytes: canonical(&calculation)?,
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
            Value {
                units: report.units,
                bytes: report.bytes,
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
                guarantee: Guarantee::Hypothesis,
            }
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
    pub(crate) units: Vec<String>,
    pub(crate) guarantee: Guarantee,
}

impl Outcome {
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
) -> Result<Outcome> {
    let request = &bundle.request;
    let Intent::IndependentRequests { items } = &bundle.meaning.intent else {
        return Err(failure(
            "COMPOUND_INPUT_MISMATCH",
            "An independent request list is required.",
        ));
    };
    let english = supports_english_output_locale(&request.locale);
    if !request.attachments.is_empty()
        || request.output_path.is_some()
        || request.context_ref.is_some()
        || request.speech.is_some()
        || request.purpose.is_some()
    {
        return Err(failure(
            "COMPOUND_SCOPE_UNAVAILABLE",
            if english {
                "Independent items currently accept a standalone text request for on-screen results."
            } else {
                "複数の依頼では、入力欄に書いた内容を画面で確認できます。添付、保存先、参照先、音声、用途指定は外してください。"
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
    let render_reserve = (request.budget.deadline_ms / 4).min(250);
    let compute_limit = Duration::from_millis(request.budget.deadline_ms - render_reserve);
    // The remaining half covers the shared plan/render and their retained bytes.
    let mut evidence_remaining = request.budget.max_evidence_bytes / 2;
    let mut output = Outcome {
        branches: Vec::new(),
        units: Vec::new(),
        guarantee: Guarantee::Unknown,
    };
    for (index, item) in items.iter().enumerate() {
        let id = node_id(index);
        let evaluated = if started.elapsed() >= compute_limit {
            Ok(Branch::Blocked("BRANCH_TIME_BUDGET"))
        } else {
            evaluate(item, bundle, english)
        };
        let (state, reason, value) = match evaluated {
            Ok(Branch::Value(value)) => (BranchState::Confirmed, None, Some(value)),
            Ok(Branch::Partial(value, code)) => {
                (BranchState::Blocked, Some(code.into()), Some(value))
            }
            Ok(Branch::Blocked(code)) => (BranchState::Blocked, Some(code.into()), None),
            Err(Error::Contract { code, .. }) => (BranchState::Failed, Some(code), None),
            Err(error) => return Err(error),
        };
        let mut observed = Observation {
            node_id: &id,
            input_fingerprint: &bundle.fingerprint,
            source_range: (item.source.start, item.source.end),
            state,
            reason_code: reason.as_deref(),
            value,
        };
        let mut bytes = canonical(&observed)?;
        let mut units = display_units(&observed, index + 1, english);
        // Carry unused bytes forward while reserving room for later outcomes.
        let available = evidence_remaining / (items.len() - index);
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
        let reference = store.put_bytes(&request.actor_id, &request.case_id, &bytes)?;
        graph.observe(&id, &reference, &BTreeMap::new())?;
        if let Some(value) = observed.value
            && (output.guarantee == Guarantee::Unknown || value.guarantee == Guarantee::Hypothesis)
        {
            output.guarantee = value.guarantee;
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
    }
    Ok(output)
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
        (true, BranchState::Failed, _) => "The operation failed.",
        (false, BranchState::Failed, _) => "処理に失敗しました。",
        (true, _, _) => "This item could not be completed.",
        (false, _, _) => "この項目は保留しました。",
    };
    vec![format!("{heading}　{status}")]
}
