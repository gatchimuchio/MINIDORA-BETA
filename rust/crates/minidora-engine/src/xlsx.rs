//! Read one worksheet from an in-memory OOXML workbook.

use minidora_domain::{Error, MAX_BINARY_ATTACHMENT_BYTES, Result, failure};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::time::Instant;
use zip::{CompressionMethod, ZipArchive};

const MAX_ENTRIES: usize = 512;
const MAX_MANIFEST_BYTES: usize = 256_000;
const MAX_SHEET_BYTES: usize = 2 * 1024 * 1024;
const MAX_STRINGS_BYTES: usize = 1024 * 1024;
const MAX_PACKAGE_XML_BYTES: usize = 4 * 1024 * 1024;
const MAX_XML_NODES: u32 = 225_000;
const MAX_XML_DEPTH: usize = 48;
const MAX_WORKBOOK_SHEETS: usize = 128;
const MAX_SHARED_STRINGS: usize = 32_768;
const MAX_ROWS: usize = 4_096;
const MAX_CELLS: usize = 65_536;
const MAX_FORMULA_RANGES: usize = 1_024;
const MAX_CELL_TEXT: usize = 131_068;
const MAX_ALL_CELL_TEXT: usize = 2 * 1024 * 1024;
const EXCEL_MAX_ROW: u32 = 1_048_576;
const EXCEL_MAX_COLUMN: u32 = 16_384;

const SPREADSHEET_NAMESPACES: [&str; 2] = [
    "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
    "http://purl.oclc.org/ooxml/spreadsheetml/main",
];
const OFFICE_REL_NAMESPACES: [&str; 2] = [
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "http://purl.oclc.org/ooxml/officeDocument/relationships",
];
const PACKAGE_RELS: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const CONTENT_TYPES: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const WORKBOOK_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const WORKSHEET_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Sheet {
    pub name: String,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Row {
    pub number: u32,
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Cell {
    pub column: u32,
    pub value: CellValue,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CellValue {
    Text(String),
    Number(String),
    Blank,
    Formula,
    Unsupported,
}

fn invalid() -> Error {
    failure(
        "XLSX_INVALID",
        "XLSXのZIP構造、部品の参照、またはセルの位置を確認できません。",
    )
}

fn resource_limit() -> Error {
    failure(
        "XLSX_RESOURCE_LIMIT",
        "XLSXの容量、部品数、行数、またはセル数が上限を超えました。",
    )
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "XLSX_DEADLINE",
            "XLSXの読み取りが依頼の期限を超えました。",
        ));
    }
    Ok(())
}

struct Package<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    bytes_left: usize,
    deadline: Instant,
}

impl<'a> Package<'a> {
    fn new(bytes: &'a [u8], deadline: Instant) -> Result<Self> {
        check_deadline(deadline)?;
        if bytes.len() > MAX_BINARY_ATTACHMENT_BYTES {
            return Err(resource_limit());
        }
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err(invalid());
        }
        let mut archive = ZipArchive::new(Cursor::new(bytes)).map_err(|_| invalid())?;
        if archive.len() > MAX_ENTRIES {
            return Err(resource_limit());
        }
        if archive.has_duplicate_names()
            || archive.has_overlapping_entries().map_err(|_| invalid())?
        {
            return Err(invalid());
        }
        let mut names = BTreeSet::new();
        for index in 0..archive.len() {
            check_deadline(deadline)?;
            let entry = archive.by_index_raw(index).map_err(|_| invalid())?;
            let name = entry.name().map_err(|_| invalid())?;
            let part = if entry.is_dir() {
                name.strip_suffix('/').ok_or_else(invalid)?
            } else {
                &name
            };
            if entry.encrypted()
                || entry.is_symlink()
                || !valid_part(part)
                || !names.insert(part.to_lowercase())
            {
                return Err(invalid());
            }
        }
        Ok(Self {
            archive,
            bytes_left: MAX_PACKAGE_XML_BYTES,
            deadline,
        })
    }

    fn xml(&mut self, name: &str, part_limit: usize) -> Result<String> {
        check_deadline(self.deadline)?;
        let limit = self.bytes_left.min(part_limit);
        let mut entry = self.archive.by_name(name).map_err(|_| invalid())?;
        if !entry.is_file()
            || !matches!(
                entry.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
        {
            return Err(invalid());
        }
        if entry.size() > limit as u64 {
            return Err(resource_limit());
        }
        let expected_len = entry.size() as usize;
        let mut data = Vec::with_capacity(expected_len);
        let mut buffer = [0u8; 8_192];
        loop {
            check_deadline(self.deadline)?;
            let count = entry.read(&mut buffer).map_err(|_| invalid())?;
            if count == 0 {
                break;
            }
            if count > limit - data.len() {
                return Err(resource_limit());
            }
            data.extend_from_slice(&buffer[..count]);
        }
        if data.len() != expected_len {
            return Err(invalid());
        }
        self.bytes_left -= data.len();
        String::from_utf8(data).map_err(|_| invalid())
    }
}

fn valid_uri_spelling(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 1024
        && !path.contains(['\\', ':', '?', '#', '%'])
        && !path.chars().any(|c| c.is_control() || c.is_whitespace())
}

fn valid_part(path: &str) -> bool {
    valid_uri_spelling(path) && path.split('/').all(|p| !matches!(p, "" | "." | ".."))
}

fn resolve_part(owner: &str, target: &str) -> Result<String> {
    if !valid_uri_spelling(target) {
        return Err(invalid());
    }
    let mut components = Vec::new();
    let target = if let Some(absolute) = target.strip_prefix('/') {
        absolute
    } else {
        if let Some((directory, _)) = owner.rsplit_once('/') {
            components.extend(directory.split('/'));
        }
        target
    };
    for part in target.split('/') {
        match part {
            "" => return Err(invalid()),
            "." => {}
            ".." => {
                components.pop().ok_or_else(invalid)?;
            }
            _ => components.push(part),
        }
    }
    let path = components.join("/");
    if !valid_part(&path) {
        return Err(invalid());
    }
    Ok(path)
}

fn relationships_part(owner: &str) -> String {
    match owner.rsplit_once('/') {
        Some((directory, filename)) => format!("{directory}/_rels/{filename}.rels"),
        None => format!("_rels/{owner}.rels"),
    }
}

fn parse_xml(xml: &str, deadline: Instant) -> Result<Document<'_>> {
    check_deadline(deadline)?;
    // The reader handles the standard escapes and has no external entity resolver.
    // Reject even an otherwise empty DTD declaration.
    if xml.contains("<!DOCTYPE") {
        return Err(invalid());
    }
    let options = ParsingOptions {
        nodes_limit: MAX_XML_NODES,
        ..ParsingOptions::default()
    };
    let xml = xml.strip_prefix('\u{feff}').unwrap_or(xml);
    let document = Document::parse_with_options(xml, options).map_err(|error| {
        if matches!(error, roxmltree::Error::NodesLimitReached) {
            resource_limit()
        } else {
            invalid()
        }
    })?;
    for (index, node) in document.descendants().enumerate() {
        if index % 256 == 0 {
            check_deadline(deadline)?;
        }
        if node.ancestors().take(MAX_XML_DEPTH + 1).count() > MAX_XML_DEPTH {
            return Err(resource_limit());
        }
    }
    Ok(document)
}

fn is_element(node: Node<'_, '_>, namespace: &str, name: &str) -> bool {
    node.is_element()
        && node.tag_name().namespace() == Some(namespace)
        && node.tag_name().name() == name
}

fn spreadsheet_root<'a, 'xml>(doc: &'a Document<'xml>, name: &str) -> Result<Node<'a, 'xml>> {
    let root = doc.root_element();
    if !SPREADSHEET_NAMESPACES
        .iter()
        .any(|ns| is_element(root, ns, name))
    {
        return Err(invalid());
    }
    Ok(root)
}

fn in_spreadsheet(node: Node<'_, '_>, parent: Node<'_, '_>, name: &str) -> bool {
    node.is_element()
        && node.tag_name().namespace() == parent.tag_name().namespace()
        && node.tag_name().name() == name
}

fn child<'a, 'xml>(parent: Node<'a, 'xml>, name: &str) -> Result<Option<Node<'a, 'xml>>> {
    let mut matches = parent
        .children()
        .filter(|&node| in_spreadsheet(node, parent, name));
    let first = matches.next();
    if matches.next().is_some() {
        return Err(invalid());
    }
    Ok(first)
}

struct Relationship {
    kind: String,
    target: Option<String>,
}

impl Relationship {
    fn local_target(&self) -> Result<&str> {
        self.target.as_deref().ok_or_else(|| {
            failure(
                "XLSX_EXTERNAL_RELATIONSHIP",
                "XLSXの外部参照先からは読み取りません。",
            )
        })
    }
}

fn read_relationships(
    package: &mut Package<'_>,
    part: &str,
    owner: &str,
) -> Result<BTreeMap<String, Relationship>> {
    let xml = package.xml(part, MAX_MANIFEST_BYTES)?;
    let doc = parse_xml(&xml, package.deadline)?;
    let root = doc.root_element();
    if !is_element(root, PACKAGE_RELS, "Relationships") {
        return Err(invalid());
    }
    let mut relationships = BTreeMap::new();
    for rel in root.children().filter(Node::is_element) {
        if !is_element(rel, PACKAGE_RELS, "Relationship") {
            return Err(invalid());
        }
        let id = rel
            .attribute("Id")
            .filter(|id| !id.is_empty())
            .ok_or_else(invalid)?;
        let kind = rel.attribute("Type").ok_or_else(invalid)?;
        let kind = OFFICE_REL_NAMESPACES
            .iter()
            .find_map(|ns| kind.strip_prefix(ns).and_then(|end| end.strip_prefix('/')))
            .unwrap_or("");
        let target = rel.attribute("Target").ok_or_else(invalid)?;
        let target = match rel.attribute("TargetMode") {
            None | Some("Internal") => Some(resolve_part(owner, target)?),
            Some("External") => None,
            _ => return Err(invalid()),
        };
        let relationship = Relationship {
            kind: kind.into(),
            target,
        };
        if relationships.insert(id.into(), relationship).is_some() {
            return Err(invalid());
        }
    }
    Ok(relationships)
}

fn related_part<'a>(
    relationships: &'a BTreeMap<String, Relationship>,
    kind: &str,
) -> Result<Option<&'a str>> {
    let mut matches = relationships.values().filter(|rel| rel.kind == kind);
    let first = matches.next();
    if matches.next().is_some() {
        return Err(invalid());
    }
    first.map(Relationship::local_target).transpose()
}

fn check_content_types(package: &mut Package<'_>) -> Result<BTreeMap<String, String>> {
    let xml = package.xml("[Content_Types].xml", MAX_MANIFEST_BYTES)?;
    let doc = parse_xml(&xml, package.deadline)?;
    let root = doc.root_element();
    if !is_element(root, CONTENT_TYPES, "Types") {
        return Err(invalid());
    }
    let mut parts = BTreeMap::new();
    for node in root.children().filter(Node::is_element) {
        if is_element(node, CONTENT_TYPES, "Override") {
            let name = node
                .attribute("PartName")
                .and_then(|name| name.strip_prefix('/'))
                .filter(|name| valid_part(name))
                .ok_or_else(invalid)?;
            let content = node.attribute("ContentType").ok_or_else(invalid)?;
            if parts.insert(name.into(), content.into()).is_some() {
                return Err(invalid());
            }
        } else if !is_element(node, CONTENT_TYPES, "Default") {
            return Err(invalid());
        }
    }
    Ok(parts)
}

fn choose_worksheet<'a>(
    workbook: &Document<'_>,
    relationships: &'a BTreeMap<String, Relationship>,
    requested: Option<&str>,
) -> Result<(String, &'a str)> {
    let workbook = spreadsheet_root(workbook, "workbook")?;
    let sheets = child(workbook, "sheets")?.ok_or_else(invalid)?;
    let mut names = BTreeSet::new();
    let mut sheet_ids = BTreeSet::new();
    let mut relationship_ids = BTreeSet::new();
    let mut targets = BTreeSet::new();
    let mut worksheets = Vec::new();
    let mut unsupported_requested = false;
    for sheet in sheets.children().filter(Node::is_element) {
        if names.len() == MAX_WORKBOOK_SHEETS {
            return Err(resource_limit());
        }
        if !in_spreadsheet(sheet, sheets, "sheet") {
            return Err(invalid());
        }
        let name = excel_string(sheet.attribute("name").ok_or_else(invalid)?)?;
        if name.is_empty()
            || name.chars().count() > 128
            || name.chars().any(char::is_control)
            || !names.insert(name.to_lowercase())
        {
            return Err(invalid());
        }
        let sheet_id = unsigned(sheet.attribute("sheetId").ok_or_else(invalid)?)?;
        let relation_id = OFFICE_REL_NAMESPACES
            .iter()
            .find_map(|ns| sheet.attribute((*ns, "id")))
            .ok_or_else(invalid)?;
        if sheet_id == 0 || !sheet_ids.insert(sheet_id) || !relationship_ids.insert(relation_id) {
            return Err(invalid());
        }
        let relation = relationships.get(relation_id).ok_or_else(invalid)?;
        if relation.kind == "worksheet" {
            let target = relation.local_target()?;
            if !targets.insert(target) {
                return Err(invalid());
            }
            worksheets.push((name, target));
        } else if requested == Some(name.as_str()) {
            unsupported_requested = true;
        }
    }
    if let Some(requested) = requested {
        return worksheets
            .into_iter()
            .find(|(name, _)| name == requested)
            .ok_or_else(|| {
                if unsupported_requested {
                    failure(
                        "XLSX_SHEET_UNSUPPORTED",
                        "指定されたシートはセルを持つワークシートではありません。",
                    )
                } else {
                    failure(
                        "XLSX_SHEET_NOT_FOUND",
                        "指定された名前のワークシートがありません。",
                    )
                }
            });
    }
    match worksheets.len() {
        1 => Ok(worksheets.pop().expect("one worksheet")),
        0 => Err(failure(
            "XLSX_SHEET_NOT_FOUND",
            "集計できるワークシートがありません。",
        )),
        _ => {
            let names = worksheets
                .iter()
                .take(8)
                .map(|(name, _)| format!("「{name}」"))
                .collect::<Vec<_>>()
                .join("、");
            Err(failure(
                "XLSX_SHEET_REQUIRED",
                format!("ワークシートが複数あります。シート名を指定してください。候補 {names}"),
            ))
        }
    }
}

pub(crate) fn read(bytes: &[u8], sheet: Option<&str>, deadline: Instant) -> Result<Sheet> {
    let mut package = Package::new(bytes, deadline)?;
    let types = check_content_types(&mut package)?;
    let root_links = read_relationships(&mut package, "_rels/.rels", "")?;
    let workbook_path = related_part(&root_links, "officeDocument")?.ok_or_else(invalid)?;
    if types.get(workbook_path).map(String::as_str) != Some(WORKBOOK_TYPE) {
        return Err(invalid());
    }
    let workbook = package.xml(workbook_path, MAX_MANIFEST_BYTES)?;
    let workbook = parse_xml(&workbook, deadline)?;
    let links = read_relationships(
        &mut package,
        &relationships_part(workbook_path),
        workbook_path,
    )?;
    let (name, sheet_path) = choose_worksheet(&workbook, &links, sheet)?;
    if types.get(sheet_path).map(String::as_str) != Some(WORKSHEET_TYPE) {
        return Err(invalid());
    }
    let strings = if let Some(path) = related_part(&links, "sharedStrings")? {
        let xml = package.xml(path, MAX_STRINGS_BYTES)?;
        shared_strings(&parse_xml(&xml, deadline)?, deadline)?
    } else {
        Vec::new()
    };
    let styles = if let Some(path) = related_part(&links, "styles")? {
        let xml = package.xml(path, MAX_MANIFEST_BYTES)?;
        number_styles(&parse_xml(&xml, deadline)?)?
    } else {
        Vec::new()
    };
    let xml = package.xml(sheet_path, MAX_SHEET_BYTES)?;
    let doc = parse_xml(&xml, deadline)?;
    let rows = read_rows(&doc, &strings, &styles, deadline)?;
    check_deadline(deadline)?;
    Ok(Sheet { name, rows })
}

fn unsigned(value: &str) -> Result<u32> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return Err(invalid());
    }
    value.parse().map_err(|_| invalid())
}

fn unsigned_coordinate(value: &str, upper: u32) -> Result<u32> {
    let value = unsigned(value)?;
    if value == 0 || value > upper {
        return Err(invalid());
    }
    Ok(value)
}

fn cell_position(reference: &str, absolute: bool) -> Result<(u32, u32)> {
    let reference = if absolute {
        reference.strip_prefix('$').unwrap_or(reference)
    } else {
        reference
    };
    let letters = reference.bytes().take_while(u8::is_ascii_uppercase).count();
    if !(1..=3).contains(&letters) {
        return Err(invalid());
    }
    let column = reference.as_bytes()[..letters]
        .iter()
        .fold(0u32, |col, &b| col * 26 + u32::from(b - b'A' + 1));
    if column > EXCEL_MAX_COLUMN {
        return Err(invalid());
    }
    let row = &reference[letters..];
    let row = if absolute {
        row.strip_prefix('$').unwrap_or(row)
    } else {
        row
    };
    let row = unsigned_coordinate(row, EXCEL_MAX_ROW)?;
    Ok((row, column))
}

struct FormulaRange {
    first_row: u32,
    last_row: u32,
    first_column: u32,
    last_column: u32,
}

impl FormulaRange {
    fn parse(value: &str, row: u32, column: u32) -> Result<Self> {
        let (first, last) = value.split_once(':').unwrap_or((value, value));
        let (first_row, first_column) = cell_position(first, true)?;
        let (last_row, last_column) = cell_position(last, true)?;
        if !(first_row..=last_row).contains(&row) || !(first_column..=last_column).contains(&column)
        {
            return Err(invalid());
        }
        Ok(Self {
            first_row,
            last_row,
            first_column,
            last_column,
        })
    }
}

fn read_rows(
    doc: &Document<'_>,
    strings: &[String],
    styles: &[bool],
    deadline: Instant,
) -> Result<Vec<Row>> {
    let sheet = spreadsheet_root(doc, "worksheet")?;
    let data = child(sheet, "sheetData")?.ok_or_else(invalid)?;
    let mut previous_row = 0u32;
    let mut total_cells = 0;
    let mut coordinates = Vec::new();
    let mut ranges = Vec::new();
    for row in data.children().filter(Node::is_element) {
        check_deadline(deadline)?;
        if coordinates.len() == MAX_ROWS {
            return Err(resource_limit());
        }
        if !in_spreadsheet(row, data, "row") {
            return Err(invalid());
        }
        let number = row
            .attribute("r")
            .map(|value| unsigned_coordinate(value, EXCEL_MAX_ROW))
            .transpose()?
            .unwrap_or(previous_row + 1);
        if number <= previous_row || number > EXCEL_MAX_ROW {
            return Err(invalid());
        }
        previous_row = number;
        let mut previous_column = 0u32;
        let mut cells = Vec::new();
        for cell in row
            .children()
            .filter(|&cell| in_spreadsheet(cell, row, "c"))
        {
            total_cells += 1;
            if total_cells > MAX_CELLS {
                return Err(resource_limit());
            }
            let column = if let Some(reference) = cell.attribute("r") {
                let (actual_row, column) = cell_position(reference, false)?;
                if actual_row != number {
                    return Err(invalid());
                }
                column
            } else {
                previous_column + 1
            };
            if column <= previous_column || column > EXCEL_MAX_COLUMN {
                return Err(invalid());
            }
            previous_column = column;
            let formula = child(cell, "f")?;
            if let Some(formula) = formula {
                if let Some(reference) = formula.attribute("ref") {
                    if ranges.len() == MAX_FORMULA_RANGES {
                        return Err(resource_limit());
                    }
                    ranges.push(FormulaRange::parse(reference, number, column)?);
                } else if matches!(formula.attribute("t"), Some("array" | "dataTable")) {
                    return Err(invalid());
                }
            }
            cells.push((column, cell, formula.is_some()));
        }
        coordinates.push((number, row.attribute("s"), cells));
    }

    let mut text_left = MAX_ALL_CELL_TEXT;
    let mut rows = Vec::with_capacity(coordinates.len());
    for (number, row_style, cells) in coordinates {
        check_deadline(deadline)?;
        let mut active: Vec<_> = ranges
            .iter()
            .filter(|range| (range.first_row..=range.last_row).contains(&number))
            .map(|range| (range.first_column, range.last_column))
            .collect();
        active.sort_unstable();
        let mut active = active.into_iter().peekable();
        let mut output = Vec::with_capacity(cells.len());
        for (column, cell, formula) in cells {
            check_deadline(deadline)?;
            while active.peek().is_some_and(|&(_, end)| end < column) {
                active.next();
            }
            let in_formula_range = active
                .peek()
                .is_some_and(|&(start, end)| (start..=end).contains(&column));
            let value = if formula || in_formula_range {
                CellValue::Formula
            } else {
                read_cell(cell, strings, styles, row_style, &mut text_left)?
            };
            output.push(Cell { column, value });
        }
        rows.push(Row {
            number,
            cells: output,
        });
    }
    Ok(rows)
}

fn decoded_text(node: Node<'_, '_>) -> Result<String> {
    let mut text = String::new();
    for part in node.children() {
        if part.is_element() {
            return Err(invalid());
        }
        if part.is_text() {
            text.push_str(part.text().expect("XML text node"));
        }
    }
    if text.len() > MAX_CELL_TEXT {
        return Err(resource_limit());
    }
    Ok(text)
}

fn shared_strings(doc: &Document<'_>, deadline: Instant) -> Result<Vec<String>> {
    let root = spreadsheet_root(doc, "sst")?;
    let mut strings = Vec::new();
    for item in root.children().filter(|&n| in_spreadsheet(n, root, "si")) {
        check_deadline(deadline)?;
        if strings.len() == MAX_SHARED_STRINGS {
            return Err(resource_limit());
        }
        strings.push(rich_string(item)?);
    }
    Ok(strings)
}

fn rich_string(node: Node<'_, '_>) -> Result<String> {
    let mut text = String::new();
    let mut has_plain = false;
    let mut has_runs = false;
    for part in node.children().filter(Node::is_element) {
        if in_spreadsheet(part, node, "t") && !has_runs && !has_plain {
            has_plain = true;
            text.push_str(&excel_string(&decoded_text(part)?)?);
        } else if in_spreadsheet(part, node, "r") && !has_plain {
            has_runs = true;
            let run = child(part, "t")?.ok_or_else(invalid)?;
            text.push_str(&excel_string(&decoded_text(run)?)?);
        } else if !(in_spreadsheet(part, node, "rPh") || in_spreadsheet(part, node, "phoneticPr")) {
            return Err(invalid());
        }
        if text.len() > MAX_CELL_TEXT {
            return Err(resource_limit());
        }
    }
    Ok(text)
}

fn excel_string(value: &str) -> Result<String> {
    // ST_Xstring encodes UTF-16 code units as _xHHHH_. Decode once so an escaped
    // underscore does not turn the following literal escape-like text into data.
    let encoded = |bytes: &[u8]| -> Option<u16> {
        let prefix = bytes.get(..7)?;
        if &prefix[..2] != b"_x" || prefix[6] != b'_' {
            return None;
        }
        let hex = std::str::from_utf8(&prefix[2..6]).ok()?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u16::from_str_radix(hex, 16).ok()
    };
    let mut text = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(offset) = rest.find("_x") {
        text.push_str(&rest[..offset]);
        rest = &rest[offset..];
        if let Some(first) = encoded(rest.as_bytes()) {
            rest = &rest[7..];
            let mut units = [first, 0];
            let len = if (0xd800..=0xdbff).contains(&first) {
                units[1] = encoded(rest.as_bytes()).ok_or_else(invalid)?;
                rest = &rest[7..];
                2
            } else {
                1
            };
            for decoded in char::decode_utf16(units[..len].iter().copied()) {
                text.push(decoded.map_err(|_| invalid())?);
            }
        } else {
            text.push('_');
            rest = &rest[1..];
        }
    }
    text.push_str(rest);
    Ok(text)
}

fn read_cell(
    cell: Node<'_, '_>,
    strings: &[String],
    styles: &[bool],
    row_style: Option<&str>,
    text_left: &mut usize,
) -> Result<CellValue> {
    let value = child(cell, "v")?;
    let inline = child(cell, "is")?;
    let kind = cell.attribute("t").unwrap_or("n");
    if kind == "inlineStr" {
        if value.is_some() {
            return Err(invalid());
        }
        let Some(inline) = inline else {
            return Ok(CellValue::Blank);
        };
        let text = rich_string(inline)?;
        return cell_text(&text, text_left);
    }
    if inline.is_some() {
        return Err(invalid());
    }
    let Some(value) = value else {
        return Ok(CellValue::Blank);
    };
    let raw = decoded_text(value)?;
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(CellValue::Blank);
    }
    match kind {
        "s" => {
            let index = unsigned(raw)? as usize;
            cell_text(strings.get(index).ok_or_else(invalid)?, text_left)
        }
        "n" if number_lexeme(raw) && numeric_style(cell.attribute("s").or(row_style), styles) => {
            *text_left = text_left
                .checked_sub(raw.len())
                .ok_or_else(resource_limit)?;
            Ok(CellValue::Number(raw.into()))
        }
        _ => Ok(CellValue::Unsupported),
    }
}

fn cell_text(value: &str, text_left: &mut usize) -> Result<CellValue> {
    if value
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\r' | '\n'))
    {
        return Ok(CellValue::Unsupported);
    }
    *text_left = text_left
        .checked_sub(value.len())
        .ok_or_else(resource_limit)?;
    Ok(CellValue::Text(value.into()))
}

fn number_lexeme(raw: &str) -> bool {
    let raw = raw.strip_prefix(['+', '-']).unwrap_or(raw);
    let (base, exponent) = raw
        .split_once(['e', 'E'])
        .map_or((raw, None), |(part, exp)| (part, Some(exp)));
    if let Some(exponent) = exponent {
        let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
        if exponent.is_empty() || !exponent.bytes().all(|b| b.is_ascii_digit()) {
            return false;
        }
    }
    let (whole, fraction) = base.split_once('.').unwrap_or((base, ""));
    !(whole.is_empty() && fraction.is_empty())
        && whole.bytes().all(|b| b.is_ascii_digit())
        && fraction.bytes().all(|b| b.is_ascii_digit())
}

fn numeric_style(index: Option<&str>, styles: &[bool]) -> bool {
    match index {
        Some(index) => unsigned(index).ok().is_some_and(|index| {
            styles
                .get(index as usize)
                .copied()
                .unwrap_or(styles.is_empty() && index == 0)
        }),
        None => styles.first().copied().unwrap_or(true),
    }
}

fn number_styles(doc: &Document<'_>) -> Result<Vec<bool>> {
    let root = spreadsheet_root(doc, "styleSheet")?;
    let mut custom = BTreeMap::new();
    if let Some(formats) = child(root, "numFmts")? {
        for format in formats
            .children()
            .filter(|&n| in_spreadsheet(n, formats, "numFmt"))
        {
            let id = unsigned(format.attribute("numFmtId").ok_or_else(invalid)?)?;
            let code = excel_string(format.attribute("formatCode").ok_or_else(invalid)?)?;
            if custom.insert(id, !date_format(&code)).is_some() {
                return Err(invalid());
            }
        }
    }
    let allow = |id: u32| {
        custom
            .get(&id)
            .copied()
            .unwrap_or(id < 164 && !matches!(id, 14..=22 | 27..=36 | 45..=47 | 50..=58))
    };
    let base = child(root, "cellStyleXfs")?;
    let mut base_styles = Vec::new();
    if let Some(base) = base {
        for style in base.children().filter(|&n| in_spreadsheet(n, base, "xf")) {
            let id = unsigned(style.attribute("numFmtId").unwrap_or("0"))?;
            base_styles.push(allow(id));
        }
    }
    let mut result = Vec::new();
    if let Some(styles) = child(root, "cellXfs")? {
        for style in styles
            .children()
            .filter(|&n| in_spreadsheet(n, styles, "xf"))
        {
            let id = unsigned(style.attribute("numFmtId").unwrap_or("0"))?;
            let inherits_number = style.attribute("numFmtId").is_none()
                || matches!(style.attribute("applyNumberFormat"), Some("0" | "false"));
            let ordinary = if inherits_number {
                let base = style
                    .attribute("xfId")
                    .map(unsigned)
                    .transpose()?
                    .unwrap_or(0) as usize;
                base_styles
                    .get(base)
                    .copied()
                    .unwrap_or(base_styles.is_empty() && base == 0)
                    && allow(id)
            } else {
                allow(id)
            };
            result.push(ordinary);
        }
    }
    Ok(result)
}

fn date_format(code: &str) -> bool {
    let mut characters = code.chars();
    while let Some(c) = characters.next() {
        match c {
            '"' => {
                for nested in characters.by_ref() {
                    if nested == '"' {
                        break;
                    }
                }
            }
            '\\' | '_' | '*' => {
                characters.next();
            }
            '[' => {
                let bracket = characters
                    .by_ref()
                    .take_while(|&c| c != ']')
                    .collect::<String>();
                if matches!(
                    bracket.to_ascii_lowercase().as_str(),
                    "h" | "hh" | "m" | "mm" | "s" | "ss" | "$-f800" | "$-f400"
                ) {
                    return true;
                }
            }
            'y' | 'Y' | 'm' | 'M' | 'd' | 'D' | 'h' | 'H' | 's' | 'S' => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::time::Duration;
    use zip::{ZipWriter, write::SimpleFileOptions};

    const SS: &str = SPREADSHEET_NAMESPACES[0];
    const REL: &str = OFFICE_REL_NAMESPACES[0];

    fn fixture(data: &str, second: bool, target: &str) -> Vec<u8> {
        let second_sheet = if second {
            r#"<sheet name="予備" sheetId="2" r:id="other"/>"#
        } else {
            ""
        };
        let types = format!(
            r#"<Types xmlns="{CONTENT_TYPES}">
            <Override PartName="/xl/workbook.xml" ContentType="{WORKBOOK_TYPE}"/>
            <Override PartName="/xl/worksheets/main.xml" ContentType="{WORKSHEET_TYPE}"/>
            <Override PartName="/xl/worksheets/other.xml" ContentType="{WORKSHEET_TYPE}"/>
            </Types>"#
        );
        let root_rels = format!(
            r#"<Relationships xmlns="{PACKAGE_RELS}">
            <Relationship Id="book" Type="{REL}/officeDocument" Target="xl/workbook.xml"/>
            </Relationships>"#
        );
        let workbook = format!(
            r#"<workbook xmlns="{SS}" xmlns:r="{REL}"><sheets>
            <sheet name="通期" sheetId="1" r:id="main"/>{second_sheet}
            </sheets></workbook>"#
        );
        let workbook_rels = format!(
            r#"<Relationships xmlns="{PACKAGE_RELS}">
            <Relationship Id="main" Type="{REL}/worksheet" {target}/>
            <Relationship Id="other" Type="{REL}/worksheet" Target="/xl/worksheets/other.xml"/>
            <Relationship Id="strings" Type="{REL}/sharedStrings" Target="sharedStrings.xml"/>
            <Relationship Id="styles" Type="{REL}/styles" Target="styles.xml"/>
            </Relationships>"#
        );
        let worksheet =
            format!(r#"<worksheet xmlns="{SS}"><sheetData>{data}</sheetData></worksheet>"#);
        let other = format!(r#"<worksheet xmlns="{SS}"><sheetData/></worksheet>"#);
        let strings = format!(
            r#"<sst xmlns="{SS}"><si><t>金額</t></si>
            <si><r><t>前</t></r><r><t>半</t></r><rPh sb="0" eb="1"><t>ぜん</t></rPh></si>
            </sst>"#
        );
        let styles = format!(
            r#"<styleSheet xmlns="{SS}"><cellXfs>
            <xf numFmtId="0"/><xf numFmtId="14"/>
            </cellXfs></styleSheet>"#
        );
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        for (part, xml) in [
            ("[Content_Types].xml", types),
            ("_rels/.rels", root_rels),
            ("xl/workbook.xml", workbook),
            ("xl/_rels/workbook.xml.rels", workbook_rels),
            ("xl/worksheets/main.xml", worksheet),
            ("xl/worksheets/other.xml", other),
            ("xl/sharedStrings.xml", strings),
            ("xl/styles.xml", styles),
        ] {
            zip.start_file(part, options).unwrap();
            zip.write_all(xml.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    #[test]
    fn sparse_coordinates_and_formulas_survive_real_xlsx_read() {
        let data = r#"
            <row r="2"><c r="A2" t="inlineStr"><is><t>期間</t></is></c><c r="C2" t="s"><v>0</v></c></row>
            <row r="5"><c r="A5" t="s"><v>1</v></c><c r="C5"><v>1.000E+02</v></c><c r="D5" s="1"><v>46210</v></c>
                <c r="F5"><f t="array" ref="F5:F6">IF(B5:B6, 10, 20)</f><v>10</v></c></row>
            <row r="6"><c r="F6"><v>20</v></c></row>
        "#;
        let workbook = fixture(data, false, r#"Target="worksheets/main.xml""#);
        let sheet = read(&workbook, None, deadline()).unwrap();
        assert_eq!(sheet.name, "通期");
        assert_eq!(
            sheet.rows.iter().map(|row| row.number).collect::<Vec<_>>(),
            [2, 5, 6]
        );
        assert_eq!(
            sheet.rows[0].cells[1],
            Cell {
                column: 3,
                value: CellValue::Text("金額".into())
            }
        );
        assert_eq!(sheet.rows[1].cells[0].value, CellValue::Text("前半".into()));
        assert_eq!(
            sheet.rows[1].cells[1].value,
            CellValue::Number("1.000E+02".into())
        );
        assert_eq!(sheet.rows[1].cells[2].value, CellValue::Unsupported);
        assert_eq!(sheet.rows[1].cells[3].value, CellValue::Formula);
        assert_eq!(sheet.rows[2].cells[0].value, CellValue::Formula);
    }

    #[test]
    fn workbook_sheet_choice_does_not_open_an_external_target() {
        let workbook = fixture("", true, r#"Target="worksheets/main.xml""#);
        assert!(matches!(
            read(&workbook, None, deadline()),
            Err(Error::Contract { code, .. }) if code == "XLSX_SHEET_REQUIRED"
        ));
        assert_eq!(
            read(&workbook, Some("予備"), deadline()).unwrap().name,
            "予備"
        );
        let workbook = fixture(
            "",
            false,
            r#"Target="https://example.invalid/sheet" TargetMode="External""#,
        );
        assert!(matches!(
            read(&workbook, None, deadline()),
            Err(Error::Contract { code, .. }) if code == "XLSX_EXTERNAL_RELATIONSHIP"
        ));
    }

    #[test]
    fn inconsistent_coordinates_do_not_produce_cells() {
        for data in [
            r#"<row r="2"><c r="A1"><v>10</v></c></row>"#,
            r#"<row r="2"><c r="A2"><v>10</v></c><c r="A2"><v>20</v></c></row>"#,
        ] {
            let workbook = fixture(data, false, r#"Target="worksheets/main.xml""#);
            assert!(matches!(
                read(&workbook, None, deadline()),
                Err(Error::Contract { code, .. }) if code == "XLSX_INVALID"
            ));
        }
    }
}
