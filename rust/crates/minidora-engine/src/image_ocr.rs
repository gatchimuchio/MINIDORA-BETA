//! OCR observations are derived from the decoded raster and retained per attachment.
use crate::ocr_worker;
use image::{ImageFormat, ImageReader, Limits};
use minidora_domain::{Result, digest, failure};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::Cursor, time::Instant};

const MAX_SIDE: u32 = 2048;
const MAX_WORDS: usize = 512;
const MAX_WORD_BYTES: usize = 384;
const MIN_CONFIDENCE: u8 = 50;
const TABLE_READER_VERSION: &str = "tesseract-5.5.3.20260724-legacy-jpn-psm6-masked-grid-tsv-v1";
const UNCLEAR: &str = "［不明］";
const TSV_HEADER: &str =
    "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bounds {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Bounds {
    fn within(self, width: u32, height: u32) -> bool {
        self.width > 0
            && self.height > 0
            && self
                .x
                .checked_add(self.width)
                .is_some_and(|right| right <= width)
            && self
                .y
                .checked_add(self.height)
                .is_some_and(|bottom| bottom <= height)
    }

    pub(crate) fn union(self, other: Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self {
            x,
            y,
            width: (self.x + self.width).max(other.x + other.width) - x,
            height: (self.y + self.height).max(other.y + other.height) - y,
        }
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Word {
    pub(crate) bounds: Bounds,
    pub(crate) confidence_floor: u8,
    pub(crate) text: Option<String>,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Line {
    pub bounds: Bounds,
    pub text: String,
    words: Vec<Word>,
}

impl Line {
    pub(crate) fn words(&self) -> impl Iterator<Item = &Word> {
        self.words.iter()
    }

    pub fn has_text(&self) -> bool {
        self.words.iter().any(|word| word.text.is_some())
    }

    pub fn contains(&self, query: &str) -> bool {
        self.text.split(UNCLEAR).any(|text| text.contains(query))
    }

    pub fn has_uncertainty(&self) -> bool {
        self.words.iter().any(|word| word.text.is_none())
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Observation {
    pub image_sha256: String,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
    pub lines: Vec<Line>,
    reader_version: String,
    coordinate_space: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    masked_pgm_sha256: Option<String>,
}

impl Observation {
    pub(crate) fn used_rule_mask(&self) -> bool {
        self.masked_pgm_sha256.is_some()
    }
}

fn invalid_image() -> minidora_domain::Error {
    failure(
        "IMAGE_INVALID",
        "画像を指定された形式と寸法上限内で読み取れませんでした。",
    )
}

fn invalid_observation() -> minidora_domain::Error {
    failure(
        "IMAGE_OCR_OBSERVATION_INVALID",
        "画像とOCRの文字・座標を照合できませんでした。",
    )
}

fn reader<'a>(bytes: &'a [u8], media_type: &str) -> Result<ImageReader<Cursor<&'a [u8]>>> {
    let format = match media_type {
        "image/png" => ImageFormat::Png,
        "image/jpeg" => ImageFormat::Jpeg,
        _ => return Err(invalid_image()),
    };
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SIDE);
    limits.max_image_height = Some(MAX_SIDE);
    limits.max_alloc = Some(40 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

pub(crate) fn deadline_check(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "RESOURCE_LIMIT",
            "画像の読取りが期限を超えました。",
        ));
    }
    Ok(())
}

pub(crate) fn decode_rgba(bytes: &[u8], media_type: &str) -> Result<image::RgbaImage> {
    Ok(reader(bytes, media_type)?
        .decode()
        .map_err(|_| invalid_image())?
        .into_rgba8())
}

pub(crate) fn luma_on_white(pixel: &image::Rgba<u8>) -> u8 {
    let [r, g, b, alpha] = pixel.0.map(u32::from);
    let luma = (77 * r + 150 * g + 29 * b + 128) / 256;
    ((alpha * luma + (255 - alpha) * 255 + 127) / 255) as u8
}

struct OcrInput {
    width: u32,
    height: u32,
    pgm: Vec<u8>,
    masked_pgm_sha256: Option<String>,
}

fn prepare(bytes: &[u8], media_type: &str, table: bool, deadline: Instant) -> Result<OcrInput> {
    deadline_check(deadline)?;
    let rgba = decode_rgba(bytes, media_type)?;
    let (width, height) = (rgba.width(), rgba.height());
    if !(1..=MAX_SIDE).contains(&width) || !(1..=MAX_SIDE).contains(&height) {
        return Err(invalid_image());
    }
    let mut pgm = format!("P5\n{width} {height}\n255\n").into_bytes();
    let first_pixel = pgm.len();
    pgm.reserve((width * height) as usize);
    pgm.extend(rgba.pixels().map(luma_on_white));
    let masked_pgm_sha256 = if table
        && crate::image_table::mask_verified_rules(
            &mut pgm[first_pixel..],
            width,
            height,
            deadline,
        )? {
        Some(digest(&pgm))
    } else {
        None
    };
    deadline_check(deadline)?;
    Ok(OcrInput {
        width,
        height,
        pgm,
        masked_pgm_sha256,
    })
}

fn observe_prepared(
    bytes: &[u8],
    media_type: &str,
    prepared: OcrInput,
    deadline: Instant,
) -> Result<Observation> {
    let report = ocr_worker::recognize(&prepared.pgm, deadline)?;
    let lines = parse_tsv(&report, prepared.width, prepared.height)?;
    deadline_check(deadline)?;
    let reader_version = reader_version(prepared.masked_pgm_sha256.as_deref());
    Ok(Observation {
        image_sha256: digest(bytes),
        media_type: media_type.into(),
        width: prepared.width,
        height: prepared.height,
        lines,
        reader_version: reader_version.into(),
        coordinate_space: "TOP_LEFT_RASTER_PIXELS".into(),
        masked_pgm_sha256: prepared.masked_pgm_sha256,
    })
}

pub(crate) fn observe(bytes: &[u8], media_type: &str, deadline: Instant) -> Result<Observation> {
    let prepared = prepare(bytes, media_type, false, deadline)?;
    observe_prepared(bytes, media_type, prepared, deadline)
}

pub(crate) fn observe_table(
    bytes: &[u8],
    media_type: &str,
    deadline: Instant,
) -> Result<Observation> {
    let prepared = prepare(bytes, media_type, true, deadline)?;
    observe_prepared(bytes, media_type, prepared, deadline)
}

fn reader_version(masked_pgm_sha256: Option<&str>) -> &'static str {
    if masked_pgm_sha256.is_some() {
        TABLE_READER_VERSION
    } else {
        ocr_worker::READER_VERSION
    }
}

pub(crate) fn verify(observation: &Observation, bytes: &[u8], media_type: &str) -> Result<()> {
    verify_prepared(observation, bytes, media_type, None)
}

pub(crate) fn verify_table(
    observation: &Observation,
    bytes: &[u8],
    media_type: &str,
    deadline: Instant,
) -> Result<()> {
    let prepared = prepare(bytes, media_type, true, deadline)?;
    verify_prepared(
        observation,
        bytes,
        media_type,
        prepared.masked_pgm_sha256.as_deref(),
    )
}

fn verify_prepared(
    observation: &Observation,
    bytes: &[u8],
    media_type: &str,
    masked_pgm_sha256: Option<&str>,
) -> Result<()> {
    let (width, height) = reader(bytes, media_type)?
        .into_dimensions()
        .map_err(|_| invalid_image())?;
    if observation.image_sha256 != digest(bytes)
        || observation.media_type != media_type
        || observation.reader_version != reader_version(masked_pgm_sha256)
        || observation.masked_pgm_sha256.as_deref() != masked_pgm_sha256
        || observation.coordinate_space != "TOP_LEFT_RASTER_PIXELS"
        || (observation.width, observation.height) != (width, height)
        || !(1..=MAX_SIDE).contains(&width)
        || !(1..=MAX_SIDE).contains(&height)
    {
        return Err(invalid_observation());
    }
    let mut words = 0usize;
    for line in &observation.lines {
        words = words.saturating_add(line.words.len());
        if words > MAX_WORDS
            || line.words.is_empty()
            || line
                .words
                .iter()
                .any(|word| !valid_word(word, width, height))
            || line_bounds(&line.words) != Some(line.bounds)
            || line_text(&line.words) != line.text
        {
            return Err(invalid_observation());
        }
    }
    Ok(())
}

fn valid_word(word: &Word, width: u32, height: u32) -> bool {
    word.bounds.within(width, height)
        && word.confidence_floor <= 100
        && (word.confidence_floor >= MIN_CONFIDENCE) == word.text.is_some()
        && word.text.as_deref().is_none_or(|text| {
            !text.is_empty()
                && text.len() <= MAX_WORD_BYTES
                && !text.chars().any(char::is_control)
                && !text.contains(UNCLEAR)
        })
}

fn line_bounds(words: &[Word]) -> Option<Bounds> {
    words.iter().map(|word| word.bounds).reduce(Bounds::union)
}

pub(crate) fn line_text<'a>(words: impl IntoIterator<Item = &'a Word>) -> String {
    let mut line = String::new();
    for word in words {
        let next = word.text.as_deref().unwrap_or(UNCLEAR);
        if line.ends_with(|ch: char| ch.is_ascii_alphanumeric())
            && next.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        {
            line.push(' ');
        }
        line.push_str(next);
    }
    line
}

fn parse_tsv(tsv: &str, width: u32, height: u32) -> Result<Vec<Line>> {
    // The pinned Windows build can translate an existing CRLF a second time.
    let mut rows = tsv.lines().map(|row| row.trim_end_matches('\r'));
    if rows.next() != Some(TSV_HEADER) {
        return Err(invalid_observation());
    }
    let mut lines: BTreeMap<(u32, u32, u32), Vec<Word>> = BTreeMap::new();
    let mut count = 0;
    let mut page_seen = false;
    for row in rows.filter(|row| !row.is_empty()) {
        let values: Vec<_> = row.splitn(12, '\t').collect();
        let [
            level,
            page,
            block,
            paragraph,
            line,
            word_number,
            x,
            y,
            w,
            h,
            conf,
            text,
        ] = values.as_slice()
        else {
            return Err(invalid_observation());
        };
        let number = |value: &str| value.parse::<u32>().map_err(|_| invalid_observation());
        if *page != "1" {
            return Err(invalid_observation());
        }
        match *level {
            "1" if !page_seen
                && *x == "0"
                && *y == "0"
                && number(w)? == width
                && number(h)? == height =>
            {
                page_seen = true;
                continue;
            }
            "2" | "3" | "4" if page_seen => continue,
            "5" if page_seen => {}
            _ => return Err(invalid_observation()),
        }
        let key = (number(block)?, number(paragraph)?, number(line)?);
        let (conf, tail) = conf.split_once('.').unwrap_or((conf, ""));
        if !tail.bytes().all(|value| value.is_ascii_digit())
            || key.0 == 0
            || key.1 == 0
            || key.2 == 0
        {
            return Err(invalid_observation());
        }
        let confidence_floor = conf.parse::<u8>().map_err(|_| invalid_observation())?;
        let words = lines.entry(key).or_default();
        let text = text.trim();
        let word = Word {
            bounds: Bounds {
                x: number(x)?,
                y: number(y)?,
                width: number(w)?,
                height: number(h)?,
            },
            confidence_floor,
            text: (confidence_floor >= MIN_CONFIDENCE).then(|| text.to_owned()),
        };
        count += 1;
        if count > MAX_WORDS
            || number(word_number)? as usize != words.len() + 1
            || !valid_word(&word, width, height)
        {
            return Err(invalid_observation());
        }
        words.push(word);
    }
    if !page_seen {
        return Err(invalid_observation());
    }
    lines
        .into_values()
        .map(|words| {
            Ok(Line {
                bounds: line_bounds(&words).ok_or_else(invalid_observation)?,
                text: line_text(&words),
                words,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{TSV_HEADER, parse_tsv};

    #[test]
    fn image_ocr_preserves_windows_pixel_ranges_and_marks_low_confidence() {
        let page = "1\t1\t0\t0\t0\t0\t0\t0\t80\t70\t-1\t";
        let first = "5\t1\t1\t1\t1\t1\t10\t20\t14\t9\t90.25\t受付";
        let second = "5\t1\t1\t1\t1\t2\t25\t20\t8\t9\t23.00\t甲";
        let report = format!("{TSV_HEADER}\r\r\n{page}\r\r\n{first}\r\r\n{second}\r\r\n");
        let lines = parse_tsv(&report, 80, 70).unwrap();
        let [line] = lines.as_slice() else {
            panic!("expected one recognized line")
        };
        assert_eq!(line.text, "受付［不明］");
        assert_eq!(
            (
                line.bounds.x,
                line.bounds.y,
                line.bounds.width,
                line.bounds.height
            ),
            (10, 20, 23, 9)
        );
        assert!(line.contains("受付"));
        assert!(!line.contains("甲"));
        assert!(parse_tsv(&report, 81, 70).is_err());
        assert!(parse_tsv(TSV_HEADER, 80, 70).is_err());
    }
}
