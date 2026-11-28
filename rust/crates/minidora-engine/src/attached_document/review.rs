//! Purpose-focused review of explicit assignments in retained attachments.
use std::collections::BTreeSet;
use std::time::Instant;

use minidora_domain::{DocumentReviewOperation, Result, SourceSpan, canonical, failure};
use serde::Serialize;

use super::{
    Computation, FieldGap, FieldMatch, FieldSearch, FileContent, PageState, PreparedFile,
    check_deadline, field_body, quote_prefix, source_heading,
};

const REPORT_KIND: &str = "minidora.attached-document-review.v1";
const MAX_REQUESTED_FIELDS: usize = 8;
const MAX_DISCOVERED_FIELDS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Status {
    Confirmed,
    NotFound,
    Conflict,
    Incomplete,
}

impl Status {
    fn visible(self) -> &'static str {
        match self {
            Self::Confirmed => "確認済み",
            Self::NotFound => "対応形式での記載なし",
            Self::Conflict => "同じ資料内に異なる値があり未確定",
            Self::Incomplete => "読めない箇所または不明な値があり未確定",
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
enum Comparison {
    Same,
    Changed,
    FirstOnly,
    SecondOnly,
    NotFound,
    Unresolved,
}

#[derive(Serialize)]
struct SourceValue<'a> {
    filename: &'a str,
    source_sha256: &'a str,
    status: Status,
    value: Option<String>,
    matched_count: usize,
    gap_count: usize,
    matches: Vec<FieldMatch>,
    gaps: Vec<FieldGap>,
}

#[derive(Serialize)]
struct Row<'a> {
    label: &'a str,
    status: Option<Comparison>,
    sources: Vec<SourceValue<'a>>,
}

#[derive(Serialize)]
struct Report<'a> {
    kind: &'static str,
    manifest_sha256: &'a str,
    operation: DocumentReviewOperation,
    selection: &'static str,
    fields: Vec<Row<'a>>,
}

fn valid_label(label: &str) -> bool {
    !label.is_empty()
        && label.chars().count() <= 64
        && label.trim() == label
        && label.chars().any(char::is_alphanumeric)
        && !label.chars().any(char::is_control)
        && !label.contains(['、', ',', '。', '！', '？', '!', '?', ':', '：'])
}

fn discovered_label(line: &str) -> Option<&str> {
    let (label, value) = field_body(line.trim_start_matches('\u{feff}')).split_once([':', '：'])?;
    let label = label.trim();
    let label = ["**", "__"]
        .into_iter()
        .find_map(|wrapper| label.strip_prefix(wrapper)?.strip_suffix(wrapper))
        .unwrap_or(label);
    if value.trim_start().starts_with("//") || !valid_label(label) {
        return None;
    }
    Some(label)
}

fn discover_fields<'a>(files: &'a [PreparedFile<'_>], deadline: Instant) -> Result<Vec<&'a str>> {
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();
    for file in files {
        check_deadline(deadline)?;
        let pages: &[super::Page] = match &file.content {
            FileContent::Pdf(pages) => pages,
            FileContent::ScannedPdf(doc) => &doc.pages,
            _ => &[],
        };
        let plain = match &file.content {
            FileContent::Text(text) => Some(*text),
            _ => None,
        };
        for text in plain.into_iter().chain(
            pages
                .iter()
                .filter(|p| p.state == PageState::Text)
                .map(|p| p.text.as_str()),
        ) {
            for (index, line) in text.lines().enumerate() {
                if index.is_multiple_of(128) {
                    check_deadline(deadline)?;
                }
                if let Some(label) = discovered_label(line)
                    && seen.insert(label)
                {
                    ordered.push(label);
                    if ordered.len() > MAX_DISCOVERED_FIELDS {
                        return Err(failure(
                            "DOCUMENT_REVIEW_FIELD_LIMIT",
                            "項目が多いため、比較する項目名を指定してください。",
                        ));
                    }
                }
            }
        }
    }
    Ok(ordered)
}

fn observe<'a>(
    file: &'a PreparedFile<'_>,
    label: &str,
    deadline: Instant,
) -> Result<SourceValue<'a>> {
    check_deadline(deadline)?;
    let mut search = FieldSearch::default();
    let source = &file.source;
    match &file.content {
        FileContent::Text(text) => search.scan(source, text, None, label, deadline)?,
        FileContent::Pdf(pages) => search.scan_pdf(source, pages, label, deadline, false)?,
        FileContent::ScannedPdf(doc) => {
            search.scan_pdf(source, &doc.pages, label, deadline, true)?
        }
        FileContent::UnreadablePdf(failure) => {
            search.gap(source, None, None, failure.code, failure.message)
        }
        FileContent::Image(_) => search.gap(
            source,
            None,
            None,
            "REVIEW_IMAGE_UNCERTAIN",
            "画像の項目ラベルを全文検索したとは確認できません。",
        ),
    }
    let status = if search.gap_count != 0 {
        Status::Incomplete
    } else if search.conflict {
        Status::Conflict
    } else if search.first_value.is_some() {
        Status::Confirmed
    } else {
        Status::NotFound
    };
    Ok(SourceValue {
        filename: source.name,
        source_sha256: &source.sha256,
        status,
        value: search.first_value.filter(|_| status == Status::Confirmed),
        matched_count: search.matched_count,
        gap_count: search.gap_count,
        matches: search.matches,
        gaps: search.gaps,
    })
}

fn compare(sources: &[SourceValue<'_>]) -> Comparison {
    let [first, second] = sources else {
        unreachable!("validated comparison width")
    };
    match (first.status, second.status) {
        (Status::Confirmed, Status::Confirmed) if first.value == second.value => Comparison::Same,
        (Status::Confirmed, Status::Confirmed) => Comparison::Changed,
        (Status::Confirmed, Status::NotFound) => Comparison::FirstOnly,
        (Status::NotFound, Status::Confirmed) => Comparison::SecondOnly,
        (Status::NotFound, Status::NotFound) => Comparison::NotFound,
        _ => Comparison::Unresolved,
    }
}

fn witness_lines(source: &SourceValue<'_>, units: &mut Vec<String>) {
    let primary = source.matches.first();
    let contrast = if source.status == Status::Confirmed {
        None
    } else {
        primary.and_then(|first| {
            source
                .matches
                .iter()
                .find(|quote| quote.value != first.value)
        })
    };
    for witness in primary.into_iter().chain(contrast) {
        let prefix = quote_prefix(
            witness.page,
            witness.line,
            witness.source_start..witness.source_end,
            None,
        );
        units.push(format!(
            "「{}」 {prefix}{}",
            witness.filename, witness.quote
        ));
    }
    if let Some(gap) = source.gaps.first() {
        let page = gap
            .page
            .map(|page| format!(" {page}ページ"))
            .unwrap_or_default();
        let line = gap
            .line
            .map(|line| format!(" {line}行目"))
            .unwrap_or_default();
        let count = if source.gap_count > 1 {
            format!("（ほか{}箇所）", source.gap_count - 1)
        } else {
            String::new()
        };
        units.push(format!(
            "「{}」{page}{line} {}{count}",
            gap.filename, gap.message
        ));
    }
}

fn summarize(rows: &[Row<'_>], files: &[PreparedFile<'_>]) -> Vec<String> {
    let mut units = vec!["指定項目をもとに資料の内容をまとめました。".to_owned()];
    for (index, file) in files.iter().enumerate() {
        units.push(source_heading(file.source.name, &file.source.sha256));
        let values = rows
            .iter()
            .filter_map(|row| {
                row.sources[index]
                    .value
                    .as_ref()
                    .map(|value| format!("{}は「{value}」", row.label))
            })
            .collect::<Vec<_>>();
        if !values.is_empty() {
            units.push(format!("{}です。", values.join("、")));
        }
        for row in rows {
            let source = &row.sources[index];
            if source.status == Status::NotFound {
                units.push(format!(
                    "{}に対応する形式の記載を確認できませんでした。",
                    row.label,
                ));
            } else if source.value.is_none() {
                units.push(format!("{}は{}です。", row.label, source.status.visible()));
            }
            witness_lines(source, &mut units);
        }
    }
    units
}

fn diff(rows: &[Row<'_>], files: &[PreparedFile<'_>]) -> Vec<String> {
    let mut units = vec!["資料ごとに確認できた項目の記載を比較しました。".to_owned()];
    for (side, file) in ["先の資料", "後の資料"].into_iter().zip(files) {
        units.push(format!(
            "{side}　{}",
            source_heading(file.source.name, &file.source.sha256)
        ));
    }
    for row in rows {
        let first = row.sources[0].value.as_deref().unwrap_or_default();
        let second = row.sources[1].value.as_deref().unwrap_or_default();
        let text = match row.status.expect("comparison row has a status") {
            Comparison::Same => format!("{}は両方とも「{first}」です。", row.label),
            Comparison::Changed => format!(
                "{}は先の資料で「{first}」、後の資料で「{second}」です。",
                row.label
            ),
            Comparison::FirstOnly => format!(
                "{}は先の資料で「{first}」。後の資料では対応形式の記載を確認できませんでした。",
                row.label
            ),
            Comparison::SecondOnly => format!(
                "{}は後の資料で「{second}」。先の資料では対応形式の記載を確認できませんでした。",
                row.label
            ),
            Comparison::NotFound => format!(
                "{}に対応する形式の記載は、両方の資料で確認できませんでした。",
                row.label
            ),
            Comparison::Unresolved => format!(
                "{}の違いは未確定です。先の資料は{}、後の資料は{}です。",
                row.label,
                describe(&row.sources[0]),
                describe(&row.sources[1]),
            ),
        };
        units.push(text);
        for source in &row.sources {
            witness_lines(source, &mut units);
        }
    }
    units
}

fn describe(source: &SourceValue<'_>) -> String {
    source.value.as_ref().map_or_else(
        || source.status.visible().to_owned(),
        |value| format!("「{value}」"),
    )
}

pub(super) fn compute(
    files: &[PreparedFile<'_>],
    manifest_ref: &str,
    operation: DocumentReviewOperation,
    fields: &[SourceSpan],
    deadline: Instant,
) -> Result<Computation> {
    let comparing = operation == DocumentReviewOperation::Compare;
    if (comparing && files.len() != 2) || (!comparing && files.is_empty()) {
        return Err(failure(
            "DOCUMENT_REVIEW_ATTACHMENT_COUNT",
            if comparing {
                "比較する資料を順に2件添付してください。"
            } else {
                "要約する資料を添付してください。"
            },
        ));
    }
    if fields.len() > MAX_REQUESTED_FIELDS || fields.iter().any(|field| !valid_label(&field.text)) {
        return Err(failure(
            "DOCUMENT_REVIEW_FIELDS_INVALID",
            "確認する項目名を8件以内で指定してください。",
        ));
    }
    let fields_given = !fields.is_empty();
    let labels = if fields_given {
        fields
            .iter()
            .map(|field| field.text.as_str())
            .collect::<Vec<_>>()
    } else if comparing {
        discover_fields(files, deadline)?
    } else {
        Vec::new()
    };
    if labels.is_empty() {
        return Err(failure(
            "DOCUMENT_REVIEW_FIELDS_REQUIRED",
            "資料に項目名と値の記載を確認できませんでした。確認する項目名を指定してください。",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut rows = Vec::with_capacity(labels.len());
    for label in labels {
        if !seen.insert(label) {
            return Err(failure(
                "DOCUMENT_REVIEW_FIELDS_INVALID",
                "同じ項目名の指定が重複しています。",
            ));
        }
        let sources = files
            .iter()
            .map(|file| observe(file, label, deadline))
            .collect::<Result<Vec<_>>>()?;
        rows.push(Row {
            label,
            status: comparing.then(|| compare(&sources)),
            sources,
        });
    }
    let units = if comparing {
        diff(&rows, files)
    } else {
        summarize(&rows, files)
    };
    let report_bytes = canonical(&Report {
        kind: REPORT_KIND,
        manifest_sha256: manifest_ref,
        operation,
        selection: if fields_given {
            "REQUESTED_FIELDS"
        } else {
            "LABELED_FIELDS"
        },
        fields: rows,
    })?;
    Ok(Computation {
        units,
        report_bytes,
    })
}
