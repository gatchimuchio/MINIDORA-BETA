//! Apply a condition found in attached documents to the original attached table rows.

use crate::{
    attached_document::lookup_field,
    document::{CsvChartData, CsvChartFileSource, CsvInput, csv_chart_data_from_files},
    input::verify_attached_analysis_input,
    language::{decimal_quantity_parts, rational_text},
    xlsx::{self, CellValue, Row},
};
use minidora_domain::{
    AttachmentTableMedia, Error, InputBundle, Intent, RequestAttachment, Result, canonical, digest,
    failure, identity,
};
use num_rational::BigRational;
use num_traits::Zero;
use regex::Regex;
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const MAX_SUM_BITS: u64 = 4_096;
const MAX_PROJECTION_BYTES: usize = 256_000;

pub(crate) struct AnalysisOutcome {
    pub condition_bytes: Vec<u8>,
    pub table_bytes: Vec<u8>,
    pub units: Vec<String>,
}

#[derive(Serialize)]
struct AnalysisTable<'a> {
    kind: &'static str,
    condition_sha256: String,
    source_sha256: String,
    filter_column: &'a str,
    value_column: &'a str,
    filter_value: Option<&'a str>,
    scope: &'static str,
    #[serde(flatten)]
    result: TableResult,
}

#[derive(Serialize)]
struct TableResult {
    status: &'static str,
    total_rows: usize,
    selected_rows: usize,
    missing_rows: usize,
    number_count: usize,
    sum: Option<String>,
    unit: Option<String>,
    sources: Vec<CsvChartFileSource>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    workbooks: Vec<WorkbookReport>,
}

#[derive(Serialize)]
struct WorkbookReport {
    name: String,
    sha256: String,
    sheet: String,
    first_record_number: usize,
    total_rows: usize,
    cells: Vec<WorkbookCell>,
}

#[derive(Serialize)]
struct WorkbookCell {
    address: String,
    row: u32,
    column: u32,
    kind: &'static str,
    value: String,
    value_kind: &'static str,
}

fn deadline_check(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "RESOURCE_LIMIT",
            "添付資料と表の集計が依頼の期限を超えました。",
        ));
    }
    Ok(())
}

pub(crate) fn produce(bundle: &InputBundle, started: Instant) -> Result<AnalysisOutcome> {
    verify_attached_analysis_input(bundle)?;
    let deadline = started
        .checked_add(Duration::from_millis(bundle.request.budget.deadline_ms))
        .ok_or_else(|| failure("RESOURCE_LIMIT", "集計の期限を確定できません。"))?;
    deadline_check(deadline)?;
    let Intent::AnalyzeAttachments {
        label,
        filter_column,
        value_column,
        table_media,
        sheet,
    } = &bundle.meaning.intent
    else {
        return Err(failure(
            "ATTACHED_ANALYSIS_INPUT_MISMATCH",
            "添付資料の集計条件を元の発話と照合できません。",
        ));
    };
    let (tables, documents): (Vec<_>, Vec<_>) =
        bundle.request.attachments.iter().partition(|file| {
            RequestAttachment::is_csv_name(&file.name)
                || RequestAttachment::is_xlsx_name(&file.name)
        });
    if tables.is_empty() {
        let (code, message) = match table_media {
            AttachmentTableMedia::Csv => (
                "ATTACHED_ANALYSIS_CSV_REQUIRED",
                "集計するデータのCSVを添付してください。",
            ),
            AttachmentTableMedia::Xlsx => (
                "ATTACHED_ANALYSIS_XLSX_REQUIRED",
                "集計するデータのExcel（XLSX）を添付してください。",
            ),
            AttachmentTableMedia::Mixed => (
                "ATTACHED_ANALYSIS_TABLE_REQUIRED",
                "集計するデータのCSVまたはExcel（XLSX）を添付してください。",
            ),
        };
        return Err(failure(code, message));
    }
    let csv_count = tables
        .iter()
        .filter(|file| RequestAttachment::is_csv_name(&file.name))
        .count();
    let xlsx_count = tables.len() - csv_count;
    if matches!(
        (*table_media, csv_count, xlsx_count),
        (AttachmentTableMedia::Csv, _, 1..) | (AttachmentTableMedia::Xlsx, 1.., _)
    ) {
        return Err(failure(
            "ATTACHED_ANALYSIS_TABLE_FORMAT",
            "指定した形式と添付表の形式が一致しません。CSVとExcelを両方集計する場合は添付表を指定してください。",
        ));
    }
    if sheet.is_some() && xlsx_count == 0 {
        return Err(failure(
            "ATTACHED_ANALYSIS_XLSX_REQUIRED",
            "指定したシートを含むExcel（XLSX）を添付してください。",
        ));
    }
    if filter_column.text == value_column.text {
        return Err(failure(
            "ATTACHED_ANALYSIS_COLUMNS",
            "絞り込みの列と合計する数値の列を別々に指定してください。",
        ));
    }
    let condition = lookup_field(&documents, &label.text, deadline)?;
    let condition_bytes = canonical(&condition)?;
    let filter = condition
        .value
        .as_deref()
        .map(|value| (filter_column.text.as_str(), value));
    let tables = tables
        .into_iter()
        .map(|file| {
            if RequestAttachment::is_csv_name(&file.name) {
                Ok(TableInput::Csv(file))
            } else {
                workbook_projection(
                    file,
                    sheet.as_ref().map(|span| span.text.as_str()),
                    [&filter_column.text, &value_column.text],
                    deadline,
                )
                .map(TableInput::Workbook)
            }
        })
        .collect::<Result<Vec<_>>>()?;
    let csv = tables
        .iter()
        .map(TableInput::math_input)
        .collect::<Vec<_>>();
    deadline_check(deadline)?;
    let mut result =
        match csv_chart_data_from_files(&csv, &filter_column.text, &value_column.text, filter) {
            Ok(data) => sum_groups(data, deadline)?,
            Err(Error::Contract { code, .. })
                if matches!(code.as_str(), "CSV_CHART_NO_ROWS" | "CSV_CHART_NO_VALUES") =>
            {
                empty_selection(&csv, &value_column.text, filter, &code, deadline)?
            }
            Err(error) => return Err(error),
        };
    let source_sha256 = source_provenance(&tables, &mut result, deadline)?;
    let table = AnalysisTable {
        kind: "minidora.attached-analysis.table.v1",
        condition_sha256: digest(&condition_bytes),
        source_sha256,
        filter_column: &filter_column.text,
        value_column: &value_column.text,
        filter_value: condition.value.as_deref(),
        scope: if filter.is_some() {
            "DOCUMENT_FILTERED"
        } else {
            "ALL_ROWS_WITHOUT_DOCUMENT_CONDITION"
        },
        result,
    };
    let table_bytes = canonical(&table)?;
    let result = &table.result;
    let mut units = condition.units;
    units.push(match filter {
        Some((column, value)) => format!(
            "資料の条件を使い、「{}」が「{}」と完全に一致する行に絞りました。",
            display(column),
            display(value)
        ),
        None if xlsx_count == 0 => {
            "資料の条件を確定できなかったため、絞り込まずに添付CSVの全行を集計しました。".into()
        }
        None => "資料の条件を確定できなかったため、絞り込まずに添付表の全行を集計しました。".into(),
    });
    let row_counts = format!(
        "対象 {} / {}行。数値 {}件、空欄 {}件。",
        result.selected_rows, result.total_rows, result.number_count, result.missing_rows
    );
    if xlsx_count == 0 {
        units.push(format!("添付CSV {csv_count}件。{row_counts}"));
    } else {
        units.push(format!(
            "添付表 {}件（CSV {csv_count}件、Excel {xlsx_count}件）。{row_counts}",
            tables.len()
        ));
        for workbook in &result.workbooks {
            units.push(format!(
                "「{}」のシート「{}」 SHA-256 {}",
                display(&workbook.name),
                display(&workbook.sheet),
                workbook.sha256,
            ));
        }
        units.push(
            "Excelは保存されたセル値で計算しました。表示書式から通貨や日付を推定しません。".into(),
        );
    }
    units.push(match &result.sum {
        Some(sum) => {
            let amount = match &result.unit {
                Some(unit) => format!("{sum} {}", display(unit)),
                None => sum.clone(),
            };
            format!(
                "「{}」の合計は {amount}（数値{}件）です。",
                display(&value_column.text),
                result.number_count
            )
        }
        None if result.status == "NO_MATCH" => {
            "一致する行がなく、合計できる値はありません。".into()
        }
        None if result.status == "NO_ROWS" && xlsx_count == 0 => {
            "添付CSVにデータ行がなく、合計できる値はありません。".into()
        }
        None if result.status == "NO_ROWS" => {
            "添付表にデータ行がなく、合計できる値はありません。".into()
        }
        None => "対象行の指定列はすべて空欄で、合計できる値はありません。".into(),
    });
    let evidence_bytes = units.iter().fold(
        condition_bytes.len().saturating_add(table_bytes.len()),
        |bytes, unit| bytes.saturating_add(unit.len()).saturating_add(1),
    );
    if evidence_bytes > bundle.request.budget.max_evidence_bytes {
        return Err(failure(
            "RESOURCE_LIMIT",
            "添付資料と表を照合した記録が根拠の上限を超えました。",
        ));
    }
    deadline_check(deadline)?;
    Ok(AnalysisOutcome {
        condition_bytes,
        table_bytes,
        units,
    })
}

enum TableInput<'a> {
    Csv(&'a RequestAttachment),
    Workbook(WorkbookProjection<'a>),
}

impl TableInput<'_> {
    fn math_input(&self) -> (&str, &str) {
        match self {
            Self::Csv(file) => (&file.name, &file.text),
            Self::Workbook(sheet) => (sheet.name, &sheet.csv),
        }
    }
}

struct WorkbookProjection<'a> {
    name: &'a str,
    sha256: String,
    sheet: String,
    columns: [u32; 2],
    rows: Vec<ProjectedRow>,
    csv: String,
}

struct ProjectedRow {
    number: u32,
    values: [ProjectedCell; 2],
}

struct ProjectedCell {
    value: String,
    kind: &'static str,
}

fn row_has_data(row: &Row) -> bool {
    row.cells.iter().any(|cell| match &cell.value {
        CellValue::Blank => false,
        CellValue::Text(text) => !text.trim().is_empty(),
        _ => true,
    })
}

fn column_for_header(row: &Row, name: &str) -> Result<u32> {
    let mut candidates = row.cells.iter().filter_map(|cell| match &cell.value {
        CellValue::Text(value) | CellValue::Number(value) if value == name => Some(cell.column),
        _ => None,
    });
    let column = candidates.next().ok_or_else(|| {
        failure(
            "XLSX_COLUMN",
            format!("Excelの表に列「{}」がありません。", display(name)),
        )
    })?;
    if candidates.next().is_some() {
        return Err(failure(
            "XLSX_COLUMN",
            format!("Excelの列名「{}」が重複しています。", display(name)),
        ));
    }
    Ok(column)
}

fn excel_address(mut column: u32, row: u32) -> Result<String> {
    if !(1..=16_384).contains(&column) || !(1..=1_048_576).contains(&row) {
        return Err(invalid_workbook_evidence());
    }
    let mut letters = [0u8; 3];
    let mut start = letters.len();
    while column > 0 {
        column -= 1;
        start -= 1;
        letters[start] = b'A' + (column % 26) as u8;
        column /= 26;
    }
    let letters =
        std::str::from_utf8(&letters[start..]).map_err(|_| invalid_workbook_evidence())?;
    Ok(format!("{letters}{row}"))
}

fn decimal_lexeme(value: &str) -> bool {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN
        .get_or_init(|| {
            Regex::new(r"\A[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?\z")
                .expect("static Excel numeric literal pattern")
        })
        .is_match(value.trim())
}

fn workbook_cell(
    value: Option<CellValue>,
    column: u32,
    row: u32,
    require_numeric: bool,
) -> Result<ProjectedCell> {
    let (value, kind) = match value {
        None => (String::new(), "ABSENT"),
        Some(CellValue::Blank) => (String::new(), "BLANK"),
        Some(CellValue::Text(value)) => {
            if require_numeric && !value.trim().is_empty() {
                let number = decimal_quantity_parts(&value)
                    .map(|(number, _)| number)
                    .or_else(|| value.trim_end().strip_suffix('円'))
                    .unwrap_or(&value);
                if !decimal_lexeme(number) {
                    let address = excel_address(column, row)?;
                    return Err(failure(
                        "XLSX_NUMERIC_CELL",
                        format!("セル {address} は数値や単位付き数値の形式で保存されていません。"),
                    ));
                }
            }
            (value, "TEXT")
        }
        Some(CellValue::Number(value)) if decimal_lexeme(&value) => (value, "NUMBER"),
        Some(CellValue::Number(_)) => {
            let address = excel_address(column, row)?;
            return Err(failure(
                "XLSX_NUMERIC_CELL",
                format!("セル {address} に有効な数値を確認できません。"),
            ));
        }
        Some(CellValue::Formula) => {
            let address = excel_address(column, row)?;
            return Err(failure(
                "XLSX_FORMULA_CELL",
                format!("対象列のセル {address} は数式です。式の計算結果は使用できません。"),
            ));
        }
        Some(CellValue::Unsupported) => {
            let address = excel_address(column, row)?;
            return Err(failure(
                "XLSX_UNSUPPORTED_CELL",
                format!("対象列のセル {address} の値には対応していません。"),
            ));
        }
    };
    Ok(ProjectedCell { value, kind })
}

fn workbook_row(row: Row, columns: [u32; 2], header: bool) -> Result<ProjectedRow> {
    let mut selected = [None, None];
    for cell in row.cells {
        if let Some(index) = columns.iter().position(|&column| column == cell.column) {
            if selected[index].replace(cell.value).is_some() {
                return Err(invalid_workbook_evidence());
            }
        }
    }
    let [group, number] = selected;
    Ok(ProjectedRow {
        number: row.number,
        values: [
            workbook_cell(group, columns[0], row.number, false)?,
            workbook_cell(number, columns[1], row.number, !header)?,
        ],
    })
}

fn append_projection(csv: &mut String, row: &ProjectedRow) -> Result<()> {
    for (index, cell) in row.values.iter().enumerate() {
        if index > 0 {
            csv.push(',');
        }
        // The shared table reader receives exactly the decoded cell string.
        // Quoting preserves commas, line breaks, and literal double quotes.
        csv.push('"');
        for part in cell.value.split_inclusive('"') {
            csv.push_str(part);
            if part.ends_with('"') {
                csv.push('"');
            }
        }
        csv.push('"');
    }
    csv.push('\n');
    if csv.len() > MAX_PROJECTION_BYTES {
        return Err(failure(
            "DOCUMENT_INPUT_LIMIT",
            "Excelの対象列に含まれるデータが集計の上限を超えています。",
        ));
    }
    Ok(())
}

fn workbook_projection<'a>(
    file: &'a RequestAttachment,
    sheet: Option<&str>,
    columns: [&str; 2],
    deadline: Instant,
) -> Result<WorkbookProjection<'a>> {
    deadline_check(deadline)?;
    let raw = file
        .xlsx_bytes()?
        .ok_or_else(|| failure("XLSX_INVALID", "Excelの添付バイトを確認できません。"))?;
    let sha256 = digest(&raw);
    let workbook = xlsx::read(&raw, sheet, deadline)?;
    let mut rows = workbook.rows.into_iter().filter(row_has_data);
    let header = rows
        .next()
        .ok_or_else(|| failure("XLSX_COLUMN", "指定したシートに列名を含む行がありません。"))?;
    let columns = [
        column_for_header(&header, columns[0])?,
        column_for_header(&header, columns[1])?,
    ];
    let mut projection = WorkbookProjection {
        name: &file.name,
        sha256,
        sheet: workbook.name,
        columns,
        rows: Vec::new(),
        csv: String::new(),
    };
    for row in std::iter::once(header).chain(rows) {
        deadline_check(deadline)?;
        let row = workbook_row(row, columns, projection.rows.is_empty())?;
        append_projection(&mut projection.csv, &row)?;
        projection.rows.push(row);
    }
    Ok(projection)
}

impl WorkbookProjection<'_> {
    fn observations(
        &self,
        source: CsvChartFileSource,
        deadline: Instant,
    ) -> Result<WorkbookReport> {
        if source.sha256 != digest(self.csv.as_bytes()) || source.total_rows + 1 != self.rows.len()
        {
            return Err(invalid_workbook_evidence());
        }
        let mut cells = Vec::new();
        let mut seen = BTreeSet::new();
        for span in source.source_spans {
            deadline_check(deadline)?;
            let row_index = span.row.ok_or_else(invalid_workbook_evidence)?;
            let column_index = span.column.ok_or_else(invalid_workbook_evidence)?;
            let row = self
                .rows
                .get(row_index)
                .ok_or_else(invalid_workbook_evidence)?;
            let column = *self
                .columns
                .get(column_index)
                .ok_or_else(invalid_workbook_evidence)?;
            let original = &row.values[column_index];
            if span.decoded_value.as_deref() != Some(original.value.as_str()) {
                return Err(invalid_workbook_evidence());
            }
            let kind = match (row_index, column_index, span.kind.as_str()) {
                (0, 0, _) => "FILTER_COLUMN_HEADER",
                (0, _, _) => "VALUE_COLUMN_HEADER",
                (_, _, "CSV_CHART_FILTER_DECISION" | "CSV_ANALYSIS_FILTER_DECISION") => {
                    "FILTER_DECISION"
                }
                (_, 0, _) => "GROUP_KEY",
                (_, _, "CSV_CHART_MISSING_VALUE" | "CSV_ANALYSIS_MISSING_VALUE") => "MISSING_VALUE",
                _ => "VALUE",
            };
            if seen.insert((row.number, column, kind)) {
                cells.push(WorkbookCell {
                    address: excel_address(column, row.number)?,
                    row: row.number,
                    column,
                    kind,
                    value: original.value.clone(),
                    value_kind: original.kind,
                });
            }
        }
        Ok(WorkbookReport {
            name: self.name.into(),
            sha256: self.sha256.clone(),
            sheet: self.sheet.clone(),
            first_record_number: source.first_record_number,
            total_rows: source.total_rows,
            cells,
        })
    }
}

fn source_provenance(
    tables: &[TableInput<'_>],
    result: &mut TableResult,
    deadline: Instant,
) -> Result<String> {
    if tables
        .iter()
        .all(|table| matches!(table, TableInput::Csv(_)))
    {
        let source_manifest = result
            .sources
            .iter()
            .map(|source| (source.name.as_str(), source.sha256.as_str()))
            .collect::<Vec<_>>();
        return identity(&(
            "minidora.attached-analysis.csv-manifest.v1",
            source_manifest,
        ));
    }
    let sources = std::mem::take(&mut result.sources);
    if sources.len() != tables.len() {
        return Err(invalid_workbook_evidence());
    }
    let mut source_manifest = Vec::with_capacity(sources.len());
    for (table, source) in tables.iter().zip(sources) {
        deadline_check(deadline)?;
        let (expected_name, _) = table.math_input();
        if source.name != expected_name {
            return Err(invalid_workbook_evidence());
        }
        match table {
            TableInput::Csv(_) => {
                source_manifest.push(("CSV", source.name.clone(), source.sha256.clone(), None));
                result.sources.push(source);
            }
            TableInput::Workbook(projection) => {
                let observed = projection.observations(source, deadline)?;
                source_manifest.push((
                    "XLSX",
                    observed.name.clone(),
                    observed.sha256.clone(),
                    Some(observed.sheet.clone()),
                ));
                result.workbooks.push(observed);
            }
        }
    }
    identity(&(
        "minidora.attached-analysis.table-manifest.v2",
        source_manifest,
    ))
}

fn invalid_workbook_evidence() -> Error {
    failure(
        "ATTACHED_ANALYSIS_XLSX_MISMATCH",
        "集計に使った内容とExcelの元のセルを照合できません。",
    )
}

fn sum_groups(data: CsvChartData, deadline: Instant) -> Result<TableResult> {
    let mut sum = BigRational::zero();
    let mut number_count = 0;
    for group in &data.groups {
        deadline_check(deadline)?;
        let exact_sum = group
            .statistics
            .sum
            .parse::<BigRational>()
            .map_err(|_| invalid_table())?;
        sum += exact_sum;
        if sum.numer().bits() > MAX_SUM_BITS || sum.denom().bits() > MAX_SUM_BITS {
            return Err(failure(
                "MATH_NUMBER_LIMIT",
                "合計が正確に扱える数値の上限を超えました。",
            ));
        }
        number_count += group.statistics.count;
    }
    if number_count == 0 || data.selected_rows.checked_sub(data.missing_rows) != Some(number_count)
    {
        return Err(invalid_table());
    }
    Ok(TableResult {
        status: "SUMMED",
        total_rows: data.total_rows,
        selected_rows: data.selected_rows,
        missing_rows: data.missing_rows,
        number_count,
        sum: Some(rational_text(&sum)),
        unit: data.unit,
        sources: data.sources,
        workbooks: Vec::new(),
    })
}

fn empty_selection(
    files: &[(&str, &str)],
    value_column: &str,
    filter: Option<(&str, &str)>,
    reason: &str,
    deadline: Instant,
) -> Result<TableResult> {
    // The shared numeric aggregate reports an empty selection as a distinct
    // error. Retain its exact CSV column observations even without a sum.
    let mut result = TableResult {
        status: match (reason, filter) {
            ("CSV_CHART_NO_ROWS", Some(_)) => "NO_MATCH",
            ("CSV_CHART_NO_ROWS", None) => "NO_ROWS",
            _ => "NO_NUMERIC_VALUES",
        },
        total_rows: 0,
        selected_rows: 0,
        missing_rows: 0,
        number_count: 0,
        sum: None,
        unit: None,
        sources: Vec::with_capacity(files.len()),
        workbooks: Vec::new(),
    };
    for &(name, original) in files {
        deadline_check(deadline)?;
        let csv = original.strip_prefix('\u{feff}').unwrap_or(original);
        let input = CsvInput::conversation(csv);
        let mut source = CsvChartFileSource {
            name: name.to_owned(),
            sha256: digest(original.as_bytes()),
            first_record_number: result.total_rows + 1,
            total_rows: 0,
            source_spans: Vec::new(),
        };
        let mut matched = BTreeSet::new();
        if let Some((column, equals)) = filter {
            for mut span in input.select(column)?.spans {
                if let Some(row @ 1..) = span.row {
                    if span.decoded_value.as_deref() == Some(equals) {
                        matched.insert(row);
                    }
                    span.kind = "CSV_ANALYSIS_FILTER_DECISION".into();
                }
                source.source_spans.push(span);
            }
        }
        for mut span in input.select(value_column)?.spans {
            let row = span.row.ok_or_else(invalid_table)?;
            if row == 0 {
                source.source_spans.push(span);
                continue;
            }
            source.total_rows += 1;
            if filter.is_none() || matched.contains(&row) {
                result.selected_rows += 1;
                if span
                    .decoded_value
                    .as_deref()
                    .is_none_or(|value| !value.trim().is_empty())
                {
                    return Err(invalid_table());
                }
                result.missing_rows += 1;
                span.kind = "CSV_ANALYSIS_MISSING_VALUE".into();
                source.source_spans.push(span);
            }
        }
        let offset = original.len() - csv.len();
        for span in &mut source.source_spans {
            span.source_start += offset;
            span.source_end += offset;
            span.output_start = None;
            span.output_end = None;
        }
        result.total_rows += source.total_rows;
        result.sources.push(source);
    }
    if (reason == "CSV_CHART_NO_ROWS") != (result.selected_rows == 0) {
        return Err(invalid_table());
    }
    Ok(result)
}

fn invalid_table() -> Error {
    failure(
        "ATTACHED_ANALYSIS_TABLE_MISMATCH",
        "記録された数値とCSVの対象行が一致しません。",
    )
}

fn display(value: &str) -> String {
    value
        .chars()
        .map(|c| match c {
            '\r' | '\n' => '↵',
            '\t' => '⇥',
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' => '�',
            _ if c.is_control() => '�',
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbook_and_csv_keep_raw_source_evidence_and_share_exact_scientific_sum() {
        let columns = [27, 4];
        let group = "西,\"特販\"";
        let rows = vec![
            ProjectedRow {
                number: 2,
                values: [
                    workbook_cell(Some(CellValue::Text("区分".into())), 27, 2, false).unwrap(),
                    workbook_cell(Some(CellValue::Text("金額".into())), 4, 2, false).unwrap(),
                ],
            },
            ProjectedRow {
                number: 5,
                values: [
                    workbook_cell(Some(CellValue::Text(group.into())), 27, 5, false).unwrap(),
                    workbook_cell(Some(CellValue::Number("1E-09".into())), 4, 5, true).unwrap(),
                ],
            },
        ];
        let workbook_hash = digest(b"workbook-fixture");
        let mut csv = String::new();
        for row in &rows {
            append_projection(&mut csv, row).unwrap();
        }
        let file = RequestAttachment {
            name: "追加.csv".into(),
            text: "区分,金額\n\"西,\"\"特販\"\"\",0.000000001\n".into(),
            binary_base64: None,
        };
        let tables = [
            TableInput::Workbook(WorkbookProjection {
                name: "期末.xlsx",
                sha256: workbook_hash.clone(),
                sheet: "実績".into(),
                columns,
                rows,
                csv,
            }),
            TableInput::Csv(&file),
        ];
        let data = csv_chart_data_from_files(
            &tables
                .iter()
                .map(TableInput::math_input)
                .collect::<Vec<_>>(),
            "区分",
            "金額",
            Some(("区分", group)),
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut result = sum_groups(data, deadline).unwrap();
        assert_eq!(result.sum.as_deref(), Some("1/500000000"));
        assert_eq!(result.number_count, 2);
        source_provenance(&tables, &mut result, deadline).unwrap();
        assert_eq!(result.sources.len(), 1);
        assert_eq!(result.sources[0].sha256, digest(file.text.as_bytes()));
        let workbook = &result.workbooks[0];
        assert_eq!(workbook.sha256, workbook_hash);
        assert_eq!(workbook.sheet, "実績");
        assert!(workbook.cells.iter().any(|cell| {
            cell.address == "D5" && cell.value == "1E-09" && cell.value_kind == "NUMBER"
        }));
        assert!(workbook.cells.iter().any(|cell| {
            cell.address == "AA5" && cell.value == group && cell.kind == "FILTER_DECISION"
        }));
    }

    #[test]
    fn workbook_rejects_formula_and_date_text_in_requested_cells() {
        assert!(matches!(
            workbook_cell(Some(CellValue::Formula), 2, 7, true),
            Err(Error::Contract { code, .. }) if code == "XLSX_FORMULA_CELL"
        ));
        assert!(matches!(
            workbook_cell(
                Some(CellValue::Text("2026-11-27".into())),
                2,
                7,
                true
            ),
            Err(Error::Contract { code, .. }) if code == "XLSX_NUMERIC_CELL"
        ));
    }
}
