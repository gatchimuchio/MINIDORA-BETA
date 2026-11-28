//! Resolve a saved purpose in Core and apply its limits to the request's grants.
use crate::{input, ports, store::Store};
use minidora_domain::{
    Error, Permissions, PurposeBinding, PurposeProfile, PurposeTask, PurposeTools, RequestEnvelope,
    Result, failure,
};

pub(crate) fn validate_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 96
        || !name
            .chars()
            .all(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err(failure(
            "PURPOSE_NAME_INVALID",
            "用途名は96バイト以内の文字・数字・ハイフン・下線で指定してください。",
        ));
    }
    Ok(())
}

pub(crate) fn validate_definition(profile: &PurposeProfile) -> Result<()> {
    validate_name(&profile.name)?;
    if let PurposeTask::Find { query } = &profile.task
        && (query.trim().is_empty()
            || query.trim() != query
            || query.len() > 512
            || query.chars().any(char::is_control))
    {
        return Err(failure(
            "PURPOSE_QUERY_INVALID",
            "箇所抽出の語句は512バイト以内で一行に収めてください。",
        ));
    }
    if profile.tools.read_hosts.len() > 16 {
        return Err(failure(
            "PURPOSE_TOOL_LIMIT",
            "用途で指定する接続先は最大16件です。",
        ));
    }
    let cap = Permissions {
        read_hosts: profile.tools.read_hosts.clone(),
        ..Permissions::default()
    };
    for host in &cap.read_hosts {
        let url = ports::approved_url(&format!("https://{host}/"), &cap).map_err(|_| {
            failure(
                "PURPOSE_HOST_INVALID",
                "用途には正規のHTTPSホスト名を指定してください。",
            )
        })?;
        if url.host_str() != Some(host.as_str()) || url.path() != "/" || url.query().is_some() {
            return Err(failure(
                "PURPOSE_HOST_INVALID",
                "用途には正規のHTTPSホスト名を指定してください。",
            ));
        }
    }
    if profile.attachments.is_empty() {
        return Err(failure(
            "PURPOSE_ATTACHMENT_REQUIRED",
            "先に用途で使う資料を添付してください。",
        ));
    }
    let mut request = RequestEnvelope::new("purpose", "attachment-validation", "purpose", "");
    request.attachments.clone_from(&profile.attachments);
    input::validate_request_attachments(&request)?;
    if request.input_bytes() > request.budget.max_input_bytes {
        return Err(failure(
            "PURPOSE_ATTACHMENT_LIMIT",
            "用途の添付資料が入力上限を超えています。",
        ));
    }
    Ok(())
}

fn restrict(grants: &Permissions, tools: &PurposeTools) -> Permissions {
    Permissions {
        read_hosts: grants
            .read_hosts
            .intersection(&tools.read_hosts)
            .cloned()
            .collect(),
        write_paths: if tools.write_files {
            grants.write_paths.clone()
        } else {
            Default::default()
        },
        wasm_execution: grants.wasm_execution && tools.wasm_execution,
    }
}

fn invalid_binding() -> Error {
    failure(
        "PURPOSE_BINDING_INVALID",
        "用途の保存版と今回の許可を照合できません。",
    )
}

pub(crate) fn bind(store: &Store, request: &mut RequestEnvelope) -> Result<()> {
    let Some(selection) = &mut request.purpose else {
        return Ok(());
    };
    if selection.binding.is_some() {
        return Err(invalid_binding());
    }
    validate_name(&selection.name)?;
    let (profile_sha256, stored) = store
        .load_purpose_profile(&request.actor_id, &selection.name)?
        .ok_or_else(|| {
            failure(
                "PURPOSE_NOT_FOUND",
                "この利用者には指定された用途の保存版がありません。",
            )
        })?;
    let effective = restrict(&request.permissions, &stored.tools);
    let grants = std::mem::replace(&mut request.permissions, effective);
    selection.binding = Some(PurposeBinding {
        profile_sha256,
        task: stored.task,
        requested_permissions: grants,
    });
    Ok(())
}

pub(crate) fn verify_binding(
    store: &Store,
    request: &RequestEnvelope,
) -> Result<Option<PurposeProfile>> {
    let Some(selection) = &request.purpose else {
        return Ok(None);
    };
    let binding = selection.binding.as_ref().ok_or_else(invalid_binding)?;
    let (digest, stored) = store
        .load_purpose_profile(&request.actor_id, &selection.name)?
        .ok_or_else(invalid_binding)?;
    if binding.profile_sha256 != digest
        || binding.task != stored.task
        || request.permissions != restrict(&binding.requested_permissions, &stored.tools)
    {
        return Err(invalid_binding());
    }
    Ok(Some(stored))
}
