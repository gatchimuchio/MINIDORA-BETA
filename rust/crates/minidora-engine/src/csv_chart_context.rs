//! Bind the chart and each subsequent selection to the same retained CSV.
use crate::csv_chart::{self, CsvChartReport};
use crate::document::revalidate_previous_document_render;
use crate::input::verify_csv_chart_input;
use crate::store::Store;
use minidora_domain::{
    CaseRevision, Guarantee, InputBundle, Intent, Lifecycle, RequestAttachment, Result,
    ResultRecord, canonical, digest, failure, identity,
};
use std::time::{Duration, Instant};

pub(crate) const EXECUTION_CSV_NODE: &str = "confirmed-execution-csv";
const ATTACHED_CSV_NODE: &str = "attached-csv";

pub(crate) struct ChartOutcome {
    pub dependencies: Vec<String>,
    pub units: Vec<String>,
    pub report_bytes: Vec<u8>,
    pub source_evidence: Option<ChartEvidence>,
}

pub(crate) struct ChartEvidence {
    pub node: &'static str,
    pub reference: String,
    /// Wasm reports are stored during execution. Attachment manifests are
    /// first recorded by the Core when it accepts this chart computation.
    pub new_bytes: Option<Vec<u8>>,
}

pub(crate) fn source_evidence_node(intent: &Intent) -> Option<&'static str> {
    match intent {
        Intent::CreateCsvChartFromExecution { .. }
        | Intent::CreateCsvChartFromExecutionValues { .. } => Some(EXECUTION_CSV_NODE),
        Intent::CreateCsvChartFromAttachments { .. } => Some(ATTACHED_CSV_NODE),
        _ => None,
    }
}

fn invalid_context() -> minidora_domain::Error {
    failure(
        "CSV_CHART_CONTEXT_INVALID",
        "同じ会話に保存されたグラフと元のCSVを照合できません。先にCSVからグラフを作ってください。",
    )
}

fn verify_deadline(deadline: Option<Instant>) -> Result<()> {
    if deadline.is_some_and(|limit| Instant::now() >= limit) {
        return Err(failure(
            "RESOURCE_LIMIT",
            "グラフの集計が要求の期限を超えました。",
        ));
    }
    Ok(())
}

fn registered_chart(
    store: &Store,
    state: &CaseRevision,
    result: &ResultRecord,
) -> Result<InputBundle> {
    let (_, bundle) = store
        .get_input_bundle(
            &state.actor_id,
            &state.case_id,
            &result.input_id,
            result.input_version,
        )?
        .ok_or_else(invalid_context)?;
    if result.lifecycle != Lifecycle::Commit
        || result.guarantee != Guarantee::Computed
        || identity(&bundle.request)? != result.id
        || bundle.request.actor_id != state.actor_id
        || bundle.request.case_id != state.case_id
    {
        return Err(invalid_context());
    }
    verify_csv_chart_input(&bundle)?;
    Ok(bundle)
}

struct ChartSource<'a> {
    data: ChartSourceData<'a>,
    group: &'a str,
    value: &'a str,
}

enum ChartSourceData<'a> {
    Inline(&'a str),
    Execution {
        text: String,
        id: String,
        report: String,
        already_grouped: bool,
    },
    Attached(&'a [RequestAttachment]),
}

impl ChartSourceData<'_> {
    fn provenance(&self) -> Result<(Vec<String>, Option<ChartEvidence>)> {
        let (mut dependencies, evidence) = match self {
            Self::Inline(_) => return Ok((Vec::new(), None)),
            Self::Execution { id, report, .. } => (
                vec![id.clone()],
                ChartEvidence {
                    node: EXECUTION_CSV_NODE,
                    reference: report.clone(),
                    new_bytes: None,
                },
            ),
            Self::Attached(files) => {
                let identities: Vec<_> = files
                    .iter()
                    .map(|file| (&file.name, file.text.len(), digest(file.text.as_bytes())))
                    .collect();
                let bytes = canonical(&("minidora.csv-attachments.v1", identities))?;
                (
                    Vec::new(),
                    ChartEvidence {
                        node: ATTACHED_CSV_NODE,
                        reference: digest(&bytes),
                        new_bytes: Some(bytes),
                    },
                )
            }
        };
        dependencies.push(evidence.reference.clone());
        Ok((dependencies, Some(evidence)))
    }
}

fn original_source<'a>(
    store: &Store,
    state: &CaseRevision,
    bundle: &'a InputBundle,
    deadline: Option<Instant>,
) -> Result<ChartSource<'a>> {
    verify_deadline(deadline)?;
    match &bundle.meaning.intent {
        Intent::CreateCsvChart {
            document,
            group_column,
            value_column,
        } => Ok(ChartSource {
            data: ChartSourceData::Inline(&document.text),
            group: &group_column.text,
            value: &value_column.text,
        }),
        Intent::CreateCsvChartFromExecution {
            artifact_name,
            group_column,
            value_column,
        }
        | Intent::CreateCsvChartFromExecutionValues {
            artifact_name,
            group_column,
            value_column,
        } => {
            let artifact = crate::wasm_execution::read_csv_artifact(
                store,
                state,
                bundle,
                &artifact_name.text,
            )?;
            verify_deadline(deadline)?;
            if digest(artifact.text.as_bytes()) != artifact.source_sha256 {
                return Err(invalid_context());
            }
            Ok(ChartSource {
                data: ChartSourceData::Execution {
                    text: artifact.text,
                    id: artifact.execution_id,
                    report: artifact.report_ref,
                    already_grouped: matches!(
                        bundle.meaning.intent,
                        Intent::CreateCsvChartFromExecutionValues { .. }
                    ),
                },
                group: &group_column.text,
                value: &value_column.text,
            })
        }
        Intent::CreateCsvChartFromAttachments {
            group_column,
            value_column,
        } => {
            if bundle.request.attachments.is_empty() {
                return Err(failure(
                    "ATTACHMENT_REQUIRED",
                    "集計するCSVを先に /attach PATH で選んでください。",
                ));
            }
            Ok(ChartSource {
                data: ChartSourceData::Attached(&bundle.request.attachments),
                group: &group_column.text,
                value: &value_column.text,
            })
        }
        _ => Err(invalid_context()),
    }
}

fn chart_text(report: &CsvChartReport, bundle: &InputBundle) -> String {
    if bundle.request.output_path.is_some() {
        report.svg.clone()
    } else {
        report.summary.clone()
    }
}

fn compare_retained_chart(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    report: &CsvChartReport,
    mut ancestry: Vec<String>,
) -> Result<()> {
    let req = &bundle.request;
    let bytes = canonical(report)?;
    let reference = digest(&bytes);
    let expected_text = chart_text(report, bundle);
    ancestry.push(reference.clone());
    if result.dependencies != ancestry
        || result.text != expected_text
        || result
            .content_plan
            .as_ref()
            .is_none_or(|plan| plan.required_units != [expected_text.as_str()])
        || store.get_bytes(&req.actor_id, &req.case_id, &reference)? != bytes
    {
        return Err(invalid_context());
    }
    revalidate_previous_document_render(
        store,
        &req.actor_id,
        &req.case_id,
        result,
        Some(&reference),
        "CSV_CHART_CONTEXT_INVALID",
        "CSV chart",
    )?;
    Ok(())
}

fn ancestry(original: &str, previous: &str) -> Vec<String> {
    let mut refs = vec![original.to_owned()];
    if original != previous {
        refs.push(previous.to_owned());
    }
    refs
}

fn build_chart(
    source: &ChartSource<'_>,
    filter: Option<(&str, &str)>,
    deadline: Option<Instant>,
) -> Result<CsvChartReport> {
    verify_deadline(deadline)?;
    let report = match &source.data {
        ChartSourceData::Inline(text) => csv_chart::build(text, source.group, source.value, filter),
        ChartSourceData::Execution {
            text,
            already_grouped: true,
            ..
        } => csv_chart::build_values(text, source.group, source.value, filter),
        ChartSourceData::Execution {
            text,
            already_grouped: false,
            ..
        } => csv_chart::build(text, source.group, source.value, filter),
        ChartSourceData::Attached(files) => {
            let input: Vec<_> = files
                .iter()
                .map(|file| (file.name.as_str(), file.text.as_str()))
                .collect();
            csv_chart::build_from_files(&input, source.group, source.value, filter)
        }
    }?;
    verify_deadline(deadline)?;
    Ok(report)
}

pub(crate) fn produce(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    started: Instant,
) -> Result<ChartOutcome> {
    verify_csv_chart_input(bundle)?;
    let budget = &bundle.request.budget;
    let deadline = started.checked_add(Duration::from_millis(budget.deadline_ms));
    let mut source_evidence = None;
    let (report, mut dependencies) = match &bundle.meaning.intent {
        Intent::CreateCsvChart { .. }
        | Intent::CreateCsvChartFromExecution { .. }
        | Intent::CreateCsvChartFromExecutionValues { .. }
        | Intent::CreateCsvChartFromAttachments { .. } => {
            let source = original_source(store, state, bundle, deadline)?;
            let (dependencies, evidence) = source.data.provenance()?;
            source_evidence = evidence;
            (build_chart(&source, None, deadline)?, dependencies)
        }
        Intent::FilterPreviousCsvChart { column, equals } => {
            let selected = bundle
                .meaning
                .context_result_ref
                .as_ref()
                .and_then(|id| {
                    state
                        .prior_results
                        .get(id)
                        .filter(|result| &result.id == id)
                })
                .ok_or_else(invalid_context)?;
            if identity(&Some(selected))? != bundle.context_sha256 {
                return Err(invalid_context());
            }
            let selected_bundle = registered_chart(store, state, selected)?;
            // A new filter is evaluated over the original CSV, even following
            // an earlier selection. Both result identities stay in its lineage.
            let original_context = match &selected_bundle.meaning.intent {
                Intent::CreateCsvChart { .. }
                | Intent::CreateCsvChartFromExecution { .. }
                | Intent::CreateCsvChartFromExecutionValues { .. }
                | Intent::CreateCsvChartFromAttachments { .. } => None,
                Intent::FilterPreviousCsvChart { .. } => {
                    let id = selected.dependencies.first().ok_or_else(invalid_context)?;
                    let original = state
                        .prior_results
                        .get(id)
                        .filter(|result| &result.id == id && result.id != selected.id)
                        .ok_or_else(invalid_context)?;
                    Some((original, registered_chart(store, state, original)?))
                }
                _ => return Err(invalid_context()),
            };
            let (original, original_bundle) = original_context
                .as_ref()
                .map_or((selected, &selected_bundle), |(result, bundle)| {
                    (*result, bundle)
                });
            let source = original_source(store, state, original_bundle, deadline)?;
            let original_report = build_chart(&source, None, deadline)?;
            let (original_dependencies, original_evidence) = source.data.provenance()?;
            if let Some(evidence) = original_evidence
                && let Some(bytes) = evidence.new_bytes
                && store.get_bytes(&state.actor_id, &state.case_id, &evidence.reference)? != bytes
            {
                return Err(invalid_context());
            }
            compare_retained_chart(
                store,
                original_bundle,
                original,
                &original_report,
                original_dependencies,
            )?;
            if let Intent::FilterPreviousCsvChart { column, equals } =
                &selected_bundle.meaning.intent
            {
                let parent = selected_bundle
                    .meaning
                    .context_result_ref
                    .as_ref()
                    .and_then(|id| {
                        state
                            .prior_results
                            .get(id)
                            .filter(|prior| &prior.id == id && prior.id != selected.id)
                    })
                    .ok_or_else(invalid_context)?;
                if identity(&Some(parent))? != selected_bundle.context_sha256 {
                    return Err(invalid_context());
                }
                if parent.id != original.id {
                    let parent_bundle = registered_chart(store, state, parent)?;
                    if !matches!(
                        parent_bundle.meaning.intent,
                        Intent::FilterPreviousCsvChart { .. }
                    ) || parent.dependencies.first() != Some(&original.id)
                    {
                        return Err(invalid_context());
                    }
                }
                let expected = build_chart(&source, Some((&column.text, &equals.text)), deadline)?;
                compare_retained_chart(
                    store,
                    &selected_bundle,
                    selected,
                    &expected,
                    ancestry(&original.id, &parent.id),
                )?;
            }
            (
                build_chart(&source, Some((&column.text, &equals.text)), deadline)?,
                ancestry(&original.id, &selected.id),
            )
        }
        _ => return Err(invalid_context()),
    };
    let report_bytes = canonical(&report)?;
    let text = chart_text(&report, bundle);
    let manifest_len = source_evidence
        .as_ref()
        .and_then(|evidence| evidence.new_bytes.as_ref())
        .map_or(0, Vec::len);
    if report_bytes
        .len()
        .saturating_add(text.len())
        .saturating_add(manifest_len)
        > budget.max_evidence_bytes
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "集計とグラフが証拠予算を超えました。",
        ));
    }
    verify_deadline(deadline)?;
    dependencies.push(digest(&report_bytes));
    Ok(ChartOutcome {
        dependencies,
        units: vec![text],
        report_bytes,
        source_evidence,
    })
}

pub(crate) fn console_summary(
    store: &Store,
    actor: &str,
    case: &str,
    result: &ResultRecord,
) -> Result<Option<String>> {
    if result.lifecycle != Lifecycle::Commit
        || result.guarantee != Guarantee::Computed
        || !csv_chart::recognized_svg(&result.text)
    {
        return Ok(None);
    }
    let Some(reference) = result.dependencies.last() else {
        return Ok(None);
    };
    let Some(bytes) = store.get_bytes_if_present(actor, case, reference)? else {
        return Ok(None);
    };
    let Ok(report) = serde_json::from_slice::<CsvChartReport>(&bytes) else {
        return Ok(None);
    };
    if digest(&bytes) == *reference
        && report.svg == result.text
        && report.svg_sha256 == digest(result.text.as_bytes())
    {
        return Ok(Some(report.summary));
    }
    Ok(None)
}
