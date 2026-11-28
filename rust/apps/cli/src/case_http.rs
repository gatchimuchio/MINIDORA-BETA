//! Authenticated HTTP access to the existing conversation history and branching operations.
use super::HttpError;
use minidora_domain::{BranchHistoryEntry, CaseBranch, Error, Lifecycle, ResultRecord};
use minidora_engine::Engine;
use serde::{Deserialize, Serialize};

pub(super) const FORK_ROUTE: &str = "/v1/cases/fork";
pub(super) const HISTORY_ROUTE: &str = "/v1/cases/history";
const API_VERSION: &str = "minidora-cases/v1";
const PAGE_SIZE: usize = 20;
const CASE_ID_MAX_BYTES: usize = 256;
const PREVIEW_MAX_CHARS: usize = 256;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForkRequest {
    actor_id: String,
    source_case_id: String,
    case_id: String,
    source_result_id: Option<String>,
}

#[derive(Serialize)]
struct ForkResponse {
    api_version: &'static str,
    branch: CaseBranch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryRequest {
    actor_id: String,
    case_id: String,
    #[serde(default = "first_page")]
    page: usize,
    #[serde(default = "first_page")]
    inherited_page: usize,
    result_id: Option<String>,
}

#[derive(Serialize)]
struct HistoryEntry<'a> {
    id: &'a str,
    lifecycle: &'a Lifecycle,
    text: String,
    reason: Option<String>,
}

#[derive(Serialize)]
struct HistoryResponse<'a> {
    api_version: &'static str,
    case_id: String,
    registered: bool,
    can_fork: bool,
    branch: Option<CaseBranch>,
    page: usize,
    entries: Vec<HistoryEntry<'a>>,
    has_more: bool,
    inherited_page: usize,
    inherited: Vec<BranchHistoryEntry>,
    inherited_has_more: bool,
    result: Option<&'a ResultRecord>,
}

fn first_page() -> usize {
    1
}

fn page_offset(page: usize) -> Result<usize, HttpError> {
    page.checked_sub(1)
        .and_then(|index| index.checked_mul(PAGE_SIZE))
        .filter(|offset| {
            offset
                .checked_add(PAGE_SIZE)
                .is_some_and(|next| i64::try_from(next).is_ok())
        })
        .ok_or(HttpError(
            400,
            "履歴のページには範囲内の正の整数を指定してください。",
        ))
}

fn require_actor(server_actor: &str, requested_actor: &str) -> Result<(), HttpError> {
    if requested_actor != server_actor {
        return Err(HttpError(403, "指定した利用者が認証先と一致しません。"));
    }
    Ok(())
}

fn validate_case_id(case: &str) -> Result<(), HttpError> {
    if case.trim().is_empty()
        || case.len() > CASE_ID_MAX_BYTES
        || case.chars().any(char::is_control)
    {
        return Err(HttpError(
            400,
            "会話IDは空白のみや制御文字を避け、256バイト以内で指定してください。",
        ));
    }
    Ok(())
}

fn validate_result_id(result: Option<&str>) -> Result<(), HttpError> {
    if result.is_some_and(|id| {
        id.len() != 64
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }) {
        return Err(HttpError(
            400,
            "結果IDは小文字の16進数64桁で指定してください。",
        ));
    }
    Ok(())
}

fn core_error(error: Error) -> HttpError {
    if let Error::Contract { code, .. } = error {
        match code.as_str() {
            "ACCESS_DENIED" => HttpError(403, "この利用者から指定の会話にアクセスできません。"),
            "CASE_NOT_FOUND" => HttpError(404, "分岐元の会話が見つかりません。"),
            "CASE_BRANCH_DESTINATION_USED" => {
                HttpError(409, "分岐先の会話IDはすでに使用されています。")
            }
            "CASE_BRANCH_SOURCE_CANCELLED" => {
                HttpError(409, "取り消し済みの会話からは分岐できません。")
            }
            "CASE_BRANCH_SOURCE_UNAVAILABLE" => {
                HttpError(409, "分岐元で現在の確定結果を確認できません。")
            }
            _ => HttpError(409, "保存された会話を照合できません。"),
        }
    } else {
        HttpError(500, "会話の処理に必要な保存状態を読み取れませんでした。")
    }
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, HttpError> {
    serde_json::to_vec(value).map_err(|_| HttpError(500, "会話の応答を生成できませんでした。"))
}

pub(super) fn fork(engine: &Engine, actor: &str, body: &[u8]) -> Result<Vec<u8>, HttpError> {
    let request = serde_json::from_slice::<ForkRequest>(body)
        .map_err(|_| HttpError(400, "会話分岐の指定をJSONで入力してください。"))?;
    require_actor(actor, &request.actor_id)?;
    validate_case_id(&request.source_case_id)?;
    validate_case_id(&request.case_id)?;
    validate_result_id(request.source_result_id.as_deref())?;
    if request.source_case_id == request.case_id {
        return Err(HttpError(
            400,
            "分岐先には元の会話と異なるIDを指定してください。",
        ));
    }
    let branch = engine
        .fork_case(
            actor,
            &request.source_case_id,
            &request.case_id,
            request.source_result_id.as_deref(),
        )
        .map_err(|error| {
            if request.source_result_id.is_some()
                && matches!(&error, Error::Contract { code, .. } if code == "CASE_BRANCH_SOURCE_UNAVAILABLE")
            {
                HttpError(404, "指定した分岐元の確定結果が見つかりません。")
            } else {
                core_error(error)
            }
        })?;
    encode(&ForkResponse {
        api_version: API_VERSION,
        branch,
    })
}

fn preview(text: &str) -> String {
    let mut characters = text.chars();
    let mut text = characters
        .by_ref()
        .take(PREVIEW_MAX_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        text.pop();
        text.push('…');
    }
    text
}

pub(super) fn history(engine: &Engine, actor: &str, body: &[u8]) -> Result<Vec<u8>, HttpError> {
    let request = serde_json::from_slice::<HistoryRequest>(body)
        .map_err(|_| HttpError(400, "会話履歴の指定をJSONで入力してください。"))?;
    require_actor(actor, &request.actor_id)?;
    validate_case_id(&request.case_id)?;
    validate_result_id(request.result_id.as_deref())?;
    let offset = page_offset(request.page)?;
    let inherited_offset = page_offset(request.inherited_page)?;

    let case = engine
        .case_with_history_order(actor, &request.case_id)
        .map_err(core_error)?;
    let result = request
        .result_id
        .as_deref()
        .map(|id| {
            case.as_ref()
                .and_then(|(state, _)| {
                    state
                        .result
                        .as_ref()
                        .filter(|record| record.id == id)
                        .or_else(|| state.prior_results.get(id))
                })
                .ok_or(HttpError(404, "この会話に指定された結果はありません。"))
        })
        .transpose()?;
    let mut records = case.iter().flat_map(|(state, order)| {
        state.result.iter().chain(
            order
                .iter()
                .filter(|id| state.result.as_ref().is_none_or(|record| &record.id != *id))
                .filter_map(|id| state.prior_results.get(id)),
        )
    });
    let mut page_records = records.by_ref().skip(offset).take(PAGE_SIZE);
    let entries = page_records
        .by_ref()
        .map(|record| HistoryEntry {
            id: &record.id,
            lifecycle: &record.lifecycle,
            text: preview(&record.text),
            reason: record.reason.as_deref().map(preview),
        })
        .collect();
    let has_more = records.next().is_some();
    let can_fork = case.as_ref().is_some_and(|(state, _)| {
        state.lifecycle != Lifecycle::Cancel
            && state
                .result
                .iter()
                .chain(state.prior_results.values())
                .any(|record| record.lifecycle == Lifecycle::Commit)
    });
    let branch = engine
        .case_branch(actor, &request.case_id)
        .map_err(core_error)?;
    let registered = if case.is_some() || branch.is_some() {
        true
    } else {
        engine
            .case_registered(actor, &request.case_id)
            .map_err(core_error)?
    };
    let inherited = if branch.is_some() {
        engine
            .branch_history(actor, &request.case_id, inherited_offset, PAGE_SIZE)
            .map_err(core_error)?
    } else {
        Vec::new()
    };
    let inherited_has_more = inherited.len() == PAGE_SIZE
        && !engine
            .branch_history(actor, &request.case_id, inherited_offset + PAGE_SIZE, 1)
            .map_err(core_error)?
            .is_empty();
    encode(&HistoryResponse {
        api_version: API_VERSION,
        case_id: request.case_id,
        registered,
        can_fork,
        branch,
        page: request.page,
        entries,
        has_more,
        inherited_page: request.inherited_page,
        inherited,
        inherited_has_more,
        result,
    })
}
