//! Retain the actual rendered PDF pages and their OCR observations together.
use crate::image_ocr::{self, Observation};
use crate::pdf_worker::{self, Page, PageState};
use crate::store::Store;
use minidora_domain::{Result, canonical, digest, failure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, time::Instant};

pub(crate) const RENDERER_VERSION: &str = "windows.data.pdf-page-png-v1";
const MAX_SCAN_PAGES: usize = 8;
const MAX_TOTAL_BYTES: usize = 12 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Reference {
    sha256: String,
    pub byte_count: usize,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScannedPage {
    raster_sha256: String,
    raster_bytes: usize,
    pub ocr: Observation,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    kind: String,
    pdf_sha256: String,
    renderer_version: String,
    pub pages: Vec<Page>,
    pub scans: BTreeMap<u32, ScannedPage>,
}

pub(crate) struct Source<'a> {
    pub store: &'a Store,
    pub actor: &'a str,
    pub case: &'a str,
}

fn invalid() -> minidora_domain::Error {
    failure(
        "PDF_SCAN_OBSERVATION_INVALID",
        "PDFの原本、ページ画像、認識文字を照合できませんでした。",
    )
}

fn limit() -> minidora_domain::Error {
    failure(
        "RESOURCE_LIMIT",
        "PDFのページ画像とOCRの根拠が証拠予算を超えました。",
    )
}

fn expired(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "PDF_SCAN_DEADLINE",
            "PDFのページの読取りが期限を超えました。",
        ));
    }
    Ok(())
}

fn scan_candidates(pages: &[Page]) -> impl Iterator<Item = u32> + '_ {
    pages
        .iter()
        .filter(|page| page.state != PageState::Text)
        .map(|page| page.number)
        .take(MAX_SCAN_PAGES)
}

impl Source<'_> {
    pub fn capture(
        &self,
        pdf: &[u8],
        pages: Vec<Page>,
        max_bytes: usize,
        deadline: Instant,
    ) -> Result<(Document, Reference)> {
        let max_bytes = max_bytes.min(MAX_TOTAL_BYTES);
        let mut scans = BTreeMap::new();
        let mut rasters = Vec::new();
        let mut byte_count = 0usize;
        for number in scan_candidates(&pages) {
            expired(deadline)?;
            let raster = pdf_worker::rasterize(pdf, number, deadline)?;
            byte_count = byte_count.saturating_add(raster.len());
            if byte_count > max_bytes {
                return Err(limit());
            }
            let ocr = image_ocr::observe(&raster, "image/png", deadline)?;
            scans.insert(
                number,
                ScannedPage {
                    raster_bytes: raster.len(),
                    raster_sha256: digest(&raster),
                    ocr,
                },
            );
            rasters.push(raster);
        }
        if scans.is_empty() {
            return Err(invalid());
        }
        let document = Document {
            kind: "minidora.pdf-page-ocr.v1".into(),
            pdf_sha256: digest(pdf),
            renderer_version: RENDERER_VERSION.into(),
            pages,
            scans,
        };
        let recorded = canonical(&document)?;
        byte_count = byte_count.saturating_add(recorded.len());
        if byte_count > max_bytes {
            return Err(limit());
        }
        expired(deadline)?;
        for (scan, raster) in document.scans.values().zip(rasters) {
            if self.store.put_bytes(self.actor, self.case, &raster)? != scan.raster_sha256 {
                return Err(invalid());
            }
        }
        let sha256 = self.store.put_bytes(self.actor, self.case, &recorded)?;
        if sha256 != digest(&recorded) {
            return Err(invalid());
        }
        Ok((document, Reference { sha256, byte_count }))
    }

    pub fn restore(
        &self,
        pdf: &[u8],
        proof: &Reference,
        max_bytes: usize,
        deadline: Instant,
    ) -> Result<Document> {
        if proof.byte_count > MAX_TOTAL_BYTES.min(max_bytes) {
            return Err(limit());
        }
        expired(deadline)?;
        let raw = self.store.get_bytes(self.actor, self.case, &proof.sha256)?;
        let document: Document = serde_json::from_slice(&raw)?;
        if document.kind != "minidora.pdf-page-ocr.v1"
            || document.pdf_sha256 != digest(pdf)
            || document.renderer_version != RENDERER_VERSION
            || document.pages.is_empty()
            || document.pages.len() > 64
            || document.pages.iter().enumerate().any(|(i, page)| {
                page.number as usize != i + 1
                    || (page.state == PageState::Text) == page.text.trim().is_empty()
            })
            || document.scans.is_empty()
            || !document
                .scans
                .keys()
                .copied()
                .eq(scan_candidates(&document.pages))
            || canonical(&document)? != raw
        {
            return Err(invalid());
        }
        let mut byte_count = raw.len();
        for page in document.scans.values() {
            expired(deadline)?;
            let raster = self
                .store
                .get_bytes(self.actor, self.case, &page.raster_sha256)?;
            if raster.len() != page.raster_bytes {
                return Err(invalid());
            }
            byte_count = byte_count.saturating_add(raster.len());
            if byte_count > proof.byte_count {
                return Err(invalid());
            }
            image_ocr::verify(&page.ocr, &raster, "image/png")?;
        }
        if byte_count != proof.byte_count {
            return Err(invalid());
        }
        expired(deadline)?;
        Ok(document)
    }
}
