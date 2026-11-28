//! Compile a sealed local CSV aggregation into a closed WASI program.
mod code;

use std::collections::BTreeMap;

use minidora_domain::{CsvAggregationOperation, InputBundle, Intent, Result, digest, failure};
use serde::{Deserialize, Serialize};

use crate::wasm_execution::ExecutionProgram;

pub(crate) const ARTIFACT_NAME: &str = "summary.csv";
pub(crate) const VALUE_COLUMN: &str = "集計値";
const MAX_ROWS: usize = 4096;
const MAX_GROUPS: usize = 48;
const MAX_LABEL_BYTES: usize = 128;
const MAX_PACKET_BYTES: usize = 65536;
const HEADER_BYTES: usize = 16;
const DESCRIPTOR_BYTES: usize = 8;
const ROW_BYTES: usize = 12;
const ROWS_OFFSET: usize = HEADER_BYTES + DESCRIPTOR_BYTES * MAX_GROUPS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CsvAggregationProof {
    compiler: String,
    source_name: String,
    source_sha256: String,
    group_column: String,
    operation: String,
    value_column: Option<String>,
    output_column: String,
    artifact_name: String,
    rows: usize,
    groups: usize,
    module_sha256: String,
    normalized_input_sha256: String,
}

fn invalid(code: &'static str, message: &'static str) -> minidora_domain::Error {
    failure(code, message)
}

fn label_valid(label: &str) -> bool {
    let label_value = label.trim_start();
    !label_value.is_empty()
        && label.len() <= MAX_LABEL_BYTES
        && !label.chars().any(char::is_control)
        && (!label_value.starts_with(['=', '+', '-', '@']) || label_value.parse::<i64>().is_ok())
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.into()
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 15) as usize] as char);
    }
    encoded
}

fn store_u32(bytes: &mut [u8], at: usize, value: usize) {
    bytes[at..at + 4].copy_from_slice(&(value as u32).to_le_bytes());
}

pub(crate) fn compile(bundle: &InputBundle) -> Result<(ExecutionProgram, CsvAggregationProof)> {
    crate::input::verify_csv_aggregation_input(bundle)?;
    let Intent::ExecuteAttachedCsvAggregation {
        group_column,
        operation,
    } = &bundle.meaning.intent
    else {
        return Err(invalid(
            "CSV_WASM_SOURCE",
            "添付CSVの集計指定を照合できません。",
        ));
    };
    let [source] = bundle.request.attachments.as_slice() else {
        return Err(invalid(
            "CSV_WASM_SOURCE",
            "集計するCSVを1件添付してください。",
        ));
    };
    let group = &group_column.text;
    if !label_valid(group) || group == VALUE_COLUMN {
        return Err(invalid(
            "CSV_WASM_GROUP_COLUMN",
            "区分列には「集計値」と異なる有効な列名を指定してください。",
        ));
    }
    let table = crate::document::parse_attached_csv(&source.text)?;
    let header = table
        .header
        .as_ref()
        .ok_or_else(|| invalid("CSV_WASM_HEADER", "CSVの最初の行に列名が必要です。"))?;
    let column = |name: &str| -> Result<usize> {
        let mut found = header.iter().enumerate().filter(|(index, cell)| {
            let text = if *index == 0 {
                cell.value.strip_prefix('\u{feff}').unwrap_or(&cell.value)
            } else {
                &cell.value
            };
            text == name
        });
        let index = found.next().map(|(index, _)| index);
        if let Some(index) = index
            && found.next().is_none()
        {
            return Ok(index);
        }
        Err(invalid(
            "CSV_WASM_COLUMN",
            "指定した列をCSV内の一つの列に照合できません。",
        ))
    };
    let group_index = column(group)?;
    let value_column = match operation {
        CsvAggregationOperation::Count => None,
        CsvAggregationOperation::Sum { value_column } => Some(value_column.text.as_str()),
    };
    let value_index = value_column.map(column).transpose()?;
    if table.rows.is_empty() || table.rows.len() > MAX_ROWS {
        return Err(invalid(
            "CSV_WASM_ROW_LIMIT",
            "分離集計で使えるデータ行は1〜4096件です。",
        ));
    }
    // The packet contains one independent record per source row. No total is computed here.
    let mut packet = vec![0u8; ROWS_OFFSET];
    let mut groups = BTreeMap::new();
    let mut labels = Vec::new();
    for row in &table.rows {
        let name = row
            .get(group_index)
            .map(|cell| cell.value.as_str())
            .filter(|name| label_valid(name))
            .ok_or_else(|| {
                invalid(
                    "CSV_WASM_GROUP",
                    "区分列に空欄または対応範囲外の値があります。",
                )
            })?;
        let index = if let Some(index) = groups.get(name) {
            *index
        } else {
            let index = labels.len();
            if index == MAX_GROUPS {
                return Err(invalid(
                    "CSV_WASM_GROUP_LIMIT",
                    "分離集計で使える区分は48件までです。",
                ));
            }
            labels.push(name);
            groups.insert(name, index);
            index
        };
        let number = if let Some(value_index) = value_index {
            row.get(value_index)
                .and_then(|cell| cell.value.trim().parse::<i64>().ok())
                .ok_or_else(|| {
                    invalid(
                        "CSV_WASM_INTEGER",
                        "合計する列には空欄のない符号付き64ビット整数を指定してください。",
                    )
                })?
        } else {
            0
        };
        packet.extend_from_slice(&(index as u32).to_le_bytes());
        packet.extend_from_slice(&number.to_le_bytes());
    }
    let output_header = format!("{},{}\n", csv_field(group), VALUE_COLUMN);
    packet.extend_from_slice(output_header.as_bytes());
    for (index, label) in labels.iter().enumerate() {
        let offset = packet.len();
        let prefix = format!("{},", csv_field(label));
        packet.extend_from_slice(prefix.as_bytes());
        store_u32(&mut packet, HEADER_BYTES + index * DESCRIPTOR_BYTES, offset);
        store_u32(
            &mut packet,
            HEADER_BYTES + index * DESCRIPTOR_BYTES + 4,
            prefix.len(),
        );
    }
    let packet_len = packet.len();
    if packet_len > MAX_PACKET_BYTES {
        return Err(invalid(
            "CSV_WASM_INPUT_LIMIT",
            "集計に渡すCSVデータが分離実行の入力上限を超えました。",
        ));
    }
    store_u32(&mut packet, 0, packet_len);
    store_u32(&mut packet, 4, table.rows.len());
    store_u32(&mut packet, 8, labels.len());
    store_u32(&mut packet, 12, output_header.len());

    let module = code::module(value_index.is_some());
    let proof = CsvAggregationProof {
        compiler: "csv-wasi-i64-aggregation-v2".into(),
        source_name: source.name.clone(),
        source_sha256: digest(source.text.as_bytes()),
        group_column: group.clone(),
        operation: if value_index.is_some() {
            "SUM"
        } else {
            "COUNT"
        }
        .into(),
        value_column: value_column.map(str::to_owned),
        output_column: VALUE_COLUMN.into(),
        artifact_name: ARTIFACT_NAME.into(),
        rows: table.rows.len(),
        groups: labels.len(),
        module_sha256: digest(&module),
        normalized_input_sha256: digest(&packet),
    };
    Ok((
        ExecutionProgram {
            module_hex: hex(&module),
            stdin_hex: hex(&packet),
            files: vec![ARTIFACT_NAME.into()],
        },
        proof,
    ))
}
