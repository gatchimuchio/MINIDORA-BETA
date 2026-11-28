//! Locate an assignment within the explicitly named candidate's document block.

use std::time::Instant;

use minidora_domain::{RequestAttachment, Result, digest, failure};
use serde::{Deserialize, Serialize};

use super::{
    FieldAssignment, FileContent, MAX_FIELD_WITNESSES, PageState, bom_offset, check_deadline,
    complete_line, field_assignment, field_body, line_starts, prepare_files,
};
use crate::decision::property_key;

const BINDING_LABELS: [&str; 3] = ["対象", "候補", "名称"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum DecisionFieldFormat {
    Text,
    Markdown,
    TextPdf,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionFieldLine {
    pub page: Option<u32>,
    pub line: usize,
    pub source_start: usize,
    pub source_end: usize,
    pub value_start: usize,
    pub value_end: usize,
    pub extracted_sha256: Option<String>,
    pub quote: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionFieldMatch {
    pub binding: DecisionFieldLine,
    pub field: DecisionFieldLine,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DecisionFieldSource {
    pub filename: String,
    pub source_sha256: String,
    pub byte_count: usize,
    pub format: DecisionFieldFormat,
    pub matches: Vec<DecisionFieldMatch>,
}

pub(crate) fn decision_field_source(
    file: &RequestAttachment,
    candidate: &str,
    key: &str,
    deadline: Instant,
) -> Result<DecisionFieldSource> {
    let format = if RequestAttachment::is_pdf_name(&file.name) {
        DecisionFieldFormat::TextPdf
    } else if RequestAttachment::is_text_name(&file.name) {
        if file
            .name
            .rsplit_once('.')
            .is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("md"))
        {
            DecisionFieldFormat::Markdown
        } else {
            DecisionFieldFormat::Text
        }
    } else {
        return Err(failure(
            "DECISION_ATTACHMENT_FORMAT",
            "訂正の根拠に使える添付は、UTF-8のTXT、Markdown、文字を抽出できるPDFです。",
        ));
    };
    if !valid_label(candidate) || !valid_label(key) || BINDING_LABELS.contains(&key) {
        return Err(failure(
            "DECISION_ATTACHMENT_FIELD_INVALID",
            "添付で照合する候補名と、候補名の欄とは別の項目名を指定してください。",
        ));
    }
    let mut files = prepare_files(std::iter::once(file), deadline, false, None)?;
    let prepared = files.pop().expect("one prepared decision attachment");
    let candidate = property_key(candidate);
    let mut scan = Survey {
        candidate: &candidate,
        key,
        seen_candidate: false,
        seen_orphan: false,
        matches: Vec::new(),
    };
    match &prepared.content {
        FileContent::Text(text) => {
            scan.page(
                text,
                None,
                format == DecisionFieldFormat::Markdown,
                deadline,
            )?;
        }
        FileContent::Pdf(pages) => {
            for page in pages {
                if page.state != PageState::Text {
                    return Err(failure(
                        "DECISION_ATTACHMENT_INCOMPLETE",
                        format!(
                            "{}ページの文字を確認できず、資料内の候補と値を確定できません。",
                            page.number
                        ),
                    ));
                }
                scan.page(&page.text, Some(page.number), false, deadline)?;
            }
        }
        _ => return Err(super::invalid_context()),
    }
    check_deadline(deadline)?;
    let matches = scan.finish()?;
    Ok(DecisionFieldSource {
        filename: prepared.source.name.to_owned(),
        source_sha256: prepared.source.sha256,
        byte_count: prepared.source.byte_count,
        format,
        matches,
    })
}

fn valid_label(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= 48
        && text.chars().any(char::is_alphabetic)
        && text.chars().all(|character| {
            character.is_alphanumeric() || matches!(character, '_' | '-' | '・' | 'ー')
        })
}

struct Survey<'a> {
    candidate: &'a str,
    key: &'a str,
    seen_candidate: bool,
    seen_orphan: bool,
    matches: Vec<DecisionFieldMatch>,
}

enum Scope {
    Unbound,
    Other,
    Selected(DecisionFieldLine),
}

impl Survey<'_> {
    fn page(
        &mut self,
        text: &str,
        page: Option<u32>,
        markdown: bool,
        deadline: Instant,
    ) -> Result<()> {
        check_deadline(deadline)?;
        let extracted_sha256 = page.map(|_| digest(text.as_bytes()));
        let starts = line_starts(text, bom_offset(text));
        let mut scope = Scope::Unbound;
        let mut hidden = HiddenMarkdown::default();
        for index in 0..starts.len() {
            if index.is_multiple_of(128) {
                check_deadline(deadline)?;
            }
            let (start, line) = complete_line(text, &starts, index);
            if !hidden.visible(line, markdown) {
                scope = Scope::Unbound;
                continue;
            }
            if let Some(assignment) = BINDING_LABELS
                .iter()
                .find_map(|label| field_assignment(line, label))
            {
                scope = match assignment {
                    FieldAssignment::Value(raw) => match binding_value(raw) {
                        Some(name) if property_key(name) == self.candidate => {
                            self.seen_candidate = true;
                            Scope::Selected(witness(
                                page,
                                &extracted_sha256,
                                index + 1,
                                start,
                                line,
                                raw,
                            )?)
                        }
                        Some(_) => Scope::Other,
                        None => Scope::Unbound,
                    },
                    FieldAssignment::Uncertain => Scope::Unbound,
                };
                continue;
            }
            if markdown && heading(line) {
                scope = Scope::Unbound;
            }
            if let Some(assignment) = field_assignment(line, self.key) {
                match &scope {
                    Scope::Selected(binding) => {
                        let FieldAssignment::Value(value) = assignment else {
                            return Err(failure(
                                "DECISION_ATTACHMENT_FIELD_UNCERTAIN",
                                format!(
                                    "資料の{}行目で項目の値を一つに確定できません。",
                                    index + 1
                                ),
                            ));
                        };
                        if self.matches.len() == MAX_FIELD_WITNESSES {
                            return Err(super::resource_limit());
                        }
                        self.matches.push(DecisionFieldMatch {
                            binding: binding.clone(),
                            field: witness(page, &extracted_sha256, index + 1, start, line, value)?,
                        });
                    }
                    Scope::Unbound => self.seen_orphan = true,
                    Scope::Other => {}
                }
            } else if !other_labeled_row(line) {
                scope = Scope::Unbound;
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<Vec<DecisionFieldMatch>> {
        if !self.seen_candidate {
            Err(failure(
                "DECISION_ATTACHMENT_CANDIDATE",
                "資料に、指定した候補名と一致する対象・候補・名称の記載がありません。",
            ))
        } else if self.seen_orphan {
            Err(failure(
                "DECISION_ATTACHMENT_SCOPE",
                "指定項目の記載に候補名と照合できない箇所があり、どの候補の値か確定できません。",
            ))
        } else if self.matches.is_empty() {
            Err(failure(
                "DECISION_ATTACHMENT_FIELD_MISSING",
                "指定した候補の記載範囲に、更新する項目がありません。",
            ))
        } else {
            Ok(self.matches)
        }
    }
}

fn binding_value(raw: &str) -> Option<&str> {
    let name = raw
        .strip_prefix('「')
        .and_then(|name| name.strip_suffix('」'))
        .unwrap_or(raw);
    valid_label(name).then_some(name)
}

fn heading(line: &str) -> bool {
    let line = line.trim_start();
    let without_marker = line.trim_start_matches('#');
    (1..=6).contains(&(line.len() - without_marker.len()))
        && (without_marker.is_empty() || without_marker.starts_with(char::is_whitespace))
}

fn other_labeled_row(line: &str) -> bool {
    let Some((name, _)) = field_body(line).split_once([':', '：']) else {
        return false;
    };
    let name = name.trim();
    let name = ["**", "__"]
        .into_iter()
        .find_map(|wrapper| name.strip_prefix(wrapper)?.strip_suffix(wrapper))
        .unwrap_or(name);
    valid_label(name)
}

fn witness(
    page: Option<u32>,
    extracted_sha256: &Option<String>,
    line: usize,
    start: usize,
    quote: &str,
    value: &str,
) -> Result<DecisionFieldLine> {
    let relative_start = value
        .as_ptr()
        .addr()
        .checked_sub(quote.as_ptr().addr())
        .ok_or_else(super::invalid_context)?;
    let relative_end = relative_start
        .checked_add(value.len())
        .ok_or_else(super::invalid_context)?;
    if quote.get(relative_start..relative_end) != Some(value) {
        return Err(super::invalid_context());
    }
    Ok(DecisionFieldLine {
        page,
        line,
        source_start: start,
        source_end: start + quote.len(),
        value_start: start + relative_start,
        value_end: start + relative_end,
        extracted_sha256: extracted_sha256.clone(),
        quote: quote.into(),
        value: value.into(),
    })
}

#[derive(Default)]
struct HiddenMarkdown {
    fence: Option<(char, usize)>,
    comment: bool,
}

impl HiddenMarkdown {
    fn visible(&mut self, line: &str, markdown: bool) -> bool {
        let trimmed = line.trim_start();
        if let Some((marker, width)) = self.fence {
            let tail = trimmed.trim_start_matches(marker);
            if trimmed.len() - tail.len() >= width && tail.trim().is_empty() {
                self.fence = None;
            }
            return false;
        }
        if self.comment {
            self.comment = !line.contains("-->");
            return false;
        }
        if let Some((_, tail)) = line.split_once("<!--") {
            self.comment = !tail.contains("-->");
            return false;
        }
        if let Some(marker @ ('`' | '~')) = trimmed.chars().next() {
            let width = trimmed.len() - trimmed.trim_start_matches(marker).len();
            if width >= 3 {
                self.fence = Some((marker, width));
                return false;
            }
        }
        !trimmed.starts_with('>')
            && !(markdown && (line.starts_with("    ") || line.starts_with('\t')))
    }
}
