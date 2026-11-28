//! Explain a recorded stop using its original request and reason.
use crate::{input, store::Store};
use minidora_domain::{
    CaseRevision, InputBundle, Lifecycle, Result, canonical, digest, failure, identity,
};
use serde::Serialize;

pub(crate) const PREDICATE: &str = "PREVIOUS_STOP_EXPLANATION_V1";
pub(crate) const SOURCE_NODE: &str = "previous-stop-evidence";

pub(crate) struct StopExplanation {
    pub units: Vec<String>,
    pub dependencies: Vec<String>,
    pub source_ref: String,
    pub source_bytes: Vec<u8>,
}

#[derive(Serialize)]
struct StopEvidence<'a> {
    kind: &'static str,
    result_id: &'a str,
    result_sha256: String,
    bundle_sha256: String,
    revision: u64,
    code: &'a str,
}

fn invalid() -> minidora_domain::Error {
    failure(
        "PREVIOUS_STOP_UNAVAILABLE",
        "同じ会話に保存された停止理由を確認できませんでした。",
    )
}

pub(crate) fn explain_previous_stop(
    store: &Store,
    state: &CaseRevision,
    bundle: &InputBundle,
) -> Result<StopExplanation> {
    let request = &bundle.request;
    let actor = &state.actor_id;
    let case = &state.case_id;
    if request.actor_id != *actor
        || request.case_id != *case
        || bundle.meaning.parser_version != input::PREVIOUS_STOP_EXPLANATION_PARSER_VERSION
    {
        return Err(invalid());
    }
    let prior = bundle
        .meaning
        .context_result_ref
        .as_ref()
        .and_then(|reference| {
            state
                .prior_results
                .get(reference)
                .filter(|value| value.id == *reference)
        })
        .ok_or_else(invalid)?;
    if prior.lifecycle != Lifecycle::Suspend || input::seal(request, Some(prior))? != *bundle {
        return Err(invalid());
    }
    let stop = if request.context_ref.is_some() {
        store.load_suspended_result_revision(actor, case, prior)?
    } else {
        let predecessor = store.input_start_predecessor_revision(state)?;
        if store
            .get_input_result(actor, case, &prior.input_id, prior.input_version)?
            .as_ref()
            != Some(prior)
        {
            return Err(invalid());
        }
        predecessor.filter(|revision| revision.result.as_ref() == Some(prior))
    }
    .ok_or_else(invalid)?;
    let (bundle_sha256, recorded) = store
        .get_input_bundle(actor, case, &prior.input_id, prior.input_version)?
        .ok_or_else(invalid)?;
    if stop.lifecycle != Lifecycle::Suspend
        || stop.actor_id != *actor
        || stop.case_id != *case
        || stop.revision >= state.revision
        || stop.bundle.as_ref() != Some(&recorded)
        || stop.reason != prior.reason
        || recorded.request.actor_id != *actor
        || recorded.request.case_id != *case
        || identity(&recorded.request)? != prior.id
    {
        return Err(invalid());
    }
    let (code, detail) = prior
        .reason
        .as_deref()
        .and_then(|reason| reason.split_once(": "))
        .filter(|(code, detail)| !code.is_empty() && !detail.trim().is_empty())
        .ok_or_else(invalid)?;
    let units = stop_explanation(code, detail);
    let source_bytes = canonical(&StopEvidence {
        kind: "minidora.previous-stop.v1",
        result_id: &prior.id,
        result_sha256: digest(&canonical(prior)?),
        bundle_sha256,
        revision: stop.revision,
        code,
    })?;
    if units.join("\n").len().saturating_add(source_bytes.len()) > request.budget.max_evidence_bytes
    {
        return Err(failure(
            "RESOURCE_LIMIT",
            "停止理由の説明が今回の根拠予算を超えます。",
        ));
    }
    let source_ref = digest(&source_bytes);
    Ok(StopExplanation {
        units,
        dependencies: vec![prior.id.clone(), source_ref.clone()],
        source_ref,
        source_bytes,
    })
}

fn stop_explanation(code: &str, detail: &str) -> Vec<String> {
    let mut units = vec!["前の依頼は保留になっています。".into()];
    match code {
        "CAPABILITY_UNAVAILABLE" => {
            units.push("依頼に合う処理を特定できませんでした。".into());
            units.push("何について、何をしてほしいかを具体的に教えてください。".into());
        }
        "WRITE_PERMISSION_MISSING" => {
            units.push("指定された保存先への書き込み許可がありませんでした。".into());
        }
        "ACTION_BUDGET_EXHAUSTED" => {
            units.push("今回の依頼で実行できる操作回数の上限に達しました。".into());
        }
        "DEADLINE" => {
            units.push("今回の依頼に設定された処理時間を超えました。".into());
        }
        _ => {
            let mut excerpt = detail.chars();
            let text: String = excerpt.by_ref().take(180).collect();
            let mut detail = text.split_whitespace().collect::<Vec<_>>().join(" ");
            detail.retain(|ch| !ch.is_control());
            if excerpt.next().is_some() {
                detail.push('…');
            }
            units.push(format!("停止時の記録には「{detail}」とあります。"));
        }
    }
    units
}
