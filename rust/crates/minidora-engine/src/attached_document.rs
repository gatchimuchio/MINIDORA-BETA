//! Extract and find text in sealed attachment snapshots. No path is opened.
mod decision_field;
mod review;

pub(crate) use decision_field::{DecisionFieldSource, decision_field_source};

use crate::document::revalidate_previous_document_render;
use crate::image_ocr::{self, Bounds, Observation};
use crate::image_table::{self, CellLookup};
use crate::input::{self, PURPOSE_DOCUMENT_PARSER_VERSION, verify_attached_document_input};
use crate::pdf_worker::{self, Page, PageState};
use crate::store::Store;
use crate::text_summary::{EXTRACTIVE_SUMMARY_LABEL, extractive_summary_candidates};
use crate::{ocr_worker, pdf_scan, purpose_profile};
use minidora_domain::{
    CaseRevision, Error, Guarantee, InputBundle, Intent, Lifecycle, MAX_REQUEST_ATTACHMENTS,
    PurposeTask, RequestAttachment, Result, ResultRecord, canonical, digest, failure, identity,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::Range;
use std::time::{Duration, Instant};

pub(crate) const SOURCE_NODE: &str = "attached-document";
const CONTEXT_ERROR: &str = "ATTACHED_DOCUMENT_CONTEXT_INVALID";
const REPORT_KIND: &str = "minidora.attached-document-report.v1";
const MAX_SOURCE_BYTES: usize = 256_000;
const MAX_EXTRACTED_PDF_BYTES: usize = 256_000;
const MAX_SUMMARY_QUOTES_PER_PDF: usize = 8;
const MAX_FIND_QUOTES_PER_FILE: usize = 4;
const MAX_FIND_LINE_BYTES: usize = 2_048;
const MAX_FIELD_WITNESSES: usize = 16;
const MAX_FIELD_VALUE_CHARS: usize = 128;
const MAX_OCR_EVIDENCE_BYTES: usize = 256_000;

pub(crate) struct DocumentOutcome {
    pub units: Vec<String>,
    pub report_bytes: Vec<u8>,
    pub manifest_bytes: Vec<u8>,
    pub manifest_ref: String,
    pub dependencies: Vec<String>,
}

#[derive(Serialize)]
struct ManifestSource<'a> {
    name: &'a str,
    byte_count: usize,
    sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    media_type: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    image_observation: Option<ImageReference>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pdf_scan: Option<pdf_scan::Reference>,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ImageReference {
    sha256: String,
    byte_count: usize,
    width: u32,
    height: u32,
}

#[derive(Deserialize)]
struct RetainedSource {
    name: String,
    image_observation: Option<ImageReference>,
    pdf_scan: Option<pdf_scan::Reference>,
}

#[derive(Default)]
struct RetainedObservations {
    images: BTreeMap<String, ImageReference>,
    scans: BTreeMap<String, pdf_scan::Reference>,
}

struct ImagePreparation<'a> {
    store: &'a Store,
    actor: &'a str,
    case: &'a str,
    retained: Option<&'a RetainedObservations>,
    max_evidence_bytes: usize,
    table_cells: bool,
}

impl ImagePreparation<'_> {
    fn prepare(
        &self,
        name: &str,
        raw: &[u8],
        media_type: &str,
        deadline: Instant,
    ) -> Result<(Observation, ImageReference)> {
        let make_ref = |observation: &Observation, bytes: &[u8]| ImageReference {
            sha256: digest(bytes),
            byte_count: bytes.len(),
            width: observation.width,
            height: observation.height,
        };
        if let Some(retained) = self.retained {
            let reference = retained.images.get(name).ok_or_else(invalid_context)?;
            let stored = self
                .store
                .get_bytes(self.actor, self.case, &reference.sha256)?;
            if stored.len() != reference.byte_count || stored.len() > MAX_OCR_EVIDENCE_BYTES {
                return Err(invalid_context());
            }
            let observation: Observation = serde_json::from_slice(&stored)?;
            if self.table_cells {
                image_ocr::verify_table(&observation, raw, media_type, deadline)?;
            } else {
                image_ocr::verify(&observation, raw, media_type)?;
            }
            let actual = make_ref(&observation, &stored);
            if actual != *reference || canonical(&observation)? != stored {
                return Err(invalid_context());
            }
            Ok((observation, actual))
        } else {
            let observation = if self.table_cells {
                image_ocr::observe_table(raw, media_type, deadline)?
            } else {
                image_ocr::observe(raw, media_type, deadline)?
            };
            let bytes = canonical(&observation)?;
            if bytes.len() > MAX_OCR_EVIDENCE_BYTES {
                return Err(resource_limit());
            }
            let reference = make_ref(&observation, &bytes);
            if self.store.put_bytes(self.actor, self.case, &bytes)? != reference.sha256 {
                return Err(invalid_context());
            }
            Ok((observation, reference))
        }
    }

    fn pdf_source(&self) -> pdf_scan::Source<'_> {
        pdf_scan::Source {
            store: self.store,
            actor: self.actor,
            case: self.case,
        }
    }
}

struct PreparedFile<'a> {
    source: ManifestSource<'a>,
    content: FileContent<'a>,
}

enum FileContent<'a> {
    Text(&'a str),
    Pdf(Vec<Page>),
    ScannedPdf(pdf_scan::Document),
    Image {
        observation: Observation,
        raster: Vec<u8>,
    },
    UnreadablePdf(PdfFailure),
}

#[derive(Clone, Copy, Serialize)]
struct PdfFailure {
    code: &'static str,
    message: &'static str,
}

impl PdfFailure {
    fn from_error(error: &Error) -> Option<Self> {
        let Error::Contract { code, .. } = error else {
            return None;
        };
        let (code, message) = match code.as_str() {
            "PDF_INVALID" => ("PDF_INVALID", "PDFの構造または本文を読み取れませんでした。"),
            "PDF_ENCRYPTED" => ("PDF_ENCRYPTED", "暗号化されたPDFは読み取れません。"),
            "PDF_RESOURCE_LIMIT" => (
                "PDF_RESOURCE_LIMIT",
                "PDFの構造または読み取り量が上限を超えました。",
            ),
            _ => return None,
        };
        Some(Self { code, message })
    }
}

#[derive(Serialize)]
struct Quote<'a> {
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<u32>,
    line: usize,
    source_start: usize,
    source_end: usize,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pixels: Option<Bounds>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset_space: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coordinate_space: Option<&'static str>,
}

#[derive(Serialize)]
struct FileReport<'a> {
    source: &'a ManifestSource<'a>,
    candidate_count: usize,
    overlong_matches: usize,
    quotes: Vec<Quote<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    offset_space: Option<&'static str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pdf_pages: Vec<PdfPageReport<'a>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<PdfFailure>,
}

#[derive(Serialize)]
struct PdfPageReport<'a> {
    number: u32,
    state: &'a PageState,
    extracted_byte_count: usize,
    extracted_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ocr: Option<PdfRasterReport<'a>>,
}

#[derive(Serialize)]
struct PdfRasterReport<'a> {
    sha256: &'a str,
    width: u32,
    height: u32,
    status: &'static str,
}

#[derive(Serialize)]
struct DocumentReport<'a> {
    kind: &'static str,
    manifest_sha256: &'a str,
    operation: &'static str,
    query: Option<&'a str>,
    files: Vec<FileReport<'a>>,
}

#[derive(Deserialize, PartialEq, Eq)]
struct RetainedQuoteSource {
    name: String,
    sha256: String,
}

#[derive(Deserialize)]
struct RetainedQuote {
    page: Option<u32>,
    line: usize,
    source_start: usize,
    source_end: usize,
    text: String,
    pixels: Option<Bounds>,
}

#[derive(Deserialize)]
struct RetainedQuoteFile {
    source: RetainedQuoteSource,
    quotes: Vec<RetainedQuote>,
}

#[derive(Deserialize)]
struct RetainedQuoteReport {
    kind: String,
    manifest_sha256: String,
    operation: String,
    query: Option<String>,
    files: Vec<RetainedQuoteFile>,
}

#[derive(Clone, Copy)]
enum Operation<'a> {
    Extract,
    Find(&'a str),
}

impl<'a> Operation<'a> {
    fn from_bundle(bundle: &'a InputBundle) -> Result<Self> {
        if bundle.meaning.parser_version == PURPOSE_DOCUMENT_PARSER_VERSION {
            let task = bundle
                .request
                .purpose
                .as_ref()
                .and_then(|purpose| purpose.binding.as_ref())
                .map(|binding| &binding.task)
                .ok_or_else(invalid_context)?;
            return Ok(match task {
                PurposeTask::Summarize => Self::Extract,
                PurposeTask::Find { query } => Self::Find(query),
            });
        }
        match &bundle.meaning.intent {
            Intent::SummarizeAttachedDocuments => Ok(Self::Extract),
            Intent::FindInAttachedDocuments { query }
            | Intent::FindInPreviousAttachedDocuments { query }
                if !query.text.chars().any(char::is_control) =>
            {
                Ok(Self::Find(&query.text))
            }
            _ => Err(invalid_context()),
        }
    }
}

struct Computation {
    units: Vec<String>,
    report_bytes: Vec<u8>,
}

fn invalid_context() -> minidora_domain::Error {
    failure(
        CONTEXT_ERROR,
        "同じ会話に保存された文書添付と引用結果を照合できません。",
    )
}

fn source_heading(name: &str, sha256: &str) -> String {
    format!("「{name}」  SHA-256 {sha256}")
}

fn quote_prefix(
    page: Option<u32>,
    line: usize,
    bytes: Range<usize>,
    pixels: Option<Bounds>,
) -> String {
    let Range { start, end } = bytes;
    match (page, pixels) {
        (Some(page), Some(bounds)) => format!(
            "- {page}ページ OCR {line}行目 左上 ({}, {}) 幅 {} 高さ {} px　",
            bounds.x, bounds.y, bounds.width, bounds.height
        ),
        (None, Some(bounds)) => format!(
            "- OCR {line}行目 左上 ({}, {}) 幅 {} 高さ {} px　",
            bounds.x, bounds.y, bounds.width, bounds.height
        ),
        (Some(page), None) => {
            format!("- {page}ページ 抽出テキスト {line}行目 byte[{start}..{end})　")
        }
        (None, None) => format!("- {line}行目 byte[{start}..{end})　"),
    }
}

fn markdown_heading(quote: &RetainedQuote, next: Option<&RetainedQuote>) -> bool {
    let text = quote.text.trim();
    let atx_length = text.bytes().take_while(|&byte| byte == b'#').count();
    if (1..=6).contains(&atx_length)
        && text[atx_length..]
            .chars()
            .next()
            .is_none_or(|character| matches!(character, ' ' | '\t'))
    {
        return true;
    }
    let underline = |value: &str| {
        let value = value.trim();
        value.starts_with(['=', '-'])
            && value
                .as_bytes()
                .iter()
                .all(|byte| byte == &value.as_bytes()[0])
    };
    underline(text)
        || next.is_some_and(|next| {
            next.page == quote.page
                && quote.line.checked_add(1) == Some(next.line)
                && underline(&next.text)
        })
}

/// Return exact byte ranges of the saved quote payloads in the adopted text.
/// The parent result and render are verified by the caller.
pub(crate) fn retained_quote_ranges(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
) -> Result<Vec<Range<usize>>> {
    let request = &bundle.request;
    let plan = result.content_plan.as_ref().ok_or_else(invalid_context)?;
    let [parents @ .., manifest_ref, report_ref] = result.dependencies.as_slice() else {
        return Err(invalid_context());
    };
    let previous = matches!(
        bundle.meaning.intent,
        Intent::FindInPreviousAttachedDocuments { .. }
    );
    let valid_lineage = if previous {
        matches!(parents.len(), 1 | 2)
            && parents.last() == bundle.meaning.context_result_ref.as_ref()
    } else {
        parents.is_empty()
    };
    if !input::is_attached_document_intent(&bundle.meaning.intent)
        || result.lifecycle != Lifecycle::Commit
        || result.guarantee != Guarantee::SourceReport
        || plan.guarantee != result.guarantee
        || plan
            .dependencies
            .split_last()
            .is_none_or(|(_, sources)| sources != result.dependencies)
        || !valid_lineage
        || plan.required_units.join("\n") != result.text
    {
        return Err(invalid_context());
    }
    let manifest_bytes = store.get_bytes(&request.actor_id, &request.case_id, manifest_ref)?;
    let (manifest_kind, sources): (String, Vec<RetainedQuoteSource>) =
        serde_json::from_slice(&manifest_bytes).map_err(|_| invalid_context())?;
    let report_bytes = store.get_bytes(&request.actor_id, &request.case_id, report_ref)?;
    let report: RetainedQuoteReport =
        serde_json::from_slice(&report_bytes).map_err(|_| invalid_context())?;
    let valid_operation = match Operation::from_bundle(bundle)? {
        Operation::Extract => report.operation == "EXTRACT" && report.query.is_none(),
        Operation::Find(query) => {
            report.operation == "FIND" && report.query.as_deref() == Some(query)
        }
    };
    if !matches!(
        manifest_kind.as_str(),
        "minidora.attached-document-manifest.v1"
            | "minidora.attached-document-manifest.v2"
            | "minidora.attached-document-manifest.v3"
    ) || report.kind != REPORT_KIND
        || report.manifest_sha256 != *manifest_ref
        || !valid_operation
        || !(1..=MAX_REQUEST_ATTACHMENTS).contains(&sources.len())
        || !report.files.iter().map(|file| &file.source).eq(&sources)
    {
        return Err(invalid_context());
    }

    let mut offset = 0;
    let units = plan
        .required_units
        .iter()
        .map(|unit| {
            let start = offset;
            offset += unit.len() + 1;
            (start, unit.as_str())
        })
        .collect::<Vec<_>>();
    let mut remaining = units.get(1..).ok_or_else(invalid_context)?;
    let mut ranges = Vec::new();
    for (index, file) in report.files.iter().enumerate() {
        let heading = source_heading(&file.source.name, &file.source.sha256);
        if remaining.first().map(|(_, unit)| *unit) != Some(heading.as_str()) {
            return Err(invalid_context());
        }
        let end = if let Some(next) = report.files.get(index + 1) {
            let next_heading = source_heading(&next.source.name, &next.source.sha256);
            remaining[1..]
                .iter()
                .position(|(_, unit)| *unit == next_heading)
                .map(|index| index + 1)
                .ok_or_else(invalid_context)?
        } else {
            remaining.len()
        };
        let (section, rest) = remaining.split_at(end);
        remaining = rest;
        let quote_start = section
            .len()
            .checked_sub(file.quotes.len())
            .filter(|start| *start > 0)
            .ok_or_else(invalid_context)?;
        for (index, (quote, &(start, unit))) in
            file.quotes.iter().zip(&section[quote_start..]).enumerate()
        {
            let prefix = quote_prefix(
                quote.page,
                quote.line,
                quote.source_start..quote.source_end,
                quote.pixels,
            );
            if quote.text.trim().is_empty()
                || quote.source_end.checked_sub(quote.source_start) != Some(quote.text.len())
                || unit.strip_prefix(&prefix) != Some(quote.text.as_str())
            {
                return Err(invalid_context());
            }
            if !quote.text.contains(['\r', '\n', '\u{2028}', '\u{2029}'])
                && !markdown_heading(quote, file.quotes.get(index + 1))
            {
                let start = start + prefix.len();
                ranges.push(start..start + quote.text.len());
            }
        }
    }
    Ok(ranges)
}

fn resource_limit() -> minidora_domain::Error {
    failure(
        "RESOURCE_LIMIT",
        "添付文書の処理が時間または証拠の上限に達しました。",
    )
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(resource_limit());
    }
    Ok(())
}

fn ensure_displayable_text(text: &str, name: &str) -> Result<()> {
    if text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\t' | '\r' | '\n'))
    {
        return Err(failure(
            "ATTACHED_DOCUMENT_FORMAT",
            format!("「{name}」の本文に表示できない制御文字が含まれています。"),
        ));
    }
    Ok(())
}

fn prepare_files<'a>(
    files: impl ExactSizeIterator<Item = &'a RequestAttachment>,
    deadline: Instant,
    allow_all_unreadable: bool,
    images: Option<&ImagePreparation<'_>>,
) -> Result<Vec<PreparedFile<'a>>> {
    if files.len() == 0 {
        return Err(failure(
            "ATTACHMENT_REQUIRED",
            "読む文書を /attach PATH で選んでください。",
        ));
    }
    if files.len() > MAX_REQUEST_ATTACHMENTS {
        return Err(resource_limit());
    }
    let mut sources = Vec::with_capacity(files.len());
    let mut total_bytes = 0usize;
    let mut extracted_bytes = 0usize;
    let mut first_unreadable = None;
    for file in files {
        check_deadline(deadline)?;
        let pdf = file.pdf_bytes()?;
        let image = file.image_bytes()?;
        let raw = match (&pdf, &image) {
            (Some(bytes), None) | (None, Some(bytes)) => bytes.as_slice(),
            (None, None) if RequestAttachment::is_text_name(&file.name) => file.text.as_bytes(),
            _ => {
                return Err(failure(
                    "ATTACHED_DOCUMENT_FORMAT",
                    "文書の抜粋と検索に使える添付はUTF-8のTXT、Markdown、PDF、PNG、JPEGです。",
                ));
            }
        };
        total_bytes = total_bytes.saturating_add(raw.len());
        if total_bytes > MAX_SOURCE_BYTES {
            return Err(resource_limit());
        }
        let mut source = ManifestSource {
            name: &file.name,
            byte_count: raw.len(),
            sha256: digest(raw),
            media_type: pdf.as_ref().map(|_| "application/pdf"),
            image_observation: None,
            pdf_scan: None,
        };
        let content = if let Some(pdf) = &pdf
            && let Some(preparation) = images
            && let Some(reference) = preparation
                .retained
                .and_then(|saved| saved.scans.get(&file.name))
        {
            let document = preparation.pdf_source().restore(
                pdf,
                reference,
                preparation.max_evidence_bytes,
                deadline,
            )?;
            source.pdf_scan = Some(reference.clone());
            FileContent::ScannedPdf(document)
        } else if let Some(pdf) = &pdf {
            let pages = match pdf_worker::read(pdf, deadline) {
                Ok(pages) => pages,
                Err(error) => {
                    let Some(observation) = PdfFailure::from_error(&error) else {
                        return Err(error);
                    };
                    first_unreadable.get_or_insert(error);
                    sources.push(PreparedFile {
                        source,
                        content: FileContent::UnreadablePdf(observation),
                    });
                    continue;
                }
            };
            if let Some(preparation) = images
                && preparation.retained.is_none()
                && ocr_worker::configured()
                && pages.iter().any(|page| page.state != PageState::Text)
            {
                let (document, reference) = preparation.pdf_source().capture(
                    pdf,
                    pages,
                    preparation.max_evidence_bytes,
                    deadline,
                )?;
                source.pdf_scan = Some(reference);
                FileContent::ScannedPdf(document)
            } else {
                FileContent::Pdf(pages)
            }
        } else if let Some(raster) = image {
            let images = images.ok_or_else(|| {
                failure(
                    "ATTACHED_DOCUMENT_FORMAT",
                    "画像は文字の読み取りまたは検索に使用できます。",
                )
            })?;
            let media = if raster.starts_with(b"\x89PNG\r\n\x1a\n") {
                "image/png"
            } else {
                "image/jpeg"
            };
            let (observation, reference) = images.prepare(&file.name, &raster, media, deadline)?;
            source.media_type = Some(media);
            source.image_observation = Some(reference);
            FileContent::Image {
                observation,
                raster,
            }
        } else {
            ensure_displayable_text(&file.text, &file.name)?;
            FileContent::Text(&file.text)
        };
        let pages = match &content {
            FileContent::Pdf(pages) => Some(pages),
            FileContent::ScannedPdf(document) => Some(&document.pages),
            _ => None,
        };
        if let Some(pages) = pages {
            if pages.is_empty() {
                return Err(failure("PDF_PAGE_TREE", "PDFのページを確認できません。"));
            }
            for (index, page) in pages.iter().enumerate() {
                if usize::try_from(page.number).ok() != Some(index + 1) {
                    return Err(failure("PDF_PAGE_TREE", "PDFのページ順を確認できません。"));
                }
                extracted_bytes = extracted_bytes.saturating_add(page.text.len());
                if extracted_bytes > MAX_EXTRACTED_PDF_BYTES {
                    return Err(resource_limit());
                }
                ensure_displayable_text(&page.text, &file.name)?;
            }
        }
        sources.push(PreparedFile { source, content });
    }
    check_deadline(deadline)?;
    if !allow_all_unreadable
        && sources
            .iter()
            .all(|source| matches!(source.content, FileContent::UnreadablePdf(_)))
    {
        return Err(first_unreadable.unwrap_or_else(invalid_context));
    }
    Ok(sources)
}

/// Byte offsets into the supplied UTF-8 text. CRLF, LF, and CR each delimit
/// one line. For UTF-8 files we skip the BOM without changing the offsets.
fn line_starts(source: &str, offset: usize) -> Vec<usize> {
    let mut starts = vec![offset];
    let bytes = source.as_bytes();
    let mut i = offset;
    while i < bytes.len() {
        match bytes[i] {
            b'\r' if bytes.get(i + 1) == Some(&b'\n') => i += 2,
            b'\r' | b'\n' => i += 1,
            _ => {
                i += 1;
                continue;
            }
        }
        if i < bytes.len() {
            starts.push(i);
        }
    }
    starts
}

fn bom_offset(text: &str) -> usize {
    text.strip_prefix('\u{feff}')
        .map_or(0, |body| text.len() - body.len())
}

fn complete_line<'a>(source: &'a str, starts: &[usize], index: usize) -> (usize, &'a str) {
    let start = starts[index];
    let end = starts.get(index + 1).copied().unwrap_or(source.len());
    (start, source[start..end].trim_end_matches(['\r', '\n']))
}

#[derive(Serialize)]
pub(crate) struct FieldObservation {
    pub value: Option<String>,
    pub units: Vec<String>,
    label: String,
    matched_count: usize,
    matches: Vec<FieldMatch>,
    gap_count: usize,
    gaps: Vec<FieldGap>,
}

impl FieldObservation {
    pub(crate) fn english_units(&self, has_documents: bool) -> Vec<String> {
        let label = &self.label;
        let header = if let Some(value) = &self.value {
            format!("The attached documents confirm the field \"{label}\" as \"{value}\".")
        } else if self.gap_count > 0 {
            format!("The field \"{label}\" is unresolved. Some source locations could not be read.")
        } else if self.matched_count > 0 {
            format!("The attached documents give conflicting values for \"{label}\".")
        } else if has_documents {
            format!("No confirmed value for \"{label}\" was found in the attached documents.")
        } else {
            format!("There are no attached documents to confirm \"{label}\".")
        };
        let mut units = vec![header];
        for found in &self.matches {
            let position = found.page.map_or_else(
                || format!("line {}", found.line),
                |page| format!("PDF page {page}, extracted line {}", found.line),
            );
            units.push(format!(
                "\"{}\", {position}, byte range [{}..{}), SHA-256 {}. {}",
                found.filename,
                found.source_start,
                found.source_end,
                found.source_sha256,
                found.quote
            ));
        }
        for gap in &self.gaps {
            let page = gap
                .page
                .map_or_else(String::new, |page| format!(", page {page}"));
            let line = gap
                .line
                .map_or_else(String::new, |line| format!(", line {line}"));
            units.push(format!(
                "\"{}\"{page}{line} could not be checked ({}).",
                gap.filename, gap.code
            ));
        }
        if self.matched_count > self.matches.len() || self.gap_count > self.gaps.len() {
            units.push(format!(
                "The report retains the count of {} readable matches and {} unchecked locations.",
                self.matched_count, self.gap_count
            ));
        }
        units
    }
}

#[derive(Serialize)]
struct FieldMatch {
    filename: String,
    source_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<u32>,
    line: usize,
    source_start: usize,
    source_end: usize,
    offset_space: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    extracted_sha256: Option<String>,
    quote: String,
    value: String,
}

#[derive(Serialize)]
struct FieldGap {
    filename: String,
    source_sha256: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    page: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    line: Option<usize>,
    code: &'static str,
    message: &'static str,
}

#[derive(Default)]
struct FieldSearch {
    first_value: Option<String>,
    conflict: bool,
    matched_count: usize,
    matches: Vec<FieldMatch>,
    gap_count: usize,
    gaps: Vec<FieldGap>,
}

enum FieldAssignment<'a> {
    Value(&'a str),
    Uncertain,
}

fn field_body(line: &str) -> &str {
    let mut body = line.trim_start();
    let unheaded = body.trim_start_matches('#');
    let hashes = body.len() - unheaded.len();
    if (1..=6).contains(&hashes) && unheaded.starts_with(char::is_whitespace) {
        body = unheaded.trim_start();
        if let Some((heading, tail)) = body.rsplit_once(char::is_whitespace)
            && !tail.is_empty()
            && tail.bytes().all(|byte| byte == b'#')
        {
            body = heading.trim_end();
        }
    }
    if let Some(item) = body.strip_prefix(['-', '*', '+'])
        && item.starts_with(char::is_whitespace)
    {
        body = item.trim_start();
    }
    body
}

fn field_assignment<'a>(line: &'a str, label: &str) -> Option<FieldAssignment<'a>> {
    let body = field_body(line);
    let suffix = if let Some(suffix) = body.strip_prefix(label) {
        suffix
    } else {
        let wrapper = ["**", "__"]
            .into_iter()
            .find(|wrapper| body.starts_with(*wrapper))?;
        body.strip_prefix(wrapper)?
            .strip_prefix(label)?
            .strip_prefix(wrapper)?
    }
    .trim_start();
    let value = if let Some(value) = suffix.strip_prefix([':', '：']) {
        value.trim()
    } else if let Some(statement) = suffix.strip_prefix('は') {
        let Some(value) = statement.trim_end().strip_suffix("です。") else {
            return Some(FieldAssignment::Uncertain);
        };
        let value = value.trim();
        if let Some(quoted) = value
            .strip_prefix('「')
            .and_then(|inner| inner.strip_suffix('」'))
            .filter(|inner| !inner.contains(['「', '」']))
        {
            quoted
        } else {
            let standalone = |c: char| c.is_alphanumeric() && !matches!(c, '\u{3040}'..='\u{309f}');
            let listed_values = value
                .chars()
                .zip(value.chars().skip(1))
                .zip(value.chars().skip(2))
                .any(|((before, separator), after)| {
                    matches!(separator, 'か' | 'と' | 'や')
                        && standalone(before)
                        && standalone(after)
                });
            if listed_values
                || value.contains(['「', '」', '。', '、', '！', '？', '!', '?', ':', '：'])
                || ["または", "もしくは", "あるいは", "ではない", "未定", "不明"]
                    .into_iter()
                    .any(|marker| value.contains(marker))
            {
                return Some(FieldAssignment::Uncertain);
            }
            value
        }
    } else {
        return None;
    };
    if value.is_empty()
        || value.chars().count() > MAX_FIELD_VALUE_CHARS
        || value.chars().any(char::is_control)
        || line.len() > MAX_FIND_LINE_BYTES
    {
        return Some(FieldAssignment::Uncertain);
    }
    Some(FieldAssignment::Value(value))
}

impl FieldSearch {
    fn gap(
        &mut self,
        source: &ManifestSource<'_>,
        page: Option<u32>,
        line: Option<usize>,
        code: &'static str,
        message: &'static str,
    ) {
        self.gap_count += 1;
        if self.gaps.len() < MAX_FIELD_WITNESSES {
            self.gaps.push(FieldGap {
                filename: source.name.into(),
                source_sha256: source.sha256.clone(),
                page,
                line,
                code,
                message,
            });
        }
    }

    fn scan(
        &mut self,
        source: &ManifestSource<'_>,
        text: &str,
        page: Option<u32>,
        label: &str,
        deadline: Instant,
    ) -> Result<()> {
        let extracted_sha256 = page.map(|_| digest(text.as_bytes()));
        let starts = line_starts(text, bom_offset(text));
        for i in 0..starts.len() {
            if i.is_multiple_of(128) {
                check_deadline(deadline)?;
            }
            let (start, line) = complete_line(text, &starts, i);
            let value = match field_assignment(line, label) {
                Some(FieldAssignment::Value(value)) => value,
                Some(FieldAssignment::Uncertain) => {
                    self.gap(
                        source,
                        page,
                        Some(i + 1),
                        "FIELD_VALUE_UNCERTAIN",
                        "この行から項目の値を一つに確定できません。",
                    );
                    continue;
                }
                None => continue,
            };
            self.matched_count += 1;
            let first_value = self.first_value.get_or_insert_with(|| value.to_owned());
            if !self.conflict && first_value.as_str() != value {
                self.conflict = true;
                // Retain at least one witness for each side of the first conflict.
                if self.matches.len() == MAX_FIELD_WITNESSES {
                    self.matches.pop();
                }
            }
            if self.matches.len() < MAX_FIELD_WITNESSES {
                self.matches.push(FieldMatch {
                    filename: source.name.into(),
                    source_sha256: source.sha256.clone(),
                    page,
                    line: i + 1,
                    source_start: start,
                    source_end: start + line.len(),
                    offset_space: if page.is_some() {
                        "EXTRACTED_PAGE_UTF8"
                    } else {
                        "ORIGINAL_UTF8"
                    },
                    extracted_sha256: extracted_sha256.clone(),
                    quote: line.into(),
                    value: value.into(),
                });
            }
        }
        Ok(())
    }

    fn scan_pdf(
        &mut self,
        source: &ManifestSource<'_>,
        pages: &[Page],
        label: &str,
        deadline: Instant,
        scanned: bool,
    ) -> Result<()> {
        for page in pages {
            check_deadline(deadline)?;
            match page.state {
                PageState::Text => {
                    self.scan(source, &page.text, Some(page.number), label, deadline)?;
                }
                PageState::ImageWithoutText => self.gap(
                    source,
                    Some(page.number),
                    None,
                    "IMAGE_WITHOUT_TEXT",
                    if scanned {
                        "画像ページのOCR結果は項目の有無の確定には使っていません。"
                    } else {
                        "画像の文字を確認できません。OCRは未実施です。"
                    },
                ),
                PageState::NoExtractableText => self.gap(
                    source,
                    Some(page.number),
                    None,
                    "NO_EXTRACTABLE_TEXT",
                    "文字を抽出できず、項目の記載有無を確認できません。",
                ),
            }
        }
        Ok(())
    }

    fn finish(self, label: &str, has_documents: bool) -> FieldObservation {
        let value = if self.conflict || self.gap_count > 0 {
            None
        } else {
            self.first_value
        };
        let header = if self.conflict {
            format!("添付文書で「{label}」の値が一致しません。")
        } else if self.gap_count > 0 {
            format!("確認できなかった資料箇所があり、「{label}」の値を確定できません。")
        } else if !has_documents {
            format!("「{label}」を確認する添付文書がありません。")
        } else if let Some(value) = &value {
            format!("添付文書で「{label}」の値「{value}」を確認しました。")
        } else {
            format!("添付文書に「{label}」の値を確認できませんでした。")
        };
        let mut units = vec![header];
        for selected in &self.matches {
            let position = selected.page.map_or_else(
                || format!("{}行目", selected.line),
                |page| format!("{page}ページ 抽出テキスト {}行目", selected.line),
            );
            units.push(format!(
                "「{}」 {position} byte[{}..{})　{}",
                selected.filename, selected.source_start, selected.source_end, selected.quote
            ));
        }
        for gap in &self.gaps {
            let mut position = String::new();
            if let Some(page) = gap.page {
                position.push_str(&format!(" {page}ページ"));
            }
            if let Some(line) = gap.line {
                position.push_str(&format!(" {line}行目"));
            }
            units.push(format!("「{}」{position} {}", gap.filename, gap.message));
        }
        if self.matched_count > self.matches.len() || self.gap_count > self.gaps.len() {
            units.push(format!(
                "値を読めた箇所は{}件、確認できなかった箇所は{}件です。一部の位置の表示を省略しました。",
                self.matched_count, self.gap_count
            ));
        }
        FieldObservation {
            value,
            units,
            label: label.into(),
            matched_count: self.matched_count,
            matches: self.matches,
            gap_count: self.gap_count,
            gaps: self.gaps,
        }
    }
}

pub(crate) fn lookup_field(
    files: &[&RequestAttachment],
    label: &str,
    deadline: Instant,
) -> Result<FieldObservation> {
    check_deadline(deadline)?;
    if label.is_empty()
        || label.trim() != label
        || label.chars().count() > MAX_FIELD_VALUE_CHARS
        || label.chars().any(char::is_control)
    {
        return Err(failure(
            "ATTACHED_DOCUMENT_FIELD",
            "添付文書で確認する項目名を指定してください。",
        ));
    }
    let mut search = FieldSearch::default();
    if files.is_empty() {
        return Ok(search.finish(label, false));
    }
    for file in prepare_files(files.iter().copied(), deadline, true, None)? {
        let source = &file.source;
        match &file.content {
            FileContent::Text(text) => search.scan(source, text, None, label, deadline)?,
            FileContent::Pdf(pages) => search.scan_pdf(source, pages, label, deadline, false)?,
            FileContent::UnreadablePdf(failure) => {
                search.gap(source, None, None, failure.code, failure.message)
            }
            FileContent::Image { .. } | FileContent::ScannedPdf(_) => return Err(invalid_context()),
        }
    }
    check_deadline(deadline)?;
    Ok(search.finish(label, true))
}

fn summarize<'a>(
    source: &'a str,
    offset: usize,
    starts: &[usize],
    page: Option<u32>,
    report: &mut FileReport<'a>,
) -> Result<()> {
    let body = &source[offset..];
    if body.trim().is_empty() {
        return Ok(());
    }
    let request = serde_json::json!({
        "text": body,
        "max_sentences": 3,
        "max_chars": 420,
    });
    let summary = extractive_summary_candidates(&serde_json::to_string(&request)?)?;
    if summary.source_text != body {
        return Err(invalid_context());
    }
    report.candidate_count += summary.sentence_count;
    for sentence in summary.sentences {
        let start = offset + sentence.source_start;
        let end = offset + sentence.source_end;
        let text = source
            .get(start..end)
            .filter(|original| *original == sentence.text)
            .ok_or_else(invalid_context)?;
        report.quotes.push(Quote {
            page,
            line: starts.partition_point(|&line_start| line_start <= start),
            source_start: start,
            source_end: end,
            text,
            pixels: None,
            offset_space: None,
            coordinate_space: None,
        });
    }
    Ok(())
}

fn find<'a>(
    source: &'a str,
    query: &str,
    starts: &[usize],
    page: Option<u32>,
    report: &mut FileReport<'a>,
    deadline: Instant,
) -> Result<()> {
    for i in 0..starts.len() {
        if i.is_multiple_of(128) {
            check_deadline(deadline)?;
        }
        let (start, text) = complete_line(source, starts, i);
        if !text.contains(query) {
            continue;
        }
        report.candidate_count += 1;
        if text.len() > MAX_FIND_LINE_BYTES {
            report.overlong_matches += 1;
        } else if report.quotes.len() < MAX_FIND_QUOTES_PER_FILE {
            report.quotes.push(Quote {
                page,
                line: i + 1,
                source_start: start,
                source_end: start + text.len(),
                text,
                pixels: None,
                offset_space: None,
                coordinate_space: None,
            });
        }
    }
    Ok(())
}

fn collect_from_text<'a>(
    text: &'a str,
    page: Option<u32>,
    operation: Operation<'_>,
    report: &mut FileReport<'a>,
    deadline: Instant,
) -> Result<()> {
    let offset = bom_offset(text);
    let starts = line_starts(text, offset);
    match operation {
        Operation::Extract => summarize(text, offset, &starts, page, report),
        Operation::Find(query) => find(text, query, &starts, page, report, deadline),
    }
}

fn collect_from_pdf<'a>(
    pages: &'a [Page],
    scans: Option<&'a BTreeMap<u32, pdf_scan::ScannedPage>>,
    operation: Operation<'_>,
    report: &mut FileReport<'a>,
    units: &mut Vec<String>,
    deadline: Instant,
) -> Result<bool> {
    report.offset_space = Some(if scans.is_some() {
        "PER_QUOTE_UTF8"
    } else {
        "EXTRACTED_PAGE_UTF8"
    });
    let mut has_text = false;
    for page in pages {
        check_deadline(deadline)?;
        let observation = scans
            .and_then(|images| images.get(&page.number))
            .map(|scan| &scan.ocr);
        report.pdf_pages.push(PdfPageReport {
            number: page.number,
            state: &page.state,
            extracted_byte_count: page.text.len(),
            extracted_sha256: digest(page.text.as_bytes()),
            ocr: observation.map(|image| PdfRasterReport {
                sha256: &image.image_sha256,
                width: image.width,
                height: image.height,
                status: if image.lines.iter().any(image_ocr::Line::has_text) {
                    "OCR_TEXT"
                } else {
                    "OCR_NO_TEXT"
                },
            }),
        });
        if let Some(image) = observation {
            has_text |=
                collect_from_image(image, Some(page.number), operation, report, units, deadline)?;
            continue;
        }
        if scans.is_some() && page.state != PageState::Text {
            units.push(format!(
                "{}ページは今回のOCR対象ページ数の上限を超えました。",
                page.number
            ));
            continue;
        }
        match page.state {
            PageState::Text => {
                has_text = true;
                let first = report.quotes.len();
                collect_from_text(&page.text, Some(page.number), operation, report, deadline)?;
                if scans.is_some() {
                    for quote in &mut report.quotes[first..] {
                        quote.offset_space = Some("EXTRACTED_PAGE_UTF8");
                    }
                }
            }
            PageState::ImageWithoutText => units.push(format!(
                "{}ページで画像を確認しました。抽出できる文字がなく、OCRは未実施です。",
                page.number
            )),
            PageState::NoExtractableText => units.push(format!(
                "{}ページでは検索・抜粋に使える文字を抽出できませんでした。",
                page.number
            )),
        }
    }
    if matches!(operation, Operation::Extract) {
        report.quotes.truncate(MAX_SUMMARY_QUOTES_PER_PDF);
    }
    Ok(has_text)
}

fn collect_from_image<'a>(
    observation: &'a Observation,
    page: Option<u32>,
    operation: Operation<'_>,
    report: &mut FileReport<'a>,
    units: &mut Vec<String>,
    deadline: Instant,
) -> Result<bool> {
    if page.is_none() {
        report.offset_space = Some("OCR_LINE_UTF8");
    }
    let context = page.map_or_else(
        || "画像".to_string(),
        |page| format!("{page}ページの描画画像"),
    );
    units.push(format!(
        "{context} {}×{} px。座標の原点は左上。",
        observation.width, observation.height
    ));
    let has_text = observation.lines.iter().any(image_ocr::Line::has_text);
    if !has_text {
        units.push(page.map_or_else(
            || "OCRで文字を確認できませんでした。".to_owned(),
            |p| format!("{p}ページはOCRで文字を確認できませんでした。"),
        ));
    }
    let uncertain = observation
        .lines
        .iter()
        .filter(|line| line.has_uncertainty())
        .count();
    if uncertain > 0 {
        units.push(format!(
            "認識が不確かな文字を含む行が {uncertain} 行あります。表示中の該当箇所は［不明］です。"
        ));
    }
    let limit = match operation {
        Operation::Extract => MAX_SUMMARY_QUOTES_PER_PDF,
        Operation::Find(_) => MAX_FIND_QUOTES_PER_FILE,
    };
    for (index, line) in observation.lines.iter().enumerate() {
        check_deadline(deadline)?;
        if !line.has_text() || matches!(operation, Operation::Find(query) if !line.contains(query))
        {
            continue;
        }
        report.candidate_count += 1;
        if line.text.len() > MAX_FIND_LINE_BYTES {
            report.overlong_matches += 1;
        } else if report.quotes.len() < limit {
            report.quotes.push(Quote {
                page,
                line: index + 1,
                source_start: 0,
                source_end: line.text.len(),
                text: &line.text,
                pixels: Some(line.bounds),
                offset_space: page.map(|_| "OCR_LINE_UTF8"),
                coordinate_space: page.map(|_| "PDF_PAGE_RASTER_PIXELS"),
            });
        }
    }
    Ok(has_text)
}

fn compute(
    files: &[PreparedFile<'_>],
    manifest_ref: &str,
    operation: Operation<'_>,
    deadline: Instant,
) -> Result<Computation> {
    let contains_pdf = files
        .iter()
        .any(|file| file.source.media_type == Some("application/pdf"));
    let contains_image = files
        .iter()
        .any(|file| file.source.image_observation.is_some() || file.source.pdf_scan.is_some());
    let (operation_kind, query, heading) = match operation {
        Operation::Extract if contains_image => (
            "EXTRACT",
            None,
            "添付資料から確認した文字を抜粋します。画像にはOCRを使いました。".into(),
        ),
        Operation::Extract => ("EXTRACT", None, EXTRACTIVE_SUMMARY_LABEL.to_owned()),
        Operation::Find(query) if contains_image => (
            "FIND",
            Some(query),
            format!("添付資料から確認した文字のうち「{query}」を含む行を検索しました。"),
        ),
        Operation::Find(query) if contains_pdf => (
            "FIND",
            Some(query),
            format!("添付文書のテキストから「{query}」を含む行を検索しました。"),
        ),
        Operation::Find(query) => (
            "FIND",
            Some(query),
            format!("添付文書から「{query}」を含む原文の行を検索しました。"),
        ),
    };
    let mut units = vec![heading];
    let mut reports = Vec::with_capacity(files.len());
    for file in files {
        check_deadline(deadline)?;
        let source = &file.source;
        units.push(source_heading(source.name, &source.sha256));
        let mut report = FileReport {
            source,
            candidate_count: 0,
            overlong_matches: 0,
            quotes: Vec::new(),
            offset_space: None,
            pdf_pages: Vec::new(),
            failure: None,
        };
        let has_text = match &file.content {
            FileContent::Text(text) => {
                collect_from_text(text, None, operation, &mut report, deadline)?;
                true
            }
            FileContent::Pdf(pages) => {
                collect_from_pdf(pages, None, operation, &mut report, &mut units, deadline)?
            }
            FileContent::ScannedPdf(document) => collect_from_pdf(
                &document.pages,
                Some(&document.scans),
                operation,
                &mut report,
                &mut units,
                deadline,
            )?,
            FileContent::Image { observation, .. } => collect_from_image(
                observation,
                None,
                operation,
                &mut report,
                &mut units,
                deadline,
            )?,
            FileContent::UnreadablePdf(observation) => {
                report.failure = Some(*observation);
                units.push(format!("{}（{}）", observation.message, observation.code));
                false
            }
        };
        let count = report.candidate_count;
        let shown = report.quotes.len();
        if has_text {
            units.push(match operation {
                Operation::Extract if report.offset_space == Some("OCR_LINE_UTF8") => {
                    format!("表示 {shown} 行／OCRで確認した {count} 行。")
                }
                Operation::Extract if report.offset_space.is_some() => {
                    format!("抜粋 {shown} 件／抽出テキストの候補 {count} 件。")
                }
                Operation::Extract => format!("抜粋 {shown} 件／原文の候補 {count} 件。"),
                Operation::Find(_) if count == 0 && report.offset_space.is_some() => {
                    "抽出できたテキストに一致する行はありません。".into()
                }
                Operation::Find(_) if count == 0 => "一致する行はありません。".into(),
                Operation::Find(_) if shown == count => format!("一致 {count} 行。"),
                Operation::Find(_) => format!(
                    "一致 {count} 行のうち {shown} 行を表示します。長さの上限を超えた行は {} 行です。",
                    report.overlong_matches
                ),
            });
        }
        for quote in &report.quotes {
            let mut unit = quote_prefix(
                quote.page,
                quote.line,
                quote.source_start..quote.source_end,
                quote.pixels,
            );
            unit.push_str(quote.text);
            units.push(unit);
        }
        reports.push(report);
    }
    let report_bytes = canonical(&DocumentReport {
        kind: REPORT_KIND,
        manifest_sha256: manifest_ref,
        operation: operation_kind,
        query,
        files: reports,
    })?;
    Ok(Computation {
        units,
        report_bytes,
    })
}

fn anchored_context<'a>(state: &'a CaseRevision, bundle: &InputBundle) -> Result<&'a ResultRecord> {
    let context = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|id| state.prior_results.get(id).filter(|prior| &prior.id == id))
        .ok_or_else(invalid_context)?;
    if identity(&Some(context))? != bundle.context_sha256 {
        return Err(invalid_context());
    }
    Ok(context)
}

fn registered(store: &Store, state: &CaseRevision, result: &ResultRecord) -> Result<InputBundle> {
    let (_, bundle) = store
        .get_input_bundle(
            &state.actor_id,
            &state.case_id,
            &result.input_id,
            result.input_version,
        )?
        .ok_or_else(invalid_context)?;
    if result.lifecycle != Lifecycle::Commit
        || result.guarantee != Guarantee::SourceReport
        || identity(&bundle.request)? != result.id
        || bundle.request.actor_id != state.actor_id
        || bundle.request.case_id != state.case_id
        || store
            .get_input_result(
                &state.actor_id,
                &state.case_id,
                &result.input_id,
                result.input_version,
            )?
            .as_ref()
            != Some(result)
    {
        return Err(invalid_context());
    }
    verify_attached_document_input(&bundle)?;
    Ok(bundle)
}

fn verify_result(
    store: &Store,
    result: &ResultRecord,
    bundle: &InputBundle,
    computed: Computation,
    manifest_ref: &str,
    mut lineage: Vec<String>,
) -> Result<()> {
    let request = &bundle.request;
    let report_ref = digest(&computed.report_bytes);
    lineage.extend([manifest_ref.to_owned(), report_ref.clone()]);
    if result.dependencies != lineage
        || result.text != computed.units.join("\n")
        || result.content_plan.as_ref().is_none_or(|plan| {
            plan.required_units != computed.units
                || !plan
                    .dependencies
                    .iter()
                    .any(|dependency| dependency == manifest_ref)
        })
        || store.get_bytes(&request.actor_id, &request.case_id, &report_ref)?
            != computed.report_bytes
    {
        return Err(invalid_context());
    }
    revalidate_previous_document_render(
        store,
        &request.actor_id,
        &request.case_id,
        result,
        Some(&report_ref),
        CONTEXT_ERROR,
        "attached document",
    )?;
    Ok(())
}

fn lineage(origin: &str, parent: &str) -> Vec<String> {
    let mut references = vec![origin.to_owned()];
    if origin != parent {
        references.push(parent.to_owned());
    }
    references
}

#[derive(Serialize)]
struct ImageTableReport<'a> {
    kind: &'static str,
    manifest_sha256: &'a str,
    source: &'a ManifestSource<'a>,
    row: &'a str,
    column: &'a str,
    amount: bool,
    selection: CellLookup,
}

fn image_table_attachment_scope() -> Error {
    failure(
        "IMAGE_TABLE_ATTACHMENT_SCOPE",
        "表のセルを読むには、PNGまたはJPEG画像を1枚指定してください。",
    )
}

fn table_coordinates(label: &str, bounds: Bounds) -> String {
    format!(
        "{label} 左上 ({}, {}) 幅 {} 高さ {} px",
        bounds.x, bounds.y, bounds.width, bounds.height
    )
}

fn is_printed_amount(text: &str) -> bool {
    static AMOUNT: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"\A(?:[￥¥][ 　]*)?(?:[0-9０-９]{1,3}(?:[,，][0-9０-９]{3})+|[0-9０-９]+)(?:[.．][0-9０-９]+)?(?:[ 　]*円)?\z",
        )
        .expect("visible currency syntax")
    });
    AMOUNT.is_match(text)
}

fn compute_image_table(
    files: &[PreparedFile<'_>],
    manifest_ref: &str,
    row: &str,
    column: &str,
    amount: bool,
    deadline: Instant,
) -> Result<Computation> {
    let [file] = files else {
        return Err(image_table_attachment_scope());
    };
    let FileContent::Image {
        observation,
        raster,
    } = &file.content
    else {
        return Err(image_table_attachment_scope());
    };
    let media_type = file.source.media_type.ok_or_else(invalid_context)?;
    let selection =
        match image_table::read_cell(raster, media_type, observation, row, column, deadline)? {
            CellLookup::Found { ref value, .. } if amount && !is_printed_amount(&value.text) => {
                CellLookup::Unknown {
                    reason: "AMOUNT_UNREADABLE",
                }
            }
            read => read,
        };
    let mut units = vec![source_heading(file.source.name, &file.source.sha256)];
    match &selection {
        CellLookup::Found {
            row: row_label,
            column: heading,
            value,
        } => {
            units.push(format!(
                "「{row}」の行と「{column}」の列が交わるセルの値は「{}」です。",
                value.text
            ));
            units.push(table_coordinates("行見出し", row_label.pixels));
            units.push(table_coordinates("列見出し", heading.pixels));
            units.push(table_coordinates("値の文字", value.pixels));
            units.push(table_coordinates("交点セル", value.cell));
        }
        CellLookup::Unknown { reason } => {
            units.push(format!(
                "「{row}」の行と「{column}」の列で指定されたセルの値を確認できませんでした。"
            ));
            let detail = match *reason {
                "LABEL_NOT_FOUND" => "指定された行名または列名が見つかりませんでした。",
                "LABEL_AMBIGUOUS" => "同じ行名または列名が複数あります。",
                "LABEL_UNREADABLE" => "行または列の見出しに読み取れない文字があります。",
                "VALUE_UNREADABLE" => "交点のセルで、表示できる文字を確認できませんでした。",
                "AMOUNT_UNREADABLE" => "読み取った文字を金額として確定できませんでした。",
                "TEXT_TOUCHES_RULE" | "TEXT_CROSSES_RULE" => {
                    "文字の範囲を罫線から分離できませんでした。"
                }
                _ => "画像上で一つの完全な罫線表を確認できませんでした。",
            };
            units.push(detail.into());
        }
    }
    let report_bytes = canonical(&ImageTableReport {
        kind: "minidora.image-table-cell-report.v1",
        manifest_sha256: manifest_ref,
        source: &file.source,
        row,
        column,
        amount,
        selection,
    })?;
    Ok(Computation {
        units,
        report_bytes,
    })
}

fn compute_bundle(
    files: &[PreparedFile<'_>],
    manifest_ref: &str,
    bundle: &InputBundle,
    deadline: Instant,
) -> Result<Computation> {
    if let Intent::ReviewAttachedDocumentFields {
        operation, fields, ..
    } = &bundle.meaning.intent
    {
        return review::compute(files, manifest_ref, *operation, fields, deadline);
    }
    if let Intent::SelectImageTableCell {
        row,
        column,
        amount,
        ..
    } = &bundle.meaning.intent
    {
        return compute_image_table(
            files,
            manifest_ref,
            &row.text,
            &column.text,
            *amount,
            deadline,
        );
    }
    compute(
        files,
        manifest_ref,
        Operation::from_bundle(bundle)?,
        deadline,
    )
}

pub(crate) fn produce(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
    started: Instant,
    retained_result: Option<&ResultRecord>,
) -> Result<DocumentOutcome> {
    verify_attached_document_input(bundle)?;
    if bundle.request.actor_id != state.actor_id || bundle.request.case_id != state.case_id {
        return Err(invalid_context());
    }
    let budget = &bundle.request.budget;
    let deadline = started
        .checked_add(Duration::from_millis(budget.deadline_ms))
        .ok_or_else(resource_limit)?;
    check_deadline(deadline)?;
    let prior = if input::is_previous_attached_document_intent(&bundle.meaning.intent) {
        let selected = anchored_context(state, bundle)?;
        let selected_bundle = registered(store, state, selected)?;
        let origin = if input::is_previous_attached_document_intent(&selected_bundle.meaning.intent)
        {
            let origin_id = selected.dependencies.first().ok_or_else(invalid_context)?;
            let origin = state
                .prior_results
                .get(origin_id)
                .filter(|record| &record.id == origin_id && record.id != selected.id)
                .ok_or_else(invalid_context)?;
            Some((origin, registered(store, state, origin)?))
        } else {
            None
        };
        Some((selected, selected_bundle, origin))
    } else {
        None
    };
    let origin_bundle = prior
        .as_ref()
        .map_or(bundle, |(_, selected_bundle, origin)| {
            origin
                .as_ref()
                .map_or(selected_bundle, |(_, source)| source)
        });
    if !input::is_attached_document_intent(&origin_bundle.meaning.intent)
        || input::is_previous_attached_document_intent(&origin_bundle.meaning.intent)
    {
        return Err(invalid_context());
    }
    let origin_profile = if origin_bundle.meaning.parser_version == PURPOSE_DOCUMENT_PARSER_VERSION
    {
        let profile = purpose_profile::verify_binding(store, &origin_bundle.request)?
            .ok_or_else(invalid_context)?;
        input::validate_purpose_attachments(&origin_bundle.request, &profile.attachments)?;
        Some(profile)
    } else {
        None
    };
    let origin_attachments = origin_profile
        .as_ref()
        .map_or(origin_bundle.request.attachments.as_slice(), |profile| {
            profile.attachments.as_slice()
        });
    if matches!(bundle.meaning.intent, Intent::SelectImageTableCell { .. })
        && !matches!(origin_attachments, [source] if RequestAttachment::is_image_name(&source.name))
    {
        return Err(image_table_attachment_scope());
    }
    let has_images = origin_attachments
        .iter()
        .any(|file| RequestAttachment::is_image_name(&file.name));
    let saved_result = prior
        .as_ref()
        .map(|(selected, _, origin)| origin.as_ref().map_or(*selected, |(record, _)| *record))
        .or(retained_result);
    let retained_observations = if let Some(record) = saved_result {
        let [.., manifest_ref, _] = record.dependencies.as_slice() else {
            return Err(invalid_context());
        };
        let raw = store.get_bytes(&state.actor_id, &state.case_id, manifest_ref)?;
        let (kind, sources): (String, Vec<RetainedSource>) = serde_json::from_slice(&raw)?;
        if !matches!(
            kind.as_str(),
            "minidora.attached-document-manifest.v1"
                | "minidora.attached-document-manifest.v2"
                | "minidora.attached-document-manifest.v3"
        ) || sources.len() != origin_attachments.len()
        {
            return Err(invalid_context());
        }
        let mut observations = RetainedObservations::default();
        for source in sources {
            if let Some(reference) = source.image_observation
                && observations
                    .images
                    .insert(source.name.clone(), reference)
                    .is_some()
            {
                return Err(invalid_context());
            }
            if let Some(reference) = source.pdf_scan
                && observations.scans.insert(source.name, reference).is_some()
            {
                return Err(invalid_context());
            }
        }
        if (kind == "minidora.attached-document-manifest.v3") == observations.scans.is_empty()
            || has_images == observations.images.is_empty()
        {
            return Err(invalid_context());
        }
        Some(observations)
    } else {
        None
    };
    let image_preparation = ImagePreparation {
        store,
        actor: &state.actor_id,
        case: &state.case_id,
        retained: retained_observations.as_ref(),
        max_evidence_bytes: budget.max_evidence_bytes,
        table_cells: matches!(
            origin_bundle.meaning.intent,
            Intent::SelectImageTableCell {
                previous: false,
                ..
            }
        ),
    };
    let files = prepare_files(
        origin_attachments.iter(),
        deadline,
        false,
        Some(&image_preparation),
    )?;
    let manifest = files.iter().map(|file| &file.source).collect::<Vec<_>>();
    let manifest_kind = if manifest.iter().any(|source| source.pdf_scan.is_some()) {
        "minidora.attached-document-manifest.v3"
    } else if has_images {
        "minidora.attached-document-manifest.v2"
    } else {
        "minidora.attached-document-manifest.v1"
    };
    let manifest_bytes = canonical(&(manifest_kind, &manifest))?;
    let manifest_ref = digest(&manifest_bytes);
    let mut dependencies = Vec::new();
    if let Some((selected, selected_bundle, source)) = &prior {
        let origin = source.as_ref().map_or(*selected, |(result, _)| *result);
        if store.get_bytes(&state.actor_id, &state.case_id, &manifest_ref)? != manifest_bytes {
            return Err(invalid_context());
        }
        let compute_saved =
            |saved: &InputBundle| compute_bundle(&files, &manifest_ref, saved, deadline);
        verify_result(
            store,
            origin,
            origin_bundle,
            compute_saved(origin_bundle)?,
            &manifest_ref,
            Vec::new(),
        )?;
        if source.is_some() {
            let parent = anchored_context(state, selected_bundle)?;
            if parent.id == selected.id {
                return Err(invalid_context());
            }
            if parent.id != origin.id {
                let parent_bundle = registered(store, state, parent)?;
                if !input::is_previous_attached_document_intent(&parent_bundle.meaning.intent)
                    || parent.dependencies.first() != Some(&origin.id)
                {
                    return Err(invalid_context());
                }
            }
            verify_result(
                store,
                selected,
                selected_bundle,
                compute_saved(selected_bundle)?,
                &manifest_ref,
                lineage(&origin.id, &parent.id),
            )?;
        }
        dependencies = lineage(&origin.id, &selected.id);
    }
    let Computation {
        units,
        report_bytes,
    } = compute_bundle(&files, &manifest_ref, bundle, deadline)?;
    let image_bytes = manifest
        .iter()
        .filter_map(|source| source.image_observation.as_ref())
        .map(|image| image.byte_count)
        .sum::<usize>();
    let scan_bytes = manifest
        .iter()
        .filter_map(|source| source.pdf_scan.as_ref())
        .map(|proof| proof.byte_count)
        .sum::<usize>();
    if report_bytes
        .len()
        .saturating_add(manifest_bytes.len())
        .saturating_add(image_bytes)
        .saturating_add(scan_bytes)
        .saturating_add(units.join("\n").len())
        > budget.max_evidence_bytes
    {
        return Err(resource_limit());
    }
    check_deadline(deadline)?;
    dependencies.extend([manifest_ref.clone(), digest(&report_bytes)]);
    Ok(DocumentOutcome {
        units,
        report_bytes,
        manifest_bytes,
        manifest_ref,
        dependencies,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attached_document_field_checks_past_the_quote_limit_and_keeps_the_late_conflict() {
        let west = " - **範囲**：西";
        let east = "## 範囲は「東」です。 ##";
        let text = format!(
            "\u{feff}{}\r\n{east}\r\n",
            std::iter::repeat_n(west, MAX_FIELD_WITNESSES + 1)
                .collect::<Vec<_>>()
                .join("\r\n")
        );
        let attachment = RequestAttachment {
            name: "資料.md".into(),
            text,
            binary_base64: None,
        };
        let report = lookup_field(
            &[&attachment],
            "範囲",
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        assert!(report.value.is_none());
        assert_eq!(report.matched_count, MAX_FIELD_WITNESSES + 2);
        assert_eq!(report.matches.len(), MAX_FIELD_WITNESSES);
        assert_eq!(report.matches[0].source_start, 3);
        let conflict = report.matches.last().unwrap();
        assert_eq!(conflict.value, "東");
        assert_eq!(conflict.line, MAX_FIELD_WITNESSES + 2);
        assert_eq!(conflict.quote, east);
        assert_eq!(
            &attachment.text[conflict.source_start..conflict.source_end],
            conflict.quote
        );
        assert!(matches!(
            field_assignment("範囲は西か東です。", "範囲"),
            Some(FieldAssignment::Uncertain)
        ));
    }

    #[test]
    fn attached_document_legacy_text_keeps_the_committed_manifest_report_and_render() {
        let inputs = [
            RequestAttachment {
                name: "展示 日程.md".into(),
                text: "\u{feff}# 秋の展示\r\n開催日は10月4日です。\r\n参加費は800円です。\r\n搬入は午前9時からです。\r\n".into(),
                binary_base64: None,
            },
            RequestAttachment {
                name: "当日連絡.txt".into(),
                text: "受付は東口です。\n参加費は1000円です。\n観覧は16時までです。\n".into(),
                binary_base64: None,
            },
        ];
        let deadline = Instant::now() + Duration::from_secs(10);
        let files = prepare_files(inputs.iter(), deadline, false, None).unwrap();
        let source_refs = files.iter().map(|file| &file.source).collect::<Vec<_>>();
        let manifest = canonical(&("minidora.attached-document-manifest.v1", source_refs)).unwrap();
        let manifest_ref = digest(&manifest);
        assert_eq!(
            manifest_ref,
            "7bbe35b60ad009c87988b28543f9080ba7d59be32cf9948b3e07622b488cfb99"
        );
        let computation = compute(&files, &manifest_ref, Operation::Extract, deadline).unwrap();
        assert_eq!(
            digest(&computation.report_bytes),
            "00db9b3c74f240437f0884ce84eae8c60a5181274b7477143548c3f5d1dbe50d"
        );
        assert_eq!(
            digest(computation.units.join("\n").as_bytes()),
            "4680f7f1d3b271f00aaf273b3084314282da413c61c70a9fc2eec3ac5482934d"
        );
    }

    #[test]
    fn attached_document_pdf_uses_the_extracted_page_coordinates_and_keeps_scan_residuals() {
        let matched = "検査の記録です。";
        let extracted = format!("先頭の行\r\n{matched}\n");
        let files = [
            PreparedFile {
                source: ManifestSource {
                    name: "現場.pdf",
                    byte_count: 120,
                    sha256: digest(b"synthetic-fixture-raw-pdf"),
                    media_type: Some("application/pdf"),
                    image_observation: None,
                    pdf_scan: None,
                },
                content: FileContent::Pdf(vec![
                    Page {
                        number: 1,
                        text: String::new(),
                        state: PageState::ImageWithoutText,
                    },
                    Page {
                        number: 2,
                        text: extracted.clone(),
                        state: PageState::Text,
                    },
                    Page {
                        number: 3,
                        text: String::new(),
                        state: PageState::NoExtractableText,
                    },
                ]),
            },
            PreparedFile {
                source: ManifestSource {
                    name: "連絡.txt",
                    byte_count: "検査済み".len(),
                    sha256: digest("検査済み".as_bytes()),
                    media_type: None,
                    image_observation: None,
                    pdf_scan: None,
                },
                content: FileContent::Text("検査済み"),
            },
            PreparedFile {
                source: ManifestSource {
                    name: "暗号化.pdf",
                    byte_count: 51,
                    sha256: digest(b"synthetic-unreadable-pdf"),
                    media_type: Some("application/pdf"),
                    image_observation: None,
                    pdf_scan: None,
                },
                content: FileContent::UnreadablePdf(
                    PdfFailure::from_error(&failure("PDF_ENCRYPTED", "fixture")).unwrap(),
                ),
            },
        ];
        let outcome = compute(
            &files,
            "report-fixture",
            Operation::Find("検査"),
            Instant::now() + Duration::from_secs(10),
        )
        .unwrap();
        let report: serde_json::Value = serde_json::from_slice(&outcome.report_bytes).unwrap();
        let pdf = &report["files"][0];
        assert_eq!(pdf["offset_space"], "EXTRACTED_PAGE_UTF8");
        assert_eq!(pdf["pdf_pages"][0]["state"], "IMAGE_WITHOUT_TEXT");
        assert_eq!(pdf["pdf_pages"][2]["state"], "NO_EXTRACTABLE_TEXT");
        assert_eq!(
            pdf["pdf_pages"][1]["extracted_sha256"],
            digest(extracted.as_bytes())
        );
        let quote = &pdf["quotes"][0];
        assert_eq!(quote["page"], 2);
        assert_eq!(quote["line"], 2);
        let start = quote["source_start"].as_u64().unwrap() as usize;
        let end = quote["source_end"].as_u64().unwrap() as usize;
        assert_eq!(&extracted[start..end], matched);
        assert_eq!(quote["text"], matched);
        assert!(report["files"][1]["offset_space"].is_null());
        assert_eq!(report["files"][1]["quotes"][0]["text"], "検査済み");
        assert_eq!(report["files"][2]["failure"]["code"], "PDF_ENCRYPTED");
        assert_eq!(
            report["files"][2]["source"]["sha256"],
            digest(b"synthetic-unreadable-pdf")
        );
        assert!(report["files"][2]["quotes"].as_array().unwrap().is_empty());
        let output = outcome.units.join("\n");
        assert!(output.contains("1ページで画像を確認しました"));
        assert!(output.contains("2ページ 抽出テキスト 2行目"));
        assert!(output.contains("3ページでは検索・抜粋に使える文字を抽出できませんでした"));
    }

    #[test]
    fn attached_document_find_counts_long_matches_without_truncating_exact_lines() {
        let files = [RequestAttachment {
            name: "資料.txt".into(),
            text: format!(
                "\u{feff}合致{}\r  合致  \r\n別件",
                "x".repeat(MAX_FIND_LINE_BYTES)
            ),
            binary_base64: None,
        }];
        let original = &files[0].text;
        let deadline = Instant::now() + Duration::from_secs(10);
        let files = prepare_files(files.iter(), deadline, false, None).unwrap();
        let computation =
            compute(&files, "report-fixture", Operation::Find("合致"), deadline).unwrap();
        let report: serde_json::Value = serde_json::from_slice(&computation.report_bytes).unwrap();
        let file = &report["files"][0];
        assert_eq!(file["candidate_count"], 2);
        assert_eq!(file["overlong_matches"], 1);
        let quote = &file["quotes"][0];
        assert_eq!(file["quotes"].as_array().unwrap().len(), 1);
        assert_eq!(quote["line"], 2);
        let start = quote["source_start"].as_u64().unwrap() as usize;
        let end = quote["source_end"].as_u64().unwrap() as usize;
        assert_eq!(&original[start..end], "  合致  ");
        assert_eq!(quote["text"], "  合致  ");
        assert!(
            computation
                .units
                .iter()
                .any(|unit| unit.contains("一致 2 行のうち 1 行"))
        );
    }
}
