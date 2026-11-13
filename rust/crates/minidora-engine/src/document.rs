//! Source-preserving explicit document operations. CSV uses RFC 4180 record and
//! quote framing in a UTF-8 projection; no numeric inference or spreadsheet
//! evaluation occurs. Editing applies only literal nonoverlapping replacements.
use crate::ports::{JsonKind, JsonNode, strict_json_document};
use minidora_domain::{
    Guarantee, Intent, Lifecycle, Result, ResultRecord, canonical, digest, failure,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
        let recomputed = csv_statistics(&report.source_document, column)?;
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
    if operation == "comparison"
        && !matches!(bundle.meaning.intent, Intent::JsonTableCompare { .. })
    {
        return Ok(None);
    }

    enum Report {
        Comparison(Box<crate::ports::JsonTableComparisonReport>),
        Filter(crate::ports::JsonTableFilterReport),
        Sort(crate::ports::JsonTableSortReport),
        Select(crate::ports::JsonTableSelectReport),
        Project(crate::ports::JsonTableProjectReport),
        Join(Box<crate::ports::JsonTableInnerJoinReport>),
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
pub(crate) struct JsonGroupMeanComparison {
    pub group: String,
    pub mean: String,
    pub highest: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JsonGroupMeanWinner {
    pub group: String,
    pub mean: String,
    pub source_row_count: usize,
    pub sampled_source_row_indices: Vec<usize>,
    pub sampled_group_source_spans: Vec<crate::ports::JsonComparisonSpan>,
    pub sampled_value_source_spans: Vec<crate::ports::JsonComparisonSpan>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JsonGroupMeanAnswer {
    pub source_sha256: String,
    pub group_by: String,
    pub value_column: String,
    pub comparison_unit: Option<String>,
    pub comparisons: Vec<JsonGroupMeanComparison>,
    pub winners: Vec<JsonGroupMeanWinner>,
    pub guarantee: String,
}

/// Revalidate a previous grouped-statistics result, then answer the bounded
/// question using its exact group means. Different units are refused until a
/// cross-group conversion is explicitly implemented and verified.
pub(crate) fn previous_json_table_highest_group_mean(
    store: &crate::store::Store,
    actor: &str,
    case: &str,
    previous: &ResultRecord,
) -> Result<Option<(String, String, JsonGroupMeanAnswer)>> {
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
    let first = report.groups.first().ok_or_else(|| {
        failure(
            "JSON_GROUP_MEAN_CONTEXT_INVALID",
            "the validated grouped-statistics report has no groups",
        )
    })?;
    let comparison_unit = first.statistics.unit.clone();
    if report
        .groups
        .iter()
        .any(|group| group.statistics.unit != comparison_unit)
    {
        return Err(failure(
            "JSON_GROUP_MEAN_UNIT_MISMATCH",
            "group means use different or mixed units; no cross-group conversion was applied",
        ));
    }
    let means: Vec<_> = report
        .groups
        .iter()
        .map(|group| group.statistics.mean.clone())
        .collect();
    let winner_indices = crate::math::maximum_exact_indices(&means)?;
    let winner_set: BTreeSet<_> = winner_indices.iter().copied().collect();
    let comparisons = report
        .groups
        .iter()
        .enumerate()
        .map(|(index, group)| JsonGroupMeanComparison {
            group: group.value.clone(),
            mean: group.statistics.mean.clone(),
            highest: winner_set.contains(&index),
        })
        .collect();
    let winners = winner_indices
        .iter()
        .map(|index| {
            let group = &report.groups[*index];
            let sample_count = group.source_row_indices.len().min(8);
            JsonGroupMeanWinner {
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
    let answer = JsonGroupMeanAnswer {
        source_sha256: report.source_sha256,
        group_by: report.group_by,
        value_column: report.value_column,
        comparison_unit,
        comparisons,
        winners,
        guarantee: "exact comparison of the revalidated per-group means from this supplied JSON string table; no population representativeness or external-world claim".into(),
    };
    Ok(Some((source_ref, report_ref, answer)))
}

pub(crate) fn json_group_mean_answer_lines(answer: &JsonGroupMeanAnswer) -> Vec<String> {
    let unit = answer
        .comparison_unit
        .as_deref()
        .map_or_else(String::new, |unit| format!(" {unit}"));
    let mut lines = vec![
        "前回のJSON表グループ統計を再検証し、全グループの平均を正確に比較しました。".into(),
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
                "  残り{}行の元セル範囲は回答artifactと元資料に保持されています。",
                winner.source_row_count - winner.sampled_source_row_indices.len()
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
fn cell(document: &str, at: &mut usize) -> Result<CsvCell> {
    let bytes = document.as_bytes();
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
        if !matches!(input_byte(bytes, *at), None | Some(b',' | b'\r')) {
            return Err(failure(
                "CSV_QUOTES",
                "only a comma, CRLF or EOF may follow a closing quote",
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
    admission(document)?;
    let bytes = document.as_bytes();
    let mut at = 0usize;
    let mut rows = Vec::new();
    let mut cells = 0usize;
    loop {
        let mut row = Vec::new();
        loop {
            row.push(cell(document, &mut at)?);
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
    Ok(CsvTable {
        source_document: document.into(),
        source_sha256: digest(document.as_bytes()),
        header,
        rows,
    })
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
    let table = parse_csv(document, true)?;
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
    let table = parse_csv(document, true)?;
    let header = table.header.as_ref().expect("explicit CSV header");
    let index = header
        .iter()
        .position(|cell| cell.value == column)
        .ok_or_else(|| failure("CSV_COLUMN", "named column does not exist"))?;
    let mut values = Vec::with_capacity(table.rows.len());
    let mut units = Vec::with_capacity(table.rows.len());
    let mut quantity_count = 0;
    let mut spans = vec![source_span(
        document,
        &header[index],
        "CSV_HEADER",
        Some(0),
        index,
        None,
    )];
    for (row_index, row) in table.rows.iter().enumerate() {
        let cell = &row[index];
        if let Some((number, unit)) = crate::language::decimal_quantity_parts(&cell.value) {
            quantity_count += 1;
            values.push(number.to_string());
            units.push(unit.to_string());
        } else {
            values.push(cell.value.clone());
            units.push(String::new());
        }
        spans.push(source_span(
            document,
            cell,
            "CSV_NUMERIC_OBSERVATION",
            Some(row_index + 1),
            index,
            None,
        ));
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
    result(document, text, "CSV_STATISTICS", parameters, spans)
}

pub fn csv_select_index(document: &str, index: usize, has_header: bool) -> Result<DocumentResult> {
    select(parse_csv(document, has_header)?, index, None)
}

/// Export an explicitly string-typed table JSON. Numeric-looking cells remain
/// JSON strings, including leading zeros, exponent spelling and huge integers.
pub fn csv_to_json(document: &str, has_header: bool) -> Result<DocumentResult> {
    let table = parse_csv(document, has_header)?;
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
    let table = parse_csv(document, true)?;
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
    let table = parse_csv(document, true)?;
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
    let table = parse_csv(document, true)?;
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
    let table = parse_csv(document, true)?;
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
    struct GroupReport {
        value: String,
        source_record_numbers: Vec<usize>,
        statistics: crate::math::Statistics,
    }
    #[derive(Serialize)]
    struct Report<'a> {
        source_sha256: String,
        group_column: &'a str,
        value_column: &'a str,
        groups: &'a [GroupReport],
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
        reports.push(GroupReport {
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

/// Compare two explicitly supplied, headered CSV tables by an exact unique
/// string key. The input body uses reserved marker lines to keep both original
/// tables in one source document:
/// `--- MINIDORA LEFT CSV ---\n<left CSV>\r\n--- MINIDORA RIGHT CSV ---\n<right CSV>`.
/// Marker text is reserved and must not occur inside either CSV payload.
pub fn csv_compare_by(document: &str, key_column: &str) -> Result<DocumentResult> {
    const LEFT_MARKER: &str = "--- MINIDORA LEFT CSV ---\n";
    const RIGHT_DELIMITER: &str = "\r\n--- MINIDORA RIGHT CSV ---\n";
    admission(document)?;
    if key_column.is_empty() {
        return Err(failure("CSV_COLUMN", "key column name must not be empty"));
    }
    if !document.starts_with(LEFT_MARKER)
        || document.matches(LEFT_MARKER).count() != 1
        || document.matches(RIGHT_DELIMITER).count() != 1
    {
        return Err(failure(
            "CSV_COMPARE_DELIMITER",
            "expected one left marker and one CRLF-prefixed right marker; marker text is reserved",
        ));
    }
    let left_start = LEFT_MARKER.len();
    let delimiter_start = document
        .find(RIGHT_DELIMITER)
        .ok_or_else(|| failure("CSV_COMPARE_DELIMITER", "right marker is missing"))?;
    let right_start = delimiter_start + RIGHT_DELIMITER.len();
    let left_source = &document[left_start..delimiter_start];
    let right_source = &document[right_start..];
    if left_source.contains("--- MINIDORA LEFT CSV ---")
        || left_source.contains("--- MINIDORA RIGHT CSV ---")
        || right_source.contains("--- MINIDORA RIGHT CSV ---")
        || right_source.contains("--- MINIDORA LEFT CSV ---")
    {
        return Err(failure(
            "CSV_COMPARE_DELIMITER",
            "reserved comparison marker text occurs more than once",
        ));
    }

    let mut left = parse_csv(left_source, true)?;
    let mut right = parse_csv(right_source, true)?;
    let shift_table = |table: &mut CsvTable, offset: usize| {
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
        table.source_sha256 = digest(document.as_bytes());
    };
    shift_table(&mut left, left_start);
    shift_table(&mut right, right_start);

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
    const LEFT_MARKER: &str = "--- MINIDORA LEFT CSV ---\n";
    const RIGHT_DELIMITER: &str = "\r\n--- MINIDORA RIGHT CSV ---\n";
    admission(document)?;
    if left_key_column.is_empty() || right_key_column.is_empty() {
        return Err(failure(
            "CSV_INNER_JOIN_COLUMN",
            "both named key columns must not be empty",
        ));
    }
    if !document.starts_with(LEFT_MARKER)
        || document.matches(LEFT_MARKER).count() != 1
        || document.matches(RIGHT_DELIMITER).count() != 1
    {
        return Err(failure(
            "CSV_INNER_JOIN_DELIMITER",
            "expected one left marker and one CRLF-prefixed right marker; marker text is reserved",
        ));
    }
    let left_start = LEFT_MARKER.len();
    let delimiter_start = document
        .find(RIGHT_DELIMITER)
        .ok_or_else(|| failure("CSV_INNER_JOIN_DELIMITER", "right marker is missing"))?;
    let right_start = delimiter_start + RIGHT_DELIMITER.len();
    let left_source = &document[left_start..delimiter_start];
    let right_source = &document[right_start..];
    if left_source.contains("--- MINIDORA LEFT CSV ---")
        || left_source.contains("--- MINIDORA RIGHT CSV ---")
        || right_source.contains("--- MINIDORA RIGHT CSV ---")
        || right_source.contains("--- MINIDORA LEFT CSV ---")
    {
        return Err(failure(
            "CSV_INNER_JOIN_DELIMITER",
            "reserved comparison marker text occurs more than once",
        ));
    }

    let mut left = parse_csv(left_source, true)?;
    let mut right = parse_csv(right_source, true)?;
    let shift_table = |table: &mut CsvTable, offset: usize| {
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
        table.source_sha256 = digest(document.as_bytes());
    };
    shift_table(&mut left, left_start);
    shift_table(&mut right, right_start);
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

pub(crate) fn previous_text_replacement(
    previous: &ResultRecord,
    old: &str,
    new: &str,
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
        || render.status != "RENDERED"
        || render.plan_id != plan.id
        || render.bytes != plan.text.as_bytes()
        || render.actual_sha256 != digest(&render.bytes)
        || plan.required_units.is_empty()
    {
        return Err(failure(
            "PREVIOUS_OUTPUT_INTEGRITY",
            "the committed content plan and its actual rendered bytes do not agree",
        ));
    }
    let source_content_plan_sha256 = digest(&canonical(plan)?);
    let source_render_sha256 = digest(&canonical(render)?);
    let source_text_sha256 = digest(plan.text.as_bytes());
    let edit = replace_unique_exact(&plan.text, old, new)?;
    Ok((
        source_content_plan_sha256.clone(),
        source_render_sha256.clone(),
        PreviousTextEditArtifact {
            operation: "PREVIOUS_OUTPUT_REPLACE_EXACT_V1".into(),
            source_result_id: previous.id.clone(),
            source_content_plan_sha256,
            source_render_sha256,
            source_text_sha256,
            edit,
        },
    ))
}

pub(crate) fn previous_text_replacement_lines(artifact: &PreviousTextEditArtifact) -> Vec<String> {
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
