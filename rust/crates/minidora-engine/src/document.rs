//! Source-preserving explicit document operations. CSV uses RFC 4180 record and
//! quote framing in a UTF-8 projection; no numeric inference or spreadsheet
//! evaluation occurs. Editing applies only literal nonoverlapping replacements.
use minidora_domain::{Result, digest, failure};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

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
    static QUANTITY: OnceLock<Regex> = OnceLock::new();
    let quantity = QUANTITY.get_or_init(|| {
        Regex::new(
            r"^\s*([+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?)\s*([A-Za-zμµΩ°%][A-Za-z0-9μµΩ°%*/^._-]{0,63})\s*$",
        )
        .expect("constant CSV quantity pattern")
    });
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
        if let Some(capture) = quantity.captures(&cell.value) {
            quantity_count += 1;
            values.push(capture[1].trim().to_string());
            units.push(capture[2].to_string());
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
    for (index, cell) in header.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        append_json_cell(
            &mut text,
            document,
            cell,
            "CSV_FILTER_HEADER",
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
                "CSV_FILTER_CELL",
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
    if left_source.contains("--- MINIDORA RIGHT CSV ---")
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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn absent_noop_empty_or_expanding_edits_stop() {
        assert!(replace_exact("abc", "", "x", None).is_err());
        assert!(replace_exact("abc", "a", "a", None).is_err());
        assert!(replace_exact("abc", "z", "x", None).is_err());
        assert!(replace_exact("abc", "a", "x", Some(0)).is_err());
        assert!(replace_exact(&"a".repeat(4097), "a", "b", None).is_err());
        assert!(replace_exact("aa", "a", &"x".repeat(128001), None).is_err());
    }
}
