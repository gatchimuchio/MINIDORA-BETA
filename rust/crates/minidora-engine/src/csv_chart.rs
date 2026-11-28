//! The chart and its Japanese summary share the same exact CSV aggregate.

use crate::document::{CsvChartData, csv_chart_data, csv_chart_data_from_files};
use minidora_domain::{Result, digest, failure};
use num_rational::BigRational;
use num_traits::{ToPrimitive, Zero};
use serde::{Deserialize, Serialize};

const VERSION: &str = "csv-grouped-mean-chart-v1";
const VALUES_VERSION: &str = "csv-grouped-value-chart-v1";
pub(crate) const SVG_PREFIX: &str =
    r#"<svg xmlns="http://www.w3.org/2000/svg" data-minidora-chart="csv-grouped-mean-chart-v1""#;
const VALUES_SVG_PREFIX: &str =
    r#"<svg xmlns="http://www.w3.org/2000/svg" data-minidora-chart="csv-grouped-value-chart-v1""#;
const MAX_GROUPS: usize = 48;
const WIDTH: i64 = 1056;
const PLOT_LEFT: i64 = 268;
const PLOT_RIGHT: i64 = 759;
const VALUE_X: i64 = 795;
const COUNT_X: i64 = 1014;
const ROW_TOP: i64 = 170;
const ROW_HEIGHT: i64 = 46;
const SUBPIXELS: i64 = 1000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CsvChartReport {
    pub version: String,
    pub data: CsvChartData,
    pub svg: String,
    pub svg_sha256: String,
    pub summary: String,
}

pub(crate) fn build(
    document: &str,
    group: &str,
    value: &str,
    filter: Option<(&str, &str)>,
) -> Result<CsvChartReport> {
    build_report(csv_chart_data(document, group, value, filter)?, false)
}

pub(crate) fn build_values(
    document: &str,
    group: &str,
    value: &str,
    filter: Option<(&str, &str)>,
) -> Result<CsvChartReport> {
    let data = csv_chart_data(document, group, value, filter)?;
    if data.missing_rows > 0
        || data.selected_rows != data.groups.len()
        || data.groups.iter().any(|group| group.statistics.count != 1)
    {
        return Err(failure(
            "CSV_CHART_GROUP_VALUES",
            "集計値を図示するCSVには、区分ごとに欠損のない値が1件必要です。",
        ));
    }
    build_report(data, true)
}

pub(crate) fn recognized_svg(text: &str) -> bool {
    text.starts_with(SVG_PREFIX) || text.starts_with(VALUES_SVG_PREFIX)
}

pub(crate) fn build_from_files(
    files: &[(&str, &str)],
    group: &str,
    value: &str,
    filter: Option<(&str, &str)>,
) -> Result<CsvChartReport> {
    build_report(
        csv_chart_data_from_files(files, group, value, filter)?,
        false,
    )
}

fn build_report(data: CsvChartData, values: bool) -> Result<CsvChartReport> {
    if data.groups.is_empty() || data.groups.len() > MAX_GROUPS {
        return Err(failure(
            "CSV_CHART_GROUP_LIMIT",
            "棒グラフには平均を計算できるグループが1〜48件必要です。",
        ));
    }
    let (title, caption) = if values {
        (
            format!(
                "「{}」別の「{}」",
                label(&data.group_column),
                label(&data.value_column)
            ),
            format!(
                "実行結果のCSVにある {} 区分の値。単位 {}。",
                data.groups.len(),
                data.unit.as_deref().map_or("なし".into(), label)
            ),
        )
    } else {
        (
            format!(
                "「{}」別の「{}」の平均",
                label(&data.group_column),
                label(&data.value_column)
            ),
            format!(
                "対象 {} / {} 行。集計 {} 行、欠損 {} 行を除外。単位 {}。",
                data.selected_rows,
                data.total_rows,
                data.selected_rows - data.missing_rows,
                data.missing_rows,
                data.unit.as_deref().map_or("なし".into(), label)
            ),
        )
    };
    let condition = data.filter.as_ref().map(|filter| {
        format!(
            "絞り込み「{}」が「{}」に一致",
            label(&filter.column),
            label(&filter.equals)
        )
    });
    let mut summary_lines = vec![title.clone(), caption.clone()];
    if let Some(condition) = &condition {
        summary_lines.push(condition.clone());
    }
    let source_note = (!data.sources.is_empty()).then(|| {
        let filenames = data
            .sources
            .iter()
            .map(|source| format!("「{}」", short_label(&label(&source.name), 36)))
            .collect::<Vec<_>>();
        format!("資料 {} 件。{}", filenames.len(), filenames.join("、"))
    });
    if let Some(note) = &source_note {
        summary_lines.push(note.clone());
    }
    for group in &data.groups {
        let name = label(&group.label);
        let value = displayed_mean(&group.statistics.mean, data.unit.as_deref());
        summary_lines.push(if values {
            format!("「{name}」は {value}")
        } else {
            format!("「{name}」は平均 {value}（{}件）", group.statistics.count)
        });
    }
    let summary = summary_lines.join("\n");
    let svg = render_svg(
        &data,
        &title,
        &caption,
        condition.as_deref(),
        source_note.as_deref(),
        &summary,
        values,
    )?;
    let svg_sha256 = digest(svg.as_bytes());
    Ok(CsvChartReport {
        version: if values { VALUES_VERSION } else { VERSION }.into(),
        data,
        svg,
        svg_sha256,
        summary,
    })
}

fn label(raw: &str) -> String {
    if raw.is_empty() {
        return "（空欄）".into();
    }
    raw.chars()
        .map(|c| match c {
            '\r' | '\n' => '↵',
            '\t' => '⇥',
            '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{fffe}' | '\u{ffff}' => '�',
            _ if c.is_control() => '�',
            _ => c,
        })
        .collect()
}

fn displayed_mean(mean: &str, unit: Option<&str>) -> String {
    match unit {
        Some(unit) => format!("{mean} {}", label(unit)),
        None => mean.into(),
    }
}

fn xml(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&apos;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

fn short_label(text: &str, columns: usize) -> String {
    let mut visible = String::new();
    let mut used = 0;
    for c in text.chars() {
        used += if c.is_ascii() { 1 } else { 2 };
        if used > columns {
            visible.push('…');
            return visible;
        }
        visible.push(c);
    }
    visible
}

fn short_xml(text: &str, columns: usize) -> String {
    xml(&short_label(text, columns))
}

fn coordinates(
    means: &[BigRational],
    low: &BigRational,
    high: &BigRational,
) -> Result<(i64, Vec<i64>)> {
    let zero = BigRational::zero();
    if high == low {
        return Ok((
            PLOT_LEFT * SUBPIXELS,
            vec![PLOT_LEFT * SUBPIXELS; means.len()],
        ));
    }
    let scale =
        BigRational::from_integer(((PLOT_RIGHT - PLOT_LEFT) * SUBPIXELS).into()) / (high - low);
    let point = |mean: &BigRational| {
        ((mean - low) * &scale)
            .to_integer()
            .to_i64()
            .map(|offset| PLOT_LEFT * SUBPIXELS + offset)
            .ok_or_else(|| failure("CSV_CHART_SCALE", "棒グラフの座標を計算できません。"))
    };
    Ok((
        point(&zero)?,
        means.iter().map(point).collect::<Result<Vec<_>>>()?,
    ))
}

fn coordinate_text(value: i64) -> String {
    format!("{}.{:03}", value / SUBPIXELS, value % SUBPIXELS)
}

fn render_svg(
    data: &CsvChartData,
    title: &str,
    caption: &str,
    condition: Option<&str>,
    source_note: Option<&str>,
    summary: &str,
    values: bool,
) -> Result<String> {
    let means =
        data.groups
            .iter()
            .map(|group| {
                group.statistics.mean.parse::<BigRational>().map_err(|_| {
                    failure("CSV_CHART_MEAN", "集計結果の平均を図の尺度にできません。")
                })
            })
            .collect::<Result<Vec<_>>>()?;
    let zero = BigRational::zero();
    let low = means.iter().min().unwrap_or(&zero).min(&zero);
    let high = means.iter().max().unwrap_or(&zero).max(&zero);
    let (axis, points) = coordinates(&means, low, high)?;
    let content_height = ROW_TOP + ROW_HEIGHT * data.groups.len() as i64 + 66;
    let height = content_height + if source_note.is_some() { 22 } else { 0 };
    let bottom = content_height - 63;
    let axis_text = coordinate_text(axis);
    let prefix = if values {
        VALUES_SVG_PREFIX
    } else {
        SVG_PREFIX
    };
    let value_header = if values { "値" } else { "平均" };
    let mut svg = vec![
        format!(
            "{prefix} width=\"{WIDTH}\" height=\"{height}\" viewBox=\"0 0 {WIDTH} {height}\" role=\"img\" aria-labelledby=\"chart-title chart-desc\">"
        ),
        format!(r#"<title id="chart-title">{}</title>"#, xml(title)),
        format!(r#"<desc id="chart-desc">{}</desc>"#, xml(summary)),
        format!(
            r#"<defs><clipPath id="chart-heading-clip"><rect x="32" y="14" width="{}" height="110"/></clipPath><clipPath id="chart-label-clip"><rect x="32" y="126" width="{}" height="{}"/></clipPath><clipPath id="chart-value-clip"><rect x="{VALUE_X}" y="126" width="{}" height="{}"/></clipPath></defs>"#,
            WIDTH - 64,
            PLOT_LEFT - 48,
            bottom - 126,
            COUNT_X - VALUE_X - 48,
            bottom - 126
        ),
        format!(
            r##"<rect x="1" y="1" width="{}" height="{}" rx="14" fill="#ffffff" stroke="#ccd7e4"/>"##,
            WIDTH - 2,
            height - 2
        ),
        r##"<g font-family="Yu Gothic UI, Segoe UI, sans-serif" fill="#182941">"##.into(),
        format!(
            r#"<text x="32" y="48" font-size="27" font-weight="700" clip-path="url(#chart-heading-clip)">{}</text>"#,
            short_xml(title, 66)
        ),
        format!(
            r##"<text x="32" y="80" fill="#4b5f75" font-size="15" clip-path="url(#chart-heading-clip)">{}</text>"##,
            short_xml(caption, 112)
        ),
        format!(
            r#"<text x="32" y="147" font-size="14" font-weight="700" clip-path="url(#chart-label-clip)">{}</text>"#,
            short_xml(&label(&data.group_column), 26)
        ),
        format!(
            r#"<text x="{VALUE_X}" y="147" font-size="14" font-weight="700">{value_header}</text>"#
        ),
    ];
    if !values {
        svg.push(format!(
            r#"<text x="{COUNT_X}" y="147" font-size="14" font-weight="700" text-anchor="end">件数</text>"#
        ));
    }
    if let Some(condition) = condition {
        svg.push(format!(
            r##"<text x="32" y="106" fill="#4b5f75" font-size="15" clip-path="url(#chart-heading-clip)">{}</text>"##,
            short_xml(condition, 112)
        ));
    }
    for (i, (group, endpoint)) in data.groups.iter().zip(points).enumerate() {
        let top = ROW_TOP + ROW_HEIGHT * i as i64;
        let y = top + 26;
        let name = label(&group.label);
        let mean = displayed_mean(&group.statistics.mean, data.unit.as_deref());
        let count = group.statistics.count;
        let fill = if means[i] < zero {
            "#957042"
        } else {
            "#3164aa"
        };
        let x = coordinate_text(axis.min(endpoint));
        let width = coordinate_text((axis - endpoint).abs());
        let row_title = if values {
            format!(r#"<g><title>{}　{}</title>"#, xml(&name), xml(&mean))
        } else {
            format!(
                r#"<g><title>{}　平均 {}（{}件）</title>"#,
                xml(&name),
                xml(&mean),
                count
            )
        };
        svg.extend([
            row_title,
            format!(r##"<line x1="32" x2="{COUNT_X}" y1="{top}" y2="{top}" stroke="#e4ebf2"/>"##),
            format!(r#"<text x="32" y="{y}" font-size="15" clip-path="url(#chart-label-clip)">{}</text>"#, short_xml(&name, 28)),
            format!(
                r#"<rect x="{x}" y="{}" width="{width}" height="17" rx="2" fill="{fill}"/>"#,
                top + 10
            ),
            format!(r#"<text x="{VALUE_X}" y="{y}" font-size="15" font-weight="600" clip-path="url(#chart-value-clip)">{}</text>"#, short_xml(&mean, 20)),
        ]);
        if values {
            svg.push("</g>".into());
        } else {
            svg.push(format!(r##"<text x="{COUNT_X}" y="{y}" font-size="14" fill="#4b5f75" text-anchor="end">{count}</text></g>"##));
        }
    }
    svg.push(format!(r##"<line x1="{axis_text}" x2="{axis_text}" y1="160" y2="{bottom}" stroke="#647a93" stroke-width="1.25"/>"##));
    let axis_label_y = bottom + 24;
    svg.push(format!(r##"<text x="{axis_text}" y="{axis_label_y}" fill="#4b5f75" font-size="13" text-anchor="middle">0</text>"##));
    for (tick, value, anchor) in [(PLOT_LEFT, low, "start"), (PLOT_RIGHT, high, "end")] {
        if low != high && (axis - tick * SUBPIXELS).abs() >= 100 * SUBPIXELS {
            svg.push(format!(
                r##"<text x="{tick}" y="{axis_label_y}" fill="#4b5f75" font-size="13" text-anchor="{anchor}">{}</text>"##,
                short_xml(&value.to_string(), 18),
            ));
        }
    }
    if let Some(note) = source_note {
        svg.push(format!(
            r##"<text x="32" y="{}" fill="#4b5f75" font-size="13">{}</text>"##,
            height - 20,
            short_xml(note, 125)
        ));
    }
    svg.extend(["</g>".into(), "</svg>".into()]);
    Ok(svg.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chart_reads_independent_csv_schemas_and_retains_their_original_coordinates() {
        let early = "\u{feff}店舗,期間,売上\r\n北店,前半,100 cm\r\n南店,後半,\r\n";
        let late = "売上,店舗,期間\n3 m,北店,後半\n300 cm,南店,後半";
        let files = [("春&夏.csv", early), ("秋.csv", late)];
        let report = build_from_files(&files, "店舗", "売上", None).unwrap();
        let data = &report.data;
        assert_eq!(data.unit.as_deref(), Some("m"));
        assert_eq!(
            (data.total_rows, data.selected_rows, data.missing_rows),
            (4, 4, 1)
        );
        assert_eq!(data.groups[0].statistics.mean, "2");
        assert_eq!(data.groups[0].source_record_numbers, [1, 3]);
        assert_eq!(data.groups[1].statistics.mean, "3");
        assert_eq!(data.groups[1].source_record_numbers, [4]);
        assert_eq!(data.sources[0].first_record_number, 1);
        assert_eq!(data.sources[1].first_record_number, 3);
        assert_eq!(data.sources[0].source_spans[0].source_start, 3);
        assert_eq!(data.sources[1].source_spans[0].column, Some(1));
        assert!(data.source_spans.is_empty());
        for (source, (name, original)) in data.sources.iter().zip(files) {
            assert_eq!(source.name, name);
            assert_eq!(source.sha256, digest(original.as_bytes()));
            assert_eq!(source.total_rows, 2);
            for span in &source.source_spans {
                assert_eq!(
                    original.get(span.source_start..span.source_end),
                    Some(span.source_text.as_str())
                );
            }
        }
        assert!(report.summary.contains("「春&夏.csv」、「秋.csv」"));
        assert!(report.svg.contains("「春&amp;夏.csv」、「秋.csv」"));

        let filtered = build_from_files(&files, "店舗", "売上", Some(("期間", "後半"))).unwrap();
        assert_eq!(filtered.data.source_sha256, data.source_sha256);
        assert_eq!(
            (filtered.data.selected_rows, filtered.data.missing_rows),
            (3, 1)
        );
        assert_eq!(
            filtered
                .data
                .groups
                .iter()
                .find(|g| g.label == "北店")
                .unwrap()
                .statistics
                .mean,
            "3"
        );
        assert_eq!(
            filtered.data.sources[0]
                .source_spans
                .iter()
                .filter(|span| span.kind == "CSV_CHART_FILTER_DECISION")
                .count(),
            2
        );
    }

    #[test]
    fn pasted_csv_chart_keeps_the_previous_canonical_report() {
        let source =
            "店舗,期間,売上\n北店,前半,10 円\n南店,前半,2 JPY\n北店,後半,30 JPY\n南店,後半,6 JPY\n";
        let report = build(source, "店舗", "売上", None).unwrap();
        assert_eq!(
            digest(&minidora_domain::canonical(&report).unwrap()),
            "156919e44937a51a7882d4710c21dd7f1cae5c2edf529d510bcea1d083274b86"
        );
    }

    #[test]
    fn zero_chart_has_one_zero_tick_and_confines_long_labels() {
        let label = "W".repeat(55);
        let document = format!("拠点,費用\n{label},0 JPY");
        let result = build(&document, "拠点", "費用", None).unwrap();
        assert_eq!(result.svg.matches(">0</text>").count(), 1);
        assert!(result.svg.contains("width=\"0.000\""));
        assert!(result.svg.contains("clip-path=\"url(#chart-label-clip)\""));
        assert!(
            result
                .svg
                .contains(&format!("<title>{label}　平均 0 JPY（1件）</title>"))
        );
    }
}
