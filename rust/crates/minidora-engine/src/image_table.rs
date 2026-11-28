//! Locate a readable cell in a single rectangular ruled raster table.
use crate::image_ocr::{self, Bounds, Observation, Word};
use minidora_domain::{Result, failure};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Instant,
};

const DARK_LUMA: u8 = 96;
const GUTTER_LUMA: u8 = 180;
const GUTTER_WIDTH: u32 = 2;
const MIN_CELL_SIDE: u32 = 12;
const MAX_RULE_WIDTH: u32 = 8;
const MAX_RULES: usize = 24;
const MAX_RULE_GROUPS: usize = 16_384;
const ENDPOINT_TOLERANCE: u32 = 2;
const MAX_AXIS_CANDIDATES: usize = 128;
const MIN_CELL_CONFIDENCE: u8 = 70;

type Interval = (u32, u32);
type RuleGroups = BTreeMap<Interval, Vec<Interval>>;

#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "status", rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum CellLookup {
    Found {
        row: CellEvidence,
        column: CellEvidence,
        value: CellEvidence,
    },
    Unknown {
        reason: &'static str,
    },
}

#[derive(Debug, PartialEq, Eq, Serialize)]
pub(crate) struct CellEvidence {
    pub(crate) text: String,
    pub(crate) pixels: Bounds,
    pub(crate) cell: Bounds,
}

#[derive(Debug, PartialEq, Eq)]
struct Grid {
    rows: Vec<Interval>,
    columns: Vec<Interval>,
}

fn rule_groups(
    length: u32,
    lanes: u32,
    luma: impl Fn(u32, u32) -> u8,
    deadline: Instant,
) -> Result<RuleGroups> {
    let mut groups = RuleGroups::new();
    for lane in 0..lanes {
        image_ocr::deadline_check(deadline)?;
        let mut start = None;
        for position in 0..=length {
            if position < length && luma(position, lane) <= DARK_LUMA {
                start.get_or_insert(position);
            } else if let Some(first) = start.take()
                && position - first >= MIN_CELL_SIDE
            {
                let bands = groups.entry((first, position)).or_default();
                if let Some(last) = bands.last_mut()
                    && last.1 == lane
                {
                    last.1 = lane + 1;
                } else {
                    bands.push((lane, lane + 1));
                }
            }
        }
        if groups.len() > MAX_RULE_GROUPS {
            return Err(failure(
                "IMAGE_TABLE_RESOURCE_LIMIT",
                "罫線の候補数が画像表の読取り上限を超えました。",
            ));
        }
    }
    Ok(groups)
}

fn outside(bands: &[Interval]) -> Interval {
    (bands[0].0, bands[bands.len() - 1].1)
}

fn usable_bands(bands: &[Interval]) -> bool {
    (3..=MAX_RULES).contains(&bands.len())
        && bands
            .iter()
            .all(|&(start, end)| end - start <= MAX_RULE_WIDTH)
        && bands
            .windows(2)
            .all(|pair| pair[1].0 - pair[0].1 >= MIN_CELL_SIDE)
}

fn has_partial_rule(groups: &RuleGroups, across: &[Interval], along: &[Interval]) -> bool {
    groups.iter().any(|(&(start, end), bands)| {
        // A competing stroke covers a cell's full interior beside a confirmed rule.
        across
            .windows(2)
            .any(|pair| start <= pair[0].1 && end >= pair[1].0)
            && bands.iter().any(|&(start, end)| {
                along
                    .windows(2)
                    .any(|pair| start < pair[1].0 && end > pair[0].1)
            })
    })
}

fn axis_candidates(
    groups: &RuleGroups,
    deadline: Instant,
) -> Result<BTreeSet<(Interval, Vec<Interval>)>> {
    let mut candidates = BTreeSet::new();
    for &(start, end) in groups.keys() {
        image_ocr::deadline_check(deadline)?;
        if end - start < 2 * MIN_CELL_SIDE + 3 {
            continue;
        }
        // Boundary pen caps can shorten a stroke at the outside corners.
        // Union only the lanes with matching endpoints within the pixel bound.
        let mut bands = Vec::new();
        let mut extent = (start, end);
        for left in start.saturating_sub(ENDPOINT_TOLERANCE)..=start + ENDPOINT_TOLERANCE {
            for right in end.saturating_sub(ENDPOINT_TOLERANCE)..=end + ENDPOINT_TOLERANCE {
                let Some(near) = groups.get(&(left, right)) else {
                    continue;
                };
                bands.extend_from_slice(near);
                extent.0 = extent.0.min(left);
                extent.1 = extent.1.max(right);
            }
        }
        bands.sort_unstable();
        let mut merged: Vec<Interval> = Vec::new();
        for (start, end) in bands {
            if let Some(last) = merged.last_mut()
                && start <= last.1
            {
                last.1 = last.1.max(end);
            } else {
                merged.push((start, end));
            }
        }
        if usable_bands(&merged) {
            candidates.insert((extent, merged));
            if candidates.len() > MAX_AXIS_CANDIDATES {
                return Err(failure(
                    "IMAGE_TABLE_RESOURCE_LIMIT",
                    "罫線の組合せが画像表の読取り上限を超えました。",
                ));
            }
        }
    }
    Ok(candidates)
}

fn continuous_rules(
    rows: &[Interval],
    columns: &[Interval],
    luma: &impl Fn(u32, u32) -> u8,
) -> bool {
    let inner_limits = |bands: &[Interval]| bands[0].1 - 1..=bands[bands.len() - 1].0;
    rows.iter().all(|&(start, end)| {
        (start..end).all(|y| inner_limits(columns).all(|x| luma(x, y) <= DARK_LUMA))
    }) && columns.iter().all(|&(start, end)| {
        (start..end).all(|x| inner_limits(rows).all(|y| luma(x, y) <= DARK_LUMA))
    })
}

fn locate_grid(
    horizontal: &RuleGroups,
    vertical: &RuleGroups,
    luma: &impl Fn(u32, u32) -> u8,
    deadline: Instant,
) -> Result<std::result::Result<Grid, &'static str>> {
    let horizontal_axes = axis_candidates(horizontal, deadline)?;
    let vertical_axes = axis_candidates(vertical, deadline)?;
    let mut selected = None;
    for (horizontal_span, rows) in horizontal_axes {
        image_ocr::deadline_check(deadline)?;
        for (vertical_span, columns) in &vertical_axes {
            if *vertical_span != outside(&rows) || outside(columns) != horizontal_span {
                continue;
            }
            // A tolerance at the ends does not relax continuity in the grid.
            if !continuous_rules(&rows, columns, luma) {
                continue;
            }
            if selected.is_some() {
                return Ok(Err("GRID_AMBIGUOUS"));
            }
            selected = Some(Grid {
                rows: rows.clone(),
                columns: columns.clone(),
            });
        }
    }
    let Some(grid) = selected else {
        return Ok(Err("GRID_NOT_CONFIRMED"));
    };
    if has_partial_rule(horizontal, &grid.columns, &grid.rows)
        || has_partial_rule(vertical, &grid.rows, &grid.columns)
    {
        return Ok(Err("GRID_INCOMPLETE"));
    }
    image_ocr::deadline_check(deadline)?;
    Ok(Ok(grid))
}

fn raster_rule_groups(
    grayscale: &[u8],
    width: u32,
    height: u32,
    deadline: Instant,
) -> Result<(RuleGroups, RuleGroups)> {
    let luma = |x, y| grayscale[(y * width + x) as usize];
    let horizontal = rule_groups(width, height, luma, deadline)?;
    let vertical = rule_groups(height, width, |y, x| luma(x, y), deadline)?;
    Ok((horizontal, vertical))
}

fn clear_gutters(
    bands: &[Interval],
    crossing: &[Interval],
    lanes: u32,
    luma: impl Fn(u32, u32) -> u8,
) -> bool {
    let (first, end) = outside(crossing);
    bands.iter().all(|&(start, stop)| {
        (start.saturating_sub(GUTTER_WIDTH)..start)
            .chain(stop..(stop + GUTTER_WIDTH).min(lanes))
            .all(|lane| {
                (first..end).all(|position| {
                    luma(position, lane) > GUTTER_LUMA
                        || crossing
                            .iter()
                            .any(|&(lo, hi)| (lo..hi).contains(&position))
                })
            })
    })
}

fn verify_gutters(grayscale: &[u8], width: u32, height: u32, grid: &Grid) -> bool {
    let luma = |x, y| grayscale[(y * width + x) as usize];
    clear_gutters(&grid.rows, &grid.columns, height, luma)
        && clear_gutters(&grid.columns, &grid.rows, width, |y, x| luma(x, y))
}

pub(crate) fn mask_verified_rules(
    grayscale: &mut [u8],
    width: u32,
    height: u32,
    deadline: Instant,
) -> Result<bool> {
    let (horizontal, vertical) = raster_rule_groups(grayscale, width, height, deadline)?;
    let luma = |x, y| grayscale[(y * width + x) as usize];
    let Ok(grid) = locate_grid(&horizontal, &vertical, &luma, deadline)? else {
        return Ok(false);
    };
    if !verify_gutters(grayscale, width, height, &grid) {
        return Ok(false);
    }
    let (left, right) = outside(&grid.columns);
    let (top, bottom) = outside(&grid.rows);
    for &(start, end) in &grid.rows {
        for y in start..end {
            grayscale[(y * width + left) as usize..(y * width + right) as usize].fill(255);
        }
    }
    for y in top..bottom {
        for &(start, end) in &grid.columns {
            grayscale[(y * width + start) as usize..(y * width + end) as usize].fill(255);
        }
    }
    image_ocr::deadline_check(deadline)?;
    Ok(true)
}

fn inner_cells(grid: &Grid) -> Vec<Bounds> {
    grid.rows
        .windows(2)
        .flat_map(|row| {
            grid.columns.windows(2).map(move |column| Bounds {
                x: column[0].1,
                y: row[0].1,
                width: column[1].0 - column[0].1,
                height: row[1].0 - row[0].1,
            })
        })
        .collect()
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer.x <= inner.x
        && inner.x + inner.width <= outer.x + outer.width
        && outer.y <= inner.y
        && inner.y + inner.height <= outer.y + outer.height
}

fn overlaps(a: Bounds, b: Bounds) -> bool {
    a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height
}

fn covered_ink(
    cell: Bounds,
    words: &[&Word],
    luma: &impl Fn(u32, u32) -> u8,
    deadline: Instant,
) -> Result<bool> {
    for y in cell.y..cell.y + cell.height {
        image_ocr::deadline_check(deadline)?;
        let mut start = cell.x;
        for word in words {
            let bounds = word.bounds;
            if (bounds.y..bounds.y + bounds.height).contains(&y) {
                if (start..bounds.x).any(|x| luma(x, y) <= DARK_LUMA) {
                    return Ok(false);
                }
                start = bounds.x + bounds.width;
            }
        }
        if (start..cell.x + cell.width).any(|x| luma(x, y) <= DARK_LUMA) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn evidence(
    cell: Bounds,
    words: &mut [&Word],
    luma: &impl Fn(u32, u32) -> u8,
    deadline: Instant,
) -> Result<Option<CellEvidence>> {
    let Some(first) = words.first() else {
        return Ok(None);
    };
    let mut shared_top = first.bounds.y;
    let mut shared_bottom = first.bounds.y + first.bounds.height;
    let mut pixels = first.bounds;
    let mut right = 0;
    words.sort_unstable_by_key(|word| word.bounds.x);
    for word in words.iter() {
        let bounds = word.bounds;
        if word.confidence_floor < MIN_CELL_CONFIDENCE || word.text.is_none() || bounds.x < right {
            return Ok(None);
        }
        right = bounds.x + bounds.width;
        shared_top = shared_top.max(bounds.y);
        shared_bottom = shared_bottom.min(bounds.y + bounds.height);
        pixels = pixels.union(bounds);
    }
    if shared_top >= shared_bottom || !covered_ink(cell, words, luma, deadline)? {
        return Ok(None);
    }
    Ok(Some(CellEvidence {
        text: image_ocr::line_text(words.iter().copied()),
        pixels,
        cell,
    }))
}

fn select_label(
    indexes: impl IntoIterator<Item = usize>,
    cells: &[Bounds],
    words: &mut [Vec<&Word>],
    label: &str,
    luma: &impl Fn(u32, u32) -> u8,
    deadline: Instant,
) -> Result<std::result::Result<(usize, CellEvidence), &'static str>> {
    let mut selected = None;
    for index in indexes {
        // An unreadable label could be another occurrence of the label sought.
        let Some(candidate) = evidence(cells[index], &mut words[index], luma, deadline)? else {
            return Ok(Err("LABEL_UNREADABLE"));
        };
        if candidate.text == label && selected.replace((index, candidate)).is_some() {
            return Ok(Err("LABEL_AMBIGUOUS"));
        }
    }
    Ok(selected.ok_or("LABEL_NOT_FOUND"))
}

pub(crate) fn read_cell(
    raw: &[u8],
    media_type: &str,
    obs: &Observation,
    row: &str,
    column: &str,
    deadline: Instant,
) -> Result<CellLookup> {
    image_ocr::deadline_check(deadline)?;
    if obs.used_rule_mask() {
        image_ocr::verify_table(obs, raw, media_type, deadline)?;
    } else {
        image_ocr::verify(obs, raw, media_type)?;
    }
    let raster = image_ocr::decode_rgba(raw, media_type)?;
    let (width, height) = raster.dimensions();
    let grayscale: Vec<_> = raster.pixels().map(image_ocr::luma_on_white).collect();
    let (horizontal, vertical) = raster_rule_groups(&grayscale, width, height, deadline)?;
    let luma = |x, y| grayscale[(y * width + x) as usize];
    let grid = match locate_grid(&horizontal, &vertical, &luma, deadline)? {
        Ok(grid) => grid,
        Err(reason) => return Ok(CellLookup::Unknown { reason }),
    };
    if !verify_gutters(&grayscale, width, height, &grid) {
        return Ok(CellLookup::Unknown {
            reason: "TEXT_TOUCHES_RULE",
        });
    }
    let (x, right) = outside(&grid.columns);
    let (y, bottom) = outside(&grid.rows);
    let rectangle = Bounds {
        x,
        y,
        width: right - x,
        height: bottom - y,
    };
    let cells = inner_cells(&grid);
    let mut words = vec![Vec::new(); cells.len()];
    for word in obs.lines.iter().flat_map(|line| line.words()) {
        if overlaps(rectangle, word.bounds) {
            let Some(index) = cells.iter().position(|cell| contains(*cell, word.bounds)) else {
                return Ok(CellLookup::Unknown {
                    reason: "TEXT_CROSSES_RULE",
                });
            };
            words[index].push(word);
        }
    }
    let ncolumns = grid.columns.len() - 1;
    let nrows = grid.rows.len() - 1;
    let (row_index, row) = match select_label(
        (1..nrows).map(|index| index * ncolumns),
        &cells,
        &mut words,
        row,
        &luma,
        deadline,
    )? {
        Ok(label) => label,
        Err(reason) => return Ok(CellLookup::Unknown { reason }),
    };
    let (column_index, column) =
        match select_label(1..ncolumns, &cells, &mut words, column, &luma, deadline)? {
            Ok(label) => label,
            Err(reason) => return Ok(CellLookup::Unknown { reason }),
        };
    let index = row_index + column_index;
    image_ocr::deadline_check(deadline)?;
    Ok(
        match evidence(cells[index], &mut words[index], &luma, deadline)? {
            Some(value) => CellLookup::Found { row, column, value },
            None => CellLookup::Unknown {
                reason: "VALUE_UNREADABLE",
            },
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_interrupted_divider_does_not_make_a_larger_cell() {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let mut image = image::GrayImage::from_pixel(144, 120, image::Luma([255]));
        for x in [10, 45, 85, 125] {
            for y in 10..=100 {
                image.put_pixel(x, y, image::Luma([0]));
            }
        }
        for y in [10, 40, 70, 100] {
            for x in 10..=125 {
                image.put_pixel(x, y, image::Luma([0]));
            }
        }
        let snapshot = |image: &image::GrayImage| {
            (
                rule_groups(144, 120, |x, y| image[(x, y)].0[0], deadline).unwrap(),
                rule_groups(120, 144, |y, x| image[(x, y)].0[0], deadline).unwrap(),
            )
        };
        let (horizontal, vertical) = snapshot(&image);
        let grid = locate_grid(&horizontal, &vertical, &|x, y| image[(x, y)].0[0], deadline)
            .unwrap()
            .unwrap();
        assert_eq!(inner_cells(&grid).len(), 9);

        image.put_pixel(62, 70, image::Luma([255]));
        let (horizontal, vertical) = snapshot(&image);
        assert_eq!(
            locate_grid(&horizontal, &vertical, &|x, y| image[(x, y)].0[0], deadline).unwrap(),
            Err("GRID_INCOMPLETE")
        );
    }

    #[test]
    fn a_visible_glyph_outside_recognized_boxes_is_unreadable() {
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        let cell = Bounds {
            x: 10,
            y: 10,
            width: 40,
            height: 30,
        };
        let recognized = Word {
            bounds: Bounds {
                x: 15,
                y: 15,
                width: 12,
                height: 11,
            },
            confidence_floor: 95,
            text: Some("12".into()),
        };
        let mut words = [&recognized];
        let recognized_ink = |x, y| {
            if (17..25).contains(&x) && (17..25).contains(&y) {
                0
            } else {
                255
            }
        };
        assert!(
            evidence(cell, &mut words, &recognized_ink, deadline)
                .unwrap()
                .is_some()
        );
        let missing_glyph = |x, y| {
            if (32..38).contains(&x) && (17..25).contains(&y) {
                0
            } else {
                recognized_ink(x, y)
            }
        };
        assert!(
            evidence(cell, &mut words, &missing_glyph, deadline)
                .unwrap()
                .is_none()
        );
    }
}
