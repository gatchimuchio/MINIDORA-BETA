//! Source-preserving explicit document operations. Public CSV parsers use
//! RFC 4180 record and quote framing in a UTF-8 projection. Conversation input
//! also admits LF record framing and retains those bytes exactly. No implicit
//! numeric inference or spreadsheet evaluation occurs.
use crate::ports::{JsonKind, JsonNode, json_node_at_pointer_with_mode, strict_json_document};
use crate::text_editing::{
    EditedText, LocatedTextEdit, edit_explicit_text, locate_explicit_replacement,
};
use crate::text_summary::{
    EXTRACTIVE_SUMMARY_LABEL, ExtractiveSummaryCandidates, direct_summary_candidates,
};
use minidora_domain::{
    CaseRevision, Guarantee, InputBundle, Intent, Lifecycle, MAX_REQUEST_ATTACHMENTS,
    ResourceBudget, Result, ResultRecord, SourceSpan, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

const MAX_DOCUMENT_BYTES: usize = 256_000;
const MAX_RESULT_BYTES: usize = 256_000;
const MAX_ROWS: usize = 4096;
const MAX_COLUMNS: usize = 256;
const MAX_CELLS: usize = 65_536;
const MAX_EDITS: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsvCell {
    pub value: String,
    /// Half-open UTF-8 byte offsets, including original quote syntax.
    pub source_start: usize,
    pub source_end: usize,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CsvTable {
    pub source_document: String,
    pub source_sha256: String,
    pub header: Option<Vec<CsvCell>>,
    pub rows: Vec<Vec<CsvCell>>,
}

/// A borrowed source with an explicit input-framing policy. Both modes retain
/// the original document bytes and UTF-8 offsets.
#[derive(Clone, Copy)]
pub(crate) struct CsvInput<'a> {
    document: &'a str,
    allow_lf: bool,
}

impl<'a> CsvInput<'a> {
    fn strict(document: &'a str) -> Self {
        Self {
            document,
            allow_lf: false,
        }
    }

    pub(crate) fn conversation(document: &'a str) -> Self {
        Self {
            document,
            allow_lf: true,
        }
    }

    pub(crate) fn select(self, column: &str) -> Result<DocumentResult> {
        csv_select_input(self, column)
    }

    pub(crate) fn select_index(self, index: usize, has_header: bool) -> Result<DocumentResult> {
        select(parse_csv_input(self, has_header)?, index, None)
    }

    pub(crate) fn statistics(self, column: &str) -> Result<DocumentResult> {
        csv_statistics_input(self, column)
    }

    pub(crate) fn to_json(self, has_header: bool) -> Result<DocumentResult> {
        csv_to_json_input(self, has_header)
    }

    pub(crate) fn filter(self, column: &str, equals: &str) -> Result<DocumentResult> {
        csv_filter_input(self, column, equals)
    }

    pub(crate) fn sort(self, column: &str, descending: bool) -> Result<DocumentResult> {
        csv_sort_input(self, column, descending)
    }

    pub(crate) fn group_count(self, column: &str) -> Result<DocumentResult> {
        csv_group_count_input(self, column)
    }

    pub(crate) fn group_statistics(self, group: &str, value: &str) -> Result<DocumentResult> {
        csv_group_statistics_input(self, group, value)
    }

    pub(crate) fn compare_by_parser(
        self,
        key: &str,
        parser_version: &str,
    ) -> Result<DocumentResult> {
        csv_compare_by_input(
            self,
            key,
            parser_version == crate::input::CSV_BEFORE_AFTER_COMPARE_PARSER_VERSION,
        )
    }

    pub(crate) fn inner_join_by_parser(
        self,
        left_key: &str,
        right_key: &str,
        parser_version: &str,
    ) -> Result<DocumentResult> {
        csv_inner_join_input(
            self,
            left_key,
            right_key,
            parser_version == crate::input::CSV_LEFT_RIGHT_JOIN_PARSER_VERSION,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentSpan {
    pub kind: String,
    pub source_start: usize,
    pub source_end: usize,
    pub source_text: String,
    pub decoded_value: Option<String>,
    pub row: Option<usize>,
    pub column: Option<usize>,
    pub output_start: Option<usize>,
    pub output_end: Option<usize>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentResult {
    pub text: String,
    pub source_document: String,
    pub source_sha256: String,
    pub output_sha256: String,
    pub operation: String,
    pub parameters: BTreeMap<String, String>,
    pub spans: Vec<DocumentSpan>,
    pub guarantee: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreviousTextEditArtifact {
    pub operation: String,
    pub source_result_id: String,
    pub source_content_plan_sha256: String,
    pub source_render_sha256: String,
    pub source_text_sha256: String,
    pub edit: DocumentResult,
}

/// Revalidate an exact prior CSV-statistics report and explain where each
/// observed value came from. This is deliberately bounded to the one existing
/// exact-statistics producer; the original result remains the numeric answer.
pub(crate) fn previous_csv_statistics_explanation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<(String, Vec<String>)>> {
    if previous.lifecycle != Lifecycle::Commit || previous.guarantee != Guarantee::Computed {
        return Ok(None);
    }
    let mut found = None;
    for dependency in &previous.dependencies {
        let Some(bytes) = store.get_bytes_if_present(actor, case, dependency)? else {
            continue;
        };
        let Ok(report) = serde_json::from_slice::<DocumentResult>(&bytes) else {
            continue;
        };
        if report.operation != "CSV_STATISTICS" {
            continue;
        }
        if found.is_some() {
            return Err(failure(
                "CSV_CONTEXT_AMBIGUOUS",
                "The prior result contains more than one CSV-statistics report",
            ));
        }
        let column = report.parameters.get("column_name").ok_or_else(|| {
            failure(
                "CSV_CONTEXT_INVALID",
                "The retained statistics report has no selected column",
            )
        })?;
        let recomputed = CsvInput::conversation(&report.source_document).statistics(column)?;
        if previous.dependencies != [dependency.clone()]
            || previous.text != report.text
            || report.source_sha256 != digest(report.source_document.as_bytes())
            || report.output_sha256 != digest(report.text.as_bytes())
            || canonical(&report)? != bytes
            || canonical(&recomputed)? != bytes
        {
            return Err(failure(
                "CSV_CONTEXT_INVALID",
                "The prior CSV-statistics report, source cells or output no longer revalidate",
            ));
        }
        let output: serde_json::Value = serde_json::from_str(&report.text)?;
        let statistics = output.get("statistics").ok_or_else(|| {
            failure(
                "CSV_CONTEXT_INVALID",
                "The exact CSV statistics object is missing",
            )
        })?;
        let observed = report
            .spans
            .iter()
            .filter(|span| span.kind == "CSV_NUMERIC_OBSERVATION")
            .collect::<Vec<_>>();
        if observed.is_empty()
            || report.spans.iter().any(|span| {
                report
                    .source_document
                    .get(span.source_start..span.source_end)
                    != Some(span.source_text.as_str())
                    || match (span.output_start, span.output_end) {
                        (Some(start), Some(end)) => report.text.get(start..end).is_none(),
                        (None, None) => false,
                        _ => true,
                    }
            })
        {
            return Err(failure(
                "CSV_CONTEXT_INVALID",
                "The prior report's source or output spans are invalid",
            ));
        }
        let mut lines = vec![
            format!("前回のCSV統計: {}列", column),
            format!("元資料SHA-256: {}", report.source_sha256),
            format!("集計artifact SHA-256: {dependency}"),
            format!("正確な集計値: {}", serde_json::to_string(statistics)?),
            format!("計算に使った元セル: {}件", observed.len()),
        ];
        for span in observed.iter().take(8) {
            lines.push(format!(
                "元セル 行{}・列{} byte[{}..{}): {:?}",
                span.row
                    .map_or_else(|| "?".into(), |row| (row + 1).to_string()),
                span.column
                    .map_or_else(|| "?".into(), |column| (column + 1).to_string()),
                span.source_start,
                span.source_end,
                span.source_text
            ));
        }
        if observed.len() > 8 {
            lines.push(format!(
                "残りの元セル {}件は保存artifactに保持されています。",
                observed.len() - 8
            ));
        }
        found = Some((dependency.clone(), lines));
    }
    Ok(found)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvTableComparisonText {
    key_column: String,
    changes: Vec<CsvTableComparisonChange>,
}

#[derive(Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum CsvTableComparisonChange {
    #[serde(rename = "CELL_CHANGED")]
    CellChanged {
        key: String,
        column: String,
        left: String,
        right: String,
    },
    #[serde(rename = "ROW_REMOVED")]
    RowRemoved { key: String, row: Vec<String> },
    #[serde(rename = "ROW_ADDED")]
    RowAdded { key: String, row: Vec<String> },
}

pub(crate) struct PriorDocumentExplanation {
    pub(crate) input_node: &'static str,
    pub(crate) report_node: &'static str,
    pub(crate) bundle_ref: String,
    pub(crate) report_ref: String,
    pub(crate) lines: Vec<String>,
}

pub(crate) const TEXT_SUMMARY_CANDIDATE_REPORT_NODE: &str =
    "previous-text-summary-candidate-report";
const TEXT_SUMMARY_CANDIDATE_CONTEXT_INVALID: &str = "TEXT_SUMMARY_CANDIDATE_CONTEXT_INVALID";

#[derive(Clone, Copy)]
enum CsvTableOperation<'a> {
    Compare(&'a str),
    Join(&'a str, &'a str),
    Filter(&'a str, &'a str),
    Sort(&'a str, bool),
    GroupCount(&'a str),
    GroupStatistics(&'a str, &'a str),
    Select(&'a str),
    SelectIndex(usize, bool),
    CsvToJson(bool),
    JsonToCsv,
}

/// Recompute a prior document operation from its immutable sealed input
/// before explaining its exact result and source ranges.
pub(crate) fn previous_document_explanation(
    store: &crate::store::Store,
    state: &CaseRevision,
    previous: &ResultRecord,
    budget: &ResourceBudget,
    explanation_parser_version: &str,
    started: Instant,
) -> Result<Option<PriorDocumentExplanation>> {
    if previous.lifecycle != Lifecycle::Commit {
        return Ok(None);
    }
    let actor = &state.actor_id;
    let case = &state.case_id;
    let conversion_parser_version = match explanation_parser_version {
        crate::input::PREVIOUS_JSON_TABLE_CONVERSION_EXPLANATION_PARSER_VERSION => {
            Some(crate::input::PREVIOUS_JSON_TABLE_TO_CSV_PARSER_VERSION)
        }
        crate::input::PREVIOUS_FILTERED_CSV_CONVERSION_EXPLANATION_PARSER_VERSION => {
            Some(crate::input::PREVIOUS_FILTERED_CSV_TO_CSV_PARSER_VERSION)
        }
        _ => None,
    };
    if let Some(parser_version) = conversion_parser_version {
        return previous_json_table_conversion_explanation(
            store,
            state,
            previous,
            budget.max_evidence_bytes,
            parser_version,
        )
        .map(Some);
    }
    if explanation_parser_version
        == crate::input::PASTED_TEXT_REPLACEMENT_EXPLANATION_PARSER_VERSION
    {
        return previous_pasted_text_replacement_explanation(
            store,
            actor,
            case,
            previous,
            budget.max_evidence_bytes,
        )
        .map(Some);
    }
    let explain_summary = matches!(
        explanation_parser_version,
        crate::input::TEXT_SUMMARY_CANDIDATE_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_SUMMARY_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_QUESTION_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    );
    let explain_text_document = matches!(
        explanation_parser_version,
        crate::input::TEXT_LOCATION_EXPLANATION_PARSER_VERSION
            | crate::input::TEXT_EDIT_EXPLANATION_PARSER_VERSION
            | crate::input::TEXT_COMPOSITION_EXPLANATION_PARSER_VERSION
            | crate::input::TEXT_SUMMARY_CANDIDATE_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_SUMMARY_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_QUESTION_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    );
    if (explain_text_document && previous.guarantee == Guarantee::UserPremise)
        || (explain_summary && previous.guarantee == Guarantee::Hypothesis)
    {
        return previous_text_document_explanation(
            store,
            actor,
            case,
            previous,
            budget,
            started,
            explanation_parser_version,
        );
    }
    if previous.guarantee != Guarantee::Computed {
        return Ok(None);
    }
    let max_evidence_bytes = budget.max_evidence_bytes;
    let explain_csv_select_index = explain_text_document
        || explanation_parser_version == crate::input::CSV_POSITIONAL_SELECTION_PARSER_VERSION;
    let explain_json_comparison = explain_csv_select_index
        || explanation_parser_version
            == crate::input::JSON_STRUCTURAL_COMPARISON_EXPLANATION_PARSER_VERSION;
    let explain_object_project = explain_json_comparison
        || explanation_parser_version
            == crate::input::JSON_OBJECT_PROJECT_EXPLANATION_PARSER_VERSION;
    let explain_json_value = explain_object_project
        || explanation_parser_version == crate::input::JSON_VALUE_EXPLANATION_PARSER_VERSION;
    let explain_conversion = explain_json_value
        || explanation_parser_version == crate::input::TABLE_CONVERSION_EXPLANATION_PARSER_VERSION;
    let explain_select = explain_conversion
        || explanation_parser_version == crate::input::CSV_SELECT_EXPLANATION_PARSER_VERSION;
    let explain_groups = explain_select
        || matches!(
            explanation_parser_version,
            crate::input::CSV_GROUP_EXPLANATION_PARSER_VERSION
                | crate::input::JSON_GROUP_COUNT_EXPLANATION_PARSER_VERSION
        );
    let explain_rows = explain_groups
        || explanation_parser_version == crate::input::CSV_ROW_EXPLANATION_PARSER_VERSION;
    let explain_join = explain_rows
        || explanation_parser_version == crate::input::CSV_JOIN_EXPLANATION_PARSER_VERSION;

    let (bundle_ref, bundle) = match store.get_input_bundle(
        actor,
        case,
        &previous.input_id,
        previous.input_version,
    )? {
        Some(value) => value,
        None => {
            let looks_like_report = serde_json::from_str::<serde_json::Value>(&previous.text)
                .ok()
                .and_then(|value| {
                    value.as_object().and_then(|object| {
                        (object.len() == 2
                            && object
                                .get("key_column")
                                .and_then(serde_json::Value::as_str)
                                .is_some()
                            && object
                                .get("changes")
                                .and_then(serde_json::Value::as_array)
                                .is_some())
                        .then_some(())
                    })
                })
                .is_some();
            if looks_like_report {
                return Err(failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    "the prior CSV comparison has no retained sealed input bundle",
                ));
            }
            if explain_join && serde_json::from_str::<CsvTableJoinText>(&previous.text).is_ok() {
                return Err(failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    "the prior CSV inner join has no retained sealed input bundle",
                ));
            }
            if explain_rows
                && let [dependency] = previous.dependencies.as_slice()
                && let Some(bytes) = store.get_bytes_if_present(actor, case, dependency)?
                && let Ok(report) = serde_json::from_slice::<DocumentResult>(&bytes)
                && match report.operation.as_str() {
                    "CSV_FILTER" | "CSV_SORT" => true,
                    "CSV_GROUP_COUNT" | "CSV_GROUP_STATISTICS" => explain_groups,
                    "CSV_SELECT" => explain_select,
                    "CSV_TO_JSON" | "JSON_TO_CSV" => explain_conversion,
                    _ => false,
                }
            {
                return Err(failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    match report.operation.as_str() {
                        "CSV_FILTER" | "CSV_SORT" => {
                            "the prior CSV row operation has no retained sealed input bundle"
                        }
                        "CSV_SELECT" => {
                            "the prior CSV selection has no retained sealed input bundle"
                        }
                        "CSV_TO_JSON" | "JSON_TO_CSV" => {
                            "the prior table conversion has no retained sealed input bundle"
                        }
                        _ => "the prior grouped CSV operation has no retained sealed input bundle",
                    },
                ));
            }
            if explain_json_value
                && let [dependency] = previous.dependencies.as_slice()
                && let Some(bytes) = store.get_bytes_if_present(actor, case, dependency)?
            {
                if serde_json::from_slice::<crate::ports::JsonStatisticsReport>(&bytes).is_ok() {
                    return Err(failure(
                        "JSON_VALUE_CONTEXT_INVALID",
                        "the prior JSON statistics have no retained sealed input bundle",
                    ));
                }
                if explain_object_project
                    && serde_json::from_slice::<crate::ports::JsonObjectProjectReport>(&bytes)
                        .is_ok()
                {
                    return Err(failure(
                        "JSON_VALUE_CONTEXT_INVALID",
                        "the prior JSON object projection has no retained sealed input bundle",
                    ));
                }
                if explain_json_comparison
                    && serde_json::from_slice::<crate::ports::JsonComparisonReport>(&bytes).is_ok()
                {
                    return Err(failure(
                        "JSON_VALUE_CONTEXT_INVALID",
                        "the prior JSON comparison has no retained sealed input bundle",
                    ));
                }
            }
            return Ok(None);
        }
    };
    let explain_json = match bundle.meaning.intent {
        Intent::JsonPointer { .. } | Intent::JsonStatistics { .. } => explain_json_value,
        Intent::JsonObjectProject { .. } => explain_object_project,
        Intent::JsonCompare { .. } => explain_json_comparison,
        _ => false,
    };
    if explain_json {
        return previous_json_value_explanation(
            store,
            actor,
            case,
            previous,
            &bundle,
            bundle_ref,
            max_evidence_bytes,
        )
        .map(Some);
    }
    let (document, operation) = match &bundle.meaning.intent {
        Intent::CsvCompare {
            document,
            key_column,
        } => (document.as_str(), CsvTableOperation::Compare(key_column)),
        Intent::CsvInnerJoin {
            document,
            left_key_column,
            right_key_column,
        } if explain_join => (
            document.as_str(),
            CsvTableOperation::Join(left_key_column, right_key_column),
        ),
        Intent::CsvFilter {
            document,
            column,
            equals,
        } if explain_rows => (document.as_str(), CsvTableOperation::Filter(column, equals)),
        Intent::CsvSort {
            document,
            column,
            descending,
        } if explain_rows => (
            document.as_str(),
            CsvTableOperation::Sort(column, *descending),
        ),
        Intent::CsvGroupCount { document, column } if explain_groups => {
            (document.as_str(), CsvTableOperation::GroupCount(column))
        }
        Intent::CsvGroupStatistics {
            document,
            group_column,
            value_column,
        } if explain_groups => (
            document.as_str(),
            CsvTableOperation::GroupStatistics(group_column, value_column),
        ),
        Intent::CsvSelect { document, column } if explain_select => {
            (document.as_str(), CsvTableOperation::Select(column))
        }
        Intent::CsvSelectIndex {
            document,
            column_index,
            has_header,
        } if explain_csv_select_index => (
            document.as_str(),
            CsvTableOperation::SelectIndex(*column_index, *has_header),
        ),
        Intent::CsvToJson {
            document,
            has_header,
        } if explain_conversion => (document.as_str(), CsvTableOperation::CsvToJson(*has_header)),
        Intent::JsonToCsv { document } if explain_conversion => {
            (document.as_str(), CsvTableOperation::JsonToCsv)
        }
        _ => return Ok(None),
    };
    let (label, input_node, report_node) = match operation {
        CsvTableOperation::Compare(_) => (
            "comparison",
            "previous-csv-table-comparison-input",
            "previous-csv-table-comparison-report",
        ),
        CsvTableOperation::Join(_, _) => (
            "inner join",
            "previous-csv-table-inner-join-input",
            "previous-csv-table-inner-join-report",
        ),
        CsvTableOperation::Filter(_, _) => (
            "filter",
            "previous-csv-table-filter-input",
            "previous-csv-table-filter-report",
        ),
        CsvTableOperation::Sort(_, _) => (
            "sort",
            "previous-csv-table-sort-input",
            "previous-csv-table-sort-report",
        ),
        CsvTableOperation::GroupCount(_) => (
            "group count",
            "previous-csv-table-group-count-input",
            "previous-csv-table-group-count-report",
        ),
        CsvTableOperation::GroupStatistics(_, _) => (
            "group statistics",
            "previous-csv-table-group-statistics-input",
            "previous-csv-table-group-statistics-report",
        ),
        CsvTableOperation::Select(_) => (
            "selection",
            "previous-csv-table-select-input",
            "previous-csv-table-select-report",
        ),
        CsvTableOperation::SelectIndex(_, _) => (
            "positional selection",
            "previous-csv-table-select-index-input",
            "previous-csv-table-select-index-report",
        ),
        CsvTableOperation::CsvToJson(_) | CsvTableOperation::JsonToCsv => (
            "conversion",
            "previous-csv-table-conversion-input",
            "previous-csv-table-conversion-report",
        ),
    };
    if bundle_ref != digest(&canonical(&bundle)?) {
        return Err(failure(
            "CSV_TABLE_CONTEXT_INVALID",
            format!("the sealed CSV {label} input bundle hash changed"),
        ));
    }

    let csv = CsvInput::conversation(document);
    let recomputed = match operation {
        CsvTableOperation::Compare(key_column) => {
            csv.compare_by_parser(key_column, &bundle.meaning.parser_version)?
        }
        CsvTableOperation::Join(left_key, right_key) => {
            csv.inner_join_by_parser(left_key, right_key, &bundle.meaning.parser_version)?
        }
        CsvTableOperation::Filter(column, equals) => csv.filter(column, equals)?,
        CsvTableOperation::Sort(column, descending) => csv.sort(column, descending)?,
        CsvTableOperation::GroupCount(column) => csv.group_count(column)?,
        CsvTableOperation::GroupStatistics(group, value) => csv.group_statistics(group, value)?,
        CsvTableOperation::Select(column) => csv.select(column)?,
        CsvTableOperation::SelectIndex(index, has_header) => csv.select_index(index, has_header)?,
        CsvTableOperation::CsvToJson(has_header) => csv.to_json(has_header)?,
        CsvTableOperation::JsonToCsv => json_to_csv(document)?,
    };
    let parameter = |key| recomputed.parameters.get(key).map(String::as_str);
    let parameters_match = match operation {
        CsvTableOperation::Compare(key_column) => {
            recomputed.operation == "CSV_TABLE_COMPARE"
                && parameter("key_column") == Some(key_column)
        }
        CsvTableOperation::Join(left_key, right_key) => {
            recomputed.operation == "CSV_INNER_JOIN"
                && parameter("left_key_column") == Some(left_key)
                && parameter("right_key_column") == Some(right_key)
        }
        CsvTableOperation::Filter(column, equals) => {
            recomputed.operation == "CSV_FILTER"
                && parameter("column_name") == Some(column)
                && parameter("equals") == Some(equals)
        }
        CsvTableOperation::Sort(column, descending) => {
            recomputed.operation == "CSV_SORT"
                && parameter("column_name") == Some(column)
                && parameter("sort_order")
                    == Some(if descending {
                        "descending"
                    } else {
                        "ascending"
                    })
        }
        CsvTableOperation::GroupCount(column) => {
            recomputed.operation == "CSV_GROUP_COUNT" && parameter("column_name") == Some(column)
        }
        CsvTableOperation::GroupStatistics(group, value) => {
            recomputed.operation == "CSV_GROUP_STATISTICS"
                && parameter("group_column") == Some(group)
                && parameter("value_column") == Some(value)
        }
        CsvTableOperation::Select(column) => {
            recomputed.operation == "CSV_SELECT"
                && parameter("column_name") == Some(column)
                && parameter("has_header") == Some("true")
                && parameter("value_type") == Some("STRING")
        }
        CsvTableOperation::SelectIndex(index, has_header) => {
            recomputed.operation == "CSV_SELECT"
                && parameter("column_index") == Some(index.to_string().as_str())
                && parameter("column_name").is_none()
                && parameter("has_header") == Some(if has_header { "true" } else { "false" })
                && parameter("value_type") == Some("STRING")
        }
        CsvTableOperation::CsvToJson(has_header) => {
            recomputed.operation == "CSV_TO_JSON"
                && parameter("has_header") == Some(if has_header { "true" } else { "false" })
                && parameter("value_type") == Some("STRING")
        }
        CsvTableOperation::JsonToCsv => {
            recomputed.operation == "JSON_TO_CSV"
                && parameter("formula_policy") == Some("REJECT_LEADING_MARKERS")
                && parameter("value_type") == Some("STRING")
        }
    };
    let report_bytes = canonical(&recomputed)?;
    let report_ref = digest(&report_bytes);
    let stored_report = store
        .get_bytes_if_present(actor, case, &report_ref)?
        .ok_or_else(|| {
            failure(
                "CSV_TABLE_CONTEXT_INVALID",
                format!("the prior CSV {label} report artifact is missing"),
            )
        })?;
    if previous.dependencies != [report_ref.clone()]
        || stored_report != report_bytes
        || previous.text != recomputed.text
        || !parameters_match
        || recomputed.source_document != document
        || recomputed.source_sha256 != digest(document.as_bytes())
        || recomputed.output_sha256 != digest(recomputed.text.as_bytes())
    {
        return Err(failure(
            "CSV_TABLE_CONTEXT_INVALID",
            format!(
                "the prior CSV {label} input, report artifact or result text no longer revalidates"
            ),
        ));
    }

    let lines = match operation {
        CsvTableOperation::Compare(key_column) => {
            csv_table_comparison_lines(&recomputed, key_column, &bundle_ref, &report_ref)?
        }
        CsvTableOperation::Join(left_key, right_key) => {
            csv_table_join_lines(&recomputed, left_key, right_key, &bundle_ref, &report_ref)?
        }
        CsvTableOperation::Filter(column, _) | CsvTableOperation::Sort(column, _) => {
            csv_table_rows_lines(&recomputed, column, &bundle_ref, &report_ref)?
        }
        CsvTableOperation::GroupCount(column) => {
            csv_table_group_lines(&recomputed, column, None, &bundle_ref, &report_ref)?
        }
        CsvTableOperation::GroupStatistics(group, value) => {
            csv_table_group_lines(&recomputed, group, Some(value), &bundle_ref, &report_ref)?
        }
        CsvTableOperation::Select(column) => {
            csv_table_select_lines(&recomputed, Some(column), true, &bundle_ref, &report_ref)?
        }
        CsvTableOperation::SelectIndex(_, has_header) => {
            csv_table_select_lines(&recomputed, None, has_header, &bundle_ref, &report_ref)?
        }
        CsvTableOperation::CsvToJson(_) | CsvTableOperation::JsonToCsv => {
            table_conversion_lines(&recomputed, &bundle_ref, &report_ref)?
        }
    };

    revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(&report_ref),
        "CSV_TABLE_CONTEXT_INVALID",
        &format!("CSV {label}"),
    )?;
    if lines.join("\n").len() > max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            format!("the exact CSV table {label} explanation exceeds the request evidence budget"),
        ));
    }
    Ok(Some(PriorDocumentExplanation {
        input_node,
        report_node,
        bundle_ref,
        report_ref,
        lines,
    }))
}

fn previous_json_table_conversion_explanation(
    store: &crate::store::Store,
    state: &CaseRevision,
    previous: &ResultRecord,
    max_evidence_bytes: usize,
    conversion_parser_version: &str,
) -> Result<PriorDocumentExplanation> {
    const CODE: &str = "PREVIOUS_JSON_TABLE_EXPLANATION_CONTEXT_INVALID";
    let invalid = || failure(CODE, "元のCSV、保存JSON、変換後CSVの対応を照合できません。");
    let actor = &state.actor_id;
    let case = &state.case_id;
    let (bundle_ref, bundle) = store
        .get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        .ok_or_else(invalid)?;
    if previous.guarantee != Guarantee::Computed
        || !previous.evidence.is_empty()
        || previous.calculation.is_some()
        || previous.id != identity(&bundle.request)?
        || bundle.meaning.parser_version != conversion_parser_version
    {
        return Err(invalid());
    }
    let original = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|reference| state.prior_results.get(reference))
        .ok_or_else(invalid)?;
    let conversion = previous_json_table_to_csv(store, &bundle, original)?;
    let report_bytes = canonical(&conversion.report)?;
    let report_ref = digest(&report_bytes);
    if previous.dependencies != conversion.dependencies(report_ref.clone())
        || previous.text != conversion.report.text
        || store
            .get_bytes_if_present(actor, case, &report_ref)?
            .as_deref()
            != Some(report_bytes.as_slice())
    {
        return Err(invalid());
    }
    revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(&report_ref),
        CODE,
        "retained JSON-table to CSV conversion",
    )?;

    let source_report = conversion
        .source_document_report
        .as_ref()
        .ok_or_else(invalid)?;
    let mut lines = if conversion_parser_version
        == crate::input::PREVIOUS_FILTERED_CSV_TO_CSV_PARSER_VERSION
    {
        let column = source_report
            .parameters
            .get("column_name")
            .ok_or_else(invalid)?;
        let mut lines =
            vec!["元CSVで選択条件を再適用し、保存したJSON表と出力CSVを照合しました。".into()];
        lines.extend(csv_table_rows_lines(
            source_report,
            column,
            &conversion.source_bundle_ref,
            &conversion.source_report_ref,
        )?);
        lines
    } else {
        let mut lines = vec![
            "元CSV、保存JSON、出力CSVを封印済みの入力から再計算し、保存結果と照合しました。".into(),
            "二つの一覧は、同じJSON Pointerと保存JSONの同じバイト範囲でつながります。".into(),
        ];
        lines.extend(table_conversion_lines(
            source_report,
            &conversion.source_bundle_ref,
            &conversion.source_report_ref,
        )?);
        lines
    };
    lines.extend(table_conversion_lines(
        &conversion.report,
        &bundle_ref,
        &report_ref,
    )?);
    if lines.join("\n").len() > max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "CSV変換の根拠説明が依頼の証拠予算を超えています。",
        ));
    }
    Ok(PriorDocumentExplanation {
        input_node: "previous-retained-table-conversion-input",
        report_node: "previous-retained-table-conversion-report",
        bundle_ref,
        report_ref,
        lines,
    })
}

const PASTED_TEXT_REPLACEMENT_CONTEXT_INVALID: &str = "PASTED_TEXT_REPLACEMENT_CONTEXT_INVALID";

fn invalid_pasted_text_replacement() -> minidora_domain::Error {
    failure(
        PASTED_TEXT_REPLACEMENT_CONTEXT_INVALID,
        "前回の置換結果と保存した入力・原文範囲を照合できません。",
    )
}

fn verify_pasted_text_replacement_report<'a>(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    bundle_ref: &str,
    bundle: &'a InputBundle,
) -> Result<(&'a str, &'a str, String, DocumentResult)> {
    let Intent::TextReplace {
        document,
        old,
        new,
        count,
    } = &bundle.meaning.intent
    else {
        return Err(invalid_pasted_text_replacement());
    };
    if previous.guarantee != Guarantee::Computed
        || !previous.evidence.is_empty()
        || previous.calculation.is_some()
        || previous.id != minidora_domain::identity(&bundle.request)?
        || bundle_ref != digest(&canonical(bundle)?)
    {
        return Err(invalid_pasted_text_replacement());
    }
    let report =
        if bundle.meaning.parser_version == crate::input::PASTED_TEXT_REPLACEMENT_PARSER_VERSION {
            replace_pasted_text(document, old, new, *count)?
        } else {
            replace_exact(document, old, new, *count)?
        };
    let report_bytes = canonical(&report)?;
    let report_ref = digest(&report_bytes);
    if previous.dependencies.as_slice() != std::slice::from_ref(&report_ref)
        || previous.text != report.text
        || store
            .get_bytes_if_present(actor, case, &report_ref)?
            .as_deref()
            != Some(report_bytes.as_slice())
    {
        return Err(invalid_pasted_text_replacement());
    }
    revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(&report_ref),
        PASTED_TEXT_REPLACEMENT_CONTEXT_INVALID,
        "pasted text replacement",
    )?;
    Ok((old, new, report_ref, report))
}

fn previous_pasted_text_replacement_explanation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    max_evidence_bytes: usize,
) -> Result<PriorDocumentExplanation> {
    let (bundle_ref, bundle) = store
        .get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        .ok_or_else(invalid_pasted_text_replacement)?;
    let (old, new, report_ref, report) =
        verify_pasted_text_replacement_report(store, actor, case, previous, &bundle_ref, &bundle)?;

    const MAX_SHOWN_CHANGES: usize = 8;
    let changes = report
        .spans
        .iter()
        .filter(|span| span.kind == "EXACT_REPLACEMENT");
    let change_count = changes.clone().count();
    let mut lines = vec![
        format!(
            "貼り付けた原文と保存結果を照合しました。完全一致で置換した箇所は{change_count}件です。"
        ),
        format!(
            "検索文字列 {:?} → 置換文字列 {:?}",
            preview(old),
            preview(new)
        ),
        format!("元本文SHA-256: {}", report.source_sha256),
        format!("置換後本文SHA-256: {}", report.output_sha256),
        format!("置換記録SHA-256: {report_ref}"),
    ];
    for span in changes.take(MAX_SHOWN_CHANGES) {
        let (start, end) = span
            .output_start
            .zip(span.output_end)
            .ok_or_else(invalid_pasted_text_replacement)?;
        let replaced = report
            .text
            .get(start..end)
            .ok_or_else(invalid_pasted_text_replacement)?;
        lines.push(format!(
            "原文byte[{}..{}) SHA-256 {} → 出力byte[{start}..{end}) SHA-256 {}",
            span.source_start,
            span.source_end,
            digest(span.source_text.as_bytes()),
            digest(replaced.as_bytes()),
        ));
    }
    if change_count > MAX_SHOWN_CHANGES {
        lines.push(format!(
            "残り{}件の範囲は置換記録にあります。",
            change_count - MAX_SHOWN_CHANGES
        ));
    }
    if lines.join("\n").len() > max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "置換の根拠説明が依頼の証拠予算を超えています。",
        ));
    }
    Ok(PriorDocumentExplanation {
        input_node: "previous-pasted-text-replacement-input",
        report_node: "previous-pasted-text-replacement-report",
        bundle_ref,
        report_ref,
        lines,
    })
}

pub(crate) fn revalidate_previous_document_render(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    required_report: Option<&str>,
    error_code: &str,
    label: &str,
) -> Result<String> {
    let (Some(prior_plan), Some(prior_render)) =
        (previous.content_plan.as_ref(), previous.render.as_ref())
    else {
        return Err(failure(
            error_code,
            format!("the prior {label} has no retained content plan or render observation"),
        ));
    };
    let mut unsigned_plan = prior_plan.clone();
    unsigned_plan.id.clear();
    if prior_plan.id != minidora_domain::identity(&unsigned_plan)?
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || required_report.is_some_and(|report| {
            !prior_plan
                .dependencies
                .iter()
                .any(|dependency| dependency == report)
        })
        || !crate::core::validate_render(prior_plan, prior_render)
    {
        return Err(failure(
            error_code,
            format!("the prior {label} content plan or rendered output is invalid"),
        ));
    }
    let prior_plan_bytes = canonical(prior_plan)?;
    let prior_render_bytes = canonical(prior_render)?;
    let render_ref = digest(&prior_render_bytes);
    for (artifact_ref, artifact_bytes, artifact_label) in [
        (digest(&prior_plan_bytes), prior_plan_bytes, "content plan"),
        (render_ref.clone(), prior_render_bytes, "render observation"),
    ] {
        match store.get_bytes_if_present(actor, case, &artifact_ref)? {
            Some(stored) if stored == artifact_bytes => {}
            Some(_) => {
                return Err(failure(
                    error_code,
                    format!("the prior {label} {artifact_label} artifact changed"),
                ));
            }
            None => {
                return Err(failure(
                    error_code,
                    format!("the prior {label} {artifact_label} artifact is missing"),
                ));
            }
        }
    }
    Ok(render_ref)
}

fn previous_text_document_explanation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    budget: &ResourceBudget,
    started: Instant,
    explanation_parser_version: &str,
) -> Result<Option<PriorDocumentExplanation>> {
    const LOCATION_INVALID: &str = "TEXT_LOCATION_CONTEXT_INVALID";
    const EDIT_INVALID: &str = "TEXT_EDIT_CONTEXT_INVALID";
    const COMPOSITION_INVALID: &str = "TEXT_COMPOSITION_CONTEXT_INVALID";
    let explain_summary = matches!(
        explanation_parser_version,
        crate::input::TEXT_SUMMARY_CANDIDATE_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_SUMMARY_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_QUESTION_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    ) && previous.guarantee == Guarantee::Hypothesis;
    let explain_composition = matches!(
        explanation_parser_version,
        crate::input::TEXT_COMPOSITION_EXPLANATION_PARSER_VERSION
            | crate::input::TEXT_SUMMARY_CANDIDATE_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_SUMMARY_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_QUESTION_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    );
    let explain_text_edit = explain_composition
        || explanation_parser_version == crate::input::TEXT_EDIT_EXPLANATION_PARSER_VERSION;
    let Some((bundle_ref, bundle)) =
        store.get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
    else {
        // The candidate heading can diagnose a missing source, but never
        // authorizes an explanation without the sealed typed request below.
        if explain_summary && previous.text.lines().next() == Some(EXTRACTIVE_SUMMARY_LABEL) {
            return Err(failure(
                TEXT_SUMMARY_CANDIDATE_CONTEXT_INVALID,
                "the prior extractive summary candidates have no retained sealed input",
            ));
        }
        if previous.guarantee != Guarantee::UserPremise {
            return Ok(None);
        }
        // A text document may itself contain JSON. Recognize a prior location
        // result here only when its displayed report bytes also name its last dependency.
        if let Some((report_ref, ancestors)) = previous.dependencies.split_last()
            && !ancestors.is_empty()
            && *report_ref == digest(previous.text.as_bytes())
            && serde_json::from_str::<LocatedTextEdit>(&previous.text)
                .is_ok_and(|report| report.consumed_artifacts.as_slice() == ancestors)
        {
            return Err(failure(
                LOCATION_INVALID,
                "the prior text location has no retained sealed input",
            ));
        }
        if explain_text_edit
            && let Some((report_ref, ancestors)) = previous.dependencies.split_last()
            && !ancestors.is_empty()
            && let Some(bytes) = store.get_bytes_if_present(actor, case, report_ref)?
            && serde_json::from_slice::<EditedText>(&bytes).is_ok_and(|report| {
                report.text == previous.text
                    && report.output_sha256 == digest(previous.text.as_bytes())
                    && report.consumed_artifacts.as_slice() == ancestors
            })
        {
            return Err(failure(
                EDIT_INVALID,
                "the prior text edit has no retained sealed input",
            ));
        }
        if explain_composition
            && let [report_ref] = previous.dependencies.as_slice()
            && let Some(bytes) = store.get_bytes_if_present(actor, case, report_ref)?
            && serde_json::from_slice::<TextCompositionResult>(&bytes).is_ok_and(|report| {
                report.operation == "TEXT_COMPOSE_EXPLICIT_FRAGMENTS"
                    && report.text == previous.text
                    && report.output_sha256 == digest(previous.text.as_bytes())
                    && canonical(&report).is_ok_and(|expected| expected == bytes)
            })
        {
            return Err(failure(
                COMPOSITION_INVALID,
                "the prior text composition has no retained sealed input",
            ));
        }
        return Ok(None);
    };
    let (code, label) = match (&bundle.meaning.intent, &previous.guarantee) {
        (Intent::LocateTextReplacement { .. }, Guarantee::UserPremise) => {
            (LOCATION_INVALID, "text location")
        }
        (Intent::EditText { .. }, Guarantee::UserPremise) if explain_text_edit => {
            (EDIT_INVALID, "text edit")
        }
        (Intent::ComposeText { .. }, Guarantee::UserPremise) if explain_composition => {
            (COMPOSITION_INVALID, "text composition")
        }
        (
            Intent::TextSummaryCandidates { .. } | Intent::SummarizePastedText { .. },
            Guarantee::Hypothesis,
        ) if explain_summary => (
            TEXT_SUMMARY_CANDIDATE_CONTEXT_INVALID,
            "extractive summary candidates",
        ),
        _ => return Ok(None),
    };
    if bundle_ref != digest(&canonical(&bundle)?) {
        return Err(failure(
            code,
            format!("the prior {label} input bundle hash changed"),
        ));
    }
    let load = |reference: &str| {
        store
            .get_bytes_if_present(actor, case, reference)?
            .ok_or_else(|| failure(code, format!("a prior {label} base artifact is missing")))
    };
    let verify_report = |report_bytes: &[u8], ancestors: &[String], display: &[u8]| {
        let report_ref = digest(report_bytes);
        if previous.dependencies.split_last() != Some((&report_ref, ancestors))
            || previous.text.as_bytes() != display
            || store
                .get_bytes_if_present(actor, case, &report_ref)?
                .as_deref()
                != Some(report_bytes)
        {
            return Err(failure(
                code,
                format!("the prior {label} no longer matches its retained report"),
            ));
        }
        revalidate_previous_document_render(
            store,
            actor,
            case,
            previous,
            Some(&report_ref),
            code,
            label,
        )?;
        Ok(report_ref)
    };
    let (input_node, report_node, report_ref, lines) = match &bundle.meaning.intent {
        Intent::EditText { document } => {
            let report = edit_explicit_text(document, load, started, budget.deadline_ms)?;
            let report_ref = verify_report(
                &canonical(&report)?,
                &report.consumed_artifacts,
                report.text.as_bytes(),
            )?;
            let lines = text_edit_explanation_lines(&report, &bundle_ref, &report_ref)?;
            (
                "previous-text-edit-input",
                "previous-text-edit-report",
                report_ref,
                lines,
            )
        }
        Intent::LocateTextReplacement { document } => {
            let report = locate_explicit_replacement(document, load, started, budget.deadline_ms)?;
            let report_bytes = canonical(&report)?;
            let report_ref =
                verify_report(&report_bytes, &report.consumed_artifacts, &report_bytes)?;
            let lines = text_location_explanation_lines(&report, &bundle_ref, &report_ref)?;
            (
                "previous-text-location-input",
                "previous-text-location-report",
                report_ref,
                lines,
            )
        }
        Intent::ComposeText { document } => {
            let report = compose_explicit_text(document)?;
            let report_ref = verify_report(&canonical(&report)?, &[], report.text.as_bytes())?;
            let lines = text_composition_explanation_lines(&report, &bundle_ref, &report_ref)?;
            (
                "previous-text-composition-input",
                "previous-text-composition-report",
                report_ref,
                lines,
            )
        }
        Intent::TextSummaryCandidates { .. } | Intent::SummarizePastedText { .. } => {
            let report = direct_summary_candidates(&bundle)?;
            let report_ref = verify_report(&canonical(&report)?, &[], report.text.as_bytes())?;
            let pasted_source = match &bundle.meaning.intent {
                Intent::SummarizePastedText { source } => Some(source),
                _ => None,
            };
            let lines = text_summary_candidate_explanation_lines(
                &report,
                &bundle_ref,
                &report_ref,
                pasted_source,
            )?;
            (
                "previous-text-summary-candidate-input",
                TEXT_SUMMARY_CANDIDATE_REPORT_NODE,
                report_ref,
                lines,
            )
        }
        _ => unreachable!("only sealed text document operations use this explanation"),
    };
    if lines.join("\n").len() > budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            format!("the {label} explanation exceeds the request evidence budget"),
        ));
    }
    Ok(Some(PriorDocumentExplanation {
        input_node,
        report_node,
        bundle_ref,
        report_ref,
        lines,
    }))
}

fn text_summary_candidate_explanation_lines(
    report: &ExtractiveSummaryCandidates,
    bundle_ref: &str,
    report_ref: &str,
    pasted_source: Option<&SourceSpan>,
) -> Result<Vec<String>> {
    let count_limit = match report.line_limit {
        Some(minidora_domain::SummaryLineLimit::Exact(count)) => format!(
            "指定行数: {count}。実際の行数: {}",
            report.text.lines().count()
        ),
        Some(minidora_domain::SummaryLineLimit::AtMost(count)) => format!(
            "行数上限: {count}。実際の行数: {}",
            report.text.lines().count()
        ),
        None => format!("文数上限: {}", report.max_sentences),
    };
    let mut lines = vec![
        "前回の抽出要約候補を、封印済みの原文から再確認しました。".into(),
        format!(
            "候補文数: {}。選択文数: {}。未選択文数: {}。文字数制限で見送った文数: {}。",
            report.sentence_count,
            report.sentences.len(),
            report.omitted_sentence_indices.len(),
            report.length_limited_sentence_indices.len(),
        ),
        format!(
            "{count_limit}。選択文の文字数: {} / 上限 {}（書式・注意書きは集計外）。出力書式: {}。",
            report.selected_character_count, report.max_chars, report.format,
        ),
        "必須指定の文を先に確保し、残りは語の頻度、先頭文の加点、長文の減点で選びます。同点は元本文の後方を優先し、表示は元本文の順です。".into(),
    ];
    if let Some(source) = pasted_source {
        lines.push(format!(
            "貼り付けた本文の入力範囲 byte[{}..{})",
            source.start, source.end
        ));
    } else {
        let token = report
            .source_document
            .get(report.source_text_json_start..report.source_text_json_end)
            .filter(|token| {
                serde_json::from_str::<String>(token).is_ok_and(|text| text == report.source_text)
            })
            .ok_or_else(|| {
                failure(
                    TEXT_SUMMARY_CANDIDATE_CONTEXT_INVALID,
                    "the retained JSON text token does not decode to the source text",
                )
            })?;
        lines.extend([
            format!("依頼JSON SHA-256: {}", report.source_sha256),
            format!(
                "依頼JSONの本文文字列トークン byte[{}..{}) SHA-256: {}",
                report.source_text_json_start,
                report.source_text_json_end,
                digest(token.as_bytes()),
            ),
        ]);
    }
    lines.extend([
        format!("元本文 SHA-256: {}", report.source_text_sha256),
        format!("候補出力 SHA-256: {}", report.output_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
    ]);
    for sentence in &report.sentences {
        let source = report
            .source_text
            .get(sentence.source_start..sentence.source_end);
        let output = report.text.get(sentence.output_start..sentence.output_end);
        if source != Some(sentence.text.as_str()) || output != source {
            return Err(failure(
                TEXT_SUMMARY_CANDIDATE_CONTEXT_INVALID,
                "the retained candidate does not match the source and output ranges",
            ));
        }
        let reason = if report
            .required_sentence_indices
            .contains(&sentence.sentence_index)
        {
            "必須指定"
        } else {
            "頻度・位置による選択"
        };
        lines.push(format!(
            "選択文 元本文第{}文 / 選択順{} / {reason} / 評点 {}/{}",
            sentence.sentence_index + 1,
            sentence.selection_rank,
            sentence.score_numerator,
            sentence.score_denominator,
        ));
        lines.push(format!(
            "  元本文 byte[{}..{}) → 候補出力 byte[{}..{}) / SHA-256: {} / 原文: {:?}",
            sentence.source_start,
            sentence.source_end,
            sentence.output_start,
            sentence.output_end,
            digest(sentence.text.as_bytes()),
            preview(&sentence.text),
        ));
    }
    lines.push(
        "選択と原文の一致を再確認しました。目的・意味・条件・否定の網羅と事実性は未検証です。"
            .into(),
    );
    Ok(lines)
}

fn text_location_explanation_lines(
    report: &LocatedTextEdit,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    const CODE: &str = "TEXT_LOCATION_CONTEXT_INVALID";
    let location = &report.location;
    let [patch] = report.proposed_edit.edits.as_slice() else {
        return Err(failure(
            CODE,
            "the verified text location has no unique proposal",
        ));
    };
    let json_token = |label, start, end, expected: &str| {
        let token = report
            .source_document
            .get(start..end)
            .filter(|text| {
                serde_json::from_str::<String>(text).is_ok_and(|value| value == expected)
            })
            .ok_or_else(|| {
                failure(
                    CODE,
                    "a prior text-location JSON token differs from its source",
                )
            })?;
        Ok::<_, minidora_domain::Error>(format!(
            "依頼JSONの{label} byte[{start}..{end}) sha256={} 原文={:?}",
            digest(token.as_bytes()),
            preview(token),
        ))
    };
    Ok(vec![
        "前回の置換箇所の特定を、保存済みの対象本文と依頼JSONから再確認しました。".into(),
        "指定された文字列と完全に一致する箇所は1件で、保護範囲との重なりはありません。".into(),
        format!("対象本文の成果物 SHA-256: {}", report.base_artifact),
        format!("対象本文 SHA-256: {}", report.base_output_sha256),
        format!("依頼JSON SHA-256: {}", report.source_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
        format!(
            "対象本文 byte[{}..{}) sha256={} 原文={:?}",
            location.start,
            location.end,
            digest(location.text.as_bytes()),
            preview(&location.text),
        ),
        json_token(
            "検索語",
            location.search_json_start,
            location.search_json_end,
            &location.text,
        )?,
        json_token(
            "置換語",
            location.replacement_json_start,
            location.replacement_json_end,
            &patch.replacement,
        )?,
        format!(
            "置換案: {:?} → {:?}",
            preview(&patch.expected),
            preview(&patch.replacement)
        ),
        "適用するには、別途 `text-edit:` で置換案を指定します。そのときに適用条件を検査します。"
            .into(),
    ])
}

fn text_edit_explanation_lines(
    report: &EditedText,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_CHANGES: usize = 8;
    let mut lines = vec![
        "前回の文章編集を、保存済みの編集前本文と依頼JSONから再確認しました。".into(),
        format!(
            "編集版: {}。変更数: {}。保護範囲数: {}。",
            report.revision,
            report.changes.len(),
            report.protected.len(),
        ),
        format!("編集前本文の成果物 SHA-256: {}", report.base_artifact),
        format!("編集前本文 SHA-256: {}", report.base_output_sha256),
        format!("編集後本文 SHA-256: {}", report.output_sha256),
        format!("依頼JSON SHA-256: {}", report.source_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
    ];
    for (index, change) in report.changes.iter().take(PREVIEW_CHANGES).enumerate() {
        let original = &change.expected;
        let output = report
            .text
            .get(change.new_start..change.new_end)
            .filter(|text| *text == change.replacement)
            .ok_or_else(|| {
                failure(
                    "TEXT_EDIT_CONTEXT_INVALID",
                    "an edit differs from its verified output range",
                )
            })?;
        let operation = if original.is_empty() {
            "挿入"
        } else if output.is_empty() {
            "削除"
        } else {
            "置換"
        };
        lines.extend([
            format!("変更{} {:?} ({operation})", index + 1, preview(&change.id)),
            format!(
                "編集前 byte[{}..{}) sha256={} 原文={:?}",
                change.old_start,
                change.old_end,
                digest(original.as_bytes()),
                preview(original),
            ),
            format!(
                "編集後 byte[{}..{}) sha256={} 出力={:?}",
                change.new_start,
                change.new_end,
                digest(output.as_bytes()),
                preview(output),
            ),
        ]);
    }
    if report.changes.len() > PREVIEW_CHANGES {
        lines.push(format!(
            "残りの変更 {}件は保存レポートに保持されています。",
            report.changes.len() - PREVIEW_CHANGES,
        ));
    }
    Ok(lines)
}

fn text_composition_explanation_lines(
    report: &TextCompositionResult,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_ITEMS: usize = 8;
    const CODE: &str = "TEXT_COMPOSITION_CONTEXT_INVALID";
    let source = |start, end| {
        report.source_document.get(start..end).ok_or_else(|| {
            failure(
                CODE,
                "a composition source range is outside the sealed JSON input",
            )
        })
    };
    let mut lines = vec![
        "前回の文章構成を、封印済みの素材と構成指示から再確認しました。".into(),
        format!(
            "採用単位数: {}。未採用単位数: {}。出力区間数: {}。",
            report.units.len(),
            report.omitted_unit_ids.len(),
            report.spans.len(),
        ),
        format!(
            "文字数: {} / 上限 {}",
            report.text.chars().count(),
            report.max_chars
        ),
        format!("依頼JSON SHA-256: {}", report.source_sha256),
        format!("構成後本文 SHA-256: {}", report.output_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
    ];
    for (index, unit) in report.units.iter().take(PREVIEW_ITEMS).enumerate() {
        lines.push(format!(
            "採用単位{} {:?} 役割={} 必須={} 保護={} 出力 byte[{}..{})",
            index + 1,
            preview(&unit.id),
            unit.role,
            unit.required,
            unit.protected,
            unit.output_start,
            unit.output_end,
        ));
    }
    for id in report.omitted_unit_ids.iter().take(PREVIEW_ITEMS) {
        lines.push(format!("未採用単位: {:?}", preview(id)));
    }
    for (index, span) in report.spans.iter().take(PREVIEW_ITEMS).enumerate() {
        let output = report
            .text
            .get(span.output_start..span.output_end)
            .filter(|text| *text == span.text)
            .ok_or_else(|| failure(CODE, "a composition span differs from its verified output"))?;
        let origin = match &span.origin {
            TextCompositionOrigin::MaterialText {
                material_id,
                material_json_start,
                material_json_end,
                text_start,
                text_end,
            } => {
                let token = source(*material_json_start, *material_json_end)?;
                let decoded: String = serde_json::from_str(token).map_err(|_| {
                    failure(
                        CODE,
                        "a composition material range has no exact JSON string",
                    )
                })?;
                if decoded.get(*text_start..*text_end) != Some(output) {
                    return Err(failure(
                        CODE,
                        "a composition range differs from its original material",
                    ));
                }
                format!(
                    "素材 {:?} 復号後 byte[{text_start}..{text_end}) 依頼JSON byte[{material_json_start}..{material_json_end}) sha256={}",
                    preview(material_id),
                    digest(token.as_bytes()),
                )
            }
            TextCompositionOrigin::UnitDirective {
                unit_id,
                unit_json_start,
                unit_json_end,
            } => {
                let directive = source(*unit_json_start, *unit_json_end)?;
                format!(
                    "書式 単位 {:?} 依頼JSON byte[{unit_json_start}..{unit_json_end}) sha256={}",
                    preview(unit_id),
                    digest(directive.as_bytes()),
                )
            }
        };
        lines.extend([
            format!(
                "出力区間{} byte[{}..{}) sha256={} 出力={:?}",
                index + 1,
                span.output_start,
                span.output_end,
                digest(output.as_bytes()),
                preview(output),
            ),
            origin,
        ]);
    }
    for (label, count) in [
        ("採用単位", report.units.len()),
        ("未採用単位", report.omitted_unit_ids.len()),
        ("出力区間", report.spans.len()),
    ] {
        if count > PREVIEW_ITEMS {
            lines.push(format!(
                "残りの{label} {}件は保存レポートに保持されています。",
                count - PREVIEW_ITEMS,
            ));
        }
    }
    Ok(lines)
}

fn previous_json_value_explanation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    bundle: &minidora_domain::InputBundle,
    bundle_ref: String,
    max_evidence_bytes: usize,
) -> Result<PriorDocumentExplanation> {
    const CODE: &str = "JSON_VALUE_CONTEXT_INVALID";
    if bundle_ref != digest(&canonical(bundle)?) {
        return Err(failure(CODE, "the sealed JSON input bundle hash changed"));
    }
    let verify_report = |bytes: &[u8], text: &str, label: &str| -> Result<String> {
        let report_ref = digest(bytes);
        if previous.dependencies != [report_ref.clone()]
            || previous.text != text
            || store
                .get_bytes_if_present(actor, case, &report_ref)?
                .as_deref()
                != Some(bytes)
        {
            return Err(failure(
                CODE,
                format!("the prior {label} no longer matches the retained report"),
            ));
        }
        revalidate_previous_document_render(
            store,
            actor,
            case,
            previous,
            Some(&report_ref),
            CODE,
            label,
        )?;
        Ok(report_ref)
    };
    let (input_node, report_node, document, report_ref, mut lines) = match &bundle.meaning.intent {
        Intent::JsonPointer { document, pointer } => {
            let root = strict_json_document(document)?;
            let object_keys_only =
                bundle.meaning.parser_version == crate::input::JSON_OBJECT_KEY_PATH_PARSER_VERSION;
            let node = json_node_at_pointer_with_mode(&root, pointer, object_keys_only)?;
            let raw = &document[node.span.clone()];
            if !previous.dependencies.is_empty() || previous.text != raw {
                return Err(failure(
                    CODE,
                    "the prior JSON Pointer result differs from the original value",
                ));
            }
            let render_ref = revalidate_previous_document_render(
                store,
                actor,
                case,
                previous,
                None,
                CODE,
                "JSON Pointer extraction",
            )?;
            let kind = match node.kind {
                JsonKind::Number => "number",
                JsonKind::Array(_) => "array",
                JsonKind::Object(_) => "object",
                JsonKind::Scalar if raw.starts_with('"') => "string",
                JsonKind::Scalar if raw == "null" => "null",
                JsonKind::Scalar => "boolean",
            };
            let lines = vec![
                "前回のJSON Pointer抽出を、封印済みの元入力から再確認しました。".into(),
                format!("JSON Pointer: {:?}", preview(pointer)),
                format!("型: {kind}"),
                format!("抽出値（原文）: {:?}", preview(raw)),
                json_value_range(raw, node.span.start, node.span.end),
                format!("抽出結果 SHA-256: {}", digest(raw.as_bytes())),
                format!("描画記録 SHA-256: {render_ref}"),
            ];
            (
                "previous-json-pointer-input",
                "previous-json-pointer-render",
                document,
                render_ref,
                lines,
            )
        }
        Intent::JsonStatistics { document, pointer } => {
            let report = crate::ports::json_pointer_statistics(document, pointer)?;
            let report_ref = verify_report(
                &canonical(&report)?,
                &serde_json::to_string_pretty(&report)?,
                "JSON statistics",
            )?;
            let lines = json_statistics_lines(document, &report, &report_ref)?;
            (
                "previous-json-statistics-input",
                "previous-json-statistics-report",
                document,
                report_ref,
                lines,
            )
        }
        Intent::JsonObjectProject { document, fields } => {
            let report = crate::ports::json_object_project(document, fields)?;
            let report_ref =
                verify_report(&canonical(&report)?, &report.json, "JSON object projection")?;
            let lines = json_object_projection_lines(document, &report, &report_ref)?;
            (
                "previous-json-object-project-input",
                "previous-json-object-project-report",
                document,
                report_ref,
                lines,
            )
        }
        Intent::JsonCompare { document } => {
            let report = crate::ports::json_compare(document)?;
            let report_ref = verify_report(
                &canonical(&report)?,
                &serde_json::to_string_pretty(&report)?,
                "JSON structural comparison",
            )?;
            let lines = json_comparison_lines(document, &report, &report_ref)?;
            (
                "previous-json-comparison-input",
                "previous-json-comparison-report",
                document,
                report_ref,
                lines,
            )
        }
        _ => unreachable!("only sealed JSON value operations use this explanation"),
    };
    lines.splice(
        1..1,
        [
            format!("元資料 SHA-256: {}", digest(document.as_bytes())),
            format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        ],
    );
    if lines.join("\n").len() > max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the JSON value explanation exceeds the request evidence budget",
        ));
    }
    Ok(PriorDocumentExplanation {
        input_node,
        report_node,
        bundle_ref,
        report_ref,
        lines,
    })
}

fn json_value_range(raw: &str, start: usize, end: usize) -> String {
    format!("元 byte[{start}..{end}) sha256={}", digest(raw.as_bytes()))
}

fn json_comparison_lines(
    document: &str,
    report: &crate::ports::JsonComparisonReport,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_DIFFERENCES: usize = 8;
    let result = if report.equal { "一致" } else { "不一致" };
    let mut lines = vec![
        "前回のJSON構造比較を、封印済みの元入力から再計算しました。".into(),
        format!("比較結果: {result}。差分数: {}。", report.differences.len()),
        "オブジェクトのキー順は無視し、配列は同じ位置、文字列は復号後の内容、数値は厳密な十進値で比較しました。".into(),
        format!("左の値 SHA-256: {}", report.left_sha256),
        format!("右の値 SHA-256: {}", report.right_sha256),
        format!("report artifact SHA-256: {report_ref}"),
    ];
    if !report.differences.is_empty() {
        lines.push(
            "JSON Pointerは左右それぞれの比較対象を起点とし、空文字は対象全体を指します。".into(),
        );
    }
    for (index, difference) in report
        .differences
        .iter()
        .take(PREVIEW_DIFFERENCES)
        .enumerate()
    {
        lines.push(format!(
            "差分{} JSON Pointer {:?} {}",
            index + 1,
            preview(&difference.pointer),
            difference.change,
        ));
        for (side, span) in [("左", &difference.left), ("右", &difference.right)] {
            let Some(span) = span else {
                lines.push(format!("{side} 対応する値なし"));
                continue;
            };
            let raw = document
                .get(span.source_start..span.source_end)
                .filter(|value| digest(value.as_bytes()) == span.source_sha256)
                .ok_or_else(|| {
                    failure(
                        "JSON_VALUE_CONTEXT_INVALID",
                        "a JSON difference does not match its original source range",
                    )
                })?;
            lines.push(format!(
                "{side} 元 byte[{}..{}) sha256={} 原文={:?}",
                span.source_start,
                span.source_end,
                span.source_sha256,
                preview(raw),
            ));
        }
    }
    if report.differences.len() > PREVIEW_DIFFERENCES {
        lines.push(format!(
            "残りの差分 {}件は保存レポートに保持されています。",
            report.differences.len() - PREVIEW_DIFFERENCES,
        ));
    }
    Ok(lines)
}

fn json_object_projection_lines(
    document: &str,
    report: &crate::ports::JsonObjectProjectReport,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_FIELDS: usize = 8;
    let invalid = || {
        failure(
            "JSON_VALUE_CONTEXT_INVALID",
            "a JSON object field does not match the verified key or value range",
        )
    };
    let tokens = |source: &crate::ports::JsonComparisonSpan, start, end| {
        let raw = document
            .get(source.source_start..source.source_end)
            .ok_or_else(invalid)?;
        let output = report.json.get(start..end).ok_or_else(invalid)?;
        if digest(raw.as_bytes()) != source.source_sha256 {
            return Err(invalid());
        }
        Ok((raw, output))
    };
    let mut lines = vec![
        "前回のJSONオブジェクト項目選択を、封印済みの元入力から再計算しました。".into(),
        format!(
            "選択項目数: {}。指定された順序で、各値の元の表記を保って出力しました。",
            report.fields.len()
        ),
        format!("選択結果 SHA-256: {}", report.output_sha256),
        format!("report artifact SHA-256: {report_ref}"),
    ];
    for (index, field) in report.fields.iter().take(PREVIEW_FIELDS).enumerate() {
        let (key, output_key) = tokens(
            &field.key_source,
            field.output_key_start,
            field.output_key_end,
        )?;
        let (value, output_value) = tokens(
            &field.value_source,
            field.output_value_start,
            field.output_value_end,
        )?;
        if serde_json::from_str::<String>(key).ok().as_deref() != Some(field.name.as_str())
            || serde_json::from_str::<String>(output_key).ok().as_deref()
                != Some(field.name.as_str())
            || value != output_value
        {
            return Err(invalid());
        }
        lines.push(format!(
            "項目{} {:?} JSON Pointer {:?}",
            index + 1,
            preview(&field.name),
            preview(&field.source_pointer),
        ));
        for (label, span, raw, start, end, output) in [
            (
                "キー",
                &field.key_source,
                key,
                field.output_key_start,
                field.output_key_end,
                output_key,
            ),
            (
                "値",
                &field.value_source,
                value,
                field.output_value_start,
                field.output_value_end,
                output_value,
            ),
        ] {
            lines.push(format!(
                "{label} {} 原文={:?} → 出力 byte[{start}..{end}) sha256={} 表記={:?}",
                json_value_range(raw, span.source_start, span.source_end),
                preview(raw),
                digest(output.as_bytes()),
                preview(output),
            ));
        }
    }
    if report.fields.len() > PREVIEW_FIELDS {
        lines.push(format!(
            "残りの項目 {}件は保存レポートに保持されています。",
            report.fields.len() - PREVIEW_FIELDS
        ));
    }
    Ok(lines)
}

fn json_statistics_lines(
    document: &str,
    report: &crate::ports::JsonStatisticsReport,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_VALUES: usize = 8;
    let invalid = || {
        failure(
            "JSON_VALUE_CONTEXT_INVALID",
            "the JSON statistics source ranges do not match the selected values",
        )
    };
    let (array, observations) = report.spans.split_first().ok_or_else(invalid)?;
    let stats = &report.statistics;
    if array.kind != "JSON_ARRAY"
        || observations.len() != stats.count
        || stats.normalized_values.len() != stats.count
    {
        return Err(invalid());
    }
    let range = |span: &crate::ports::JsonSourceSpan| {
        document
            .get(span.source_start..span.source_end)
            .filter(|raw| *raw == span.source_text)
            .map(|raw| json_value_range(raw, span.source_start, span.source_end))
            .ok_or_else(invalid)
    };
    let mut lines = vec![
        "前回のJSON統計を、封印済みの元入力から再計算しました。".into(),
        format!("JSON Pointer: {:?}", preview(&report.pointer)),
        format!("report artifact SHA-256: {report_ref}"),
        format!("元の配列 {}", range(array)?),
        format!(
            "件数: {}、合計: {}、平均: {}",
            stats.count, stats.sum, stats.mean
        ),
        format!(
            "中央値: {}、最小: {}、最大: {}",
            stats.median, stats.min, stats.max
        ),
        format!(
            "母分散: {}、標本分散: {}",
            stats.population_variance,
            stats
                .sample_variance
                .as_deref()
                .unwrap_or("算出不可（1件）")
        ),
    ];
    for (index, (span, normalized)) in observations
        .iter()
        .zip(&stats.normalized_values)
        .take(PREVIEW_VALUES)
        .enumerate()
    {
        if span.kind != "JSON_NUMBER" || span.array_index != Some(index) {
            return Err(invalid());
        }
        lines.push(format!(
            "要素{index} JSON Pointer {}/{}。数値表記: {:?}、厳密値: {normalized}。{}",
            preview(&report.pointer),
            index,
            preview(&span.source_text),
            range(span)?
        ));
    }
    if observations.len() > PREVIEW_VALUES {
        lines.push(format!(
            "残りの数値 {}件は保存レポートに保持されています。",
            observations.len() - PREVIEW_VALUES
        ));
    }
    Ok(lines)
}

fn table_conversion_lines(
    report: &DocumentResult,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_CELLS: usize = 8;
    let to_json = report.operation == "CSV_TO_JSON";
    let has_header = match report.parameters.get("has_header").map(String::as_str) {
        Some("true") => true,
        Some("false") => false,
        _ => {
            return Err(failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "the table conversion has no header mode",
            ));
        }
    };
    let (title, header_kind) = if to_json {
        ("CSV→JSON", "CSV_HEADER")
    } else {
        ("JSON→CSV", "JSON_HEADER")
    };
    let (headers, cells): (Vec<_>, Vec<_>) = report
        .spans
        .iter()
        .partition(|span| span.kind == header_kind);
    let column_count = report
        .spans
        .iter()
        .filter_map(|span| span.column)
        .max()
        .map_or(0, |index| index + 1);
    // CSV producer rows include the original header record. JSON data rows start at zero.
    let header_offset = usize::from(to_json && has_header);
    let row_count = cells
        .iter()
        .filter_map(|span| span.row)
        .max()
        .map_or(0, |row| row + 1 - header_offset);
    let mut lines = vec![
        format!("前回の{title}変換を、封印済みの元入力から再計算しました。"),
        format!("ヘッダー: {}", if has_header { "あり" } else { "なし" }),
        format!("データ行数: {row_count}、列数: {column_count}"),
        "見出しとセルは文字列のまま保ちます。JSON PointerはJSON側の位置です。".into(),
        format!("元資料 SHA-256: {}", report.source_sha256),
        format!("変換結果 SHA-256: {}", report.output_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
    ];
    for span in headers
        .iter()
        .take(PREVIEW_CELLS)
        .chain(cells.iter().take(PREVIEW_CELLS))
    {
        let column = span.column.ok_or_else(|| {
            failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "a table conversion span has no column",
            )
        })?;
        let (location, pointer) = if span.kind == header_kind {
            (
                format!("見出し列{}", column + 1),
                format!("/columns/{column}"),
            )
        } else {
            let row = span
                .row
                .and_then(|row| row.checked_sub(header_offset))
                .ok_or_else(|| {
                    failure(
                        "CSV_TABLE_CONTEXT_INVALID",
                        "a conversion cell has no original data row",
                    )
                })?;
            (
                format!("元行{}・列{}", row + 1, column + 1),
                format!("/rows/{row}/{column}"),
            )
        };
        lines.push(format!(
            "{location} JSON Pointer {pointer} {}",
            table_conversion_span(report, span, to_json)?
        ));
    }
    for (label, count) in [("見出し", headers.len()), ("データセル", cells.len())] {
        if count > PREVIEW_CELLS {
            lines.push(format!(
                "残りの{label} {}件の対応は保存レポートに保持されています。",
                count - PREVIEW_CELLS
            ));
        }
    }
    Ok(lines)
}

fn table_conversion_span(
    report: &DocumentResult,
    span: &DocumentSpan,
    to_json: bool,
) -> Result<String> {
    let invalid = || {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "a table conversion range does not match the verified cell",
        )
    };
    let source = report
        .source_document
        .get(span.source_start..span.source_end)
        .ok_or_else(invalid)?;
    let (start, end) = span.output_start.zip(span.output_end).ok_or_else(invalid)?;
    let output = report.text.get(start..end).ok_or_else(invalid)?;
    let decoded = span.decoded_value.as_deref().ok_or_else(invalid)?;
    let (json, csv) = if to_json {
        (output, source)
    } else {
        (source, output)
    };
    let mut csv_end = 0;
    let csv_value = cell(CsvInput::strict(csv), &mut csv_end).map_err(|_| invalid())?;
    if source != span.source_text
        || csv_end != csv.len()
        || csv_value.value != decoded
        || serde_json::from_str::<String>(json).ok().as_deref() != Some(decoded)
    {
        return Err(invalid());
    }
    Ok(format!(
        "値 {:?}。元 byte[{}..{}) sha256={} 原文={:?} → 出力 byte[{start}..{end}) sha256={} 表記={:?}",
        preview(decoded),
        span.source_start,
        span.source_end,
        digest(source.as_bytes()),
        preview(source),
        digest(output.as_bytes()),
        preview(output)
    ))
}

fn csv_table_select_lines(
    report: &DocumentResult,
    column: Option<&str>,
    has_header: bool,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_ROWS: usize = 8;
    let values: Vec<String> = serde_json::from_str(&report.text).map_err(|_| {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed CSV selection is not a string array",
        )
    })?;
    let column_index = report
        .parameters
        .get("column_index")
        .and_then(|index| index.parse::<usize>().ok())
        .ok_or_else(|| {
            failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "the selected CSV column has no index",
            )
        })?;
    let header = if has_header {
        Some(csv_table_find_span(
            report,
            "CSV_HEADER",
            Some(0),
            Some(column_index),
            column,
        )?)
    } else {
        None
    };
    let selected: Vec<_> = report
        .spans
        .iter()
        .filter(|span| span.kind == "CSV_SELECTED_CELL")
        .collect();
    if selected.len() != values.len()
        || selected
            .iter()
            .zip(&values)
            .enumerate()
            .any(|(index, (span, value))| {
                span.row != Some(index + usize::from(has_header))
                    || span.column != Some(column_index)
                    || span.decoded_value.as_ref() != Some(value)
                    || span.output_start.is_none()
                    || span.output_end.is_none()
            })
    {
        return Err(failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed CSV selection does not match its original cell map",
        ));
    }
    let (selection, method) = if let Some(column) = column {
        (
            format!(
                "選択列: {:?} (元列番号 {})",
                preview(column),
                column_index + 1
            ),
            "指定した名前に完全一致する見出しを使い、元のデータ行順で文字列を取り出します。",
        )
    } else {
        (
            format!("選択列: 元列番号 {}", column_index + 1),
            if has_header {
                "先頭行を見出しとして扱い、元のデータ行順で文字列を取り出します。"
            } else {
                "先頭行もデータとして扱い、元の行順で文字列を取り出します。"
            },
        )
    };
    let mut lines = vec![
        "前回のCSV列選択を、封印済みの元入力から再計算しました。".into(),
        selection,
        method.into(),
        format!("選択値数: {}", values.len()),
        format!("元資料 SHA-256: {}", report.source_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
    ];
    if let Some(header) = header {
        lines.push(format!("列見出し: {}", csv_table_span(report, header)?));
    }
    for (index, (span, value)) in selected.iter().zip(&values).take(PREVIEW_ROWS).enumerate() {
        lines.push(format!(
            "元行{}: 値 {:?}。{}",
            index + 1,
            preview(value),
            csv_table_span(report, span)?
        ));
    }
    if values.len() > PREVIEW_ROWS {
        lines.push(format!(
            "残りの選択値 {}件は保存レポートに保持されています。",
            values.len() - PREVIEW_ROWS
        ));
    }
    Ok(lines)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvTableRowsText {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
}

fn csv_table_rows_lines(
    report: &DocumentResult,
    column: &str,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_ROWS: usize = 8;
    let typed: CsvTableRowsText = serde_json::from_str(&report.text).map_err(|_| {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed CSV table has an unsupported shape",
        )
    })?;
    let column_index = typed
        .columns
        .iter()
        .position(|name| name == column)
        .ok_or_else(|| {
            failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "the selected CSV column is missing",
            )
        })?;
    let header = csv_table_find_span(
        report,
        &format!("{}_HEADER", report.operation),
        Some(0),
        Some(column_index),
        Some(column),
    )?;
    let cell_kind = format!("{}_CELL", report.operation);
    // The producer emits cells in output order and stores each original row number.
    let output_keys: Vec<_> = report
        .spans
        .iter()
        .filter(|span| span.kind == cell_kind && span.column == Some(column_index))
        .collect();
    if output_keys.len() != typed.rows.len()
        || output_keys.iter().zip(&typed.rows).any(|(span, row)| {
            span.row.is_none_or(|source_row| source_row == 0)
                || span.output_start.is_none()
                || span.output_end.is_none()
                || row.len() != typed.columns.len()
                || span.decoded_value.as_deref() != row.get(column_index).map(String::as_str)
        })
    {
        return Err(failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed CSV output rows do not correspond to their source cells",
        ));
    }

    let mut lines = if let Some(equals) = report.parameters.get("equals") {
        let decisions: Vec<_> = report
            .spans
            .iter()
            .filter(|span| span.kind == "CSV_FILTER_DECISION")
            .collect();
        let matched_rows = decisions
            .iter()
            .filter(|span| span.decoded_value.as_ref() == Some(equals))
            .map(|span| span.row);
        if !matched_rows.eq(output_keys.iter().map(|span| span.row)) {
            return Err(failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "the CSV filter decisions do not select the reported source rows",
            ));
        }
        let mut lines = vec![
            "前回のCSV表絞り込みを、封印済みの元入力から再計算しました。".into(),
            format!(
                "条件: {:?} 列のデコード後の文字列が {:?} と完全一致する行を、元の順序で残します。",
                preview(column),
                preview(equals)
            ),
            format!(
                "一致行: {}、不一致行: {}",
                output_keys.len(),
                decisions.len() - output_keys.len()
            ),
        ];
        for (index, span) in decisions.iter().take(PREVIEW_ROWS).enumerate() {
            let decision = if span.decoded_value.as_ref() == Some(equals) {
                "一致"
            } else {
                "不一致"
            };
            lines.push(format!(
                "元行{}: {decision}。{}",
                index + 1,
                csv_table_span(report, span)?
            ));
        }
        if decisions.len() > PREVIEW_ROWS {
            lines.push(format!(
                "残りの判定 {}行は保存artifactに保持されています。",
                decisions.len() - PREVIEW_ROWS
            ));
        }
        lines
    } else {
        let direction = match report.parameters.get("sort_order").map(String::as_str) {
            Some("ascending") => "昇順",
            Some("descending") => "降順",
            _ => {
                return Err(failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    "the recomputed CSV sort direction is missing or invalid",
                ));
            }
        };
        vec![
            "前回のCSV表並べ替えを、封印済みの元入力から再計算しました。".into(),
            format!(
                "並べ替え: {:?} 列のデコード後の値をUnicodeの符号位置順で比較した{direction}です。数値に見える値も文字列として比較します。同じ値の行は元の順序を保ちます。",
                preview(column)
            ),
            format!("出力行数: {}", output_keys.len()),
        ]
    };
    lines.extend([
        format!("元資料 SHA-256: {}", report.source_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
        format!("対象列の見出し: {}", csv_table_span(report, header)?),
    ]);
    for (index, span) in output_keys.iter().take(PREVIEW_ROWS).enumerate() {
        lines.push(format!(
            "出力行{} ← 元行{}。{}",
            index + 1,
            span.row.expect("validated source row"),
            csv_table_span(report, span)?
        ));
    }
    if output_keys.len() > PREVIEW_ROWS {
        lines.push(format!(
            "残りの出力行 {}行の元セルは保存artifactに保持されています。",
            output_keys.len() - PREVIEW_ROWS
        ));
    }
    Ok(lines)
}

#[derive(Deserialize)]
struct CsvGroupedTableText {
    groups: Vec<CsvGroupText>,
}

#[derive(Deserialize)]
struct CsvGroupText {
    value: String,
    count: Option<usize>,
    source_record_numbers: Option<Vec<usize>>,
    statistics: Option<crate::math::Statistics>,
}

fn csv_table_group_lines(
    report: &DocumentResult,
    group_column: &str,
    value_column: Option<&str>,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    const PREVIEW_GROUPS: usize = 8;
    const PREVIEW_MEMBERS: usize = 4;
    let typed: CsvGroupedTableText = serde_json::from_str(&report.text).map_err(|_| {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed grouped CSV table has an unsupported shape",
        )
    })?;
    let mut members_by_key = BTreeMap::<&str, Vec<&DocumentSpan>>::new();
    let mut value_spans = BTreeMap::new();
    for span in &report.spans {
        match span.kind.as_str() {
            "CSV_GROUP_MEMBER" => {
                let key = span.decoded_value.as_deref().ok_or_else(|| {
                    failure(
                        "CSV_TABLE_CONTEXT_INVALID",
                        "a CSV group key has no decoded value",
                    )
                })?;
                members_by_key.entry(key).or_default().push(span);
            }
            "CSV_GROUP_STATISTIC_VALUE" => {
                value_spans.insert(span.row, span);
            }
            _ => {}
        }
    }
    let title = if value_column.is_some() {
        "前回のCSVグループ統計"
    } else {
        "前回のCSVグループ件数"
    };
    let header_kind = if value_column.is_some() {
        "CSV_GROUP_COLUMN_HEADER"
    } else {
        "CSV_GROUP_COLUMN"
    };
    let header = csv_table_find_span(report, header_kind, Some(0), None, Some(group_column))?;
    let row_count: usize = members_by_key.values().map(Vec::len).sum();
    let mut lines = vec![
        format!("{title}を、封印済みの元入力から再計算しました。"),
        format!(
            "グループ列: {:?}。デコード後の文字列が完全一致する行をまとめ、グループを初出順に示します。",
            preview(group_column)
        ),
        format!("行数: {row_count}、グループ数: {}", typed.groups.len()),
        format!("元資料 SHA-256: {}", report.source_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
        format!("グループ列の見出し: {}", csv_table_span(report, header)?),
    ];
    if let Some(column) = value_column {
        let header = csv_table_find_span(
            report,
            "CSV_GROUP_VALUE_HEADER",
            Some(0),
            None,
            Some(column),
        )?;
        lines.push(format!(
            "計測列: {:?}。単位付きの値はグループ内で単位をそろえ、合計を件数で割って平均を求めます。{}",
            preview(column),
            csv_table_span(report, header)?
        ));
    }

    for (index, group) in typed.groups.iter().take(PREVIEW_GROUPS).enumerate() {
        let members = members_by_key.get(group.value.as_str()).ok_or_else(|| {
            failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "a reported CSV group has no source members",
            )
        })?;
        let prefix = format!(
            "グループ{} {:?}。件数: {}",
            index + 1,
            preview(&group.value),
            members.len()
        );
        match (&group.statistics, &group.source_record_numbers) {
            (None, None) if value_column.is_none() && group.count == Some(members.len()) => {
                lines.push(prefix);
            }
            (Some(stats), Some(rows))
                if value_column.is_some()
                    && stats.count == members.len()
                    && stats.normalized_values.len() == members.len()
                    && rows
                        .iter()
                        .copied()
                        .map(Some)
                        .eq(members.iter().map(|span| span.row)) =>
            {
                lines.push(format!(
                    "{prefix}、合計: {}、平均: {}、単位: {}。中央値: {}、最小: {}、最大: {}。",
                    stats.sum,
                    stats.mean,
                    stats.unit.as_deref().unwrap_or("なし"),
                    stats.median,
                    stats.min,
                    stats.max
                ));
                lines.push(format!(
                    "母分散: {}、標本分散: {}、分散の単位: {}。",
                    stats.population_variance,
                    stats
                        .sample_variance
                        .as_deref()
                        .unwrap_or("算出不可（1件）"),
                    stats.variance_unit.as_deref().unwrap_or("なし")
                ));
            }
            _ => {
                return Err(failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    "a reported CSV group disagrees with the retained source members",
                ));
            }
        }
        for (member_index, member) in members.iter().take(PREVIEW_MEMBERS).enumerate() {
            let row = member.row.filter(|row| *row != 0).ok_or_else(|| {
                failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    "a CSV group member has no original row",
                )
            })?;
            let mut line = format!("元行{row} キー {}", csv_table_span(report, member)?);
            if let Some(stats) = &group.statistics {
                let value = value_spans.get(&Some(row)).ok_or_else(|| {
                    failure(
                        "CSV_TABLE_CONTEXT_INVALID",
                        "a CSV group member has no measured source cell",
                    )
                })?;
                line.push_str(&format!(
                    "。計測セル {}。集計に使った値: {}",
                    csv_table_span(report, value)?,
                    stats.normalized_values[member_index]
                ));
            }
            lines.push(line);
        }
        if members.len() > PREVIEW_MEMBERS {
            lines.push(format!(
                "このグループの残り {}行は保存artifactに保持されています。",
                members.len() - PREVIEW_MEMBERS
            ));
        }
    }
    if typed.groups.len() > PREVIEW_GROUPS {
        lines.push(format!(
            "残り {}グループは保存artifactに保持されています。",
            typed.groups.len() - PREVIEW_GROUPS
        ));
    }
    Ok(lines)
}

fn csv_table_comparison_lines(
    recomputed: &DocumentResult,
    key_column: &str,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    let typed: CsvTableComparisonText = serde_json::from_str(&recomputed.text).map_err(|_| {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed CSV comparison text has an unsupported shape",
        )
    })?;
    if typed.key_column != key_column {
        return Err(failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed CSV comparison key differs from the sealed request",
        ));
    }
    for span in &recomputed.spans {
        csv_table_span(recomputed, span)?;
    }
    let key_header = csv_table_find_span(
        recomputed,
        "CSV_COMPARE_KEY_HEADER",
        Some(0),
        None,
        Some(key_column),
    )?;

    let mut cell_changes = 0usize;
    let mut removed_rows = 0usize;
    let mut added_rows = 0usize;
    for change in &typed.changes {
        match change {
            CsvTableComparisonChange::CellChanged { .. } => cell_changes += 1,
            CsvTableComparisonChange::RowRemoved { .. } => removed_rows += 1,
            CsvTableComparisonChange::RowAdded { .. } => added_rows += 1,
        }
    }
    let mut lines = vec![
        "前回のCSV表比較を、封印済みの元入力から再計算しました。".into(),
        format!(
            "比較キー: {:?}; 結果: {} (デコード後文字列の完全一致)",
            typed.key_column,
            if typed.changes.is_empty() {
                "一致"
            } else {
                "差分あり"
            }
        ),
        format!(
            "差分: {}件 (セル変更: {}; 左側のみの行: {}; 右側のみの行: {})",
            typed.changes.len(),
            cell_changes,
            removed_rows,
            added_rows
        ),
        "値の意味、数値としての同一性、現実の真偽は判定していません。".into(),
        format!("元資料 SHA-256: {}", recomputed.source_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
        format!("キー見出し: {}", csv_table_span(recomputed, key_header)?),
    ];
    for change in typed.changes.iter().take(8) {
        match change {
            CsvTableComparisonChange::CellChanged {
                key,
                column,
                left,
                right,
            } => {
                let left_key = csv_table_find_span(
                    recomputed,
                    "CSV_COMPARE_CHANGED_KEY",
                    None,
                    None,
                    Some(key),
                )?;
                let right_key = csv_table_find_span(
                    recomputed,
                    "CSV_COMPARE_MATCHED_KEY",
                    None,
                    None,
                    Some(key),
                )?;
                let column_span = csv_table_find_span(
                    recomputed,
                    "CSV_COMPARE_COLUMN",
                    Some(0),
                    None,
                    Some(column),
                )?;
                let column_index = column_span.column.ok_or_else(|| {
                    failure(
                        "CSV_TABLE_CONTEXT_INVALID",
                        "a changed comparison column has no original column index",
                    )
                })?;
                let left_span = csv_table_find_span(
                    recomputed,
                    "CSV_COMPARE_LEFT_CELL",
                    left_key.row,
                    Some(column_index),
                    Some(left),
                )?;
                let right_span = csv_table_find_span(
                    recomputed,
                    "CSV_COMPARE_RIGHT_CELL",
                    right_key.row,
                    Some(column_index),
                    Some(right),
                )?;
                lines.push(format!(
                    "セル変更 キー {:?} / 列 {:?}: 左 元行{} 値 {:?} {}; 右 元行{} 値 {:?} {}; キー位置 左 {}; 右 {}",
                    preview(key),
                    preview(column),
                    left_key.row.unwrap_or(0),
                    preview(left),
                    csv_table_span(recomputed, left_span)?,
                    right_key.row.unwrap_or(0),
                    preview(right),
                    csv_table_span(recomputed, right_span)?,
                    csv_table_span(recomputed, left_key)?,
                    csv_table_span(recomputed, right_key)?
                ));
            }
            CsvTableComparisonChange::RowRemoved { key, row } => {
                lines.push(csv_table_comparison_row_line(
                    recomputed,
                    "CSV_COMPARE_REMOVED_KEY",
                    "CSV_COMPARE_REMOVED_CELL",
                    "左側のみの行",
                    key,
                    row,
                )?);
            }
            CsvTableComparisonChange::RowAdded { key, row } => {
                lines.push(csv_table_comparison_row_line(
                    recomputed,
                    "CSV_COMPARE_ADDED_KEY",
                    "CSV_COMPARE_ADDED_CELL",
                    "右側のみの行",
                    key,
                    row,
                )?);
            }
        }
    }
    if typed.changes.len() > 8 {
        lines.push(format!(
            "残りの差分 {}件はreport artifactに保持されています。",
            typed.changes.len() - 8
        ));
    }
    Ok(lines)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvTableJoinText {
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    left_source_record_numbers: Vec<usize>,
    right_source_record_numbers: Vec<usize>,
    unmatched_left: Vec<CsvTableUnmatchedKey>,
    unmatched_right: Vec<CsvTableUnmatchedKey>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CsvTableUnmatchedKey {
    key: String,
    source_record_number: usize,
}

fn csv_table_join_lines(
    report: &DocumentResult,
    left_key: &str,
    right_key: &str,
    bundle_ref: &str,
    report_ref: &str,
) -> Result<Vec<String>> {
    let invalid = || {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "the recomputed CSV inner join has inconsistent rows or key columns",
        )
    };
    let joined: CsvTableJoinText = serde_json::from_str(&report.text).map_err(|_| invalid())?;
    if joined.rows.len() != joined.left_source_record_numbers.len()
        || joined.rows.len() != joined.right_source_record_numbers.len()
        || joined
            .rows
            .iter()
            .any(|row| row.len() != joined.columns.len())
    {
        return Err(invalid());
    }
    for span in &report.spans {
        csv_table_span(report, span)?;
    }
    let left_header = csv_table_find_span(
        report,
        "CSV_INNER_JOIN_LEFT_HEADER",
        Some(0),
        None,
        Some(left_key),
    )?;
    let right_header = csv_table_find_span(
        report,
        "CSV_INNER_JOIN_RIGHT_HEADER",
        Some(0),
        None,
        Some(right_key),
    )?;
    let left_key_index = left_header.column.ok_or_else(invalid)?;
    let right_key_index = right_header.column.ok_or_else(invalid)?;
    let left_columns = report
        .spans
        .iter()
        .filter(|span| span.kind == "CSV_INNER_JOIN_LEFT_HEADER")
        .count();
    let mut lines = vec![
        "前回のCSV表内部結合を、封印済みの元入力から再計算しました。".into(),
        format!("左のキー {:?} と右のキー {:?} は、CSVをデコードした後の文字列が完全に等しい場合に一致します。", preview(left_key), preview(right_key)),
        "左右でキーはそれぞれ一意です。一致した組を左の元行順に出力し、片側にしかない行は別に記録します。".into(),
        format!(
            "一致行: {}、左の未一致行: {}、右の未一致行: {}",
            joined.rows.len(),
            joined.unmatched_left.len(),
            joined.unmatched_right.len()
        ),
        format!("元資料 SHA-256: {}", report.source_sha256),
        format!("封印済みInputBundle SHA-256: {bundle_ref}"),
        format!("report artifact SHA-256: {report_ref}"),
        format!("左のキー見出し {}", csv_table_span(report, left_header)?),
        format!("右のキー見出し {}", csv_table_span(report, right_header)?),
    ];
    for (index, (row, (left_row, right_row))) in joined
        .rows
        .iter()
        .zip(
            joined
                .left_source_record_numbers
                .iter()
                .zip(&joined.right_source_record_numbers),
        )
        .take(8)
        .enumerate()
    {
        let key = row.get(left_key_index).ok_or_else(invalid)?;
        if row.get(left_columns + right_key_index) != Some(key) {
            return Err(invalid());
        }
        let left = csv_table_find_span(
            report,
            "CSV_INNER_JOIN_LEFT_CELL",
            Some(*left_row),
            Some(left_key_index),
            Some(key),
        )?;
        let right = csv_table_find_span(
            report,
            "CSV_INNER_JOIN_RIGHT_CELL",
            Some(*right_row),
            Some(right_key_index),
            Some(key),
        )?;
        lines.push(format!(
            "出力行{} ← 左元行{} + 右元行{}。キー {:?}、左 {}、右 {}",
            index + 1,
            left_row,
            right_row,
            preview(key),
            csv_table_span(report, left)?,
            csv_table_span(report, right)?,
        ));
        let (left_values, right_values) = row.split_at_checked(left_columns).ok_or_else(invalid)?;
        for (side, kind, row_number, key_index, values) in [
            (
                "左",
                "CSV_INNER_JOIN_LEFT_CELL",
                *left_row,
                left_key_index,
                left_values,
            ),
            (
                "右",
                "CSV_INNER_JOIN_RIGHT_CELL",
                *right_row,
                right_key_index,
                right_values,
            ),
        ] {
            for (column, value) in values.iter().take(8).enumerate() {
                if column != key_index {
                    let span = csv_table_find_span(
                        report,
                        kind,
                        Some(row_number),
                        Some(column),
                        Some(value),
                    )?;
                    lines.push(format!(
                        "{side}元行{row_number} 列{} 値 {:?} {}",
                        column + 1,
                        preview(value),
                        csv_table_span(report, span)?,
                    ));
                }
            }
            if values.len() > 8 {
                lines.push(format!(
                    "{side}元行{row_number}の9列目以降も保存artifactに保持されています。"
                ));
            }
        }
    }
    if joined.rows.len() > 8 {
        lines.push(format!(
            "残りの一致行 {}件は保存artifactに保持されています。",
            joined.rows.len() - 8
        ));
    }
    for (side, kind, key_index, unmatched) in [
        (
            "左",
            "CSV_INNER_JOIN_UNMATCHED_LEFT_KEY",
            left_key_index,
            &joined.unmatched_left,
        ),
        (
            "右",
            "CSV_INNER_JOIN_UNMATCHED_RIGHT_KEY",
            right_key_index,
            &joined.unmatched_right,
        ),
    ] {
        for entry in unmatched.iter().take(8) {
            let span = csv_table_find_span(
                report,
                kind,
                Some(entry.source_record_number),
                Some(key_index),
                Some(&entry.key),
            )?;
            lines.push(format!(
                "{side}の未一致: 元行{} キー {:?} {}",
                entry.source_record_number,
                preview(&entry.key),
                csv_table_span(report, span)?,
            ));
        }
        if unmatched.len() > 8 {
            lines.push(format!(
                "{side}の残りの未一致行 {}件は保存artifactに保持されています。",
                unmatched.len() - 8
            ));
        }
    }
    Ok(lines)
}

fn csv_table_find_span<'a>(
    report: &'a DocumentResult,
    kind: &str,
    row: Option<usize>,
    column: Option<usize>,
    decoded_value: Option<&str>,
) -> Result<&'a DocumentSpan> {
    report
        .spans
        .iter()
        .find(|span| {
            span.kind == kind
                && row.is_none_or(|value| span.row == Some(value))
                && column.is_none_or(|value| span.column == Some(value))
                && decoded_value.is_none_or(|value| span.decoded_value.as_deref() == Some(value))
        })
        .ok_or_else(|| {
            failure(
                "CSV_TABLE_CONTEXT_INVALID",
                format!("a required {kind} source span is missing"),
            )
        })
}

fn csv_table_span(report: &DocumentResult, span: &DocumentSpan) -> Result<String> {
    let raw = report
        .source_document
        .get(span.source_start..span.source_end)
        .ok_or_else(|| {
            failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "a retained CSV source range is outside the sealed input",
            )
        })?;
    if raw != span.source_text {
        return Err(failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "a retained CSV source range differs from its original bytes",
        ));
    }
    let decoded = span.decoded_value.as_deref().ok_or_else(|| {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "a retained CSV cell has no decoded value",
        )
    })?;
    match (span.output_start, span.output_end) {
        (Some(start), Some(end)) => {
            let token = report.text.get(start..end).ok_or_else(|| {
                failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    "a retained CSV output range is outside the report",
                )
            })?;
            let output = serde_json::from_str::<String>(token).ok();
            let projected = output
                .as_deref()
                .and_then(|value| match span.kind.as_str() {
                    "CSV_INNER_JOIN_LEFT_HEADER" => value.strip_prefix("left."),
                    "CSV_INNER_JOIN_RIGHT_HEADER" => value.strip_prefix("right."),
                    _ => Some(value),
                });
            if projected != Some(decoded) {
                return Err(failure(
                    "CSV_TABLE_CONTEXT_INVALID",
                    "a retained CSV output token differs from its decoded source cell",
                ));
            }
        }
        (None, None) => {}
        _ => {
            return Err(failure(
                "CSV_TABLE_CONTEXT_INVALID",
                "a retained CSV output range is incomplete",
            ));
        }
    }
    Ok(format!(
        "byte[{}..{}) sha256={} 原文={:?}",
        span.source_start,
        span.source_end,
        digest(raw.as_bytes()),
        preview(raw)
    ))
}

fn csv_table_comparison_row_line(
    report: &DocumentResult,
    key_kind: &str,
    cell_kind: &str,
    label: &str,
    key: &str,
    row: &[String],
) -> Result<String> {
    let key_span = csv_table_find_span(report, key_kind, None, None, Some(key))?;
    let row_index = key_span.row.ok_or_else(|| {
        failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "an added or removed comparison key has no source row",
        )
    })?;
    if row.is_empty() {
        return Err(failure(
            "CSV_TABLE_CONTEXT_INVALID",
            "an added or removed comparison row has no cells",
        ));
    }
    let mut cells = Vec::new();
    for (column, value) in row.iter().enumerate().take(8) {
        let span = csv_table_find_span(
            report,
            cell_kind,
            Some(row_index),
            Some(column),
            Some(value),
        )?;
        cells.push(format!(
            "列{} {:?} {}",
            column + 1,
            preview(value),
            csv_table_span(report, span)?
        ));
    }
    if row.len() > 8 {
        cells.push(format!(
            "残り{}列はreport artifactに保持されています",
            row.len() - 8
        ));
    }
    Ok(format!(
        "{label} 元行{} キー {:?}: [{}]; キー位置 {}",
        row_index,
        preview(key),
        cells.join("; "),
        csv_table_span(report, key_span)?
    ))
}

/// Revalidate a prior JSON string-table grouped-statistics report against its
/// exact source document and explain bounded group results with original cell
/// ranges. The source and typed report are separate immutable artifacts.
pub(crate) fn previous_json_table_group_statistics_explanation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<(String, String, Vec<String>)>> {
    if previous.lifecycle != Lifecycle::Commit || previous.guarantee != Guarantee::Computed {
        return Ok(None);
    }
    let Ok(report) =
        serde_json::from_str::<crate::ports::JsonTableGroupStatisticsReport>(&previous.text)
    else {
        return Ok(None);
    };
    let source_ref = report.source_sha256.clone();
    if !previous.dependencies.contains(&source_ref) {
        // Earlier alpha 8 grouped-statistics results retained the report only.
        // Preserve their old display behavior because no source artifact is
        // available for an exact same-case recomputation.
        if previous.dependencies.len() == 1 {
            return Ok(None);
        }
        return Err(failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "the prior grouped-statistics result does not retain its source artifact",
        ));
    }
    let source_bytes = store
        .get_bytes_if_present(actor, case, &source_ref)?
        .ok_or_else(|| {
            failure(
                "JSON_TABLE_CONTEXT_INVALID",
                "the prior grouped-statistics source artifact is missing",
            )
        })?;
    let source_document = std::str::from_utf8(&source_bytes).map_err(|_| {
        failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "the prior grouped-statistics source is not valid UTF-8",
        )
    })?;
    let report_bytes = canonical(&report)?;
    let report_ref = digest(&report_bytes);
    let stored_report = store
        .get_bytes_if_present(actor, case, &report_ref)?
        .ok_or_else(|| {
            failure(
                "JSON_TABLE_CONTEXT_INVALID",
                "the prior grouped-statistics report artifact is missing",
            )
        })?;
    if source_ref == report_ref
        || previous.dependencies.len() != 2
        || !previous.dependencies.contains(&report_ref)
        || digest(&source_bytes) != source_ref
        || stored_report != report_bytes
        || previous.text != serde_json::to_string_pretty(&report)?
    {
        return Err(failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "the prior grouped-statistics source or report identity no longer matches",
        ));
    }
    let recomputed = crate::ports::json_table_group_statistics(
        source_document,
        &report.group_by,
        &report.value_column,
    )?;
    if canonical(&recomputed)? != report_bytes {
        return Err(failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "the saved JSON grouped-statistics report does not match exact recomputation",
        ));
    }

    let check_source_span = |span: &crate::ports::JsonComparisonSpan| -> Result<()> {
        let raw = source_document
            .get(span.source_start..span.source_end)
            .ok_or_else(|| {
                failure(
                    "JSON_TABLE_CONTEXT_INVALID",
                    "a retained JSON source-cell range is invalid",
                )
            })?;
        if digest(raw.as_bytes()) != span.source_sha256 {
            return Err(failure(
                "JSON_TABLE_CONTEXT_INVALID",
                "a retained JSON source-cell digest does not match its source bytes",
            ));
        }
        Ok(())
    };
    check_source_span(&report.group_by_source)?;
    check_source_span(&report.value_column_source)?;
    for group in &report.groups {
        for span in group
            .group_source_spans
            .iter()
            .chain(&group.value_source_spans)
        {
            check_source_span(span)?;
        }
    }

    let preview = |value: &str| {
        let mut chars = value.chars();
        let prefix: String = chars.by_ref().take(128).collect();
        if chars.next().is_some() {
            format!("{prefix}…")
        } else {
            prefix
        }
    };
    let mut lines = vec![
        "前回のJSON表グループ統計を元資料から再計算し、保存レポートと照合しました。".into(),
        format!(
            "グループ列: {}; 集計列: {}",
            report.group_by, report.value_column
        ),
        format!("元資料 SHA-256: {}", report.source_sha256),
        format!("集計artifact SHA-256: {report_ref}"),
        format!(
            "行数: {}; グループ数: {}",
            report.row_count,
            report.groups.len()
        ),
    ];
    for (group_index, group) in report.groups.iter().take(8).enumerate() {
        let statistics = &group.statistics;
        lines.push(format!(
            "グループ {:?}: n={}; 合計={}; 平均={}; 中央値={}; 最小={}; 最大={}; 母分散={}; 標本分散={}",
            preview(&group.value),
            statistics.count,
            preview(&statistics.sum),
            preview(&statistics.mean),
            preview(&statistics.median),
            preview(&statistics.min),
            preview(&statistics.max),
            preview(&statistics.population_variance),
            statistics
                .sample_variance
                .as_deref()
                .map_or_else(|| "未定義".into(), preview),
        ));
        if let Some(unit) = &statistics.unit {
            lines.push(format!("  単位: {}", preview(unit)));
        }
        for (row, span) in group
            .source_row_indices
            .iter()
            .zip(&group.value_source_spans)
            .take(8)
        {
            let raw = &source_document[span.source_start..span.source_end];
            lines.push(format!(
                "  元値 行{}・列{} byte[{}..{}): {}",
                row + 1,
                report.value_column_index + 1,
                span.source_start,
                span.source_end,
                preview(raw)
            ));
        }
        if group.source_row_indices.len() > 8 {
            lines.push(format!(
                "  残りの元値 {}件は保存artifactに保持されています。",
                group.source_row_indices.len() - 8
            ));
        }
        if group_index + 1 == 8 && report.groups.len() > 8 {
            lines.push(format!(
                "残りのグループ {}件は保存artifactに保持されています。",
                report.groups.len() - 8
            ));
        }
    }
    Ok(Some((source_ref, report_ref, lines)))
}

/// Recompute a prior JSON string-table operation from its immutable sealed
/// input, then explain the exact retained report and original byte ranges. The
/// source input bundle and the report are returned separately so Core and
/// local-file recovery can bind the same lineage.
pub(crate) fn previous_json_table_operation_explanation(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
    max_evidence_bytes: usize,
    explanation_parser_version: &str,
) -> Result<Option<(String, String, Vec<String>)>> {
    if previous.lifecycle != Lifecycle::Commit
        || !matches!(
            &previous.guarantee,
            Guarantee::Computed | Guarantee::SourceReport
        )
    {
        return Ok(None);
    }
    let value: serde_json::Value = match serde_json::from_str(&previous.text) {
        Ok(value) => value,
        Err(_) => return Ok(None),
    };
    let comparison_shape = value.as_object().is_some_and(|object| {
        object.len() == 7
            && [
                "source_sha256",
                "left_sha256",
                "right_sha256",
                "key_column",
                "equal",
                "changes",
                "guarantee",
            ]
            .iter()
            .all(|field| object.contains_key(*field))
            && value.get("source_sha256").and_then(serde_json::Value::as_str).is_some()
            && value.get("left_sha256").and_then(serde_json::Value::as_str).is_some()
            && value.get("right_sha256").and_then(serde_json::Value::as_str).is_some()
            && value.get("key_column").and_then(serde_json::Value::as_str).is_some()
            && value.get("equal").and_then(serde_json::Value::as_bool).is_some()
            && value.get("changes").and_then(serde_json::Value::as_array).is_some()
            && value.get("guarantee").and_then(serde_json::Value::as_str)
                == Some("bounded keyed comparison of explicitly string-typed tables; exact key/value matching, identical column order and unique keys required; source ranges retained; no semantic truth inferred")
    });
    let operation = if comparison_shape {
        "comparison"
    } else if value.get("left_key_column").is_some()
        && value.get("right_key_column").is_some()
        && value.get("left_source_row_indices").is_some()
    {
        "inner_join"
    } else if value.get("filter_column").is_some() {
        "filter"
    } else if value.get("sort_column").is_some() {
        "sort"
    } else if value.get("column_name").is_some() && value.get("source_spans").is_some() {
        "select"
    } else if value.get("selected_column_indices").is_some() {
        "project"
    } else if matches!(
        explanation_parser_version,
        crate::input::JSON_GROUP_COUNT_EXPLANATION_PARSER_VERSION
            | crate::input::CSV_SELECT_EXPLANATION_PARSER_VERSION
            | crate::input::TABLE_CONVERSION_EXPLANATION_PARSER_VERSION
            | crate::input::JSON_VALUE_EXPLANATION_PARSER_VERSION
            | crate::input::JSON_OBJECT_PROJECT_EXPLANATION_PARSER_VERSION
            | crate::input::JSON_STRUCTURAL_COMPARISON_EXPLANATION_PARSER_VERSION
            | crate::input::CSV_POSITIONAL_SELECTION_PARSER_VERSION
            | crate::input::TEXT_LOCATION_EXPLANATION_PARSER_VERSION
            | crate::input::TEXT_EDIT_EXPLANATION_PARSER_VERSION
            | crate::input::TEXT_COMPOSITION_EXPLANATION_PARSER_VERSION
            | crate::input::TEXT_SUMMARY_CANDIDATE_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_SUMMARY_EXPLANATION_PARSER_VERSION
            | crate::input::SOURCE_QUESTION_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_LISTING_EXPLANATION_PARSER_VERSION
            | crate::input::SEARCH_CANDIDATE_BODY_EXPLANATION_PARSER_VERSION
    ) && value.as_object().is_some_and(|object| {
        object.len() == 7
            && [
                "source_sha256",
                "group_by",
                "group_by_source",
                "group_by_index",
                "row_count",
                "groups",
                "guarantee",
            ]
            .iter()
            .all(|field| object.contains_key(*field))
    }) {
        "group_count"
    } else {
        return Ok(None);
    };
    if (operation == "inner_join" && previous.guarantee != Guarantee::SourceReport)
        || (operation != "inner_join" && previous.guarantee != Guarantee::Computed)
    {
        return Ok(None);
    }
    let (bundle_ref, bundle) = store
        .get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        .ok_or_else(|| {
            failure(
                "JSON_TABLE_CONTEXT_INVALID",
                "the prior table operation has no retained sealed input bundle",
            )
        })?;
    let applicable_intent = match operation {
        "comparison" => matches!(bundle.meaning.intent, Intent::JsonTableCompare { .. }),
        "group_count" => matches!(bundle.meaning.intent, Intent::JsonTableGroupCount { .. }),
        _ => matches!(
            bundle.meaning.intent,
            Intent::JsonTableCompare { .. }
                | Intent::JsonTableInnerJoin { .. }
                | Intent::JsonTableFilter { .. }
                | Intent::JsonTableSort { .. }
                | Intent::JsonTableSelect { .. }
                | Intent::JsonTableProject { .. }
        ),
    };
    if !applicable_intent {
        return Ok(None);
    }

    enum Report {
        Comparison(Box<crate::ports::JsonTableComparisonReport>),
        Filter(crate::ports::JsonTableFilterReport),
        Sort(crate::ports::JsonTableSortReport),
        Select(crate::ports::JsonTableSelectReport),
        Project(crate::ports::JsonTableProjectReport),
        Join(Box<crate::ports::JsonTableInnerJoinReport>),
        GroupCount(crate::ports::JsonTableGroupCountReport),
    }

    let report = match (operation, &bundle.meaning.intent) {
        (
            "comparison",
            Intent::JsonTableCompare {
                document,
                key_column,
            },
        ) => Report::Comparison(Box::new(crate::ports::json_table_compare(
            document, key_column,
        )?)),
        (
            "inner_join",
            Intent::JsonTableInnerJoin {
                document,
                left_key_column,
                right_key_column,
            },
        ) => Report::Join(Box::new(crate::ports::json_table_inner_join(
            document,
            left_key_column,
            right_key_column,
        )?)),
        (
            "filter",
            Intent::JsonTableFilter {
                document,
                column,
                equals,
            },
        ) => Report::Filter(crate::ports::json_table_filter(document, column, equals)?),
        (
            "sort",
            Intent::JsonTableSort {
                document,
                column,
                descending,
            },
        ) => Report::Sort(crate::ports::json_table_sort(
            document,
            column,
            *descending,
        )?),
        ("select", Intent::JsonTableSelect { document, column }) => {
            Report::Select(crate::ports::json_table_select(document, column)?)
        }
        ("project", Intent::JsonTableProject { document, columns }) => {
            Report::Project(crate::ports::json_table_project(document, columns)?)
        }
        ("group_count", Intent::JsonTableGroupCount { document, column }) => {
            Report::GroupCount(crate::ports::json_table_group_count(document, column)?)
        }
        _ => {
            return Err(failure(
                "JSON_TABLE_CONTEXT_INVALID",
                "the prior table report does not match its sealed input operation",
            ));
        }
    };

    let (report_bytes, report_text, source_document, mut lines) = match report {
        Report::Comparison(report) => {
            let bytes = canonical(&report)?;
            let text = serde_json::to_string_pretty(&report)?;
            let document = match &bundle.meaning.intent {
                Intent::JsonTableCompare { document, .. } => document.as_str(),
                _ => unreachable!("operation and sealed intent were checked"),
            };
            let cell_changes = report
                .changes
                .iter()
                .filter(|change| change.change == "CELL_CHANGED")
                .count();
            let removed_rows = report
                .changes
                .iter()
                .filter(|change| change.change == "ROW_REMOVED")
                .count();
            let added_rows = report
                .changes
                .iter()
                .filter(|change| change.change == "ROW_ADDED")
                .count();
            if cell_changes + removed_rows + added_rows != report.changes.len() {
                return Err(failure(
                    "JSON_TABLE_CONTEXT_INVALID",
                    "the recomputed JSON table comparison contains an unknown change kind",
                ));
            }
            let mut lines = vec![
                "前回のJSON表比較を、保存済みの元入力から再計算しました。".into(),
                format!(
                    "比較キー: {:?}; 結果: {} (厳密な文字列比較)",
                    report.key_column,
                    if report.equal {
                        "一致"
                    } else {
                        "差分あり"
                    }
                ),
                format!(
                    "差分: {}件 (セル変更: {}; 左側のみの行: {}; 右側のみの行: {})",
                    report.changes.len(),
                    cell_changes,
                    removed_rows,
                    added_rows
                ),
                "値の意味や現実の真偽は判定していません。".into(),
            ];
            for change in report.changes.iter().take(8) {
                match change.change.as_str() {
                    "CELL_CHANGED" => {
                        let column = change.column.as_deref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a changed comparison cell has no column name",
                            )
                        })?;
                        let left_value = change.left_value.as_deref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a changed comparison cell has no left value",
                            )
                        })?;
                        let right_value = change.right_value.as_deref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a changed comparison cell has no right value",
                            )
                        })?;
                        let left_pointer = change.left_pointer.as_deref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a changed comparison cell has no left pointer",
                            )
                        })?;
                        let right_pointer = change.right_pointer.as_deref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a changed comparison cell has no right pointer",
                            )
                        })?;
                        let left_span = change.left.as_ref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a changed comparison cell has no left source range",
                            )
                        })?;
                        let right_span = change.right.as_ref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a changed comparison cell has no right source range",
                            )
                        })?;
                        lines.push(format!(
                            "セル変更 キー {:?} / 列 {:?}: 左 {:?} {} {}; 右 {:?} {} {}",
                            preview(&change.key),
                            preview(column),
                            preview(left_value),
                            left_pointer,
                            table_span(document, left_span)?,
                            preview(right_value),
                            right_pointer,
                            table_span(document, right_span)?
                        ));
                    }
                    "ROW_REMOVED" => {
                        let pointer = change.left_pointer.as_deref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a removed comparison row has no left pointer",
                            )
                        })?;
                        let span = change.left.as_ref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "a removed comparison row has no left source range",
                            )
                        })?;
                        lines.push(format!(
                            "左側のみの行 キー {:?}: 左 {} {}",
                            preview(&change.key),
                            pointer,
                            table_span(document, span)?
                        ));
                    }
                    "ROW_ADDED" => {
                        let pointer = change.right_pointer.as_deref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "an added comparison row has no right pointer",
                            )
                        })?;
                        let span = change.right.as_ref().ok_or_else(|| {
                            failure(
                                "JSON_TABLE_CONTEXT_INVALID",
                                "an added comparison row has no right source range",
                            )
                        })?;
                        lines.push(format!(
                            "右側のみの行 キー {:?}: 右 {} {}",
                            preview(&change.key),
                            pointer,
                            table_span(document, span)?
                        ));
                    }
                    _ => {
                        return Err(failure(
                            "JSON_TABLE_CONTEXT_INVALID",
                            "the recomputed JSON table comparison contains an unknown change kind",
                        ));
                    }
                }
            }
            if report.changes.len() > 8 {
                lines.push(format!(
                    "残りの差分 {}件はreport artifactに保持されています。",
                    report.changes.len() - 8
                ));
            }
            (bytes, text, document, lines)
        }
        Report::Join(report) => {
            let bytes = canonical(&report)?;
            let text = serde_json::to_string_pretty(&report)?;
            let document = match &bundle.meaning.intent {
                Intent::JsonTableInnerJoin { document, .. } => document.as_str(),
                _ => unreachable!("operation and sealed intent were checked"),
            };
            let mut lines = vec![
                "前回のJSON表内部結合を、保存済みの元入力から再計算しました。".into(),
                format!(
                    "結合条件: 左列 {:?} = 右列 {:?}。一意な文字列キーの完全一致です。",
                    report.left_key_column, report.right_key_column
                ),
                format!(
                    "一致行: {}; 左の未一致行: {}; 右の未一致行: {}",
                    report.rows.len(),
                    report.unmatched_left.len(),
                    report.unmatched_right.len()
                ),
            ];
            for (index, row) in report.rows.iter().take(6).enumerate() {
                let left_spans = &report.left_cell_source_spans[index];
                let right_spans = &report.right_cell_source_spans[index];
                let left_values = row
                    .iter()
                    .take(left_spans.len())
                    .zip(left_spans)
                    .map(|(value, span)| {
                        Ok(format!(
                            "{:?} {}",
                            preview(value),
                            table_span(document, span)?
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                let right_values = row
                    .iter()
                    .skip(left_spans.len())
                    .zip(right_spans)
                    .map(|(value, span)| {
                        Ok(format!(
                            "{:?} {}",
                            preview(value),
                            table_span(document, span)?
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                lines.push(format!(
                    "出力行{} ← 左元行{} + 右元行{}: 左 [{}]; 右 [{}]",
                    index + 1,
                    report.left_source_row_indices[index] + 1,
                    report.right_source_row_indices[index] + 1,
                    left_values.join("; "),
                    right_values.join("; ")
                ));
            }
            if report.rows.len() > 6 {
                lines.push(format!(
                    "残りの一致行 {}件はreport artifactに保持されています。",
                    report.rows.len() - 6
                ));
            }
            for unmatched in report.unmatched_left.iter().take(4) {
                lines.push(format!(
                    "左の未一致: 元行{} キー {:?}; {}",
                    unmatched.source_row_index + 1,
                    preview(&unmatched.key),
                    table_span(document, &unmatched.key_source)?
                ));
            }
            if report.unmatched_left.len() > 4 {
                lines.push(format!(
                    "残りの左未一致キー {}件はreport artifactに保持されています。",
                    report.unmatched_left.len() - 4
                ));
            }
            for unmatched in report.unmatched_right.iter().take(4) {
                lines.push(format!(
                    "右の未一致: 元行{} キー {:?}; {}",
                    unmatched.source_row_index + 1,
                    preview(&unmatched.key),
                    table_span(document, &unmatched.key_source)?
                ));
            }
            if report.unmatched_right.len() > 4 {
                lines.push(format!(
                    "残りの右未一致キー {}件はreport artifactに保持されています。",
                    report.unmatched_right.len() - 4
                ));
            }
            (bytes, text, document, lines)
        }
        Report::Filter(report) => {
            let bytes = canonical(&report)?;
            let text = serde_json::to_string_pretty(&report)?;
            let document = match &bundle.meaning.intent {
                Intent::JsonTableFilter { document, .. } => document.as_str(),
                _ => unreachable!("operation and sealed intent were checked"),
            };
            let matched = report
                .decisions
                .iter()
                .filter(|decision| decision.matched)
                .count();
            let mut lines = vec![
                "前回のJSON表フィルターを、保存済みの元入力から再計算しました。".into(),
                format!(
                    "条件: 列 {:?} が {:?} と完全一致",
                    report.filter_column, report.equals
                ),
                format!("一致行: {} / {}", matched, report.decisions.len()),
            ];
            for decision in report.decisions.iter().take(8) {
                lines.push(format!(
                    "元行{} {} {:?} / 値 {:?} / {}",
                    decision.source_row_index + 1,
                    table_span(document, &decision.source)?,
                    report.filter_column,
                    preview(&decision.value),
                    if decision.matched { "採用" } else { "除外" }
                ));
            }
            if report.decisions.len() > 8 {
                lines.push(format!(
                    "残りの判定 {}件はreport artifactに保持されています。",
                    report.decisions.len() - 8
                ));
            }
            for (output_index, (source_row, row)) in report
                .source_row_indices
                .iter()
                .zip(&report.rows)
                .take(4)
                .enumerate()
            {
                let cell = &report.cell_source_spans[output_index][report.filter_column_index];
                lines.push(format!(
                    "出力行{} ← 元行{}: {:?}; 条件セル {}",
                    output_index + 1,
                    source_row + 1,
                    row.iter().map(|value| preview(value)).collect::<Vec<_>>(),
                    table_span(document, cell)?
                ));
            }
            if report.rows.len() > 4 {
                lines.push(format!(
                    "残りの出力行 {}件はreport artifactに保持されています。",
                    report.rows.len() - 4
                ));
            }
            (bytes, text, document, lines)
        }
        Report::Sort(report) => {
            let bytes = canonical(&report)?;
            let text = serde_json::to_string_pretty(&report)?;
            let document = match &bundle.meaning.intent {
                Intent::JsonTableSort { document, .. } => document.as_str(),
                _ => unreachable!("operation and sealed intent were checked"),
            };
            let mut lines = vec![
                "前回のJSON表整列を、保存済みの元入力から再計算しました。".into(),
                format!(
                    "整列列: {:?}; 順序: {}",
                    report.sort_column, report.sort_order
                ),
                format!("出力行数: {}", report.rows.len()),
            ];
            for (index, (source_row, row)) in report
                .source_row_indices
                .iter()
                .zip(&report.rows)
                .take(8)
                .enumerate()
            {
                let key = &row[report.sort_column_index];
                let span = &report.cell_source_spans[index][report.sort_column_index];
                lines.push(format!(
                    "出力行{} ← 元行{}: 整列値 {:?}; {}",
                    index + 1,
                    source_row + 1,
                    preview(key),
                    table_span(document, span)?
                ));
            }
            if report.rows.len() > 8 {
                lines.push(format!(
                    "残りの整列行 {}件はreport artifactに保持されています。",
                    report.rows.len() - 8
                ));
            }
            (bytes, text, document, lines)
        }
        Report::Select(report) => {
            let bytes = canonical(&report)?;
            let text = serde_json::to_string_pretty(&report)?;
            let document = match &bundle.meaning.intent {
                Intent::JsonTableSelect { document, .. } => document.as_str(),
                _ => unreachable!("operation and sealed intent were checked"),
            };
            let mut lines = vec![
                "前回のJSON表列選択を、保存済みの元入力から再計算しました。".into(),
                format!(
                    "選択列: {:?} (元列番号 {})",
                    report.column_name,
                    report.column_index + 1
                ),
                format!("選択値数: {}", report.values.len()),
                format!("列見出し {}", table_span(document, &report.column_source)?),
            ];
            for ((source_row, value), span) in report
                .source_row_indices
                .iter()
                .zip(&report.values)
                .zip(&report.source_spans)
                .take(8)
            {
                lines.push(format!(
                    "元行{}: {:?}; {}",
                    source_row + 1,
                    preview(value),
                    table_span(document, span)?
                ));
            }
            if report.values.len() > 8 {
                lines.push(format!(
                    "残りの選択値 {}件はreport artifactに保持されています。",
                    report.values.len() - 8
                ));
            }
            (bytes, text, document, lines)
        }
        Report::Project(report) => {
            let bytes = canonical(&report)?;
            let text = serde_json::to_string_pretty(&report)?;
            let document = match &bundle.meaning.intent {
                Intent::JsonTableProject { document, .. } => document.as_str(),
                _ => unreachable!("operation and sealed intent were checked"),
            };
            let mut lines = vec![
                "前回のJSON表投影を、保存済みの元入力から再計算しました。".into(),
                format!("投影列の順序: {:?}", report.columns),
                format!("投影行数: {}", report.rows.len()),
            ];
            for (index, (source_row, row)) in report
                .source_row_indices
                .iter()
                .zip(&report.rows)
                .take(8)
                .enumerate()
            {
                let mapped = row
                    .iter()
                    .zip(&report.cell_source_spans[index])
                    .take(8)
                    .map(|(value, span)| {
                        Ok(format!(
                            "{:?} {}",
                            preview(value),
                            table_span(document, span)?
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                lines.push(format!("元行{} → [{}]", source_row + 1, mapped.join("; ")));
            }
            if report.rows.len() > 8 {
                lines.push(format!(
                    "残りの投影行 {}件はreport artifactに保持されています。",
                    report.rows.len() - 8
                ));
            }
            (bytes, text, document, lines)
        }
        Report::GroupCount(report) => {
            let Intent::JsonTableGroupCount { document, .. } = &bundle.meaning.intent else {
                unreachable!("operation and sealed intent were checked");
            };
            (
                canonical(&report)?,
                serde_json::to_string_pretty(&report)?,
                document.as_str(),
                json_table_group_count_lines(document, &report)?,
            )
        }
    };

    let report_ref = digest(&report_bytes);
    let stored_report = store
        .get_bytes_if_present(actor, case, &report_ref)?
        .ok_or_else(|| {
            failure(
                "JSON_TABLE_CONTEXT_INVALID",
                "the prior JSON table report artifact is missing",
            )
        })?;
    if previous.dependencies != [report_ref.clone()]
        || stored_report != report_bytes
        || previous.text != report_text
    {
        return Err(failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "the prior JSON table input, report artifact or prior result text no longer matches",
        ));
    }
    let (Some(prior_plan), Some(prior_render)) =
        (previous.content_plan.as_ref(), previous.render.as_ref())
    else {
        return Err(failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "the prior JSON table result has no retained content plan or render observation",
        ));
    };
    let mut unsigned_plan = prior_plan.clone();
    unsigned_plan.id.clear();
    if prior_plan.id != minidora_domain::identity(&unsigned_plan)?
        || prior_plan.text != previous.text
        || prior_plan.guarantee != previous.guarantee
        || !prior_plan.dependencies.contains(&report_ref)
        || !crate::core::validate_render(prior_plan, prior_render)
    {
        return Err(failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "the prior JSON table content plan or actual rendered output is invalid",
        ));
    }
    let prior_plan_bytes = canonical(prior_plan)?;
    let prior_render_bytes = canonical(prior_render)?;
    for (reference, expected, label) in [
        (digest(&prior_plan_bytes), prior_plan_bytes, "content plan"),
        (
            digest(&prior_render_bytes),
            prior_render_bytes,
            "render observation",
        ),
    ] {
        match store.get_bytes_if_present(actor, case, &reference)? {
            Some(stored) if stored == expected => {}
            Some(_) => {
                return Err(failure(
                    "JSON_TABLE_CONTEXT_INVALID",
                    format!("the prior JSON table {label} artifact changed"),
                ));
            }
            None => {
                return Err(failure(
                    "JSON_TABLE_CONTEXT_INVALID",
                    format!("the prior JSON table {label} artifact is missing"),
                ));
            }
        }
    }
    lines.splice(
        1..1,
        [
            format!("元資料 SHA-256: {}", digest(source_document.as_bytes())),
            format!("封印済みInputBundle SHA-256: {bundle_ref}"),
            format!("report artifact SHA-256: {report_ref}"),
        ],
    );
    if lines.join("\n").len() > max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "the exact JSON table explanation exceeds the request evidence budget",
        ));
    }
    Ok(Some((bundle_ref, report_ref, lines)))
}

fn json_table_group_count_lines(
    document: &str,
    report: &crate::ports::JsonTableGroupCountReport,
) -> Result<Vec<String>> {
    const PREVIEW_GROUPS: usize = 8;
    const PREVIEW_MEMBERS: usize = 4;
    let column_index = report.group_by_index;
    let mut lines = vec![
        "前回のJSON表グループ件数を、封印済みの元入力から再計算しました。".into(),
        format!(
            "グループ列: {:?}。JSON文字列をデコードし、文字が完全一致する行をまとめて初出順に示します。",
            preview(&report.group_by)
        ),
        format!(
            "行数: {}、グループ数: {}",
            report.row_count,
            report.groups.len()
        ),
        format!(
            "列見出し JSON Pointer /columns/{column_index} {}",
            table_span(document, &report.group_by_source)?
        ),
    ];
    for (index, group) in report.groups.iter().take(PREVIEW_GROUPS).enumerate() {
        lines.push(format!(
            "グループ{} {:?}。件数: {}",
            index + 1,
            preview(&group.value),
            group.count
        ));
        for (source_row, span) in group
            .source_row_indices
            .iter()
            .zip(&group.member_source_spans)
            .take(PREVIEW_MEMBERS)
        {
            lines.push(format!(
                "元行{} JSON Pointer /rows/{source_row}/{column_index} {}",
                source_row + 1,
                table_span(document, span)?
            ));
        }
        if group.count > PREVIEW_MEMBERS {
            lines.push(format!(
                "このグループの残り {}行は保存レポートに保持されています。",
                group.count - PREVIEW_MEMBERS
            ));
        }
    }
    if report.groups.len() > PREVIEW_GROUPS {
        lines.push(format!(
            "残り {}グループは保存レポートに保持されています。",
            report.groups.len() - PREVIEW_GROUPS
        ));
    }
    Ok(lines)
}

fn preview(value: &str) -> String {
    let mut chars = value.chars();
    let prefix: String = chars.by_ref().take(128).collect();
    if chars.next().is_some() {
        format!("{prefix}…")
    } else {
        prefix
    }
}

fn table_span(document: &str, span: &crate::ports::JsonComparisonSpan) -> Result<String> {
    let raw = document
        .get(span.source_start..span.source_end)
        .ok_or_else(|| {
            failure(
                "JSON_TABLE_CONTEXT_INVALID",
                "a retained table source range is outside the exact sealed input",
            )
        })?;
    if digest(raw.as_bytes()) != span.source_sha256 {
        return Err(failure(
            "JSON_TABLE_CONTEXT_INVALID",
            "a retained table source range does not match its source digest",
        ));
    }
    Ok(format!(
        "byte[{}..{})={:?} sha256={}",
        span.source_start,
        span.source_end,
        preview(raw),
        span.source_sha256
    ))
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GroupMeanComparison {
    pub group: String,
    pub mean: String,
    pub highest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GroupMeanWinner {
    pub group: String,
    pub mean: String,
    pub source_row_count: usize,
    pub sampled_source_row_indices: Vec<usize>,
    pub sampled_group_source_spans: Vec<crate::ports::JsonComparisonSpan>,
    pub sampled_value_source_spans: Vec<crate::ports::JsonComparisonSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GroupMeanAnswer {
    pub source_sha256: String,
    pub group_by: String,
    pub value_column: String,
    pub comparison_unit: Option<String>,
    pub comparisons: Vec<GroupMeanComparison>,
    pub winners: Vec<GroupMeanWinner>,
    pub guarantee: String,
}

pub(crate) fn group_mean_graph_ids(csv: bool) -> [&'static str; 3] {
    if csv {
        [
            "previous-csv-group-statistics-input",
            "previous-csv-group-statistics-report",
            "previous-csv-group-mean-answer",
        ]
    } else {
        [
            "previous-json-group-statistics-source",
            "previous-json-group-statistics-report",
            "previous-json-group-mean-answer",
        ]
    }
}

pub(crate) fn previous_group_mean(
    store: &crate::store::Store,
    followup: &InputBundle,
    previous: &ResultRecord,
    deadline: Option<Instant>,
) -> Result<Option<(String, String, GroupMeanAnswer)>> {
    let request = &followup.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    if followup.meaning.parser_version != crate::input::PREVIOUS_CSV_GROUP_MEAN_PARSER_VERSION {
        return previous_json_table_highest_group_mean(store, actor, case, previous);
    }
    const INVALID: &str = "CSV_GROUP_MEAN_CONTEXT_INVALID";
    let invalid = || failure(INVALID, "CSVの元入力・集計記録・表示結果を照合できません。");
    let evidence_limit = || {
        failure(
            "RESOURCE_LIMIT",
            "CSVグループ平均の比較根拠が要求の証拠予算を超えました。",
        )
    };
    let check_time = || {
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return Err(failure(
                "DEADLINE",
                "CSVグループ統計の照合中に締切へ達しました。",
            ));
        }
        Ok(())
    };
    if !matches!(followup.meaning.intent, Intent::AskPreviousGroupMean)
        || followup.meaning.context_result_ref.as_ref() != Some(&previous.id)
        || followup.context_sha256 != identity(&Some(previous))?
        || request
            .context_ref
            .as_ref()
            .is_some_and(|id| id != &previous.id)
        || previous.lifecycle != Lifecycle::Commit
        || previous.guarantee != Guarantee::Computed
        || previous.calculation.is_some()
        || !previous.evidence.is_empty()
    {
        return Err(invalid());
    }
    let [report_ref] = previous.dependencies.as_slice() else {
        return Err(invalid());
    };
    check_time()?;
    let (source_ref, source) = store.with_verified_case_history(actor, case, |history| {
        if history
            .input_result(&previous.input_id, previous.input_version)?
            .as_ref()
            != Some(previous)
        {
            return Err(invalid());
        }
        history
            .input_bundle(&previous.input_id, previous.input_version)?
            .ok_or_else(invalid)
    })?;
    if identity(&source.request)? != previous.id {
        return Err(invalid());
    }
    let Intent::CsvGroupStatistics {
        document,
        group_column,
        value_column,
    } = &source.meaning.intent
    else {
        return Err(invalid());
    };
    let report_bytes = store.get_bytes(actor, case, report_ref)?;
    let proof_bytes = document.len().saturating_add(report_bytes.len());
    if proof_bytes > request.budget.max_evidence_bytes {
        return Err(evidence_limit());
    }
    check_time()?;
    let report = CsvInput::conversation(document).group_statistics(group_column, value_column)?;
    if previous.text != report.text || canonical(&report)? != report_bytes {
        return Err(invalid());
    }
    revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(report_ref),
        INVALID,
        "CSV group statistics",
    )?;
    let answer = csv_highest_group_mean(&report, group_column, value_column)?;
    if proof_bytes.saturating_add(canonical(&answer)?.len()) > request.budget.max_evidence_bytes {
        return Err(evidence_limit());
    }
    check_time()?;
    Ok(Some((source_ref, report_ref.clone(), answer)))
}

#[derive(Serialize, Deserialize)]
struct CsvGroupStatisticsGroup {
    value: String,
    source_record_numbers: Vec<usize>,
    statistics: crate::math::Statistics,
}

fn csv_highest_group_mean(
    report: &DocumentResult,
    group_column: &str,
    value_column: &str,
) -> Result<GroupMeanAnswer> {
    #[derive(Deserialize)]
    struct Groups {
        groups: Vec<CsvGroupStatisticsGroup>,
    }
    let groups: Groups = serde_json::from_str(&report.text)?;
    let rows: Vec<_> = groups
        .groups
        .iter()
        .map(|group| (group.value.as_str(), &group.statistics))
        .collect();
    let (comparison_unit, comparisons, winner_indices) = compare_group_means(&rows, true)?;
    let invalid = || {
        failure(
            "CSV_GROUP_MEAN_CONTEXT_INVALID",
            "CSVの元セルの位置を照合できません。",
        )
    };
    let mut spans = BTreeMap::new();
    for span in &report.spans {
        let column = match span.kind.as_str() {
            "CSV_GROUP_MEMBER" => 0,
            "CSV_GROUP_STATISTIC_VALUE" => 1,
            _ => continue,
        };
        let raw = report
            .source_document
            .get(span.source_start..span.source_end)
            .filter(|raw| *raw == span.source_text)
            .ok_or_else(invalid)?;
        if spans
            .insert(
                (span.row.ok_or_else(invalid)?, column),
                crate::ports::JsonComparisonSpan {
                    source_start: span.source_start,
                    source_end: span.source_end,
                    source_sha256: digest(raw.as_bytes()),
                },
            )
            .is_some()
        {
            return Err(invalid());
        }
    }
    let mut winners = Vec::with_capacity(winner_indices.len());
    for index in winner_indices {
        let group = &groups.groups[index];
        let mut winner = GroupMeanWinner {
            group: group.value.clone(),
            mean: group.statistics.mean.clone(),
            source_row_count: group.source_record_numbers.len(),
            sampled_source_row_indices: Vec::new(),
            sampled_group_source_spans: Vec::new(),
            sampled_value_source_spans: Vec::new(),
        };
        for &record in group.source_record_numbers.iter().take(8) {
            winner
                .sampled_source_row_indices
                .push(record.checked_sub(1).ok_or_else(invalid)?);
            winner
                .sampled_group_source_spans
                .push(spans.remove(&(record, 0)).ok_or_else(invalid)?);
            winner
                .sampled_value_source_spans
                .push(spans.remove(&(record, 1)).ok_or_else(invalid)?);
        }
        winners.push(winner);
    }
    Ok(GroupMeanAnswer {
        source_sha256: report.source_sha256.clone(),
        group_by: group_column.into(),
        value_column: value_column.into(),
        comparison_unit,
        comparisons,
        winners,
        guarantee: "貼り付けたCSVの行から計算した平均の比較に限ります。母集団の傾向や外部の事実は確認していません".into(),
    })
}

/// Revalidate a previous grouped-statistics result, then answer the bounded
/// question using its exact group means. Different units are refused until a
/// cross-group conversion is explicitly implemented and verified.
pub(crate) fn previous_json_table_highest_group_mean(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<(String, String, GroupMeanAnswer)>> {
    let Some((source_ref, report_ref, _)) =
        previous_json_table_group_statistics_explanation(store, actor, case, previous)?
    else {
        return Ok(None);
    };
    let report: crate::ports::JsonTableGroupStatisticsReport = serde_json::from_str(&previous.text)
        .map_err(|_| {
            failure(
                "JSON_GROUP_MEAN_CONTEXT_INVALID",
                "the prior grouped-statistics report cannot be decoded",
            )
        })?;
    Ok(Some((
        source_ref,
        report_ref,
        json_table_highest_group_mean(&report)?,
    )))
}

pub(crate) fn json_table_highest_group_mean(
    report: &crate::ports::JsonTableGroupStatisticsReport,
) -> Result<GroupMeanAnswer> {
    let groups: Vec<_> = report
        .groups
        .iter()
        .map(|group| (group.value.as_str(), &group.statistics))
        .collect();
    let (comparison_unit, comparisons, winner_indices) = compare_group_means(&groups, false)?;
    let winners = winner_indices
        .iter()
        .map(|index| {
            let group = &report.groups[*index];
            let sample_count = group.source_row_indices.len().min(8);
            GroupMeanWinner {
                group: group.value.clone(),
                mean: group.statistics.mean.clone(),
                source_row_count: group.source_row_indices.len(),
                sampled_source_row_indices: group
                    .source_row_indices
                    .iter()
                    .take(sample_count)
                    .copied()
                    .collect(),
                sampled_group_source_spans: group
                    .group_source_spans
                    .iter()
                    .take(sample_count)
                    .cloned()
                    .collect(),
                sampled_value_source_spans: group
                    .value_source_spans
                    .iter()
                    .take(sample_count)
                    .cloned()
                    .collect(),
            }
        })
        .collect();
    Ok(GroupMeanAnswer {
        source_sha256: report.source_sha256.clone(),
        group_by: report.group_by.clone(),
        value_column: report.value_column.clone(),
        comparison_unit,
        comparisons,
        winners,
        guarantee: "exact comparison of the revalidated per-group means from this supplied JSON string table; no population representativeness or external-world claim".into(),
    })
}

fn compare_group_means(
    groups: &[(&str, &crate::math::Statistics)],
    csv: bool,
) -> Result<(Option<String>, Vec<GroupMeanComparison>, Vec<usize>)> {
    let (_, first) = groups.first().ok_or_else(|| {
        failure(
            if csv {
                "CSV_GROUP_MEAN_CONTEXT_INVALID"
            } else {
                "JSON_GROUP_MEAN_CONTEXT_INVALID"
            },
            "the validated grouped-statistics report has no groups",
        )
    })?;
    let comparison_unit = first.unit.clone();
    if groups
        .iter()
        .any(|(_, statistics)| statistics.unit != comparison_unit)
    {
        return Err(failure(
            if csv {
                "CSV_GROUP_MEAN_UNIT_MISMATCH"
            } else {
                "JSON_GROUP_MEAN_UNIT_MISMATCH"
            },
            "group means use different or mixed units; no cross-group conversion was applied",
        ));
    }
    let means: Vec<_> = groups
        .iter()
        .map(|(_, statistics)| statistics.mean.clone())
        .collect();
    let winners = crate::math::maximum_exact_indices(&means)?;
    let comparisons = groups
        .iter()
        .enumerate()
        .map(|(index, (group, statistics))| GroupMeanComparison {
            group: (*group).into(),
            mean: statistics.mean.clone(),
            highest: winners.binary_search(&index).is_ok(),
        })
        .collect();
    Ok((comparison_unit, comparisons, winners))
}

pub(crate) fn group_mean_answer_lines(answer: &GroupMeanAnswer, csv: bool) -> Vec<String> {
    let unit = answer
        .comparison_unit
        .as_deref()
        .map_or_else(String::new, |unit| format!(" {unit}"));
    let mut lines = vec![
        if csv {
            "前回のCSVグループ統計を再検証し、全グループの平均を正確に比較しました。"
        } else {
            "前回のJSON表グループ統計を再検証し、全グループの平均を正確に比較しました。"
        }
        .into(),
        format!(
            "集計対象: {}列ごとの{}列の平均。元資料 SHA-256: {}",
            answer.group_by, answer.value_column, answer.source_sha256
        ),
    ];
    if answer.winners.len() == 1 {
        let winner = &answer.winners[0];
        lines.push(format!(
            "平均が最も高いグループ: {:?}（平均={}{}）",
            winner.group, winner.mean, unit
        ));
    } else {
        lines.push(format!(
            "平均が最も高いグループは同率で{}件です。いずれか一つには絞れません。",
            answer.winners.len()
        ));
        for winner in &answer.winners {
            lines.push(format!(
                "同率グループ: {:?}（平均={}{}）",
                winner.group, winner.mean, unit
            ));
        }
    }
    lines.push("比較内訳（グループの初出順）:".into());
    for comparison in &answer.comparisons {
        lines.push(format!(
            "{:?}: 平均={}{}{}",
            comparison.group,
            comparison.mean,
            unit,
            if comparison.highest {
                "（最高）"
            } else {
                ""
            }
        ));
    }
    for winner in &answer.winners {
        lines.push(format!(
            "根拠セル: グループ {:?} の元値{}件。",
            winner.group, winner.source_row_count
        ));
        for ((row, group_span), value_span) in winner
            .sampled_source_row_indices
            .iter()
            .zip(&winner.sampled_group_source_spans)
            .zip(&winner.sampled_value_source_spans)
        {
            lines.push(format!(
                "  データ行{}: グループbyte[{}..{}), 値byte[{}..{})",
                row + 1,
                group_span.source_start,
                group_span.source_end,
                value_span.source_start,
                value_span.source_end
            ));
        }
        if winner.source_row_count > winner.sampled_source_row_indices.len() {
            lines.push(format!(
                "  残り{}行の元セル範囲は{}に保持されています。",
                winner.source_row_count - winner.sampled_source_row_indices.len(),
                if csv {
                    "元のCSV統計記録"
                } else {
                    "回答artifactと元資料"
                },
            ));
        }
    }
    lines.push(format!("保証範囲: {}。", answer.guarantee));
    lines
}

fn admission(document: &str) -> Result<()> {
    if document.is_empty() || document.len() > MAX_DOCUMENT_BYTES {
        return Err(failure(
            "DOCUMENT_INPUT_LIMIT",
            "expected 1..256000 source bytes",
        ));
    }
    Ok(())
}
fn result(
    document: &str,
    text: String,
    operation: &str,
    parameters: BTreeMap<String, String>,
    spans: Vec<DocumentSpan>,
) -> Result<DocumentResult> {
    if text.len() > MAX_RESULT_BYTES {
        return Err(failure(
            "DOCUMENT_OUTPUT_LIMIT",
            "transformation exceeds 256000 output bytes",
        ));
    }
    for span in &spans {
        if document.get(span.source_start..span.source_end) != Some(span.source_text.as_str()) {
            return Err(failure(
                "DOCUMENT_SOURCE_SPAN",
                "source span does not match exact original bytes",
            ));
        }
        match (span.output_start, span.output_end) {
            (Some(start), Some(end)) if text.get(start..end).is_some() => {}
            (None, None) => {}
            _ => return Err(failure("DOCUMENT_OUTPUT_SPAN", "invalid output span")),
        }
    }
    Ok(DocumentResult { output_sha256:digest(text.as_bytes()), text, source_document:document.into(), source_sha256:digest(document.as_bytes()), operation:operation.into(), parameters, spans, guarantee:"explicit syntactic transformation with original bytes and source spans; no inferred numeric type, factual truth or arbitrary writing-quality claim".into() })
}
fn input_byte(bytes: &[u8], at: usize) -> Option<u8> {
    bytes.get(at).copied()
}
fn csv_character(byte: u8) -> bool {
    byte >= 32 && byte != 127
}
fn cell(input: CsvInput<'_>, at: &mut usize) -> Result<CsvCell> {
    let bytes = input.document.as_bytes();
    let start = *at;
    let mut value = Vec::new();
    if input_byte(bytes, *at) == Some(b'"') {
        *at += 1;
        loop {
            match input_byte(bytes, *at) {
                None => return Err(failure("CSV_QUOTES", "unterminated quoted field")),
                Some(b'"') => {
                    *at += 1;
                    if input_byte(bytes, *at) == Some(b'"') {
                        value.push(b'"');
                        *at += 1;
                    } else {
                        break;
                    }
                }
                Some(byte) if csv_character(byte) || matches!(byte, b'\r' | b'\n') => {
                    value.push(byte);
                    *at += 1;
                }
                Some(_) => {
                    return Err(failure(
                        "CSV_CHARACTER",
                        "unsupported control character in quoted field",
                    ));
                }
            }
        }
        let suffix = input_byte(bytes, *at);
        if !matches!(suffix, None | Some(b',' | b'\r'))
            && !(input.allow_lf && suffix == Some(b'\n'))
        {
            return Err(failure(
                "CSV_QUOTES",
                if input.allow_lf {
                    "only a comma, record separator or EOF may follow a closing quote"
                } else {
                    "only a comma, CRLF or EOF may follow a closing quote"
                },
            ));
        }
    } else {
        while let Some(byte) = input_byte(bytes, *at) {
            if matches!(byte, b',' | b'\r' | b'\n') {
                break;
            }
            if byte == b'"' || !csv_character(byte) {
                return Err(failure(
                    "CSV_CHARACTER",
                    "unquoted quotes and unsupported controls are invalid",
                ));
            }
            value.push(byte);
            *at += 1;
        }
    }
    let value = String::from_utf8(value)
        .map_err(|_| failure("CSV_UTF8", "field decoding did not preserve UTF-8"))?;
    Ok(CsvCell {
        value,
        source_start: start,
        source_end: *at,
    })
}
fn validate_table(header: Option<&[CsvCell]>, rows: &[Vec<CsvCell>]) -> Result<()> {
    let columns = header.map_or_else(|| rows.first().map_or(0, Vec::len), <[CsvCell]>::len);
    if !(1..=MAX_COLUMNS).contains(&columns)
        || rows.len() > MAX_ROWS
        || rows.iter().any(|row| row.len() != columns)
    {
        return Err(failure(
            "CSV_SHAPE",
            "expected consistent rows with 1..256 columns and at most 4096 data rows",
        ));
    }
    let cells = rows.iter().map(Vec::len).sum::<usize>() + header.map_or(0, <[CsvCell]>::len);
    if cells > MAX_CELLS {
        return Err(failure("CSV_CELL_LIMIT", "table exceeds 65536 cells"));
    }
    if let Some(header) = header {
        let unique: BTreeSet<_> = header.iter().map(|cell| cell.value.as_str()).collect();
        if unique.len() != header.len() {
            return Err(failure(
                "DUPLICATE_COLUMNS",
                "decoded column names must be unique",
            ));
        }
    }
    Ok(())
}

/// Parse all CSV bytes; CRLF separates records, and quote escapes/embedded line
/// breaks retain exact source offsets. Header presence is explicit, never guessed.
pub fn parse_csv(document: &str, has_header: bool) -> Result<CsvTable> {
    parse_csv_input(CsvInput::strict(document), has_header)
}

pub(crate) fn parse_attached_csv(document: &str) -> Result<CsvTable> {
    parse_csv_input(CsvInput::conversation(document), true)
}

fn parse_csv_input(input: CsvInput<'_>, has_header: bool) -> Result<CsvTable> {
    parse_csv_section(input, has_header, None).map(|(table, _)| table)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum JapaneseCsvPairLabel {
    Left,
    Right,
}

#[derive(Clone, Copy)]
enum JapaneseCsvPairFormat {
    BeforeAfter,
    LeftRight,
}

impl JapaneseCsvPairFormat {
    fn labels(self) -> (&'static str, &'static str) {
        match self {
            Self::BeforeAfter => ("変更前", "変更後"),
            Self::LeftRight => ("左の表", "右の表"),
        }
    }

    fn invalid(self) -> minidora_domain::Error {
        match self {
            Self::BeforeAfter => failure(
                "CSV_COMPARE_DELIMITER",
                "変更前: と変更後: の見出しをこの順に各1行置き、それぞれにCSV表を続けてください。",
            ),
            Self::LeftRight => failure(
                "CSV_INNER_JOIN_DELIMITER",
                "左の表: と右の表: の見出しをこの順に各1行置き、それぞれにCSV表を続けてください。",
            ),
        }
    }
}

fn japanese_csv_pair_label(
    line: &str,
    format: JapaneseCsvPairFormat,
) -> Option<(JapaneseCsvPairLabel, usize)> {
    let (left, right) = format.labels();
    let (label, rest) = if let Some(rest) = line.strip_prefix(left) {
        (JapaneseCsvPairLabel::Left, rest)
    } else {
        (JapaneseCsvPairLabel::Right, line.strip_prefix(right)?)
    };
    let rest = rest.strip_prefix(':').or_else(|| rest.strip_prefix('：'))?;
    let remaining = if rest.is_empty() {
        ""
    } else {
        rest.strip_prefix("\r\n")
            .or_else(|| rest.strip_prefix('\n'))?
    };
    Some((label, line.len() - remaining.len()))
}

pub(crate) fn starts_japanese_csv_comparison(document: &str) -> bool {
    matches!(
        japanese_csv_pair_label(document, JapaneseCsvPairFormat::BeforeAfter),
        Some((JapaneseCsvPairLabel::Left, _))
    )
}

pub(crate) fn starts_japanese_csv_inner_join(document: &str) -> bool {
    matches!(
        japanese_csv_pair_label(document, JapaneseCsvPairFormat::LeftRight),
        Some((JapaneseCsvPairLabel::Left, _))
    )
}

fn parse_csv_section(
    input: CsvInput<'_>,
    has_header: bool,
    section: Option<(JapaneseCsvPairFormat, JapaneseCsvPairLabel)>,
) -> Result<(CsvTable, Option<usize>)> {
    let document = input.document;
    admission(document)?;
    let bytes = document.as_bytes();
    let mut at = 0usize;
    let mut rows = Vec::new();
    let mut cells = 0usize;
    let mut next_section = None;
    loop {
        // A quoted cell is consumed in full by `cell`, so this check only
        // sees logical record boundaries.
        if let Some((format, section)) = section
            && let Some((label, length)) = japanese_csv_pair_label(&document[at..], format)
        {
            if section != JapaneseCsvPairLabel::Left
                || label != JapaneseCsvPairLabel::Right
                || rows.is_empty()
                || at + length == bytes.len()
            {
                return Err(format.invalid());
            }
            next_section = Some(at + length);
            break;
        }
        let mut row = Vec::new();
        loop {
            row.push(cell(input, &mut at)?);
            cells += 1;
            if row.len() > MAX_COLUMNS || cells > MAX_CELLS {
                return Err(failure(
                    "CSV_CELL_LIMIT",
                    "table column/cell limit exceeded",
                ));
            }
            match input_byte(bytes, at) {
                Some(b',') => {
                    at += 1;
                }
                Some(b'\r') => {
                    if input_byte(bytes, at + 1) != Some(b'\n') {
                        return Err(failure("CSV_LINE_ENDING", "record separator must be CRLF"));
                    }
                    at += 2;
                    break;
                }
                Some(b'\n') if input.allow_lf => {
                    at += 1;
                    break;
                }
                Some(b'\n') => {
                    return Err(failure(
                        "CSV_LINE_ENDING",
                        "bare LF is not a CRLF record separator",
                    ));
                }
                None => break,
                _ => return Err(failure("CSV_SYNTAX", "unparsed field suffix")),
            }
        }
        rows.push(row);
        if rows.len() > MAX_ROWS + usize::from(has_header) {
            return Err(failure("CSV_ROW_LIMIT", "table row limit exceeded"));
        }
        if at == bytes.len() {
            break;
        }
    }
    let header = if has_header {
        Some(rows.remove(0))
    } else {
        None
    };
    validate_table(header.as_deref(), &rows)?;
    let section_source = &document[..at];
    Ok((
        CsvTable {
            source_document: section_source.into(),
            source_sha256: digest(section_source.as_bytes()),
            header,
            rows,
        },
        next_section,
    ))
}
fn source_span(
    document: &str,
    cell: &CsvCell,
    kind: &str,
    row: Option<usize>,
    column: usize,
    output: Option<(usize, usize)>,
) -> DocumentSpan {
    DocumentSpan {
        kind: kind.into(),
        source_start: cell.source_start,
        source_end: cell.source_end,
        source_text: document[cell.source_start..cell.source_end].into(),
        decoded_value: Some(cell.value.clone()),
        row,
        column: Some(column),
        output_start: output.map(|v| v.0),
        output_end: output.map(|v| v.1),
    }
}
fn append_json_cell(
    text: &mut String,
    document: &str,
    cell: &CsvCell,
    kind: &str,
    row: Option<usize>,
    column: usize,
    spans: &mut Vec<DocumentSpan>,
) -> Result<()> {
    let start = text.len();
    text.push_str(&serde_json::to_string(&cell.value)?);
    if text.len() > MAX_RESULT_BYTES {
        return Err(failure(
            "DOCUMENT_OUTPUT_LIMIT",
            "JSON transformation exceeds output limit",
        ));
    }
    spans.push(source_span(
        document,
        cell,
        kind,
        row,
        column,
        Some((start, text.len())),
    ));
    Ok(())
}
fn select(table: CsvTable, index: usize, column_name: Option<&str>) -> Result<DocumentResult> {
    let width = table
        .header
        .as_ref()
        .map_or_else(|| table.rows.first().map_or(0, Vec::len), Vec::len);
    if index >= width {
        return Err(failure("CSV_COLUMN", "column index is out of range"));
    }
    let mut text = String::from("[");
    let mut spans = Vec::new();
    if let Some(header) = &table.header {
        spans.push(source_span(
            &table.source_document,
            &header[index],
            "CSV_HEADER",
            Some(0),
            index,
            None,
        ));
    }
    for (row_index, row) in table.rows.iter().enumerate() {
        if row_index > 0 {
            text.push(',');
        }
        append_json_cell(
            &mut text,
            &table.source_document,
            &row[index],
            "CSV_SELECTED_CELL",
            Some(row_index + usize::from(table.header.is_some())),
            index,
            &mut spans,
        )?;
    }
    text.push(']');
    let mut parameters = BTreeMap::from([
        ("column_index".into(), index.to_string()),
        ("has_header".into(), table.header.is_some().to_string()),
        ("value_type".into(), "STRING".into()),
    ]);
    if let Some(name) = column_name {
        parameters.insert("column_name".into(), name.into());
    }
    result(
        &table.source_document,
        text,
        "CSV_SELECT",
        parameters,
        spans,
    )
}
/// Select an exact decoded named header. A numeric name is still a name; use
/// csv_select_index for an explicit headerless/indexed selection.
pub fn csv_select(document: &str, column: &str) -> Result<DocumentResult> {
    CsvInput::strict(document).select(column)
}

fn csv_select_input(input: CsvInput<'_>, column: &str) -> Result<DocumentResult> {
    let table = parse_csv_input(input, true)?;
    let index = table
        .header
        .as_ref()
        .expect("explicit header")
        .iter()
        .position(|cell| cell.value == column)
        .ok_or_else(|| failure("CSV_COLUMN", "named column does not exist"))?;
    select(table, index, Some(column))
}

/// Compute exact descriptive statistics for one explicitly named CSV column.
/// A column is treated either as unitless exact expressions or as one exact
/// decimal literal plus a unit in every row; mixed/ambiguous interpretation
/// stops before producing a result.
pub fn csv_statistics(document: &str, column: &str) -> Result<DocumentResult> {
    CsvInput::strict(document).statistics(column)
}

fn csv_statistics_input(input: CsvInput<'_>, column: &str) -> Result<DocumentResult> {
    let document = input.document;
    let table = parse_csv_input(input, true)?;
    let header = table.header.as_ref().expect("explicit CSV header");
    let index = header
        .iter()
        .position(|cell| cell.value == column)
        .ok_or_else(|| failure("CSV_COLUMN", "named column does not exist"))?;
    let mut spans = vec![source_span(
        document,
        &header[index],
        "CSV_HEADER",
        Some(0),
        index,
        None,
    )];
    spans.extend(table.rows.iter().enumerate().map(|(row_index, row)| {
        source_span(
            document,
            &row[index],
            "CSV_NUMERIC_OBSERVATION",
            Some(row_index + 1),
            index,
            None,
        )
    }));
    csv_statistics_from_spans(document, column, index, spans, "CSV_STATISTICS")
        .map(|(report, _)| report)
}

fn csv_statistics_from_spans(
    document: &str,
    column: &str,
    index: usize,
    spans: Vec<DocumentSpan>,
    operation: &str,
) -> Result<(DocumentResult, crate::math::Statistics)> {
    let mut values = Vec::with_capacity(spans.len().saturating_sub(1));
    let mut units = Vec::with_capacity(values.capacity());
    let mut quantity_count = 0;
    for span in spans.iter().skip(1) {
        let value = span.decoded_value.as_deref().ok_or_else(|| {
            failure(
                "CSV_STATISTICS_SOURCE_CELL",
                "a selected numeric cell has no decoded value",
            )
        })?;
        if let Some((number, unit)) = crate::language::decimal_quantity_parts(value) {
            quantity_count += 1;
            values.push(number.to_string());
            units.push(unit.to_string());
        } else {
            values.push(value.into());
            units.push(String::new());
        }
    }
    let statistics = match quantity_count {
        0 => crate::math::summarize(&values, None)?,
        count if count == values.len() => crate::math::summarize(&values, Some(&units))?,
        _ => {
            return Err(failure(
                "CSV_STATISTICS_MIXED_VALUES",
                "selected column mixes unit-bearing numeric literals with unitless or nonnumeric cells",
            ));
        }
    };
    #[derive(Serialize)]
    struct Report<'a> {
        source_sha256: String,
        column_name: &'a str,
        column_index: usize,
        statistics: &'a crate::math::Statistics,
        source_spans: &'a [DocumentSpan],
        guarantee: &'static str,
    }
    let text = serde_json::to_string_pretty(&Report {
        source_sha256: digest(document.as_bytes()),
        column_name: column,
        column_index: index,
        statistics: &statistics,
        source_spans: &spans,
        guarantee: "exact descriptive statistics of explicitly selected supplied cells; source locations retained; no population inference",
    })?;
    let parameters = BTreeMap::from([
        ("has_header".into(), "true".into()),
        ("column_name".into(), column.into()),
        ("column_index".into(), index.to_string()),
        ("observation_count".into(), statistics.count.to_string()),
        (
            "interpretation".into(),
            if quantity_count == values.len() && quantity_count > 0 {
                "exact_numeric_literal_with_unit"
            } else {
                "exact_unitless_expression"
            }
            .into(),
        ),
    ]);
    Ok((
        result(document, text, operation, parameters, spans)?,
        statistics,
    ))
}

pub(crate) struct FilteredCsvStatistics {
    pub(crate) report_bytes: Vec<u8>,
    pub(crate) dependencies: Vec<String>,
    pub(crate) units: Vec<String>,
}

pub(crate) struct FilteredCsvChoices {
    pub(crate) row_count: usize,
    pub(crate) columns: Vec<String>,
    pub(crate) dependencies: Vec<String>,
}

struct FilteredCsvSource {
    filter: DocumentResult,
    filter_column: String,
    equals: String,
    dependencies: Vec<String>,
    source_bytes: usize,
}

const FILTERED_CSV_STATISTICS_CONTEXT_INVALID: &str = "FILTERED_CSV_STATISTICS_CONTEXT_INVALID";

fn filtered_csv_context_invalid() -> minidora_domain::Error {
    failure(
        FILTERED_CSV_STATISTICS_CONTEXT_INVALID,
        "絞り込んだCSVを元の入力・抽出記録・表示結果と照合できません。",
    )
}

fn filtered_csv_statistics_limit() -> minidora_domain::Error {
    failure(
        "RESOURCE_LIMIT",
        "元CSVと集計結果が要求の証拠予算を超えています。",
    )
}

fn check_filtered_csv_deadline(budget: &ResourceBudget, started: Instant) -> Result<()> {
    if started.elapsed().as_millis() >= u128::from(budget.deadline_ms) {
        return Err(failure(
            "DEADLINE",
            "CSVの抽出記録と集計の照合中に締切へ達しました。",
        ));
    }
    Ok(())
}

/// Reproduce a registered filter result, then use only the original source
/// cells that it emitted. No CSV export or re-encoding is involved.
pub(crate) fn previous_filtered_csv_statistics(
    store: &crate::store::Store,
    bundle: &InputBundle,
    prior: &ResultRecord,
    started: Instant,
) -> Result<FilteredCsvStatistics> {
    let Intent::PreviousFilteredCsvStatistics { column } = &bundle.meaning.intent else {
        return Err(filtered_csv_context_invalid());
    };
    if !crate::input::verified_previous_filtered_csv_statistics(bundle)? {
        return Err(filtered_csv_context_invalid());
    }
    filtered_csv_statistics_with_column(store, bundle, prior, &column.text, started)
}

fn verified_filtered_csv_source(
    store: &crate::store::Store,
    bundle: &InputBundle,
    prior: &ResultRecord,
    started: Instant,
) -> Result<FilteredCsvSource> {
    let invalid = filtered_csv_context_invalid;
    let request = &bundle.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    if prior.lifecycle != Lifecycle::Commit
        || bundle.meaning.context_result_ref.as_ref() != Some(&prior.id)
        || bundle.context_sha256 != identity(&Some(prior))?
        || request
            .context_ref
            .as_ref()
            .is_some_and(|id| id != &prior.id)
    {
        return Err(invalid());
    }
    if request.budget.max_actions < 6 + u32::from(request.output_path.is_some()) {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "元CSVの抽出条件の照合と集計に必要な行動予算が不足しています。",
        ));
    }
    check_filtered_csv_deadline(&request.budget, started)?;
    let (bundle_ref, original) = store.with_verified_case_history(actor, case, |history| {
        if history
            .input_result(&prior.input_id, prior.input_version)?
            .as_ref()
            != Some(prior)
        {
            return Err(invalid());
        }
        history
            .input_bundle(&prior.input_id, prior.input_version)?
            .ok_or_else(invalid)
    })?;
    let Intent::CsvFilter {
        document,
        column: filter_column,
        equals,
    } = &original.meaning.intent
    else {
        return Err(failure(
            "MISSING_FILTERED_CSV_CONTEXT",
            "同じ会話で確定したCSVの行選択結果を指定してください。",
        ));
    };
    let [filter_ref] = prior.dependencies.as_slice() else {
        return Err(invalid());
    };
    if original.request.actor_id != *actor
        || original.request.case_id != *case
        || identity(&original.request)? != prior.id
        || prior.guarantee != Guarantee::Computed
        || prior.calculation.is_some()
        || !prior.evidence.is_empty()
    {
        return Err(invalid());
    }
    let stored = store.get_bytes(actor, case, filter_ref)?;
    let source_bytes = document.len().saturating_add(stored.len());
    if source_bytes > request.budget.max_evidence_bytes {
        return Err(filtered_csv_statistics_limit());
    }
    let filter = CsvInput::conversation(document).filter(filter_column, equals)?;
    if prior.text != filter.text
        || canonical(&filter)? != stored
        || prior
            .content_plan
            .as_ref()
            .is_none_or(|plan| plan.required_units != [filter.text.as_str()])
    {
        return Err(invalid());
    }
    revalidate_previous_document_render(
        store,
        actor,
        case,
        prior,
        Some(filter_ref),
        FILTERED_CSV_STATISTICS_CONTEXT_INVALID,
        "CSV filter",
    )?;
    check_filtered_csv_deadline(&request.budget, started)?;
    Ok(FilteredCsvSource {
        filter,
        filter_column: filter_column.clone(),
        equals: equals.clone(),
        dependencies: vec![prior.id.clone(), bundle_ref, filter_ref.clone()],
        source_bytes,
    })
}

pub(crate) fn filtered_csv_statistics_with_column(
    store: &crate::store::Store,
    bundle: &InputBundle,
    prior: &ResultRecord,
    column: &str,
    started: Instant,
) -> Result<FilteredCsvStatistics> {
    let source = verified_filtered_csv_source(store, bundle, prior, started)?;
    let header = source
        .filter
        .spans
        .iter()
        .find(|span| {
            span.kind == "CSV_FILTER_HEADER" && span.decoded_value.as_deref() == Some(column)
        })
        .ok_or_else(|| failure("CSV_COLUMN", "絞り込んだ表に指定された列がありません。"))?;
    let cells = source
        .filter
        .spans
        .iter()
        .filter(|span| span.kind == "CSV_FILTER_CELL" && span.column == header.column);
    filtered_csv_statistics_report(&source, header, cells, &bundle.request.budget, started)
}

/// List only columns whose selected original cells admit the same exact
/// statistics operation as an explicitly named column.
pub(crate) fn filtered_csv_statistics_choices(
    store: &crate::store::Store,
    bundle: &InputBundle,
    prior: &ResultRecord,
    started: Instant,
) -> Result<FilteredCsvChoices> {
    let source = verified_filtered_csv_source(store, bundle, prior, started)?;
    let budget = &bundle.request.budget;
    let row_count: usize = source
        .filter
        .parameters
        .get("matched_rows")
        .and_then(|count| count.parse().ok())
        .ok_or_else(filtered_csv_context_invalid)?;
    let mut choices = FilteredCsvChoices {
        row_count,
        columns: Vec::new(),
        dependencies: source.dependencies.clone(),
    };
    if row_count == 0 {
        return Ok(choices);
    }
    let headers: Vec<_> = source
        .filter
        .spans
        .iter()
        .filter(|span| span.kind == "CSV_FILTER_HEADER")
        .collect();
    if headers.len()
        > budget
            .max_actions
            .saturating_sub(5 + u32::from(bundle.request.output_path.is_some())) as usize
    {
        return Err(failure(
            "ACTION_BUDGET_EXHAUSTED",
            "絞り込んだ表の候補列を照合するための行動予算が不足しています。",
        ));
    }
    let mut cells_by_column = BTreeMap::<usize, Vec<&DocumentSpan>>::new();
    for cell in source
        .filter
        .spans
        .iter()
        .filter(|span| span.kind == "CSV_FILTER_CELL")
    {
        cells_by_column
            .entry(cell.column.ok_or_else(filtered_csv_context_invalid)?)
            .or_default()
            .push(cell);
    }
    let mut evidence_bytes = source.source_bytes;
    for header in headers {
        let cells = cells_by_column
            .remove(&header.column.ok_or_else(filtered_csv_context_invalid)?)
            .filter(|cells| cells.len() == row_count)
            .ok_or_else(filtered_csv_context_invalid)?;
        let column = header
            .decoded_value
            .as_ref()
            .ok_or_else(filtered_csv_context_invalid)?;
        match filtered_csv_statistics_report(&source, header, cells, budget, started) {
            Ok(_) => {
                evidence_bytes = evidence_bytes.saturating_add(column.len());
                if evidence_bytes > budget.max_evidence_bytes {
                    return Err(filtered_csv_statistics_limit());
                }
                choices.columns.push(column.clone());
            }
            Err(minidora_domain::Error::Contract { code, .. })
                if matches!(
                    code.as_str(),
                    "INVALID_EXPRESSION"
                        | "INVALID_EXACT_NUMBER"
                        | "DIVISION_BY_ZERO"
                        | "INTEGER_REQUIRED"
                        | "UNDEFINED_POWER"
                        | "UNIT_OPERATION_UNSUPPORTED"
                        | "UNIT_MISMATCH"
                        | "STATISTICS_UNIT"
                        | "CSV_STATISTICS_MIXED_VALUES"
                        | "MATH_INPUT_LIMIT"
                        | "MATH_NUMBER_LIMIT"
                        | "MATH_OPERATION_LIMIT"
                        | "EXPRESSION_LIMIT"
                        | "EXPRESSION_DEPTH"
                        | "NUMBER_LIMIT"
                        | "EXPONENT_LIMIT"
                        | "ARITHMETIC_LIMIT"
                ) => {}
            Err(error) => return Err(error),
        }
    }
    check_filtered_csv_deadline(budget, started)?;
    Ok(choices)
}

fn filtered_csv_statistics_report<'a>(
    source: &FilteredCsvSource,
    header: &DocumentSpan,
    cells: impl IntoIterator<Item = &'a DocumentSpan>,
    budget: &ResourceBudget,
    started: Instant,
) -> Result<FilteredCsvStatistics> {
    check_filtered_csv_deadline(budget, started)?;
    let invalid = filtered_csv_context_invalid;
    let column = header.decoded_value.as_deref().ok_or_else(invalid)?;
    let mut header = header.clone();
    let index = header.column.ok_or_else(invalid)?;
    header.kind = "CSV_HEADER".into();
    header.output_start = None;
    header.output_end = None;
    let mut spans = vec![header];
    for cell in cells {
        let mut span = cell.clone();
        // Output offsets referred to the filter's rendered JSON, not the new
        // report. Retain original cell bytes and the original row ordinal.
        span.kind = "CSV_NUMERIC_OBSERVATION".into();
        span.output_start = None;
        span.output_end = None;
        spans.push(span);
    }
    if spans.len() == 1 {
        return Err(failure(
            "CSV_FILTERED_STATISTICS_EMPTY",
            "絞り込み結果に集計できる行がありません。",
        ));
    }
    let (report, stats) = csv_statistics_from_spans(
        &source.filter.source_document,
        column,
        index,
        spans,
        "CSV_FILTERED_STATISTICS",
    )?;
    let suffix = |unit: Option<&str>| unit.map_or_else(String::new, |unit| format!(" {unit}"));
    let unit = suffix(stats.unit.as_deref());
    let variance_unit = suffix(stats.variance_unit.as_deref());
    let mut units = vec![
        format!("前回絞り込んだ行の列 {column:?} の統計量です。"),
        format!(
            "抽出条件: 列 {:?} が {:?}",
            source.filter_column, source.equals
        ),
        format!("件数: {}", stats.count),
        format!("平均: {}{unit}", stats.mean),
        format!("合計: {}{unit} / 中央値: {}{unit}", stats.sum, stats.median),
        format!("最小値: {}{unit} / 最大値: {}{unit}", stats.min, stats.max),
        format!("母分散: {}{variance_unit}", stats.population_variance),
        stats.sample_variance.as_deref().map_or_else(
            || "標本分散: 算出対象外（1件）".into(),
            |value| format!("標本分散: {value}{variance_unit}"),
        ),
        format!("元CSV SHA-256: {}", report.source_sha256),
    ];
    for span in report.spans.iter().skip(1).take(8) {
        units.push(format!(
            "元CSVのデータ行{} byte[{}..{})",
            span.row.ok_or_else(invalid)?,
            span.source_start,
            span.source_end
        ));
    }
    if stats.count > 8 {
        units.push(format!(
            "残り{}件のセル範囲は集計記録に保持しました。",
            stats.count - 8
        ));
    }
    let report_bytes = canonical(&report)?;
    if source
        .source_bytes
        .saturating_add(report_bytes.len())
        .saturating_add(units.join("\n").len())
        > budget.max_evidence_bytes
    {
        return Err(filtered_csv_statistics_limit());
    }
    check_filtered_csv_deadline(budget, started)?;
    let report_ref = digest(&report_bytes);
    let mut dependencies = source.dependencies.clone();
    dependencies.push(report_ref);
    Ok(FilteredCsvStatistics {
        report_bytes,
        dependencies,
        units,
    })
}

pub fn csv_select_index(document: &str, index: usize, has_header: bool) -> Result<DocumentResult> {
    CsvInput::strict(document).select_index(index, has_header)
}

/// Export an explicitly string-typed table JSON. Numeric-looking cells remain
/// JSON strings, including leading zeros, exponent spelling and huge integers.
pub fn csv_to_json(document: &str, has_header: bool) -> Result<DocumentResult> {
    CsvInput::strict(document).to_json(has_header)
}

fn csv_to_json_input(input: CsvInput<'_>, has_header: bool) -> Result<DocumentResult> {
    let document = input.document;
    let table = parse_csv_input(input, has_header)?;
    let mut text = String::from("{\"columns\":");
    let mut spans = Vec::new();
    if let Some(header) = &table.header {
        text.push('[');
        for (index, cell) in header.iter().enumerate() {
            if index > 0 {
                text.push(',');
            }
            append_json_cell(
                &mut text,
                document,
                cell,
                "CSV_HEADER",
                Some(0),
                index,
                &mut spans,
            )?;
        }
        text.push(']');
    } else {
        text.push_str("null");
    }
    text.push_str(",\"rows\":[");
    for (row_index, row) in table.rows.iter().enumerate() {
        if row_index > 0 {
            text.push(',');
        }
        text.push('[');
        for (column, cell) in row.iter().enumerate() {
            if column > 0 {
                text.push(',');
            }
            append_json_cell(
                &mut text,
                document,
                cell,
                "CSV_CELL",
                Some(row_index + usize::from(has_header)),
                column,
                &mut spans,
            )?;
        }
        text.push(']');
    }
    text.push_str("]}");
    result(
        document,
        text,
        "CSV_TO_JSON",
        BTreeMap::from([
            ("has_header".into(), has_header.to_string()),
            ("value_type".into(), "STRING".into()),
        ]),
        spans,
    )
}

fn named_rows_as_json(
    document: &str,
    header: &[CsvCell],
    rows: &[(usize, &Vec<CsvCell>)],
    operation: &str,
    parameters: BTreeMap<String, String>,
    mut spans: Vec<DocumentSpan>,
) -> Result<DocumentResult> {
    let mut text = String::from("{\"columns\":[");
    let header_kind = format!("{operation}_HEADER");
    let cell_kind = format!("{operation}_CELL");
    for (index, cell) in header.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        append_json_cell(
            &mut text,
            document,
            cell,
            &header_kind,
            Some(0),
            index,
            &mut spans,
        )?;
    }
    text.push_str("],\"rows\":[");
    for (output_row, (source_row, row)) in rows.iter().enumerate() {
        if output_row > 0 {
            text.push(',');
        }
        text.push('[');
        for (index, cell) in row.iter().enumerate() {
            if index > 0 {
                text.push(',');
            }
            append_json_cell(
                &mut text,
                document,
                cell,
                &cell_kind,
                Some(source_row + 1),
                index,
                &mut spans,
            )?;
        }
        text.push(']');
    }
    text.push_str("]}");
    result(document, text, operation, parameters, spans)
}

/// Select rows whose explicitly named CSV cell exactly matches a supplied
/// string. Output remains a string-typed JSON table and every emitted cell
/// retains its original source byte range. Predicate spans also include
/// rejected rows so the exact filter decision remains inspectable.
pub fn csv_filter(document: &str, column: &str, equals: &str) -> Result<DocumentResult> {
    CsvInput::strict(document).filter(column, equals)
}

fn csv_filter_input(input: CsvInput<'_>, column: &str, equals: &str) -> Result<DocumentResult> {
    let document = input.document;
    let table = parse_csv_input(input, true)?;
    let header = table.header.as_ref().expect("explicit CSV header");
    let column_index = header
        .iter()
        .position(|cell| cell.value == column)
        .ok_or_else(|| failure("CSV_COLUMN", "named column does not exist"))?;
    let rows: Vec<_> = table
        .rows
        .iter()
        .enumerate()
        .filter(|(_, row)| row[column_index].value == equals)
        .collect();
    let mut spans = Vec::with_capacity(table.rows.len());
    for (row_index, row) in table.rows.iter().enumerate() {
        spans.push(source_span(
            document,
            &row[column_index],
            "CSV_FILTER_DECISION",
            Some(row_index + 1),
            column_index,
            None,
        ));
    }
    named_rows_as_json(
        document,
        header,
        &rows,
        "CSV_FILTER",
        BTreeMap::from([
            ("column_name".into(), column.into()),
            ("column_index".into(), column_index.to_string()),
            ("equals".into(), equals.into()),
            ("matched_rows".into(), rows.len().to_string()),
            ("value_type".into(), "STRING".into()),
        ]),
        spans,
    )
}

/// Stably sort a named CSV column by exact decoded string order, without
/// locale collation, case folding or numeric interpretation.
pub fn csv_sort(document: &str, column: &str, descending: bool) -> Result<DocumentResult> {
    CsvInput::strict(document).sort(column, descending)
}

fn csv_sort_input(input: CsvInput<'_>, column: &str, descending: bool) -> Result<DocumentResult> {
    let document = input.document;
    let table = parse_csv_input(input, true)?;
    let header = table.header.as_ref().expect("explicit CSV header");
    let column_index = header
        .iter()
        .position(|cell| cell.value == column)
        .ok_or_else(|| failure("CSV_COLUMN", "named column does not exist"))?;
    let mut rows: Vec<_> = table.rows.iter().enumerate().collect();
    rows.sort_by(|(_, left), (_, right)| {
        if descending {
            right[column_index].value.cmp(&left[column_index].value)
        } else {
            left[column_index].value.cmp(&right[column_index].value)
        }
    });
    let row_count = rows.len();
    named_rows_as_json(
        document,
        header,
        &rows,
        "CSV_SORT",
        BTreeMap::from([
            ("column_name".into(), column.into()),
            ("column_index".into(), column_index.to_string()),
            (
                "sort_order".into(),
                if descending {
                    "descending"
                } else {
                    "ascending"
                }
                .into(),
            ),
            ("row_count".into(), row_count.to_string()),
            ("value_type".into(), "STRING".into()),
        ]),
        Vec::new(),
    )
}

/// Count exact decoded string values in one named CSV column. Groups retain
/// first-seen order, while every member cell remains in the source map.
pub fn csv_group_count(document: &str, column: &str) -> Result<DocumentResult> {
    CsvInput::strict(document).group_count(column)
}

fn csv_group_count_input(input: CsvInput<'_>, column: &str) -> Result<DocumentResult> {
    let document = input.document;
    let table = parse_csv_input(input, true)?;
    let header = table.header.as_ref().expect("explicit CSV header");
    let column_index = header
        .iter()
        .position(|cell| cell.value == column)
        .ok_or_else(|| failure("CSV_COLUMN", "named column does not exist"))?;

    let mut indices = BTreeMap::<String, usize>::new();
    let mut groups = Vec::<Vec<(usize, &CsvCell)>>::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        let cell = &row[column_index];
        let group_index = if let Some(index) = indices.get(&cell.value) {
            *index
        } else {
            let index = groups.len();
            indices.insert(cell.value.clone(), index);
            groups.push(Vec::new());
            index
        };
        groups[group_index].push((row_index, cell));
    }

    let mut text = String::from("{\"group_by\":");
    let header_start = text.len();
    text.push_str(&serde_json::to_string(&header[column_index].value)?);
    let mut spans = vec![source_span(
        document,
        &header[column_index],
        "CSV_GROUP_COLUMN",
        Some(0),
        column_index,
        Some((header_start, text.len())),
    )];
    text.push_str(",\"groups\":[");
    for (group_index, members) in groups.iter().enumerate() {
        if group_index > 0 {
            text.push(',');
        }
        text.push_str("{\"value\":");
        for (row_index, cell) in members {
            spans.push(source_span(
                document,
                cell,
                "CSV_GROUP_MEMBER",
                Some(row_index + 1),
                column_index,
                None,
            ));
        }
        let (first_row, representative) = members.first().expect("group has one member");
        append_json_cell(
            &mut text,
            document,
            representative,
            "CSV_GROUP_VALUE",
            Some(first_row + 1),
            column_index,
            &mut spans,
        )?;
        text.push_str(",\"count\":");
        text.push_str(&members.len().to_string());
        text.push('}');
    }
    text.push_str("]}");
    result(
        document,
        text,
        "CSV_GROUP_COUNT",
        BTreeMap::from([
            ("column_name".into(), column.into()),
            ("column_index".into(), column_index.to_string()),
            ("group_count".into(), groups.len().to_string()),
            ("row_count".into(), table.rows.len().to_string()),
            ("group_order".into(), "first_seen".into()),
        ]),
        spans,
    )
}

/// Compute exact descriptive statistics for a numeric CSV column within each
/// exact decoded string group. Groups retain first-seen order and both group
/// keys and measured cells retain their original source locations.
pub fn csv_group_statistics(
    document: &str,
    group_column: &str,
    value_column: &str,
) -> Result<DocumentResult> {
    CsvInput::strict(document).group_statistics(group_column, value_column)
}

fn csv_group_statistics_input(
    input: CsvInput<'_>,
    group_column: &str,
    value_column: &str,
) -> Result<DocumentResult> {
    let document = input.document;
    let table = parse_csv_input(input, true)?;
    if table.rows.is_empty() {
        return Err(failure(
            "CSV_GROUP_STATISTICS_EMPTY",
            "grouped statistics require at least one data row",
        ));
    }
    let header = table.header.as_ref().expect("explicit CSV header");
    let group_index = header
        .iter()
        .position(|cell| cell.value == group_column)
        .ok_or_else(|| failure("CSV_COLUMN", "group column does not exist"))?;
    let value_index = header
        .iter()
        .position(|cell| cell.value == value_column)
        .ok_or_else(|| failure("CSV_COLUMN", "value column does not exist"))?;
    if group_index == value_index {
        return Err(failure(
            "CSV_GROUP_STATISTICS_COLUMNS",
            "group and measured-value columns must be distinct",
        ));
    }

    let mut group_indices = BTreeMap::<String, usize>::new();
    let mut grouped = Vec::<(String, Vec<(usize, &CsvCell, &CsvCell)>)>::new();
    for (row_index, row) in table.rows.iter().enumerate() {
        let key = row[group_index].value.clone();
        let group_index_in_result = if let Some(index) = group_indices.get(&key) {
            *index
        } else {
            let index = grouped.len();
            group_indices.insert(key.clone(), index);
            grouped.push((key, Vec::new()));
            index
        };
        grouped[group_index_in_result]
            .1
            .push((row_index, &row[group_index], &row[value_index]));
    }

    #[derive(Serialize)]
    struct Report<'a> {
        source_sha256: String,
        group_column: &'a str,
        value_column: &'a str,
        groups: &'a [CsvGroupStatisticsGroup],
        source_spans: &'a [DocumentSpan],
        guarantee: &'static str,
    }

    let mut spans = vec![
        source_span(
            document,
            &header[group_index],
            "CSV_GROUP_COLUMN_HEADER",
            Some(0),
            group_index,
            None,
        ),
        source_span(
            document,
            &header[value_index],
            "CSV_GROUP_VALUE_HEADER",
            Some(0),
            value_index,
            None,
        ),
    ];
    let mut reports = Vec::with_capacity(grouped.len());
    for (group_value, members) in &grouped {
        let mut values = Vec::with_capacity(members.len());
        let mut units = Vec::with_capacity(members.len());
        let mut quantity_count = 0usize;
        let mut source_record_numbers = Vec::with_capacity(members.len());
        for (row_index, group_cell, value_cell) in members {
            source_record_numbers.push(row_index + 1);
            spans.push(source_span(
                document,
                group_cell,
                "CSV_GROUP_MEMBER",
                Some(row_index + 1),
                group_index,
                None,
            ));
            if let Some((number, unit)) = crate::language::decimal_quantity_parts(&value_cell.value)
            {
                quantity_count += 1;
                values.push(number.to_string());
                units.push(unit.to_string());
            } else {
                values.push(value_cell.value.clone());
                units.push(String::new());
            }
            spans.push(source_span(
                document,
                value_cell,
                "CSV_GROUP_STATISTIC_VALUE",
                Some(row_index + 1),
                value_index,
                None,
            ));
        }
        if quantity_count != 0 && quantity_count != members.len() {
            return Err(failure(
                "CSV_GROUP_STATISTICS_MIXED_VALUES",
                format!(
                    "group {group_value:?} mixes unit-bearing numbers with unitless or nonnumeric cells"
                ),
            ));
        }
        let statistics = if quantity_count == members.len() {
            crate::math::summarize(&values, Some(&units))?
        } else {
            crate::math::summarize(&values, None)?
        };
        reports.push(CsvGroupStatisticsGroup {
            value: group_value.clone(),
            source_record_numbers,
            statistics,
        });
    }
    let text = serde_json::to_string_pretty(&Report {
        source_sha256: digest(document.as_bytes()),
        group_column,
        value_column,
        groups: &reports,
        source_spans: &spans,
        guarantee: "exact descriptive statistics grouped by exact decoded CSV strings; first-seen group order and original source cells are retained; no population inference",
    })?;
    result(
        document,
        text,
        "CSV_GROUP_STATISTICS",
        BTreeMap::from([
            ("group_column".into(), group_column.into()),
            ("group_column_index".into(), group_index.to_string()),
            ("value_column".into(), value_column.into()),
            ("value_column_index".into(), value_index.to_string()),
            ("group_count".into(), reports.len().to_string()),
            ("row_count".into(), table.rows.len().to_string()),
            ("group_order".into(), "first_seen".into()),
        ]),
        spans,
    )
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CsvChartData {
    pub source_sha256: String,
    pub group_column: String,
    pub value_column: String,
    pub filter: Option<CsvChartFilter>,
    pub total_rows: usize,
    pub selected_rows: usize,
    pub missing_rows: usize,
    pub unit: Option<String>,
    pub groups: Vec<CsvChartGroup>,
    pub source_spans: Vec<DocumentSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<CsvChartFileSource>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CsvChartFileSource {
    pub name: String,
    pub sha256: String,
    /// First row of this file in the aggregate's one-based record numbering.
    /// Each span instead uses a byte range and row number local to this file.
    pub first_record_number: usize,
    pub total_rows: usize,
    pub source_spans: Vec<DocumentSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CsvChartFilter {
    pub column: String,
    pub equals: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct CsvChartGroup {
    pub label: String,
    pub source_record_numbers: Vec<usize>,
    pub statistics: crate::math::Statistics,
}

#[derive(Default)]
struct CsvChartObservations {
    rows: Vec<usize>,
    values: Vec<String>,
    units: Vec<String>,
}

/// Aggregate the original parsed rows so the filter decisions and mean
/// observations retain their CSV byte positions.
pub(crate) fn csv_chart_data(
    document: &str,
    group: &str,
    value: &str,
    filter: Option<(&str, &str)>,
) -> Result<CsvChartData> {
    csv_chart_aggregate(&[("", document)], group, value, filter, false)
}

pub(crate) fn csv_chart_data_from_files(
    files: &[(&str, &str)],
    group: &str,
    value: &str,
    filter: Option<(&str, &str)>,
) -> Result<CsvChartData> {
    if files.is_empty() || files.len() > MAX_REQUEST_ATTACHMENTS {
        return Err(failure(
            "CSV_CHART_FILE_COUNT",
            "集計するCSVファイルの件数が上限を外れています。",
        ));
    }
    let byte_count = files.iter().try_fold(0usize, |total, (_, document)| {
        total.checked_add(document.len())
    });
    if byte_count.is_none_or(|bytes| bytes > MAX_DOCUMENT_BYTES) {
        return Err(failure(
            "DOCUMENT_INPUT_LIMIT",
            "CSVファイルの合計サイズが256000バイトを超えています。",
        ));
    }
    csv_chart_aggregate(files, group, value, filter, true)
}

fn csv_chart_aggregate(
    files: &[(&str, &str)],
    group: &str,
    value: &str,
    filter: Option<(&str, &str)>,
    attached: bool,
) -> Result<CsvChartData> {
    let mut group_indices = BTreeMap::<String, usize>::new();
    let mut grouped = Vec::<(String, CsvChartObservations)>::new();
    let mut source_spans = Vec::new();
    let mut sources = Vec::new();
    let mut total_rows = 0;
    let mut total_cells = 0;
    let mut selected_rows = 0;
    let mut missing_rows = 0;
    let mut observed_units = BTreeSet::new();
    for &(filename, document) in files {
        let csv = if attached {
            document.strip_prefix('\u{feff}').unwrap_or(document)
        } else {
            document
        };
        let table = parse_csv_input(CsvInput::conversation(csv), true)?;
        let header = table.header.as_ref().expect("explicit CSV header");
        let row_count = table.rows.len();
        total_cells += header.len() * (row_count + 1);
        if total_rows + row_count > MAX_ROWS || total_cells > MAX_CELLS {
            return Err(failure(
                "CSV_CHART_FILE_SHAPE",
                "CSVファイルの合計が4096データ行または65536セルを超えています。",
            ));
        }
        let column_index = |name: &str| {
            header
                .iter()
                .position(|cell| cell.value == name)
                .ok_or_else(|| {
                    let message = if attached {
                        format!("「{filename}」に列「{name}」がありません。")
                    } else {
                        format!("CSVに列「{name}」がありません。")
                    };
                    failure("CSV_COLUMN", message)
                })
        };
        let group_index = column_index(group)?;
        let value_index = column_index(value)?;
        if group_index == value_index {
            return Err(failure(
                "CSV_CHART_COLUMNS",
                "グループを表す列と数値の列を別々に指定してください。",
            ));
        }
        let filter_index = filter
            .map(|(name, equals)| column_index(name).map(|index| (index, equals)))
            .transpose()?;
        let column_span =
            |index, kind| source_span(csv, &header[index], kind, Some(0), index, None);
        let mut spans = vec![
            column_span(group_index, "CSV_CHART_GROUP_HEADER"),
            column_span(value_index, "CSV_CHART_VALUE_HEADER"),
        ];
        if let Some((index, _)) = filter_index {
            spans.push(column_span(index, "CSV_CHART_FILTER_HEADER"));
        }
        for (index, row) in table.rows.iter().enumerate() {
            let record = index + 1;
            let cell_span =
                |column, kind| source_span(csv, &row[column], kind, Some(record), column, None);
            if let Some((column, equals)) = filter_index {
                spans.push(cell_span(column, "CSV_CHART_FILTER_DECISION"));
                if row[column].value != equals {
                    continue;
                }
            }
            selected_rows += 1;
            let label = &row[group_index].value;
            let observation_index = *group_indices.entry(label.clone()).or_insert_with(|| {
                let index = grouped.len();
                grouped.push((label.clone(), CsvChartObservations::default()));
                index
            });
            spans.push(cell_span(group_index, "CSV_CHART_GROUP"));
            let raw_value = row[value_index].value.as_str();
            if raw_value.trim().is_empty() {
                missing_rows += 1;
                spans.push(cell_span(value_index, "CSV_CHART_MISSING_VALUE"));
                continue;
            }

            let (number, unit) = crate::language::decimal_quantity_parts(raw_value)
                .or_else(|| {
                    raw_value
                        .trim_end()
                        .strip_suffix('円')
                        .map(|number| (number.trim(), "円"))
                })
                .unwrap_or((raw_value, ""));
            observed_units.insert(if unit == "円" {
                "JPY".into()
            } else {
                unit.to_owned()
            });
            let observations = &mut grouped[observation_index].1;
            observations.rows.push(total_rows + record);
            observations.values.push(number.into());
            observations.units.push(unit.into());
            spans.push(cell_span(value_index, "CSV_CHART_VALUE"));
        }
        if attached {
            let offset = document.len() - csv.len();
            for span in &mut spans {
                span.source_start += offset;
                span.source_end += offset;
            }
            sources.push(CsvChartFileSource {
                name: filename.into(),
                sha256: digest(document.as_bytes()),
                first_record_number: total_rows + 1,
                total_rows: row_count,
                source_spans: spans,
            });
        } else {
            source_spans = spans;
        }
        total_rows += row_count;
    }
    if selected_rows == 0 {
        return Err(failure(
            "CSV_CHART_NO_ROWS",
            "集計条件に一致するデータ行がありません。",
        ));
    }
    if selected_rows == missing_rows {
        return Err(failure(
            "CSV_CHART_NO_VALUES",
            "選択した行の数値列がすべて空欄です。",
        ));
    }

    let convert_lengths = observed_units.len() > 1;
    let unit: Option<String> = if convert_lengths {
        if !observed_units
            .iter()
            .all(|unit| crate::math::exact_length_scale_to_meters(unit).is_some())
        {
            return Err(failure(
                "CSV_CHART_UNIT_MISMATCH",
                "選択した数値を共通の単位で比較できません。",
            ));
        }
        Some("m".into())
    } else {
        observed_units
            .first()
            .filter(|unit| !unit.is_empty())
            .cloned()
    };

    let mut groups = Vec::with_capacity(grouped.len());
    for (label, observations) in grouped {
        if observations.rows.is_empty() {
            continue;
        }
        let mut statistics = if convert_lengths {
            // Normalize before grouping so an all-cm group shares the axis
            // with an all-m group. Keep the original inputs in the report.
            let normalized = observations
                .values
                .iter()
                .zip(&observations.units)
                .map(|(number, unit)| {
                    let scale = crate::math::exact_length_scale_to_meters(unit)
                        .expect("chart units were validated");
                    Ok((crate::math::scalar(number)? * scale).to_string())
                })
                .collect::<Result<Vec<_>>>()?;
            let meters = vec!["m".to_owned(); normalized.len()];
            crate::math::summarize(&normalized, Some(&meters))?
        } else {
            let units = unit
                .as_ref()
                .map(|unit| vec![unit.clone(); observations.values.len()]);
            crate::math::summarize(&observations.values, units.as_deref())?
        };
        statistics.input_values = observations.values;
        statistics.input_units = unit.is_some().then_some(observations.units);
        groups.push(CsvChartGroup {
            label,
            source_record_numbers: observations.rows,
            statistics,
        });
    }
    let source_sha256 = if attached {
        let manifest = sources
            .iter()
            .map(|source| (source.name.as_str(), source.sha256.as_str()))
            .collect::<Vec<_>>();
        identity(&("minidora.csv-chart.file-manifest.v1", manifest))?
    } else {
        digest(files[0].1.as_bytes())
    };
    Ok(CsvChartData {
        source_sha256,
        group_column: group.into(),
        value_column: value.into(),
        filter: filter.map(|(column, equals)| CsvChartFilter {
            column: column.into(),
            equals: equals.into(),
        }),
        total_rows,
        selected_rows,
        missing_rows,
        unit,
        groups,
        source_spans,
        sources,
    })
}

fn csv_pair_tables(input: CsvInput<'_>, delimiter_error: &str) -> Result<(CsvTable, CsvTable)> {
    const LEFT_LABEL: &str = "--- MINIDORA LEFT CSV ---";
    const RIGHT_LABEL: &str = "--- MINIDORA RIGHT CSV ---";
    const LEFT_MARKER: &str = "--- MINIDORA LEFT CSV ---\n";
    const RIGHT_DELIMITER: &str = "\r\n--- MINIDORA RIGHT CSV ---\n";
    const RIGHT_LINE: &str = "\n--- MINIDORA RIGHT CSV ---";
    let document = input.document;
    let invalid_markers = || {
        failure(
            delimiter_error,
            if input.allow_lf {
                "expected one left marker and one right marker on LF or CRLF lines; marker text is reserved"
            } else {
                "expected one left marker and one CRLF-prefixed right marker; marker text is reserved"
            },
        )
    };
    let (left_start, delimiter_start, right_start) = if input.allow_lf {
        let after_marker = |rest: &str| -> Result<usize> {
            if rest.starts_with("\r\n") {
                Ok(2)
            } else if rest.starts_with('\n') {
                Ok(1)
            } else {
                Err(invalid_markers())
            }
        };
        let left_start = LEFT_LABEL.len()
            + after_marker(
                document
                    .strip_prefix(LEFT_LABEL)
                    .ok_or_else(invalid_markers)?,
            )?;
        let marker = document.find(RIGHT_LINE).ok_or_else(invalid_markers)?;
        let right_marker_end = marker + RIGHT_LINE.len();
        let right_start = right_marker_end + after_marker(&document[right_marker_end..])?;
        let delimiter_start = marker - usize::from(document[..marker].ends_with('\r'));
        (left_start, delimiter_start, right_start)
    } else {
        if !document.starts_with(LEFT_MARKER)
            || document.matches(LEFT_MARKER).count() != 1
            || document.matches(RIGHT_DELIMITER).count() != 1
        {
            return Err(invalid_markers());
        }
        let delimiter_start = document
            .find(RIGHT_DELIMITER)
            .ok_or_else(|| failure(delimiter_error, "right marker is missing"))?;
        (
            LEFT_MARKER.len(),
            delimiter_start,
            delimiter_start + RIGHT_DELIMITER.len(),
        )
    };
    let left_source = document
        .get(left_start..delimiter_start)
        .ok_or_else(invalid_markers)?;
    let right_source = &document[right_start..];
    if [left_source, right_source]
        .iter()
        .any(|source| source.contains(LEFT_LABEL) || source.contains(RIGHT_LABEL))
    {
        return Err(failure(
            delimiter_error,
            "reserved comparison marker text occurs more than once",
        ));
    }

    let mut left = parse_csv_input(
        CsvInput {
            document: left_source,
            ..input
        },
        true,
    )?;
    let mut right = parse_csv_input(
        CsvInput {
            document: right_source,
            ..input
        },
        true,
    )?;
    retain_csv_pair_source(
        document,
        [(&mut left, left_start), (&mut right, right_start)],
    );
    Ok((left, right))
}

fn japanese_csv_pair_tables(
    input: CsvInput<'_>,
    format: JapaneseCsvPairFormat,
) -> Result<(CsvTable, CsvTable)> {
    let document = input.document;
    let Some((JapaneseCsvPairLabel::Left, left_start)) = japanese_csv_pair_label(document, format)
    else {
        return Err(format.invalid());
    };
    if left_start == document.len() {
        return Err(format.invalid());
    }
    let (mut left, next_section) = parse_csv_section(
        CsvInput {
            document: &document[left_start..],
            ..input
        },
        true,
        Some((format, JapaneseCsvPairLabel::Left)),
    )?;
    let right_start = left_start + next_section.ok_or_else(|| format.invalid())?;
    let (mut right, _) = parse_csv_section(
        CsvInput {
            document: &document[right_start..],
            ..input
        },
        true,
        Some((format, JapaneseCsvPairLabel::Right)),
    )?;
    retain_csv_pair_source(
        document,
        [(&mut left, left_start), (&mut right, right_start)],
    );
    Ok((left, right))
}

fn retain_csv_pair_source(document: &str, tables: [(&mut CsvTable, usize); 2]) {
    let source_sha256 = digest(document.as_bytes());
    for (table, offset) in tables {
        for cell in table
            .header
            .iter_mut()
            .flatten()
            .chain(table.rows.iter_mut().flatten())
        {
            cell.source_start += offset;
            cell.source_end += offset;
        }
        table.source_document = document.into();
        table.source_sha256 = source_sha256.clone();
    }
}

/// Compare two explicitly supplied, headered CSV tables by an exact unique
/// string key. The input body uses reserved marker lines to keep both original
/// tables in one source document:
/// `--- MINIDORA LEFT CSV ---\n<left CSV>\r\n--- MINIDORA RIGHT CSV ---\n<right CSV>`.
/// Marker text is reserved and must not occur inside either CSV payload.
pub fn csv_compare_by(document: &str, key_column: &str) -> Result<DocumentResult> {
    csv_compare_by_input(CsvInput::strict(document), key_column, false)
}

fn csv_compare_by_input(
    input: CsvInput<'_>,
    key_column: &str,
    japanese_labels: bool,
) -> Result<DocumentResult> {
    let document = input.document;
    admission(document)?;
    if key_column.is_empty() {
        return Err(failure("CSV_COLUMN", "key column name must not be empty"));
    }
    let (left, right) = if japanese_labels {
        japanese_csv_pair_tables(input, JapaneseCsvPairFormat::BeforeAfter)?
    } else {
        csv_pair_tables(input, "CSV_COMPARE_DELIMITER")?
    };

    let left_header = left.header.as_deref().expect("header parsing is explicit");
    let right_header = right.header.as_deref().expect("header parsing is explicit");
    if left_header.len() != right_header.len()
        || left_header
            .iter()
            .zip(right_header)
            .any(|(left, right)| left.value != right.value)
    {
        return Err(failure(
            "CSV_COMPARE_SCHEMA",
            "both CSV headers must contain the same decoded names in the same order",
        ));
    }
    let key_index = left_header
        .iter()
        .position(|cell| cell.value == key_column)
        .ok_or_else(|| failure("CSV_COLUMN", "key column does not exist"))?;

    let make_key_index = |rows: &[Vec<CsvCell>]| -> Result<BTreeMap<String, usize>> {
        let mut index = BTreeMap::new();
        for (row_index, row) in rows.iter().enumerate() {
            let key = &row[key_index].value;
            if index.insert(key.clone(), row_index).is_some() {
                return Err(failure(
                    "CSV_COMPARE_DUPLICATE_KEY",
                    "key column values must be unique within each table",
                ));
            }
        }
        Ok(index)
    };
    let left_keys = make_key_index(&left.rows)?;
    let right_keys = make_key_index(&right.rows)?;

    let mut text = String::from("{\"key_column\":");
    let mut spans = Vec::new();
    append_json_cell(
        &mut text,
        document,
        &left_header[key_index],
        "CSV_COMPARE_KEY_HEADER",
        Some(0),
        key_index,
        &mut spans,
    )?;
    text.push_str(",\"changes\":[");
    let mut first_change = true;
    let mut changed_cells = 0usize;
    let mut removed_rows = 0usize;
    let mut added_rows = 0usize;

    for (left_row_index, left_row) in left.rows.iter().enumerate() {
        let key = &left_row[key_index].value;
        let Some(&right_row_index) = right_keys.get(key) else {
            if !first_change {
                text.push(',');
            }
            first_change = false;
            removed_rows += 1;
            text.push_str("{\"kind\":\"ROW_REMOVED\",\"key\":");
            append_json_cell(
                &mut text,
                document,
                &left_row[key_index],
                "CSV_COMPARE_REMOVED_KEY",
                Some(left_row_index + 1),
                key_index,
                &mut spans,
            )?;
            text.push_str(",\"row\":[");
            for (column, cell) in left_row.iter().enumerate() {
                if column > 0 {
                    text.push(',');
                }
                append_json_cell(
                    &mut text,
                    document,
                    cell,
                    "CSV_COMPARE_REMOVED_CELL",
                    Some(left_row_index + 1),
                    column,
                    &mut spans,
                )?;
            }
            text.push_str("]}");
            continue;
        };

        let right_row = &right.rows[right_row_index];
        for (column, (left_cell, right_cell)) in left_row.iter().zip(right_row).enumerate() {
            if column == key_index || left_cell.value == right_cell.value {
                continue;
            }
            if !first_change {
                text.push(',');
            }
            first_change = false;
            changed_cells += 1;
            text.push_str("{\"kind\":\"CELL_CHANGED\",\"key\":");
            append_json_cell(
                &mut text,
                document,
                &left_row[key_index],
                "CSV_COMPARE_CHANGED_KEY",
                Some(left_row_index + 1),
                key_index,
                &mut spans,
            )?;
            spans.push(source_span(
                document,
                &right_row[key_index],
                "CSV_COMPARE_MATCHED_KEY",
                Some(right_row_index + 1),
                key_index,
                None,
            ));
            text.push_str(",\"column\":");
            append_json_cell(
                &mut text,
                document,
                &left_header[column],
                "CSV_COMPARE_COLUMN",
                Some(0),
                column,
                &mut spans,
            )?;
            text.push_str(",\"left\":");
            append_json_cell(
                &mut text,
                document,
                left_cell,
                "CSV_COMPARE_LEFT_CELL",
                Some(left_row_index + 1),
                column,
                &mut spans,
            )?;
            text.push_str(",\"right\":");
            append_json_cell(
                &mut text,
                document,
                right_cell,
                "CSV_COMPARE_RIGHT_CELL",
                Some(right_row_index + 1),
                column,
                &mut spans,
            )?;
            text.push('}');
        }
    }

    for (right_row_index, right_row) in right.rows.iter().enumerate() {
        if left_keys.contains_key(&right_row[key_index].value) {
            continue;
        }
        if !first_change {
            text.push(',');
        }
        first_change = false;
        added_rows += 1;
        text.push_str("{\"kind\":\"ROW_ADDED\",\"key\":");
        append_json_cell(
            &mut text,
            document,
            &right_row[key_index],
            "CSV_COMPARE_ADDED_KEY",
            Some(right_row_index + 1),
            key_index,
            &mut spans,
        )?;
        text.push_str(",\"row\":[");
        for (column, cell) in right_row.iter().enumerate() {
            if column > 0 {
                text.push(',');
            }
            append_json_cell(
                &mut text,
                document,
                cell,
                "CSV_COMPARE_ADDED_CELL",
                Some(right_row_index + 1),
                column,
                &mut spans,
            )?;
        }
        text.push_str("]}");
    }
    text.push_str("]}");
    result(
        document,
        text,
        "CSV_TABLE_COMPARE",
        BTreeMap::from([
            ("key_column".into(), key_column.into()),
            ("key_index".into(), key_index.to_string()),
            (
                "matched_key_count".into(),
                left_keys
                    .keys()
                    .filter(|key| right_keys.contains_key(*key))
                    .count()
                    .to_string(),
            ),
            ("changed_cell_count".into(), changed_cells.to_string()),
            ("removed_row_count".into(), removed_rows.to_string()),
            ("added_row_count".into(), added_rows.to_string()),
            (
                "ordering".into(),
                "left_row_then_column_then_right_row".into(),
            ),
            ("comparison".into(), "exact_decoded_strings".into()),
        ]),
        spans,
    )
}

fn append_csv_join_cell(
    output: &mut String,
    document: &str,
    cell: &CsvCell,
    kind: &str,
    location: (usize, usize),
    spans: &mut Vec<DocumentSpan>,
    source_span_budget: &mut usize,
) -> Result<()> {
    let span_index = spans.len();
    append_json_cell(
        output,
        document,
        cell,
        kind,
        Some(location.0),
        location.1,
        spans,
    )?;
    *source_span_budget =
        source_span_budget.saturating_add(serde_json::to_vec(&spans[span_index])?.len());
    if output.len() > MAX_RESULT_BYTES || *source_span_budget > MAX_RESULT_BYTES {
        return Err(failure(
            "CSV_INNER_JOIN_OUTPUT_LIMIT",
            "joined table and source spans exceed the 256000-byte output envelope",
        ));
    }
    Ok(())
}

/// Join two explicitly supplied headered CSV tables using unique exact-string
/// keys. Output rows retain left-table order and map every emitted cell back to
/// the combined original source document.
pub fn csv_inner_join(
    document: &str,
    left_key_column: &str,
    right_key_column: &str,
) -> Result<DocumentResult> {
    csv_inner_join_input(
        CsvInput::strict(document),
        left_key_column,
        right_key_column,
        false,
    )
}

fn csv_inner_join_input(
    input: CsvInput<'_>,
    left_key_column: &str,
    right_key_column: &str,
    japanese_labels: bool,
) -> Result<DocumentResult> {
    let document = input.document;
    admission(document)?;
    if left_key_column.is_empty() || right_key_column.is_empty() {
        return Err(failure(
            "CSV_INNER_JOIN_COLUMN",
            "both named key columns must not be empty",
        ));
    }
    let (left, right) = if japanese_labels {
        japanese_csv_pair_tables(input, JapaneseCsvPairFormat::LeftRight)?
    } else {
        csv_pair_tables(input, "CSV_INNER_JOIN_DELIMITER")?
    };
    let left_header = left.header.as_deref().expect("header parsing is explicit");
    let right_header = right.header.as_deref().expect("header parsing is explicit");
    let left_key_index = left_header
        .iter()
        .position(|cell| cell.value == left_key_column)
        .ok_or_else(|| failure("CSV_INNER_JOIN_COLUMN", "left key column does not exist"))?;
    let right_key_index = right_header
        .iter()
        .position(|cell| cell.value == right_key_column)
        .ok_or_else(|| failure("CSV_INNER_JOIN_COLUMN", "right key column does not exist"))?;

    let mut right_by_key = BTreeMap::<String, usize>::new();
    for (row_index, row) in right.rows.iter().enumerate() {
        if right_by_key
            .insert(row[right_key_index].value.clone(), row_index)
            .is_some()
        {
            return Err(failure(
                "CSV_INNER_JOIN_DUPLICATE_KEY",
                "right join keys must be unique",
            ));
        }
    }
    let mut left_keys = BTreeSet::<String>::new();
    for row in &left.rows {
        if !left_keys.insert(row[left_key_index].value.clone()) {
            return Err(failure(
                "CSV_INNER_JOIN_DUPLICATE_KEY",
                "left join keys must be unique",
            ));
        }
    }

    let mut text = String::from("{\"columns\":[");
    let mut spans = Vec::new();
    let mut source_span_budget = 0usize;
    for (side, header) in [("left.", left_header), ("right.", right_header)] {
        for (column_index, cell) in header.iter().enumerate() {
            if !spans.is_empty() {
                text.push(',');
            }
            let output_start = text.len();
            text.push_str(&serde_json::to_string(&format!("{side}{}", cell.value))?);
            let span = source_span(
                document,
                cell,
                if side == "left." {
                    "CSV_INNER_JOIN_LEFT_HEADER"
                } else {
                    "CSV_INNER_JOIN_RIGHT_HEADER"
                },
                Some(0),
                column_index,
                Some((output_start, text.len())),
            );
            source_span_budget =
                source_span_budget.saturating_add(serde_json::to_vec(&span)?.len());
            if text.len() > MAX_RESULT_BYTES || source_span_budget > MAX_RESULT_BYTES {
                return Err(failure(
                    "CSV_INNER_JOIN_OUTPUT_LIMIT",
                    "joined table and source spans exceed the 256000-byte output envelope",
                ));
            }
            spans.push(span);
        }
    }

    text.push_str("],\"rows\":[");
    let mut source_record_pairs = Vec::<(usize, usize)>::new();
    let mut unmatched_left = Vec::<(usize, usize)>::new();
    let mut matched_right = vec![false; right.rows.len()];
    for (left_row_index, left_row) in left.rows.iter().enumerate() {
        let Some(&right_row_index) = right_by_key.get(&left_row[left_key_index].value) else {
            unmatched_left.push((left_row_index, left_key_index));
            continue;
        };
        if !source_record_pairs.is_empty() {
            text.push(',');
        }
        text.push('[');
        for (column_index, cell) in left_row.iter().enumerate() {
            if column_index > 0 {
                text.push(',');
            }
            append_csv_join_cell(
                &mut text,
                document,
                cell,
                "CSV_INNER_JOIN_LEFT_CELL",
                (left_row_index + 1, column_index),
                &mut spans,
                &mut source_span_budget,
            )?;
        }
        for (column_index, cell) in right.rows[right_row_index].iter().enumerate() {
            // Each right cell follows the nonempty left side of this row.
            text.push(',');
            append_csv_join_cell(
                &mut text,
                document,
                cell,
                "CSV_INNER_JOIN_RIGHT_CELL",
                (right_row_index + 1, column_index),
                &mut spans,
                &mut source_span_budget,
            )?;
        }
        text.push(']');
        source_record_pairs.push((left_row_index, right_row_index));
        matched_right[right_row_index] = true;
        if text.len() > MAX_RESULT_BYTES {
            return Err(failure(
                "CSV_INNER_JOIN_OUTPUT_LIMIT",
                "joined table exceeds 256000 output bytes",
            ));
        }
    }

    text.push_str("],\"left_source_record_numbers\":[");
    for (index, (left_row, _)) in source_record_pairs.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&(left_row + 1).to_string());
    }
    text.push_str("],\"right_source_record_numbers\":[");
    for (index, (_, right_row)) in source_record_pairs.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&(right_row + 1).to_string());
    }
    text.push_str("],\"unmatched_left\":[");
    for (index, (row_index, key_index)) in unmatched_left.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str("{\"key\":");
        append_csv_join_cell(
            &mut text,
            document,
            &left.rows[*row_index][*key_index],
            "CSV_INNER_JOIN_UNMATCHED_LEFT_KEY",
            (row_index + 1, *key_index),
            &mut spans,
            &mut source_span_budget,
        )?;
        text.push_str(",\"source_record_number\":");
        text.push_str(&(row_index + 1).to_string());
        text.push('}');
    }
    text.push_str("],\"unmatched_right\":[");
    let mut unmatched_right_output_index = 0usize;
    for (row_index, row) in right.rows.iter().enumerate() {
        if matched_right[row_index] {
            continue;
        }
        if unmatched_right_output_index > 0 {
            text.push(',');
        }
        unmatched_right_output_index += 1;
        text.push_str("{\"key\":");
        append_csv_join_cell(
            &mut text,
            document,
            &row[right_key_index],
            "CSV_INNER_JOIN_UNMATCHED_RIGHT_KEY",
            (row_index + 1, right_key_index),
            &mut spans,
            &mut source_span_budget,
        )?;
        text.push_str(",\"source_record_number\":");
        text.push_str(&(row_index + 1).to_string());
        text.push('}');
    }
    text.push_str("]}");
    let matched_count = source_record_pairs.len();
    let unmatched_left_count = unmatched_left.len();
    let unmatched_right_count = matched_right.iter().filter(|matched| !**matched).count();
    result(
        document,
        text,
        "CSV_INNER_JOIN",
        BTreeMap::from([
            ("left_key_column".into(), left_key_column.into()),
            ("left_key_index".into(), left_key_index.to_string()),
            ("right_key_column".into(), right_key_column.into()),
            ("right_key_index".into(), right_key_index.to_string()),
            ("matched_row_count".into(), matched_count.to_string()),
            (
                "unmatched_left_row_count".into(),
                unmatched_left_count.to_string(),
            ),
            (
                "unmatched_right_row_count".into(),
                unmatched_right_count.to_string(),
            ),
            ("ordering".into(), "left_row_order".into()),
            ("comparison".into(), "exact_decoded_strings".into()),
        ]),
        spans,
    )
}

struct JsonTableParser<'a> {
    text: &'a str,
    at: usize,
    cells: usize,
}
impl JsonTableParser<'_> {
    fn space(&mut self) {
        while self
            .text
            .as_bytes()
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\r' | b'\n' | b'\t'))
        {
            self.at += 1;
        }
    }
    fn take(&mut self, byte: u8) -> bool {
        self.space();
        if input_byte(self.text.as_bytes(), self.at) == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, byte: u8) -> Result<()> {
        if self.take(byte) {
            Ok(())
        } else {
            Err(failure(
                "JSON_TABLE_SYNTAX",
                format!("expected byte {} at {}", byte, self.at),
            ))
        }
    }
    fn string(&mut self) -> Result<CsvCell> {
        self.space();
        let start = self.at;
        self.expect(b'"')?;
        let mut escaped = false;
        loop {
            let byte = input_byte(self.text.as_bytes(), self.at)
                .ok_or_else(|| failure("JSON_TABLE_SYNTAX", "unterminated JSON string"))?;
            self.at += 1;
            if !escaped && byte == b'"' {
                break;
            }
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            }
        }
        let value: String = serde_json::from_str(&self.text[start..self.at])?;
        Ok(CsvCell {
            value,
            source_start: start,
            source_end: self.at,
        })
    }
    fn strings(&mut self) -> Result<Vec<CsvCell>> {
        self.expect(b'[')?;
        let mut values = Vec::new();
        if self.take(b']') {
            return Ok(values);
        }
        loop {
            values.push(self.string()?);
            self.cells += 1;
            if values.len() > MAX_COLUMNS || self.cells > MAX_CELLS {
                return Err(failure(
                    "CSV_CELL_LIMIT",
                    "table column/cell limit exceeded",
                ));
            }
            if self.take(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(values)
    }
    fn rows(&mut self) -> Result<Vec<Vec<CsvCell>>> {
        self.expect(b'[')?;
        let mut rows = Vec::new();
        if self.take(b']') {
            return Ok(rows);
        }
        loop {
            rows.push(self.strings()?);
            if rows.len() > MAX_ROWS {
                return Err(failure("CSV_ROW_LIMIT", "table row limit exceeded"));
            }
            if self.take(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(rows)
    }
    fn table(&mut self) -> Result<CsvTable> {
        self.expect(b'{')?;
        let mut keys = BTreeSet::new();
        let mut header = None;
        let mut rows = None;
        if self.take(b'}') {
            return Err(failure(
                "JSON_TABLE_SHAPE",
                "table requires columns and rows fields",
            ));
        }
        loop {
            let key = self.string()?.value;
            if !keys.insert(key.clone()) {
                return Err(failure("DUPLICATE_JSON_KEY", "table field is duplicated"));
            }
            self.expect(b':')?;
            match key.as_str() {
                "columns" => {
                    self.space();
                    if self.text[self.at..].starts_with("null") {
                        self.at += 4;
                    } else {
                        header = Some(self.strings()?);
                    }
                }
                "rows" => {
                    rows = Some(self.rows()?);
                }
                _ => {
                    return Err(failure(
                        "JSON_TABLE_SHAPE",
                        "only explicit columns and rows fields are supported",
                    ));
                }
            }
            if self.take(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        self.space();
        if self.at != self.text.len() {
            return Err(failure(
                "JSON_TABLE_SYNTAX",
                "trailing JSON input is not allowed",
            ));
        }
        if !keys.contains("columns") || !keys.contains("rows") {
            return Err(failure(
                "JSON_TABLE_SHAPE",
                "table requires columns and rows fields",
            ));
        }
        let rows = rows.expect("validated rows key");
        validate_table(header.as_deref(), &rows)?;
        Ok(CsvTable {
            source_document: self.text.into(),
            source_sha256: digest(self.text.as_bytes()),
            header,
            rows,
        })
    }
}
fn formula_guard(value: &str) -> Result<()> {
    let visible = value.trim_start_matches(|ch: char| ch.is_whitespace() || ch == '\u{feff}');
    if visible.starts_with(['=', '+', '-', '@']) {
        return Err(failure(
            "CSV_FORMULA_RISK",
            "formula-leading cells require an explicit different export policy; content is not silently altered",
        ));
    }
    if value
        .bytes()
        .any(|byte| !csv_character(byte) && !matches!(byte, b'\r' | b'\n'))
    {
        return Err(failure(
            "CSV_CHARACTER",
            "unsupported control character in CSV export",
        ));
    }
    Ok(())
}
fn csv_field(value: &str) -> Result<String> {
    formula_guard(value)?;
    Ok(
        if value.is_empty() || value.contains([',', '"', '\r', '\n']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.into()
        },
    )
}
/// The input is exactly {"columns": [string,...] | null,"rows": [[string,...],...] }.
/// Unknown/duplicate fields, duplicate decoded headers and nonstring cells fail.
/// Spreadsheet formula-leading strings are withheld instead of prefixed/changed.
pub fn json_to_csv(document: &str) -> Result<DocumentResult> {
    admission(document)?;
    let mut parser = JsonTableParser {
        text: document,
        at: 0,
        cells: 0,
    };
    let table = parser.table()?;
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut emit_row = |row: &[CsvCell], kind: &str, row_index: Option<usize>| -> Result<()> {
        for (column, cell) in row.iter().enumerate() {
            if column > 0 {
                text.push(',');
            }
            let start = text.len();
            text.push_str(&csv_field(&cell.value)?);
            if text.len() > MAX_RESULT_BYTES {
                return Err(failure(
                    "DOCUMENT_OUTPUT_LIMIT",
                    "CSV export exceeds output limit",
                ));
            }
            spans.push(source_span(
                document,
                cell,
                kind,
                row_index,
                column,
                Some((start, text.len())),
            ));
        }
        text.push_str("\r\n");
        Ok(())
    };
    if let Some(header) = &table.header {
        emit_row(header, "JSON_HEADER", None)?;
    }
    for (index, row) in table.rows.iter().enumerate() {
        emit_row(row, "JSON_CELL", Some(index))?;
    }
    result(
        document,
        text,
        "JSON_TO_CSV",
        BTreeMap::from([
            ("has_header".into(), table.header.is_some().to_string()),
            ("value_type".into(), "STRING".into()),
            ("formula_policy".into(), "REJECT_LEADING_MARKERS".into()),
        ]),
        spans,
    )
}

pub(crate) struct PreviousJsonTableConversion {
    source_result_id: String,
    source_bundle_ref: String,
    source_report_ref: String,
    source_document_report: Option<DocumentResult>,
    pub(crate) report: DocumentResult,
}

impl PreviousJsonTableConversion {
    pub(crate) fn dependencies(&self, report_ref: String) -> [String; 4] {
        [
            self.source_result_id.clone(),
            self.source_bundle_ref.clone(),
            self.source_report_ref.clone(),
            report_ref,
        ]
    }
}

pub(crate) fn previous_json_table_to_csv(
    store: &crate::store::Store,
    followup: &InputBundle,
    previous: &ResultRecord,
) -> Result<PreviousJsonTableConversion> {
    const INVALID: &str = "PREVIOUS_JSON_TABLE_CONTEXT_INVALID";
    let invalid = || {
        failure(
            INVALID,
            "変換元を同じ会話の封印入力と確定結果に結び付けられません。",
        )
    };
    let request = &followup.request;
    let actor = &request.actor_id;
    let case = &request.case_id;
    let parser_version = followup.meaning.parser_version.as_str();
    if !matches!(
        followup.meaning.intent,
        Intent::ConvertPreviousJsonTableToCsv
    ) || !matches!(
        parser_version,
        crate::input::PREVIOUS_JSON_TABLE_TO_CSV_PARSER_VERSION
            | crate::input::PREVIOUS_FILTERED_CSV_TO_CSV_PARSER_VERSION
            | crate::input::PREVIOUS_FILTERED_JSON_TO_CSV_PARSER_VERSION
    ) || followup.meaning.context_result_ref.as_ref() != Some(&previous.id)
        || followup.context_sha256 != identity(&Some(previous))?
        || request
            .context_ref
            .as_ref()
            .is_some_and(|id| id != &previous.id)
        || previous.lifecycle != Lifecycle::Commit
    {
        return Err(invalid());
    }

    let (source_bundle_ref, source_bundle) =
        store.with_verified_case_history(actor, case, |history| {
            if history
                .input_result(&previous.input_id, previous.input_version)?
                .as_ref()
                != Some(previous)
            {
                return Err(invalid());
            }
            history
                .input_bundle(&previous.input_id, previous.input_version)?
                .ok_or_else(invalid)
        })?;
    let verify_source_identity = || {
        if identity(&source_bundle.request)? != previous.id
            || previous.guarantee != Guarantee::Computed
        {
            return Err(invalid());
        }
        Ok(())
    };
    enum SourceReport {
        Document(DocumentResult),
        FilteredJson(crate::ports::JsonTableFilterReport),
    }
    let (source, source_label) = match (&source_bundle.meaning.intent, parser_version) {
        (
            Intent::CsvToJson {
                document,
                has_header,
            },
            crate::input::PREVIOUS_JSON_TABLE_TO_CSV_PARSER_VERSION,
        ) => {
            verify_source_identity()?;
            (
                SourceReport::Document(CsvInput::conversation(document).to_json(*has_header)?),
                "CSV-to-JSON conversion",
            )
        }
        (
            Intent::CsvFilter {
                document,
                column,
                equals,
            },
            crate::input::PREVIOUS_FILTERED_CSV_TO_CSV_PARSER_VERSION,
        ) => {
            verify_source_identity()?;
            (
                SourceReport::Document(CsvInput::conversation(document).filter(column, equals)?),
                "CSV filter result",
            )
        }
        (
            Intent::JsonTableFilter {
                document,
                column,
                equals,
            },
            crate::input::PREVIOUS_FILTERED_JSON_TO_CSV_PARSER_VERSION,
        ) => {
            verify_source_identity()?;
            (
                SourceReport::FilteredJson(crate::ports::json_table_filter(
                    document, column, equals,
                )?),
                "JSON table filter result",
            )
        }
        (_, crate::input::PREVIOUS_JSON_TABLE_TO_CSV_PARSER_VERSION) => {
            return Err(failure(
                "PREVIOUS_JSON_TABLE_UNAVAILABLE",
                "参照先にCSVから変換した確定済みJSON表がありません。",
            ));
        }
        _ => return Err(invalid()),
    };
    let (source_report_bytes, valid_text) = match &source {
        SourceReport::Document(report) => (canonical(report)?, previous.text == report.text),
        SourceReport::FilteredJson(report) => (
            canonical(report)?,
            previous.text == serde_json::to_string_pretty(report)?,
        ),
    };
    let source_evidence_bytes = match &source {
        SourceReport::Document(_) => 0,
        SourceReport::FilteredJson(_) => source_report_bytes.len(),
    };
    let evidence_limit = || failure("RESOURCE_LIMIT", "CSV変換の記録が証拠予算を超えています。");
    if source_evidence_bytes > request.budget.max_evidence_bytes {
        return Err(evidence_limit());
    }
    let source_report_ref = digest(&source_report_bytes);
    if previous.dependencies.as_slice() != [source_report_ref.as_str()]
        || !valid_text
        || store
            .get_bytes_if_present(actor, case, &source_report_ref)?
            .as_deref()
            != Some(source_report_bytes.as_slice())
    {
        return Err(invalid());
    }
    revalidate_previous_document_render(
        store,
        actor,
        case,
        previous,
        Some(&source_report_ref),
        INVALID,
        source_label,
    )?;

    let (report, source_document_report) = match source {
        SourceReport::Document(report) => (json_to_csv(&report.text)?, Some(report)),
        SourceReport::FilteredJson(report) => {
            // The filter report is not the table input. Pass only its typed
            // columns and selected rows to the existing exact CSV converter.
            #[derive(serde::Serialize)]
            struct SelectedTable<'a> {
                columns: &'a [String],
                rows: &'a [Vec<String>],
            }
            let table = serde_json::to_string(&SelectedTable {
                columns: &report.columns,
                rows: &report.rows,
            })?;
            (json_to_csv(&table)?, None)
        }
    };
    if canonical(&report)?
        .len()
        .saturating_add(source_evidence_bytes)
        > request.budget.max_evidence_bytes
    {
        return Err(evidence_limit());
    }
    Ok(PreviousJsonTableConversion {
        source_result_id: previous.id.clone(),
        source_bundle_ref,
        source_report_ref,
        source_document_report,
        report,
    })
}

/// Replace all original nonoverlapping literal matches. The optional expected
/// count is checked before building any result; original bytes are retained.
/// No surrounding text is rewritten and introduced text is not rescanned.
pub fn replace_exact(
    document: &str,
    old: &str,
    new: &str,
    expected_count: Option<usize>,
) -> Result<DocumentResult> {
    admission(document)?;
    if old.is_empty()
        || old == new
        || old.len() > MAX_DOCUMENT_BYTES
        || new.len() > MAX_RESULT_BYTES
    {
        return Err(failure(
            "INVALID_REPLACEMENT",
            "search must be nonempty, differ from replacement and fit its byte limits",
        ));
    }
    if expected_count.is_some_and(|n| n == 0 || n > MAX_EDITS) {
        return Err(failure(
            "INVALID_REPLACEMENT_COUNT",
            "expected count must be 1..4096",
        ));
    }
    let matches: Vec<usize> = document
        .match_indices(old)
        .map(|(start, _)| start)
        .take(MAX_EDITS + 1)
        .collect();
    if matches.is_empty() {
        return Err(failure(
            "REPLACEMENT_NOT_FOUND",
            "the exact original search text does not occur",
        ));
    }
    if matches.len() > MAX_EDITS {
        return Err(failure(
            "REPLACEMENT_LIMIT",
            "replacement exceeds 4096 edits",
        ));
    }
    if expected_count.is_some_and(|count| count != matches.len()) {
        return Err(failure(
            "REPLACEMENT_COUNT_MISMATCH",
            "actual nonoverlapping original matches differ from the expected count",
        ));
    }
    let size = document
        .len()
        .checked_sub(
            old.len()
                .checked_mul(matches.len())
                .ok_or_else(|| failure("DOCUMENT_OUTPUT_LIMIT", "replacement size overflow"))?,
        )
        .and_then(|n| {
            new.len()
                .checked_mul(matches.len())
                .and_then(|extra| n.checked_add(extra))
        })
        .ok_or_else(|| failure("DOCUMENT_OUTPUT_LIMIT", "replacement size overflow"))?;
    if size > MAX_RESULT_BYTES {
        return Err(failure(
            "DOCUMENT_OUTPUT_LIMIT",
            "replacement result exceeds byte limit",
        ));
    }
    let mut text = String::with_capacity(size);
    let mut spans = Vec::new();
    let mut cursor = 0usize;
    for start in matches
        .iter()
        .copied()
        .chain(std::iter::once(document.len()))
    {
        if cursor < start {
            let output_start = text.len();
            text.push_str(&document[cursor..start]);
            spans.push(DocumentSpan {
                kind: "UNCHANGED".into(),
                source_start: cursor,
                source_end: start,
                source_text: document[cursor..start].into(),
                decoded_value: None,
                row: None,
                column: None,
                output_start: Some(output_start),
                output_end: Some(text.len()),
            });
        }
        if start == document.len() {
            break;
        }
        let output_start = text.len();
        text.push_str(new);
        spans.push(DocumentSpan {
            kind: "EXACT_REPLACEMENT".into(),
            source_start: start,
            source_end: start + old.len(),
            source_text: old.into(),
            decoded_value: None,
            row: None,
            column: None,
            output_start: Some(output_start),
            output_end: Some(text.len()),
        });
        cursor = start + old.len();
    }
    result(
        document,
        text,
        "TEXT_REPLACE_EXACT",
        BTreeMap::from([
            ("old".into(), old.into()),
            ("new".into(), new.into()),
            ("actual_count".into(), matches.len().to_string()),
            (
                "expected_count".into(),
                expected_count.map_or("ALL_NONOVERLAPPING_MATCHES".into(), |n| n.to_string()),
            ),
        ]),
        spans,
    )
}

/// Replace one exact original literal only when it occurs once, counting
/// overlapping matches. This is for natural-language edits of a retained
/// output; it never treats a partial match as an instruction to choose a span.
pub fn replace_unique_exact(document: &str, old: &str, new: &str) -> Result<DocumentResult> {
    if old.is_empty() {
        return Err(failure(
            "INVALID_REPLACEMENT",
            "search text must be nonempty",
        ));
    }
    let Some(start) = document.find(old) else {
        return Err(failure(
            "PREVIOUS_TEXT_NOT_FOUND",
            "the exact literal does not occur in the retained previous output",
        ));
    };
    let next_boundary = start
        + document[start..]
            .chars()
            .next()
            .map(char::len_utf8)
            .unwrap_or(1);
    if document[next_boundary..].find(old).is_some() {
        return Err(failure(
            "PREVIOUS_TEXT_AMBIGUOUS",
            "the exact literal occurs more than once, including overlapping occurrences",
        ));
    }
    replace_exact(document, old, new, Some(1))
}

pub(crate) fn replace_pasted_text(
    document: &str,
    old: &str,
    new: &str,
    count: Option<usize>,
) -> Result<DocumentResult> {
    if let Some(count) = count {
        return replace_exact(document, old, new, Some(count));
    }
    admission(document)?;
    replace_unique_exact(document, old, new).map_err(|error| match &error {
        minidora_domain::Error::Contract { code, .. } if code == "PREVIOUS_TEXT_NOT_FOUND" => {
            failure("REPLACEMENT_NOT_FOUND", "貼り付けた本文に指定の文字列がありません。")
        }
        minidora_domain::Error::Contract { code, .. } if code == "PREVIOUS_TEXT_AMBIGUOUS" => {
            failure(
                "REPLACEMENT_AMBIGUOUS",
                "重なる位置も含めて指定の文字列が複数あります。非重複の置換件数を指定してください。",
            )
        }
        _ => error,
    })
}

pub(crate) fn previous_text_replacement(
    previous: &ResultRecord,
    old: &str,
    new: &str,
) -> Result<(String, String, PreviousTextEditArtifact)> {
    replace_retained_text(previous, old, new, None)
}

const PREVIOUS_ANSWER_EDIT: &str = "PREVIOUS_OUTPUT_REPLACE_EXACT_V1";
const PREVIOUS_DOCUMENT_EDIT: &str = "PREVIOUS_DOCUMENT_REPLACE_EXACT_V1";

pub(crate) fn previous_text_replacement_for_bundle(
    store: &crate::store::Store,
    bundle: &InputBundle,
    prior_results: &BTreeMap<String, ResultRecord>,
    previous: &ResultRecord,
    old: &str,
    new: &str,
) -> Result<(String, String, PreviousTextEditArtifact)> {
    let version = &bundle.meaning.parser_version;
    if !crate::input::is_previous_document_text_replacement_version(version) {
        return previous_text_replacement(previous, old, new);
    }
    let pasted_summary_body =
        version == crate::input::PASTED_SUMMARY_BODY_REPLACEMENT_PARSER_VERSION;
    let invalid = || {
        failure(
            "PREVIOUS_DOCUMENT_BODY_UNVERIFIABLE",
            "前回の文章の本文を保存済みの結果から照合できません。",
        )
    };
    let request = &bundle.request;
    if !crate::input::is_previous_document_text_replacement(&request.text)
        || bundle.meaning.context_result_ref.as_deref() != Some(previous.id.as_str())
        || bundle.context_sha256 != identity(&Some(previous))?
        || prior_results.get(&previous.id) != Some(previous)
    {
        return Err(invalid());
    }
    let actor = &request.actor_id;
    let case = &request.case_id;
    let (source_ref, source) = store
        .get_input_bundle(actor, case, &previous.input_id, previous.input_version)?
        .ok_or_else(invalid)?;
    if identity(&source.request)? != previous.id {
        return Err(invalid());
    }
    match &source.meaning.intent {
        Intent::SummarizePastedText { .. } if pasted_summary_body => {
            let plan = previous.content_plan.as_ref().ok_or_else(invalid)?;
            let (report_ref, report) =
                crate::recovery::verify_text_summary_candidate(store, &source, previous, plan)?;
            revalidate_previous_document_render(
                store,
                actor,
                case,
                previous,
                Some(&report_ref),
                "PREVIOUS_OUTPUT_INTEGRITY",
                "pasted text summary",
            )?;
            // The canonical, re-derived pasted summary has one fixed notice.
            // Keep the exact displayed bullets, including their line breaks.
            let body = report
                .text
                .strip_prefix(EXTRACTIVE_SUMMARY_LABEL)
                .and_then(|text| text.strip_prefix('\n'))
                .filter(|text| !text.is_empty())
                .ok_or_else(invalid)?;
            replace_retained_text(previous, old, new, Some(body))
        }
        Intent::TextReplace { .. } => {
            let (_, _, _, report) = verify_pasted_text_replacement_report(
                store,
                actor,
                case,
                previous,
                &source_ref,
                &source,
            )?;
            replace_retained_text(previous, old, new, Some(&report.text))
        }
        Intent::ReplacePreviousText {
            old: source_old,
            new: source_new,
        } => {
            let [parent_id, parent_plan, parent_render, report_ref] =
                previous.dependencies.as_slice()
            else {
                return Err(invalid());
            };
            let report_bytes = store.get_bytes(actor, case, report_ref)?;
            let report: PreviousTextEditArtifact = serde_json::from_slice(&report_bytes)?;
            if canonical(&report)? != report_bytes
                || report.source_result_id != *parent_id
                || report.source_content_plan_sha256 != *parent_plan
                || report.source_render_sha256 != *parent_render
                || source.meaning.context_result_ref.as_ref() != Some(parent_id)
                || previous.guarantee != Guarantee::UserPremise
                || previous.calculation.is_some()
                || !previous.evidence.is_empty()
                || !previous.content_plan.as_ref().is_some_and(|plan| {
                    plan.locale == source.request.locale
                        && plan.required_units == previous_text_replacement_lines(&report)
                })
            {
                return Err(invalid());
            }
            revalidate_previous_document_render(
                store,
                actor,
                case,
                previous,
                Some(report_ref),
                "PREVIOUS_OUTPUT_INTEGRITY",
                "edited document",
            )?;
            // Old editions could pass a complete pasted-summary response off
            // as document text, including after additional document edits.
            // A fresh document edit must not inherit that older classification.
            if pasted_summary_body
                && source.meaning.parser_version
                    != crate::input::PASTED_SUMMARY_BODY_REPLACEMENT_PARSER_VERSION
                && store.with_verified_case_history(actor, case, |history| {
                    let mut reference = parent_id;
                    for _ in 0..request.budget.max_actions {
                        let ancestor = prior_results
                            .get(reference)
                            .filter(|result| result.lifecycle == Lifecycle::Commit)
                            .ok_or_else(invalid)?;
                        let (_, ancestor_input) = history
                            .input_bundle(&ancestor.input_id, ancestor.input_version)?
                            .ok_or_else(invalid)?;
                        if identity(&ancestor_input.request)? != *reference {
                            return Err(invalid());
                        }
                        match &ancestor_input.meaning.intent {
                            Intent::SummarizePastedText { .. } => return Ok(true),
                            Intent::ReplacePreviousText { .. }
                                if ancestor_input.meaning.parser_version
                                    != crate::input::PASTED_SUMMARY_BODY_REPLACEMENT_PARSER_VERSION =>
                            {
                                reference = ancestor
                                    .dependencies
                                    .first()
                                    .filter(|id| {
                                        ancestor_input.meaning.context_result_ref.as_ref() == Some(*id)
                                    })
                                    .ok_or_else(invalid)?;
                            }
                            _ => return Ok(false),
                        }
                    }
                    Err(failure(
                        "RESOURCE_LIMIT",
                        "過去の文章編集の由来を今回の処理予算内で照合できません。",
                    ))
                })?
            {
                return Err(invalid());
            }
            match report.operation.as_str() {
                PREVIOUS_DOCUMENT_EDIT
                    if crate::input::is_previous_document_text_replacement_version(
                        &source.meaning.parser_version,
                    ) && crate::input::is_previous_document_text_replacement(
                        &source.request.text,
                    ) && previous.text == report.edit.text
                        && report.source_text_sha256 == report.edit.source_sha256
                        && replace_unique_exact(
                            &report.edit.source_document,
                            source_old,
                            source_new,
                        )? == report.edit => {}
                PREVIOUS_ANSWER_EDIT => {
                    let parent = prior_results.get(parent_id).ok_or_else(invalid)?;
                    if !parent
                        .content_plan
                        .as_ref()
                        .is_some_and(|plan| plan.required_units.as_slice() == [plan.text.as_str()])
                        || source.context_sha256 != identity(&Some(parent))?
                    {
                        return Err(invalid());
                    }
                    revalidate_previous_document_render(
                        store,
                        actor,
                        case,
                        parent,
                        None,
                        "PREVIOUS_OUTPUT_INTEGRITY",
                        "source document",
                    )?;
                    let (_, _, replay) = previous_text_replacement(parent, source_old, source_new)?;
                    if replay != report {
                        return Err(invalid());
                    }
                }
                _ => return Err(invalid()),
            }
            replace_retained_text(previous, old, new, Some(&report.edit.text))
        }
        _ => previous_text_replacement(previous, old, new),
    }
}

fn replace_retained_text(
    previous: &ResultRecord,
    old: &str,
    new: &str,
    body: Option<&str>,
) -> Result<(String, String, PreviousTextEditArtifact)> {
    if previous.lifecycle != Lifecycle::Commit {
        return Err(failure(
            "MISSING_ACTUAL_RESULT",
            "the exact previous result is not committed",
        ));
    }
    let plan = previous.content_plan.as_ref().ok_or_else(|| {
        failure(
            "PREVIOUS_OUTPUT_UNAVAILABLE",
            "the previous committed result has no retained content plan",
        )
    })?;
    let render = previous.render.as_ref().ok_or_else(|| {
        failure(
            "PREVIOUS_OUTPUT_UNAVAILABLE",
            "the previous committed result has no actual rendered output",
        )
    })?;
    let mut unsigned_plan = plan.clone();
    unsigned_plan.id.clear();
    if plan.id != minidora_domain::identity(&unsigned_plan)?
        || plan.text != previous.text
        || plan.guarantee != previous.guarantee
        || !crate::core::validate_render(plan, render)
        || plan.required_units.is_empty()
    {
        return Err(failure(
            "PREVIOUS_OUTPUT_INTEGRITY",
            "the committed content plan and its actual rendered bytes do not agree",
        ));
    }
    let source_content_plan_sha256 = digest(&canonical(plan)?);
    let source_render_sha256 = digest(&canonical(render)?);
    let source_text = body.unwrap_or(&plan.text);
    let source_text_sha256 = digest(source_text.as_bytes());
    let edit = replace_unique_exact(source_text, old, new)?;
    Ok((
        source_content_plan_sha256.clone(),
        source_render_sha256.clone(),
        PreviousTextEditArtifact {
            operation: if body.is_some() {
                PREVIOUS_DOCUMENT_EDIT
            } else {
                PREVIOUS_ANSWER_EDIT
            }
            .into(),
            source_result_id: previous.id.clone(),
            source_content_plan_sha256,
            source_render_sha256,
            source_text_sha256,
            edit,
        },
    ))
}

pub(crate) fn previous_text_replacement_lines(artifact: &PreviousTextEditArtifact) -> Vec<String> {
    if artifact.operation == PREVIOUS_DOCUMENT_EDIT {
        return vec![artifact.edit.text.clone()];
    }
    vec![
        "前回の回答で一致した文字列を1箇所置換しました。意味・事実の正確さは検証していません。"
            .into(),
        artifact.edit.text.clone(),
    ]
}

/// An exact output segment from explicit text composition. Material offsets
/// address the decoded JSON string in UTF-8 bytes; JSON offsets address the
/// original request body. Formatting tokens point to their declaring unit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum TextCompositionOrigin {
    MaterialText {
        material_id: String,
        material_json_start: usize,
        material_json_end: usize,
        text_start: usize,
        text_end: usize,
    },
    UnitDirective {
        unit_id: String,
        unit_json_start: usize,
        unit_json_end: usize,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextCompositionSpan {
    pub output_start: usize,
    pub output_end: usize,
    pub text: String,
    pub origin: TextCompositionOrigin,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextCompositionUnit {
    pub id: String,
    pub role: String,
    pub required: bool,
    pub protected: bool,
    pub directive_json_start: usize,
    pub directive_json_end: usize,
    pub output_start: usize,
    pub output_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextCompositionResult {
    pub text: String,
    pub source_document: String,
    pub source_sha256: String,
    pub output_sha256: String,
    pub operation: String,
    pub max_chars: usize,
    pub selected_unit_ids: Vec<String>,
    pub omitted_unit_ids: Vec<String>,
    pub units: Vec<TextCompositionUnit>,
    pub spans: Vec<TextCompositionSpan>,
    pub guarantee: String,
}

#[derive(Clone, Debug)]
struct CompositionMaterial {
    text: String,
    json_span: std::ops::Range<usize>,
}

#[derive(Clone, Debug)]
struct CompositionFragment {
    material_id: String,
    start: usize,
    end: usize,
}

#[derive(Clone, Debug)]
struct CompositionUnit {
    id: String,
    role: String,
    fragments: Vec<CompositionFragment>,
    companions: Vec<String>,
    required: bool,
    protected: bool,
    json_span: std::ops::Range<usize>,
}

fn composition_schema(message: impl Into<String>) -> minidora_domain::Error {
    failure("TEXT_COMPOSITION_SCHEMA", message)
}

fn composition_fields<'a>(
    node: &'a JsonNode,
    required: &[&str],
    label: &str,
) -> Result<&'a BTreeMap<String, JsonNode>> {
    let JsonKind::Object(fields) = &node.kind else {
        return Err(composition_schema(format!("{label} must be an object")));
    };
    let expected: BTreeSet<_> = required.iter().copied().collect();
    let actual: BTreeSet<_> = fields.keys().map(String::as_str).collect();
    if actual != expected {
        return Err(composition_schema(format!(
            "{label} fields must be exactly {}",
            required.join(", ")
        )));
    }
    Ok(fields)
}

fn composition_field<'a>(
    fields: &'a BTreeMap<String, JsonNode>,
    name: &str,
) -> Result<&'a JsonNode> {
    fields
        .get(name)
        .ok_or_else(|| composition_schema(format!("missing field {name}")))
}

fn composition_string(document: &str, node: &JsonNode, label: &str) -> Result<String> {
    if !matches!(node.kind, JsonKind::Scalar) || !document[node.span.clone()].starts_with('"') {
        return Err(composition_schema(format!("{label} must be a string")));
    }
    serde_json::from_str(&document[node.span.clone()])
        .map_err(|_| composition_schema(format!("{label} is not a valid string")))
}

fn composition_array<'a>(node: &'a JsonNode, label: &str) -> Result<&'a [JsonNode]> {
    let JsonKind::Array(values) = &node.kind else {
        return Err(composition_schema(format!("{label} must be an array")));
    };
    Ok(values)
}

fn composition_usize(document: &str, node: &JsonNode, label: &str) -> Result<usize> {
    if !matches!(node.kind, JsonKind::Number) {
        return Err(composition_schema(format!(
            "{label} must be a nonnegative integer"
        )));
    }
    document[node.span.clone()]
        .parse::<usize>()
        .map_err(|_| composition_schema(format!("{label} is out of range or not an integer")))
}

fn composition_bool(document: &str, node: &JsonNode, label: &str) -> Result<bool> {
    match &document[node.span.clone()] {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(composition_schema(format!("{label} must be a boolean"))),
    }
}

fn composition_name(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(composition_schema(format!(
            "{label} must be a nonempty token of at most 128 bytes"
        )));
    }
    Ok(())
}

fn composition_text(value: &str, label: &str) -> Result<()> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r'))
    {
        return Err(composition_schema(format!(
            "{label} is empty or contains an unsupported control character"
        )));
    }
    Ok(())
}

fn visit_composition_unit(
    id: &str,
    units: &BTreeMap<String, usize>,
    declarations: &[CompositionUnit],
    visited: &mut BTreeSet<String>,
    selected: &mut Vec<usize>,
) -> Result<()> {
    let Some(index) = units.get(id).copied() else {
        return Err(composition_schema(format!("unknown unit reference {id}")));
    };
    if !visited.insert(id.to_owned()) {
        return Ok(());
    }
    selected.push(index);
    for companion in &declarations[index].companions {
        visit_composition_unit(companion, units, declarations, visited, selected)?;
    }
    Ok(())
}

fn push_composition_span(
    text: &mut String,
    spans: &mut Vec<TextCompositionSpan>,
    piece: &str,
    origin: TextCompositionOrigin,
) -> Result<()> {
    if piece.is_empty() {
        return Ok(());
    }
    if spans.len() >= 8192 || text.len().saturating_add(piece.len()) > MAX_RESULT_BYTES {
        return Err(failure(
            "TEXT_COMPOSITION_OUTPUT_LIMIT",
            "composition exceeds the output byte or provenance-span budget",
        ));
    }
    let output_start = text.len();
    text.push_str(piece);
    spans.push(TextCompositionSpan {
        output_start,
        output_end: text.len(),
        text: piece.into(),
        origin,
    });
    Ok(())
}

fn append_composition_marker(
    text: &mut String,
    spans: &mut Vec<TextCompositionSpan>,
    piece: &str,
    unit: &CompositionUnit,
) -> Result<()> {
    push_composition_span(
        text,
        spans,
        piece,
        TextCompositionOrigin::UnitDirective {
            unit_id: unit.id.clone(),
            unit_json_start: unit.json_span.start,
            unit_json_end: unit.json_span.end,
        },
    )
}

fn composition_line_ranges(content: &str) -> Vec<(usize, usize)> {
    let bytes = content.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0usize;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
            end += 1;
        }
        if end < bytes.len() {
            if bytes[end] == b'\r' && bytes.get(end + 1) == Some(&b'\n') {
                end += 2;
            } else {
                end += 1;
            }
        }
        ranges.push((start, end));
        start = end;
    }
    if ranges.is_empty() {
        ranges.push((0, content.len()));
    }
    ranges
}

/// Compose caller-selected UTF-8 byte slices from explicit JSON materials.
/// This performs only bounded layout and exact source copying; it does not
/// infer a purpose, select facts, rewrite prose or claim semantic quality.
pub fn compose_explicit_text(document: &str) -> Result<TextCompositionResult> {
    admission(document)?;
    let root = strict_json_document(document)?;
    let root_fields = composition_fields(
        &root,
        &["materials", "units", "order", "max_chars"],
        "composition document",
    )?;
    let max_chars = composition_usize(
        document,
        composition_field(root_fields, "max_chars")?,
        "max_chars",
    )?;
    if !(1..=100_000).contains(&max_chars) {
        return Err(composition_schema("max_chars must be in 1..=100000"));
    }

    let material_rows =
        composition_array(composition_field(root_fields, "materials")?, "materials")?;
    if !(1..=128).contains(&material_rows.len()) {
        return Err(composition_schema("materials must contain 1..=128 entries"));
    }
    let mut materials = BTreeMap::<String, CompositionMaterial>::new();
    for row in material_rows {
        let fields = composition_fields(row, &["id", "text"], "material")?;
        let id = composition_string(document, composition_field(fields, "id")?, "material.id")?;
        composition_name(&id, "material.id")?;
        let text_node = composition_field(fields, "text")?;
        let value = composition_string(document, text_node, "material.text")?;
        composition_text(&value, "material.text")?;
        if materials
            .insert(
                id,
                CompositionMaterial {
                    text: value,
                    json_span: text_node.span.clone(),
                },
            )
            .is_some()
        {
            return Err(composition_schema("material IDs must be unique"));
        }
    }

    let unit_rows = composition_array(composition_field(root_fields, "units")?, "units")?;
    if !(1..=128).contains(&unit_rows.len()) {
        return Err(composition_schema("units must contain 1..=128 entries"));
    }
    let mut declarations = Vec::with_capacity(unit_rows.len());
    let mut units = BTreeMap::<String, usize>::new();
    for row in unit_rows {
        let fields = composition_fields(
            row,
            &["id", "kind", "fragments", "with", "required", "protected"],
            "unit",
        )?;
        let id = composition_string(document, composition_field(fields, "id")?, "unit.id")?;
        composition_name(&id, "unit.id")?;
        let role = composition_string(document, composition_field(fields, "kind")?, "unit.kind")?;
        if !matches!(
            role.as_str(),
            "heading" | "paragraph" | "bullet" | "numbered" | "quote" | "condition" | "caveat"
        ) {
            return Err(composition_schema(
                "unit.kind must be heading, paragraph, bullet, numbered, quote, condition or caveat",
            ));
        }
        let fragment_rows =
            composition_array(composition_field(fields, "fragments")?, "unit.fragments")?;
        if !(1..=32).contains(&fragment_rows.len()) {
            return Err(composition_schema(
                "each unit must contain 1..=32 fragments",
            ));
        }
        let mut fragments = Vec::with_capacity(fragment_rows.len());
        for fragment in fragment_rows {
            let fields = composition_fields(fragment, &["material", "start", "end"], "fragment")?;
            let material_id = composition_string(
                document,
                composition_field(fields, "material")?,
                "fragment.material",
            )?;
            composition_name(&material_id, "fragment.material")?;
            let start = composition_usize(
                document,
                composition_field(fields, "start")?,
                "fragment.start",
            )?;
            let end =
                composition_usize(document, composition_field(fields, "end")?, "fragment.end")?;
            let Some(material) = materials.get(&material_id) else {
                return Err(composition_schema(
                    "fragment.material must name a declared material",
                ));
            };
            if start >= end || material.text.get(start..end).is_none() {
                return Err(composition_schema(
                    "fragment byte range must be nonempty, in bounds and on UTF-8 boundaries",
                ));
            }
            fragments.push(CompositionFragment {
                material_id,
                start,
                end,
            });
        }
        let companion_rows = composition_array(composition_field(fields, "with")?, "unit.with")?;
        if companion_rows.len() > 128 {
            return Err(composition_schema(
                "unit.with may contain at most 128 unit IDs",
            ));
        }
        let mut companions = Vec::with_capacity(companion_rows.len());
        let mut companion_set = BTreeSet::new();
        for companion in companion_rows {
            let value = composition_string(document, companion, "unit.with[]")?;
            composition_name(&value, "unit.with[]")?;
            if !companion_set.insert(value.clone()) {
                return Err(composition_schema("unit.with IDs must be unique"));
            }
            companions.push(value);
        }
        let required = composition_bool(
            document,
            composition_field(fields, "required")?,
            "unit.required",
        )?;
        let protected = composition_bool(
            document,
            composition_field(fields, "protected")?,
            "unit.protected",
        )?;
        let index = declarations.len();
        if units.insert(id.clone(), index).is_some() {
            return Err(composition_schema("unit IDs must be unique"));
        }
        declarations.push(CompositionUnit {
            id,
            role,
            fragments,
            companions,
            required,
            protected,
            json_span: row.span.clone(),
        });
    }
    for unit in &declarations {
        if unit.companions.iter().any(|id| !units.contains_key(id)) {
            return Err(composition_schema("unit.with must name declared units"));
        }
    }

    let order_rows = composition_array(composition_field(root_fields, "order")?, "order")?;
    if !(1..=128).contains(&order_rows.len()) {
        return Err(composition_schema("order must contain 1..=128 unit IDs"));
    }
    let mut order = Vec::with_capacity(order_rows.len());
    let mut order_set = BTreeSet::new();
    for item in order_rows {
        let id = composition_string(document, item, "order[]")?;
        composition_name(&id, "order[]")?;
        if !order_set.insert(id.clone()) {
            return Err(composition_schema("order IDs must be unique"));
        }
        order.push(id);
    }

    let mut selected_indices = Vec::new();
    let mut selected_ids = BTreeSet::new();
    for id in &order {
        visit_composition_unit(
            id,
            &units,
            &declarations,
            &mut selected_ids,
            &mut selected_indices,
        )?;
    }
    for unit in &declarations {
        if unit.required || matches!(unit.role.as_str(), "condition" | "caveat") {
            visit_composition_unit(
                &unit.id,
                &units,
                &declarations,
                &mut selected_ids,
                &mut selected_indices,
            )?;
        }
    }

    #[derive(Clone)]
    struct LocalFragment {
        start: usize,
        end: usize,
        id: String,
        material_span: std::ops::Range<usize>,
        material_start: usize,
    }

    let mut text = String::new();
    let mut spans = Vec::new();
    let mut output_units = Vec::with_capacity(selected_indices.len());
    let mut number = 0usize;
    for (selected_position, unit_index) in selected_indices.iter().copied().enumerate() {
        let unit = &declarations[unit_index];
        if selected_position > 0 {
            append_composition_marker(&mut text, &mut spans, "\n\n", unit)?;
        }
        let output_start = text.len();
        let mut content = String::new();
        let mut local_fragments = Vec::with_capacity(unit.fragments.len());
        for fragment in &unit.fragments {
            let material = &materials[&fragment.material_id];
            let selected_text =
                material
                    .text
                    .get(fragment.start..fragment.end)
                    .ok_or_else(|| {
                        composition_schema("fragment source range changed during composition")
                    })?;
            let start = content.len();
            content.push_str(selected_text);
            local_fragments.push(LocalFragment {
                start,
                end: content.len(),
                id: fragment.material_id.clone(),
                material_span: material.json_span.clone(),
                material_start: fragment.start,
            });
        }
        if content.trim().is_empty() {
            return Err(composition_schema(
                "a unit may not consist only of whitespace",
            ));
        }
        if unit.role == "heading" && content.contains(['\r', '\n']) {
            return Err(composition_schema("heading content must be a single line"));
        }
        number = if unit.role == "numbered" {
            number.saturating_add(1)
        } else {
            0
        };
        let mut local_start = 0usize;
        let lines = composition_line_ranges(&content);
        for (line_index, (line_start, line_end)) in lines.iter().copied().enumerate() {
            let marker = match unit.role.as_str() {
                "heading" if line_index == 0 => "# ".to_owned(),
                "bullet" if line_index == 0 => "- ".to_owned(),
                "numbered" if line_index == 0 => format!("{number}. "),
                "quote" => "> ".to_owned(),
                "condition" if line_index == 0 => "条件: ".to_owned(),
                "caveat" if line_index == 0 => "留保: ".to_owned(),
                "bullet" | "numbered" if line_index > 0 => "  ".to_owned(),
                _ => String::new(),
            };
            append_composition_marker(&mut text, &mut spans, &marker, unit)?;
            for fragment in &local_fragments {
                let start = line_start.max(fragment.start);
                let end = line_end.min(fragment.end);
                if start < end {
                    let material = &materials[&fragment.id];
                    let decoded_start = fragment.material_start + start - fragment.start;
                    let decoded_end = fragment.material_start + end - fragment.start;
                    let piece = content
                        .get(start..end)
                        .ok_or_else(|| composition_schema("internal UTF-8 boundary mismatch"))?;
                    if material.text.get(decoded_start..decoded_end) != Some(piece) {
                        return Err(composition_schema(
                            "source text provenance does not match selected material",
                        ));
                    }
                    push_composition_span(
                        &mut text,
                        &mut spans,
                        piece,
                        TextCompositionOrigin::MaterialText {
                            material_id: fragment.id.clone(),
                            material_json_start: fragment.material_span.start,
                            material_json_end: fragment.material_span.end,
                            text_start: decoded_start,
                            text_end: decoded_end,
                        },
                    )?;
                }
            }
            local_start = line_end;
        }
        if local_start != content.len() {
            return Err(composition_schema(
                "internal line partition did not cover unit content",
            ));
        }
        if text.chars().count() > max_chars {
            return Err(failure(
                "TEXT_COMPOSITION_CHAR_LIMIT",
                "the explicit composition exceeds max_chars; content is retained and no text is truncated",
            ));
        }
        output_units.push(TextCompositionUnit {
            id: unit.id.clone(),
            role: unit.role.clone(),
            required: unit.required || matches!(unit.role.as_str(), "condition" | "caveat"),
            protected: unit.protected
                || matches!(unit.role.as_str(), "quote" | "condition" | "caveat"),
            directive_json_start: unit.json_span.start,
            directive_json_end: unit.json_span.end,
            output_start,
            output_end: text.len(),
        });
    }

    let mut output_cursor = 0usize;
    for span in &spans {
        if span.output_start != output_cursor
            || span.output_end <= span.output_start
            || text.get(span.output_start..span.output_end) != Some(span.text.as_str())
        {
            return Err(failure(
                "TEXT_COMPOSITION_PROVENANCE",
                "composition spans must continuously cover exact output bytes",
            ));
        }
        match &span.origin {
            TextCompositionOrigin::MaterialText {
                material_id,
                material_json_start,
                material_json_end,
                text_start,
                text_end,
            } => {
                let material = materials.get(material_id).ok_or_else(|| {
                    failure(
                        "TEXT_COMPOSITION_PROVENANCE",
                        "span references unknown material",
                    )
                })?;
                if material.json_span.start != *material_json_start
                    || material.json_span.end != *material_json_end
                    || material.text.get(*text_start..*text_end) != Some(span.text.as_str())
                {
                    return Err(failure(
                        "TEXT_COMPOSITION_PROVENANCE",
                        "material provenance does not match exact source text",
                    ));
                }
            }
            TextCompositionOrigin::UnitDirective {
                unit_id,
                unit_json_start,
                unit_json_end,
            } => {
                let Some(unit_index) = units.get(unit_id) else {
                    return Err(failure(
                        "TEXT_COMPOSITION_PROVENANCE",
                        "layout provenance references unknown unit",
                    ));
                };
                let directive = &declarations[*unit_index].json_span;
                if directive.start != *unit_json_start
                    || directive.end != *unit_json_end
                    || document.get(directive.clone()).is_none()
                {
                    return Err(failure(
                        "TEXT_COMPOSITION_PROVENANCE",
                        "layout provenance does not match its explicit directive",
                    ));
                }
            }
        }
        output_cursor = span.output_end;
    }
    if output_cursor != text.len() || text.chars().count() > max_chars {
        return Err(failure(
            "TEXT_COMPOSITION_PROVENANCE",
            "composition has unmapped output or exceeds its character limit",
        ));
    }

    let omitted_unit_ids = declarations
        .iter()
        .filter(|unit| !selected_ids.contains(&unit.id))
        .map(|unit| unit.id.clone())
        .collect();
    let output_sha256 = digest(text.as_bytes());
    let result = TextCompositionResult {
        text,
        source_document: document.into(),
        source_sha256: digest(document.as_bytes()),
        output_sha256,
        operation: "TEXT_COMPOSE_EXPLICIT_FRAGMENTS".into(),
        max_chars,
        selected_unit_ids: output_units.iter().map(|unit| unit.id.clone()).collect(),
        omitted_unit_ids,
        units: output_units,
        spans,
        guarantee: "caller-selected literal material fragments with explicit layout prefixes and byte provenance; no inferred purpose, fact checking, semantic equivalence or arbitrary writing-quality claim".into(),
    };
    let artifact_size = serde_json::to_vec(&result)?.len();
    if artifact_size > 2_000_000 {
        return Err(failure(
            "TEXT_COMPOSITION_ARTIFACT_LIMIT",
            "composition artifact exceeds 2000000 serialized bytes",
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_chart_data_tracks_filtered_missing_and_unit_converted_observations() {
        let document = concat!(
            "地区,期間,距離\r\n",
            "北,前半,1 m\r\n",
            "南,後半,\"\"\r\n",
            "北,後半,-50 cm\r\n",
            "南,後半,3 m\r\n",
            "西,後半, \r\n",
            "南,前半,200 cm\r\n",
        );
        let all = csv_chart_data(document, "地区", "距離", None).unwrap();
        assert_eq!(all.total_rows, 6);
        assert_eq!(all.selected_rows, 6);
        assert_eq!(all.missing_rows, 2);
        assert_eq!(all.unit.as_deref(), Some("m"));
        assert_eq!(all.groups[0].statistics.mean, "1/4");
        assert_eq!(all.groups[1].statistics.mean, "5/2");

        let filtered = csv_chart_data(document, "地区", "距離", Some(("期間", "後半"))).unwrap();
        assert_eq!(filtered.source_sha256, all.source_sha256);
        assert_eq!(filtered.selected_rows, 4);
        assert_eq!(filtered.missing_rows, 2);
        assert_eq!(filtered.unit.as_deref(), Some("m"));
        assert_eq!(filtered.groups.len(), 2);
        assert_eq!(filtered.groups[0].label, "南");
        assert_eq!(filtered.groups[0].source_record_numbers, [4]);
        assert_eq!(filtered.groups[0].statistics.mean, "3");
        assert_eq!(filtered.groups[1].label, "北");
        assert_eq!(filtered.groups[1].source_record_numbers, [3]);
        assert_eq!(filtered.groups[1].statistics.mean, "-1/2");
        assert_eq!(filtered.groups[1].statistics.input_values, ["-50"]);
        assert_eq!(
            filtered.groups[1].statistics.input_units,
            Some(vec!["cm".into()])
        );

        for span in &filtered.source_spans {
            assert_eq!(
                document.get(span.source_start..span.source_end),
                Some(span.source_text.as_str())
            );
        }
        let missing = filtered
            .source_spans
            .iter()
            .find(|span| span.row == Some(2) && span.kind == "CSV_CHART_MISSING_VALUE")
            .unwrap();
        assert_eq!(missing.source_text, "\"\"");
        assert_eq!(missing.decoded_value.as_deref(), Some(""));
        assert_eq!(
            filtered
                .source_spans
                .iter()
                .filter(|span| span.kind == "CSV_CHART_FILTER_DECISION")
                .count(),
            6
        );
    }

    #[test]
    fn csv_chart_data_combines_explicit_yen_labels_without_currency_conversion() {
        let document = "地区,売上\n東,10 円\n東,20 JPY\n西,5円";
        let chart = csv_chart_data(document, "地区", "売上", None).unwrap();
        assert_eq!(chart.unit.as_deref(), Some("JPY"));
        assert_eq!(chart.groups[0].statistics.mean, "15");
        assert_eq!(
            chart.groups[0].statistics.input_units,
            Some(vec!["円".into(), "JPY".into()])
        );
        assert_eq!(chart.groups[1].statistics.mean, "5");
        assert!(
            chart
                .source_spans
                .iter()
                .any(|span| span.source_text == "10 円")
        );

        let error =
            csv_chart_data("地区,売上\n東,10 円\n西,20 CNY", "地区", "売上", None).unwrap_err();
        assert!(
            matches!(error, minidora_domain::Error::Contract { ref code, .. } if code == "CSV_CHART_UNIT_MISMATCH")
        );
        assert!(csv_chart_data("地区,売上\n東,10 ¥", "地区", "売上", None).is_err());
    }

    #[test]
    fn csv_chart_data_declines_incompatible_units_and_empty_observations() {
        use minidora_domain::Error;
        for (document, filter, expected) in [
            (
                "地域,金額\n東,20 kg\n西,21 g",
                None,
                "CSV_CHART_UNIT_MISMATCH",
            ),
            (
                "地域,金額\n東,20 kg\n西,21",
                None,
                "CSV_CHART_UNIT_MISMATCH",
            ),
            (
                "地域,金額\n東,20\n西,21",
                Some(("地域", "北")),
                "CSV_CHART_NO_ROWS",
            ),
            ("地域,金額\n東, \n西,\"\"", None, "CSV_CHART_NO_VALUES"),
        ] {
            let error = csv_chart_data(document, "地域", "金額", filter).unwrap_err();
            assert!(
                matches!(error, Error::Contract { ref code, .. } if code == expected),
                "{error:?}"
            );
        }
    }

    #[test]
    fn japanese_csv_inner_join_keeps_quoted_labels_and_original_byte_spans() {
        let left = "id,name\n2,\"左の値\n右の表:\n続き\"\n右の表:,表の値";
        let right = "record_id,kind\n2,\"右の値\n左の表：\n続き\"\n右の表:,別の値";
        let source = format!("左の表：\r\n{left}\r\n右の表:\n{right}");
        let legacy =
            format!("--- MINIDORA LEFT CSV ---\n{left}\r\n--- MINIDORA RIGHT CSV ---\n{right}");
        let join = |body: &str, expected_parser: &str| {
            let text = format!("CSVを「id」と「record_id」で内部結合して:\n{body}");
            let request = minidora_domain::RequestEnvelope::new("case", "join", "actor", &text);
            let bundle = crate::input::seal(&request, None).unwrap();
            assert_eq!(bundle.request.text, text);
            assert_eq!(bundle.meaning.parser_version, expected_parser);
            let Intent::CsvInnerJoin {
                document,
                left_key_column,
                right_key_column,
            } = &bundle.meaning.intent
            else {
                panic!("expected a CSV inner join")
            };
            assert_eq!(document, body);
            CsvInput::conversation(document)
                .inner_join_by_parser(
                    left_key_column,
                    right_key_column,
                    &bundle.meaning.parser_version,
                )
                .unwrap()
        };
        let report = join(&source, crate::input::CSV_LEFT_RIGHT_JOIN_PARSER_VERSION);
        let legacy_report = join(&legacy, crate::input::PARSER_VERSION);
        assert_eq!(report.text, legacy_report.text);
        assert_eq!(report.source_document, source);
        assert_eq!(report.source_sha256, digest(source.as_bytes()));
        assert_eq!(report.parameters, legacy_report.parameters);
        for span in &report.spans {
            assert_eq!(
                &source[span.source_start..span.source_end],
                span.source_text
            );
        }
        assert!(report.spans.iter().any(|span| {
            span.kind == "CSV_INNER_JOIN_LEFT_CELL"
                && span.source_text == "\"左の値\n右の表:\n続き\""
        }));
    }

    #[test]
    fn japanese_csv_inner_join_holds_missing_tables_or_repeated_labels() {
        for source in [
            "左の表:",
            "左の表:\nid,name\n2,犬",
            "左の表:\n右の表:\nrecord_id,kind\n2,イヌ類",
            "左の表:\nid,name\n2,犬\n右の表:",
            "左の表:\n左の表:\nid,name\n2,犬\n右の表:\nrecord_id,kind\n2,イヌ類",
            "左の表:\nid,name\n2,犬\n右の表:\nrecord_id,kind\n2,イヌ類\n右の表:",
        ] {
            let failure = CsvInput::conversation(source)
                .inner_join_by_parser(
                    "id",
                    "record_id",
                    crate::input::CSV_LEFT_RIGHT_JOIN_PARSER_VERSION,
                )
                .unwrap_err();
            assert!(
                matches!(failure, minidora_domain::Error::Contract { code, .. } if code == "CSV_INNER_JOIN_DELIMITER"),
                "{source:?}"
            );
        }
    }

    #[test]
    fn pasted_text_replacement_requires_an_unambiguous_match_or_explicit_count() {
        for (document, old) in [("猫と猫", "猫"), ("あああ", "ああ")] {
            let error = replace_pasted_text(document, old, "犬", None).unwrap_err();
            assert!(matches!(
                error,
                minidora_domain::Error::Contract { code, .. } if code == "REPLACEMENT_AMBIGUOUS"
            ));
        }
        let source = "Ａ\r\n猫の予定\r\n";
        let report = replace_pasted_text(source, "猫", "犬", None).unwrap();
        assert_eq!(report.text, "Ａ\r\n犬の予定\r\n");
        let span = &report.spans[1];
        assert_eq!(
            (span.kind.as_str(), span.source_start, span.source_end),
            ("EXACT_REPLACEMENT", 5, 8)
        );
        assert_eq!(&source[span.source_start..span.source_end], "猫");

        let source = "猫と猫";
        assert_eq!(
            replace_pasted_text(source, "猫", "犬", Some(2)).unwrap(),
            replace_exact(source, "猫", "犬", Some(2)).unwrap()
        );
        let error = replace_pasted_text(source, "猫", "犬", Some(3)).unwrap_err();
        assert!(matches!(
            error,
            minidora_domain::Error::Contract { code, .. } if code == "REPLACEMENT_COUNT_MISMATCH"
        ));
    }

    #[test]
    fn japanese_csv_comparison_preserves_sealed_source_and_ignores_labels_inside_fields() {
        let before = "id,note\nA,\"旧文\n変更後:\n続き\"\n変更後:,左";
        let after = "id,note\nA,\"新文\n変更前：\n続き\"\n変更後:,右";
        let source = format!("変更前:\r\n{before}\r\n変更後：\n{after}");
        let legacy =
            format!("--- MINIDORA LEFT CSV ---\n{before}\r\n--- MINIDORA RIGHT CSV ---\n{after}");
        let compare = |body: &str, expected_parser: &str| {
            let text = format!("CSVを列「id」で2つの表を比較して:\n{body}");
            let request = minidora_domain::RequestEnvelope::new("case", "compare", "actor", &text);
            let bundle = crate::input::seal(&request, None).unwrap();
            assert_eq!(bundle.request.text, text);
            assert_eq!(bundle.meaning.parser_version, expected_parser);
            let Intent::CsvCompare {
                document,
                key_column,
            } = &bundle.meaning.intent
            else {
                panic!("expected a CSV comparison")
            };
            assert_eq!(document, body);
            CsvInput::conversation(document)
                .compare_by_parser(key_column, &bundle.meaning.parser_version)
                .unwrap()
        };
        let report = compare(
            &source,
            crate::input::CSV_BEFORE_AFTER_COMPARE_PARSER_VERSION,
        );
        let legacy_report = compare(&legacy, crate::input::PARSER_VERSION);
        assert_eq!(report.text, legacy_report.text);
        assert_eq!(report.source_document, source);
        assert_eq!(report.source_sha256, digest(source.as_bytes()));
        assert_eq!(report.parameters, legacy_report.parameters);
        for span in &report.spans {
            assert_eq!(
                &source[span.source_start..span.source_end],
                span.source_text
            );
        }
        assert!(report.spans.iter().any(|span| {
            span.kind == "CSV_COMPARE_LEFT_CELL" && span.source_text == "\"旧文\n変更後:\n続き\""
        }));
    }

    #[test]
    fn japanese_csv_comparison_holds_incomplete_or_repeated_labels() {
        for source in [
            "変更前:",
            "変更前:\nid,note\nA,左",
            "変更前:\n変更後:\nid,note\nA,右",
            "変更前:\nid,note\nA,左\n変更後:\n",
            "変更前:\n変更前:\nid,note\nA,左\n変更後:\nid,note\nA,右",
            "変更前:\nid,note\nA,左\n変更後:\nid,note\nA,右\n変更後:",
        ] {
            let error = CsvInput::conversation(source)
                .compare_by_parser("id", crate::input::CSV_BEFORE_AFTER_COMPARE_PARSER_VERSION)
                .unwrap_err();
            assert!(
                matches!(
                    error,
                    minidora_domain::Error::Contract { code, .. } if code == "CSV_COMPARE_DELIMITER"
                ),
                "{source:?}"
            );
        }
    }

    fn explicit_composition(max_chars: usize) -> String {
        let material = "ねこ\n地元で開催\n雨天中止";
        let title_end = "ねこ".len();
        let event_start = "ねこ\n".len();
        let event_end = event_start + "地元で開催".len();
        let condition_start = "ねこ\n地元で開催\n".len();
        serde_json::json!({
            "materials":[{"id":"source","text":material}],
            "units":[
                {"id":"title","kind":"heading","fragments":[{"material":"source","start":0,"end":title_end}],"with":[],"required":false,"protected":false},
                {"id":"event","kind":"bullet","fragments":[{"material":"source","start":event_start,"end":event_end}],"with":[],"required":false,"protected":false},
                {"id":"weather","kind":"condition","fragments":[{"material":"source","start":condition_start,"end":material.len()}],"with":[],"required":false,"protected":false},
                {"id":"unused","kind":"paragraph","fragments":[{"material":"source","start":0,"end":title_end}],"with":[],"required":false,"protected":false}
            ],
            "order":["title","event"],
            "max_chars":max_chars
        }).to_string()
    }

    #[test]
    fn explicit_text_composition_maps_material_and_layout_bytes() {
        let source = explicit_composition(128);
        let result = compose_explicit_text(&source).unwrap();
        assert_eq!(result.text, "# ねこ\n\n- 地元で開催\n\n条件: 雨天中止");
        assert_eq!(result.selected_unit_ids, ["title", "event", "weather"]);
        assert_eq!(result.omitted_unit_ids, ["unused"]);
        assert_eq!(result.source_document, source);
        assert_eq!(result.source_sha256, digest(source.as_bytes()));
        assert_eq!(result.output_sha256, digest(result.text.as_bytes()));
        assert!(result.units[2].required);
        assert!(result.units[2].protected);
        let mut cursor = 0;
        for span in &result.spans {
            assert_eq!(span.output_start, cursor);
            assert_eq!(
                result.text.get(span.output_start..span.output_end),
                Some(span.text.as_str())
            );
            match &span.origin {
                TextCompositionOrigin::MaterialText {
                    material_id,
                    material_json_start,
                    material_json_end,
                    text_start,
                    text_end,
                } => {
                    assert_eq!(material_id, "source");
                    let raw = &source[*material_json_start..*material_json_end];
                    let material: String = serde_json::from_str(raw).unwrap();
                    assert_eq!(
                        material.get(*text_start..*text_end),
                        Some(span.text.as_str())
                    );
                }
                TextCompositionOrigin::UnitDirective {
                    unit_id,
                    unit_json_start,
                    unit_json_end,
                } => {
                    assert!(matches!(unit_id.as_str(), "title" | "event" | "weather"));
                    assert!(
                        source
                            .get(*unit_json_start..*unit_json_end)
                            .unwrap()
                            .starts_with('{')
                    );
                }
            }
            cursor = span.output_end;
        }
        assert_eq!(cursor, result.text.len());
    }

    #[test]
    fn explicit_text_composition_rejects_ambiguous_or_over_budget_input() {
        let source = explicit_composition(128);
        let mut invalid: serde_json::Value = serde_json::from_str(&source).unwrap();
        invalid["units"][0]["fragments"][0]["start"] = serde_json::json!(1);
        assert_eq!(
            compose_explicit_text(&invalid.to_string())
                .unwrap_err()
                .to_string()
                .split(':')
                .next(),
            Some("TEXT_COMPOSITION_SCHEMA")
        );

        let too_short = explicit_composition(3);
        assert_eq!(
            compose_explicit_text(&too_short)
                .unwrap_err()
                .to_string()
                .split(':')
                .next(),
            Some("TEXT_COMPOSITION_CHAR_LIMIT")
        );

        for invalid_json in [
            r#"{"materials":[],"materials":[],"units":[],"order":[],"max_chars":1}"#,
            r#"{"materials":[],"units":[],"order":[],"max_chars":1,"extra":0}"#,
        ] {
            assert!(compose_explicit_text(invalid_json).is_err());
        }
    }

    #[test]
    fn rfc_quote_and_source_byte_spans() {
        let source = "名前,値,備考\r\n猫,001,\"a,b\"\r\n犬,1.2300e+40,\"二重\"\"引用\r\n続き\"\r\n";
        let table = parse_csv(source, true).unwrap();
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][1].value, "001");
        assert_eq!(table.rows[1][2].value, "二重\"引用\r\n続き");
        for row in &table.rows {
            for cell in row {
                assert!(source.get(cell.source_start..cell.source_end).is_some());
            }
        }
        let selected = csv_select(source, "値").unwrap();
        assert_eq!(selected.text, "[\"001\",\"1.2300e+40\"]");
        assert_eq!(selected.source_document, source);
        assert_eq!(selected.source_sha256, digest(source.as_bytes()));
        assert_eq!(selected.spans[1].source_text, "001");
        assert_eq!(selected.spans[2].decoded_value, Some("1.2300e+40".into()));
    }
    #[test]
    fn malformed_csv_never_uses_a_valid_prefix() {
        for source in [
            "a,b\nx,y",
            "a,b\rx,y",
            "a,\"unterminated",
            "a,b\r\nx\"y,z",
            "a,b\r\n\"x\" trailing,y",
            "a,b\r\nx",
            "a,a\r\nx,y",
            "\"a\",a\r\nx,y",
            "a,b\r\nx,\t",
        ] {
            assert!(parse_csv(source, true).is_err(), "{source:?}");
        }
        assert!(csv_select("a,b\r\n1,2", "missing").is_err());
    }
    #[test]
    fn explicit_headerless_index_and_final_empty_cell() {
        let source = "001,\r\n002,\r\n";
        let table = parse_csv(source, false).unwrap();
        assert_eq!(table.rows.len(), 2);
        assert_eq!(table.rows[0][1].value, "");
        assert_eq!(
            csv_select_index(source, 0, false).unwrap().text,
            "[\"001\",\"002\"]"
        );
        assert!(csv_select_index(source, 2, false).is_err());
        assert_eq!(parse_csv("\"\"", false).unwrap().rows[0][0].value, "");
    }
    #[test]
    fn csv_json_roundtrip_preserves_string_types_and_source() {
        let source = "名前,値\r\n猫,001\r\n犬,\"a,b\"\r\n";
        let json = csv_to_json(source, true).unwrap();
        let back = json_to_csv(&json.text).unwrap();
        assert_eq!(back.text, source);
        assert_eq!(back.source_document, json.text);
        let selected = csv_select(&back.text, "値").unwrap();
        assert_eq!(selected.text, "[\"001\",\"a,b\"]");
        assert!(json.spans.iter().all(|span| span.decoded_value.is_some()));
    }
    #[test]
    fn json_cells_are_strict_strings_with_exact_token_spans() {
        let source = r#"{"rows":[["\u732b","9007199254740993"]],"columns":["name","value"]}"#;
        let exported = json_to_csv(source).unwrap();
        assert_eq!(exported.text, "name,value\r\n猫,9007199254740993\r\n");
        assert!(
            exported
                .spans
                .iter()
                .any(|span| span.source_text == r#""\u732b""#
                    && span.decoded_value.as_deref() == Some("猫"))
        );
        for invalid in [
            r#"{"columns":["a"],"rows":[[3]]}"#,
            r#"{"columns":["a"],"rows":[[true]]}"#,
            r#"{"columns":["a"],"rows":[],"rows":[]}"#,
            r#"{"columns":["a","a"],"rows":[["x","y"]]}"#,
            r#"{"rows":[["x"]]}"#,
            r#"{"columns":["a"],"rows":[["x"]],"extra":0}"#,
            r#"{"columns":["a"],"rows":[["x"]]} trailing"#,
        ] {
            assert!(json_to_csv(invalid).is_err(), "{invalid}");
        }
    }
    #[test]
    fn formula_export_is_withheld_without_changing_original() {
        for value in ["=SUM(1)", "+1", "-2", "@command", "  =x"] {
            let document = serde_json::json!({"columns":["value"],"rows":[[value]]}).to_string();
            assert!(json_to_csv(&document).is_err(), "{value}");
        }
        // Selection as JSON strings is inert data; it does not execute or
        // silently sanitize the supplied spreadsheet-like text.
        assert_eq!(csv_select("value\r\n=x", "value").unwrap().text, "[\"=x\"]");
    }
    #[test]
    fn exact_edit_count_and_unicode_unchanged_regions() {
        let original = "猫の猫";
        let edited = replace_exact(original, "猫", "犬", Some(2)).unwrap();
        assert_eq!(edited.text, "犬の犬");
        assert_eq!(edited.source_document, original);
        assert_eq!(edited.spans[1].kind, "UNCHANGED");
        assert_eq!(edited.spans[1].source_text, "の");
        for span in &edited.spans {
            if span.kind == "UNCHANGED" {
                assert_eq!(
                    &edited.text[span.output_start.unwrap()..span.output_end.unwrap()],
                    span.source_text
                );
            }
        }
        assert!(replace_exact(original, "猫", "犬", Some(1)).is_err());
        assert_eq!(
            replace_exact("aaaa", "aa", "x", Some(2)).unwrap().text,
            "xx"
        );
    }

    #[test]
    fn unique_exact_edit_rejects_missing_duplicate_and_overlapping_literals() {
        let edited = replace_unique_exact("前の文章: 猫です。", "猫", "犬").unwrap();
        assert_eq!(edited.text, "前の文章: 犬です。");
        assert_eq!(edited.operation, "TEXT_REPLACE_EXACT");
        assert_eq!(edited.parameters["actual_count"], "1");
        assert!(
            replace_unique_exact("aaaa", "aaa", "x")
                .unwrap_err()
                .to_string()
                .contains("PREVIOUS_TEXT_AMBIGUOUS")
        );
        assert!(
            replace_unique_exact("猫", "犬", "鳥")
                .unwrap_err()
                .to_string()
                .contains("PREVIOUS_TEXT_NOT_FOUND")
        );
    }
    #[test]
    fn absent_noop_empty_or_expanding_edits_stop() {
        assert!(replace_exact("abc", "", "x", None).is_err());
        assert!(replace_exact("abc", "a", "a", None).is_err());
        assert!(replace_exact("abc", "z", "x", None).is_err());
        assert!(replace_exact("abc", "a", "x", Some(0)).is_err());
        assert!(replace_exact(&"a".repeat(4097), "a", "b", None).is_err());
        assert!(replace_exact("aa", "a", &"x".repeat(128001), None).is_err());
    }
}
