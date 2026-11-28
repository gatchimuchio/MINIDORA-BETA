//! Apply a child conversation's text task to a verified, frozen parent answer.
use crate::{attached_document, document, input, store::Store, text_summary};
use minidora_domain::{
    CaseBranch, ContentPlan, Error, Guarantee, InputBundle, Intent, Lifecycle, Result,
    ResultRecord, canonical, digest, failure, identity,
};
use serde::Serialize;
use std::time::{Duration, Instant};

pub(crate) const PREDICATE: &str = "BRANCH_SOURCE_TEXT_V1";
pub(crate) const SOURCE_NODE: &str = "branch-source-result";
pub(crate) const MISMATCH: &str = "BRANCH_SOURCE_MISMATCH";

pub(crate) fn handles(intent: &Intent) -> bool {
    matches!(
        intent,
        Intent::SummarizeBranchSource { .. } | Intent::ReplaceBranchSource { .. }
    )
}

#[derive(Serialize)]
struct Origin<'a> {
    kind: &'static str,
    branch: &'a CaseBranch,
    source_input_ref: String,
    source_render_ref: String,
    source_text_sha256: String,
}

pub(crate) struct Outcome {
    pub dependencies: Vec<String>,
    pub units: Vec<String>,
    pub guarantee: Guarantee,
    origin: Vec<u8>,
    report: Vec<u8>,
}

fn mismatch() -> Error {
    failure(
        MISMATCH,
        "分岐時に固定した元の回答と処理結果を照合できません。",
    )
}

fn evaluate(store: &Store, bundle: &InputBundle, started: Instant) -> Result<Outcome> {
    input::verify_branch_text_input(bundle)?;
    let request = &bundle.request;
    let actor = &request.actor_id;
    let deadline = started
        .checked_add(Duration::from_millis(request.budget.deadline_ms))
        .ok_or_else(|| failure("DEADLINE", "分岐元照合の締切を確定できません。"))?;
    let Some((branch, state)) = store.branch_source(actor, &request.case_id)? else {
        return Err(failure(
            "CASE_BRANCH_UNAVAILABLE",
            "この会話には分岐元の回答がありません。",
        ));
    };
    let source = state.result.as_ref().ok_or_else(mismatch)?;
    if state.actor_id != *actor
        || state.case_id != branch.source_case_id
        || source.id != branch.source_result_id
        || source.lifecycle != Lifecycle::Commit
        || source.text.trim().is_empty()
    {
        return Err(mismatch());
    }
    let parent = &branch.source_case_id;
    store.inherit_memory_context(actor, parent, source)?;
    let (source_input_ref, source_input) = store
        .get_input_bundle(actor, parent, &source.input_id, source.input_version)?
        .ok_or_else(mismatch)?;
    if source_input.request.actor_id != *actor
        || source_input.request.case_id != *parent
        || identity(&source_input.request)? != source.id
        || state.bundle.as_ref() != Some(&source_input)
        || store
            .get_input_result(actor, parent, &source.input_id, source.input_version)?
            .as_ref()
            != Some(source)
    {
        return Err(mismatch());
    }
    let source_render_ref = document::revalidate_previous_document_render(
        store,
        actor,
        parent,
        source,
        None,
        MISMATCH,
        "branch source",
    )?;
    let origin = canonical(&Origin {
        kind: "minidora.branch-source-text.v1",
        branch: &branch,
        source_input_ref,
        source_render_ref,
        source_text_sha256: digest(source.text.as_bytes()),
    })?;
    let (report, unit, guarantee) = match &bundle.meaning.intent {
        Intent::SummarizeBranchSource { limit } => {
            if matches!(
                source_input.meaning.intent,
                Intent::ReviewAttachedDocumentFields { .. }
            ) {
                return Err(failure(
                    "BRANCH_DOCUMENT_REVIEW_SUMMARY_UNAVAILABLE",
                    "分岐元の文書レビューは履歴で確認できます。分岐先で要点を整理するには、資料を添付して項目を指定してください。",
                ));
            }
            let plan = source.content_plan.as_ref().ok_or_else(mismatch)?;
            let render = source.render.as_ref().ok_or_else(mismatch)?;
            let quotes = if input::is_attached_document_intent(&source_input.meaning.intent) {
                Some(attached_document::retained_quote_ranges(
                    store,
                    &source_input,
                    source,
                )?)
            } else {
                None
            };
            let report = text_summary::summarize_branch_answer(
                &identity(request)?,
                &source.id,
                &plan.id,
                &render.actual_sha256,
                &source.text,
                *limit,
                quotes.as_deref(),
            )?;
            (
                canonical(&report)?,
                report.output_unit(),
                Guarantee::Hypothesis,
            )
        }
        Intent::ReplaceBranchSource { old, new } => {
            let revision = document::replace_unique_exact(&source.text, old, new).map_err(
                |error| match &error {
                    Error::Contract { code, .. } if code == "PREVIOUS_TEXT_NOT_FOUND" => {
                        failure(code, "分岐元の回答に指定の文字列が見つかりませんでした。")
                    }
                    Error::Contract { code, .. } if code == "PREVIOUS_TEXT_AMBIGUOUS" => failure(
                        code,
                        "分岐元の回答で指定の文字列が複数あります。置換する箇所を絞ってください。",
                    ),
                    _ => error,
                },
            )?;
            (canonical(&revision)?, revision.text, Guarantee::Computed)
        }
        _ => return Err(mismatch()),
    };
    if source
        .text
        .len()
        .saturating_add(origin.len())
        .saturating_add(report.len())
        .saturating_add(unit.len())
        > request.budget.max_evidence_bytes
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "分岐元の本文と処理結果が証拠予算を超えました。",
        ));
    }
    if Instant::now() >= deadline {
        return Err(failure(
            "DEADLINE",
            "分岐元の照合と本文の処理が期限を超えました。",
        ));
    }
    Ok(Outcome {
        dependencies: vec![digest(&origin), digest(&report)],
        units: vec![unit],
        guarantee,
        origin,
        report,
    })
}

pub(crate) fn produce(store: &Store, bundle: &InputBundle, started: Instant) -> Result<Outcome> {
    let result = evaluate(store, bundle, started)?;
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    let source = store.put_bytes(actor, case, &result.origin)?;
    let report = store.put_bytes(actor, case, &result.report)?;
    if result.dependencies != [source, report] {
        return Err(mismatch());
    }
    Ok(result)
}

pub(crate) fn verify_delivery(
    store: &Store,
    bundle: &InputBundle,
    result: &ResultRecord,
    plan: &ContentPlan,
) -> Result<String> {
    let expected = evaluate(store, bundle, Instant::now())?;
    let [source_ref, report_ref] = result.dependencies.as_slice() else {
        return Err(mismatch());
    };
    let actor = &bundle.request.actor_id;
    let case = &bundle.request.case_id;
    if result.dependencies != expected.dependencies
        || result.guarantee != expected.guarantee
        || plan.guarantee != expected.guarantee
        || result.calculation.is_some()
        || !result.evidence.is_empty()
        || plan.required_units != expected.units
        || result.text != expected.units[0]
        || plan.text != result.text
        || store.get_bytes(actor, case, source_ref)? != expected.origin
        || store.get_bytes(actor, case, report_ref)? != expected.report
    {
        return Err(mismatch());
    }
    Ok(source_ref.clone())
}
