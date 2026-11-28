//! Inspect PDF bytes in the bounded PDF worker. No file access or PDF actions.

use crate::pdf_worker::{MAX_PAGES, MAX_TEXT_BYTES, Page, PageState};
use lopdf::{
    DecompressError, Dictionary, Document, LoadOptions, Object, ObjectId, Stream, content::Content,
};
use minidora_domain::{Error, MAX_PDF_ATTACHMENT_BYTES, Result, failure};
use std::collections::BTreeSet;
use std::time::Instant;

const MAX_OBJECTS: usize = 8_192;
const MAX_PAGE_TREE_DEPTH: usize = 32;
const MAX_FORM_DEPTH: usize = 12;
const MAX_STREAMS_PER_PAGE: usize = 64;
const MAX_FONTS_PER_PAGE: usize = 64;
const MAX_STREAM_BYTES: usize = 512 * 1024;
const MAX_CONTENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_OPERATIONS: usize = 65_536;

fn deadline_check(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "PDF_DEADLINE",
            "PDFの読み取りが依頼の期限を超えました。",
        ));
    }
    Ok(())
}

fn limit() -> Error {
    failure(
        "PDF_RESOURCE_LIMIT",
        "PDFの容量、ページ数、構造、または抽出量が上限を超えました。",
    )
}

fn invalid() -> Error {
    failure(
        "PDF_INVALID",
        "PDFの構造または本文のストリームを読み取れませんでした。",
    )
}

fn encrypted() -> Error {
    failure("PDF_ENCRYPTED", "暗号化されたPDFは読み取れません。")
}

fn pdf_error(error: lopdf::Error) -> Error {
    match error {
        lopdf::Error::InvalidPassword
        | lopdf::Error::Decryption(_)
        | lopdf::Error::UnsupportedSecurityHandler(_) => encrypted(),
        lopdf::Error::Decompress(DecompressError::MemoryLimitExceeded { .. })
        | lopdf::Error::ReferenceLimit
        | lopdf::Error::RecursionLimit => limit(),
        _ => invalid(),
    }
}

struct Work {
    deadline: Instant,
    content_left: usize,
    operations_left: usize,
}

impl Work {
    fn charge_content(&mut self, bytes: usize) -> Result<()> {
        self.content_left = self.content_left.checked_sub(bytes).ok_or_else(limit)?;
        Ok(())
    }

    fn charge_operations(&mut self, count: usize) -> Result<()> {
        self.operations_left = self.operations_left.checked_sub(count).ok_or_else(limit)?;
        deadline_check(self.deadline)
    }
}

pub(crate) fn read(bytes: &[u8], deadline: Instant) -> Result<Vec<Page>> {
    deadline_check(deadline)?;
    if bytes.len() > MAX_PDF_ATTACHMENT_BYTES {
        return Err(limit());
    }
    if !bytes.starts_with(b"%PDF-") {
        return Err(invalid());
    }
    let document = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            strict: true,
            max_decompressed_size: Some(MAX_STREAM_BYTES),
            ..LoadOptions::default()
        },
    )
    .map_err(pdf_error)?;
    deadline_check(deadline)?;
    if document.was_encrypted() || document.trailer.has(b"Encrypt") || document.is_encrypted() {
        return Err(encrypted());
    }
    if document.xref_recovery.is_some() {
        return Err(invalid());
    }
    if document.objects.len() > MAX_OBJECTS || document.reference_table.entries.len() > MAX_OBJECTS
    {
        return Err(limit());
    }

    let page_ids = page_ids(&document, deadline)?;
    let mut work = Work {
        deadline,
        content_left: MAX_CONTENT_BYTES,
        operations_left: MAX_OPERATIONS,
    };
    let mut text_left = MAX_TEXT_BYTES;
    let mut pages = Vec::with_capacity(page_ids.len());
    for (index, id) in page_ids.into_iter().enumerate() {
        deadline_check(deadline)?;
        let number = (index + 1) as u32;
        let page = document.get_dictionary(id).map_err(pdf_error)?;
        let resources = page_resources(&document, id)?;
        let content = page_content(&document, page, &mut work)?;
        let mut observation = Observation::default();
        inspect_content(
            &document,
            &content,
            resources,
            &mut observation,
            &mut BTreeSet::new(),
            &mut work,
        )?;
        let mut text = if observation.shows_text {
            let fonts = document.get_page_fonts(id).map_err(pdf_error)?;
            if fonts.len() > MAX_FONTS_PER_PAGE {
                return Err(limit());
            }
            document
                .extract_text_with_limit(&[number], MAX_STREAM_BYTES)
                .map_err(pdf_error)?
        } else {
            String::new()
        };
        deadline_check(deadline)?;
        text_left = text_left.checked_sub(text.len()).ok_or_else(limit)?;
        let state = if text
            .chars()
            .any(|c| !c.is_whitespace() && !c.is_control() && c != '\u{fffd}')
        {
            PageState::Text
        } else if observation.paints_image && !observation.shows_text && !observation.unresolved {
            PageState::ImageWithoutText
        } else {
            PageState::NoExtractableText
        };
        if state != PageState::Text {
            text.clear();
        }
        pages.push(Page {
            number,
            text,
            state,
        });
    }
    Ok(pages)
}

fn page_ids(document: &Document, deadline: Instant) -> Result<Vec<ObjectId>> {
    let root = document
        .catalog()
        .and_then(|catalog| catalog.get(b"Pages"))
        .and_then(Object::as_reference)
        .map_err(pdf_error)?;
    let count = document
        .get_dictionary(root)
        .and_then(|root| root.get_deref(b"Count", document))
        .and_then(Object::as_i64)
        .map_err(pdf_error)?;
    let count = usize::try_from(count).map_err(|_| invalid())?;
    if count > MAX_PAGES {
        return Err(limit());
    }

    let mut pages = Vec::with_capacity(count);
    let mut seen = BTreeSet::new();
    let mut pending = vec![(root, None, 0)];
    while let Some((id, parent, depth)) = pending.pop() {
        deadline_check(deadline)?;
        if depth > MAX_PAGE_TREE_DEPTH || seen.len().saturating_add(pending.len()) >= MAX_OBJECTS {
            return Err(limit());
        }
        if !seen.insert(id) {
            return Err(invalid());
        }
        let node = document.get_dictionary(id).map_err(pdf_error)?;
        match parent {
            Some(parent)
                if node.get(b"Parent").and_then(Object::as_reference).ok() != Some(parent) =>
            {
                return Err(invalid());
            }
            None if node.has(b"Parent") => return Err(invalid()),
            _ => {}
        }
        match node
            .get(b"Type")
            .and_then(Object::as_name)
            .map_err(pdf_error)?
        {
            b"Pages" => {
                let kids = node
                    .get_deref(b"Kids", document)
                    .and_then(Object::as_array)
                    .map_err(pdf_error)?;
                if seen
                    .len()
                    .saturating_add(pending.len())
                    .saturating_add(kids.len())
                    > MAX_OBJECTS
                {
                    return Err(limit());
                }
                for child in kids.iter().rev() {
                    pending.push((
                        child.as_reference().map_err(pdf_error)?,
                        Some(id),
                        depth + 1,
                    ));
                }
            }
            b"Page" if parent.is_some() => {
                if pages.len() >= MAX_PAGES {
                    return Err(limit());
                }
                pages.push(id);
            }
            _ => return Err(invalid()),
        }
    }
    if pages.is_empty() || pages.len() != count || !document.page_iter().eq(pages.iter().copied()) {
        return Err(invalid());
    }
    Ok(pages)
}

fn page_resources(document: &Document, mut id: ObjectId) -> Result<Option<&Dictionary>> {
    for _ in 0..=MAX_PAGE_TREE_DEPTH {
        let node = document.get_dictionary(id).map_err(pdf_error)?;
        if node.has(b"Resources") {
            return node
                .get_deref(b"Resources", document)
                .and_then(Object::as_dict)
                .map(Some)
                .map_err(pdf_error);
        }
        if !node.has(b"Parent") {
            return Ok(None);
        }
        id = node
            .get(b"Parent")
            .and_then(Object::as_reference)
            .map_err(pdf_error)?;
    }
    Err(limit())
}

fn page_content(document: &Document, page: &Dictionary, work: &mut Work) -> Result<Vec<u8>> {
    if !page.has(b"Contents") {
        return Ok(Vec::new());
    }
    let contents = page.get_deref(b"Contents", document).map_err(pdf_error)?;
    let mut data = Vec::new();
    match contents {
        Object::Null => {}
        Object::Stream(stream) => append_stream(stream, &mut data, work)?,
        Object::Array(streams) if streams.len() <= MAX_STREAMS_PER_PAGE => {
            for stream in streams {
                let id = stream.as_reference().map_err(pdf_error)?;
                let stream = document
                    .get_object(id)
                    .and_then(Object::as_stream)
                    .map_err(pdf_error)?;
                append_stream(stream, &mut data, work)?;
            }
        }
        Object::Array(_) => return Err(limit()),
        _ => return Err(invalid()),
    }
    Ok(data)
}

fn append_stream(stream: &Stream, output: &mut Vec<u8>, work: &mut Work) -> Result<()> {
    deadline_check(work.deadline)?;
    if stream.dict.has(b"F") {
        return Err(invalid());
    }
    // get_page_content_with_limit falls back to raw encoded bytes on decode
    // errors. Use the bounded stream API directly and fail on those errors.
    let room = MAX_STREAM_BYTES
        .saturating_sub(output.len())
        .min(work.content_left)
        .checked_sub(1)
        .ok_or_else(limit)?;
    let bytes = stream
        .get_plain_content_with_limit(room)
        .map_err(pdf_error)?;
    work.charge_content(bytes.len() + 1)?;
    output.extend_from_slice(&bytes);
    output.push(b'\n');
    Ok(())
}

#[derive(Default)]
struct Observation {
    paints_image: bool,
    shows_text: bool,
    unresolved: bool,
}

fn inspect_content(
    document: &Document,
    bytes: &[u8],
    resources: Option<&Dictionary>,
    observation: &mut Observation,
    visited_forms: &mut BTreeSet<ObjectId>,
    work: &mut Work,
) -> Result<()> {
    deadline_check(work.deadline)?;
    if bytes.is_empty() {
        return Ok(());
    }
    let content = Content::decode_strict(bytes).map_err(pdf_error)?;
    work.charge_operations(content.operations.len())?;
    for operation in content.operations {
        match operation.operator.as_str() {
            "Tj" | "'" => match operation.operands.as_slice() {
                [Object::String(text, _)] => observation.shows_text |= !text.is_empty(),
                _ => return Err(invalid()),
            },
            "\"" => match operation.operands.as_slice() {
                [_, _, Object::String(text, _)] => observation.shows_text |= !text.is_empty(),
                _ => return Err(invalid()),
            },
            "TJ" => match operation.operands.as_slice() {
                [Object::Array(items)] => {
                    for item in items {
                        match item {
                            Object::String(text, _) => observation.shows_text |= !text.is_empty(),
                            Object::Integer(_) | Object::Real(_) => {}
                            _ => return Err(invalid()),
                        }
                    }
                }
                _ => return Err(invalid()),
            },
            "BI" => match operation.operands.as_slice() {
                [Object::Stream(image)] if !image.content.is_empty() => {
                    observation.paints_image = true;
                }
                _ => observation.unresolved = true,
            },
            "Do" => {
                let [Object::Name(name)] = operation.operands.as_slice() else {
                    return Err(invalid());
                };
                let xobject = resources
                    .ok_or_else(invalid)?
                    .get_deref(b"XObject", document)
                    .and_then(Object::as_dict)
                    .and_then(|objects| objects.get(name))
                    .map_err(pdf_error)?;
                let (id, value) = document.dereference(xobject).map_err(pdf_error)?;
                let stream = value.as_stream().map_err(pdf_error)?;
                match stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .map_err(pdf_error)?
                {
                    b"Image" => {
                        let dimension = |key| {
                            stream
                                .dict
                                .get_deref(key, document)
                                .and_then(Object::as_i64)
                                .map_err(pdf_error)
                        };
                        if !stream.content.is_empty()
                            && !stream.dict.has(b"F")
                            && dimension(b"Width")? > 0
                            && dimension(b"Height")? > 0
                        {
                            observation.paints_image = true;
                        } else {
                            observation.unresolved = true;
                        }
                    }
                    b"Form" => {
                        let id = id.ok_or_else(invalid)?;
                        if visited_forms.len() >= MAX_FORM_DEPTH {
                            return Err(limit());
                        }
                        if !visited_forms.insert(id) {
                            return Err(invalid());
                        }
                        let form_resources = if stream.dict.has(b"Resources") {
                            Some(
                                stream
                                    .dict
                                    .get_deref(b"Resources", document)
                                    .and_then(Object::as_dict)
                                    .map_err(pdf_error)?,
                            )
                        } else {
                            resources
                        };
                        let mut nested = Vec::new();
                        append_stream(stream, &mut nested, work)?;
                        inspect_content(
                            document,
                            &nested,
                            form_resources,
                            observation,
                            visited_forms,
                            work,
                        )?;
                        visited_forms.remove(&id);
                    }
                    // Other forms of rendered content cannot establish a scan.
                    _ => observation.unresolved = true,
                }
            }
            _ => {}
        }
    }
    Ok(())
}
