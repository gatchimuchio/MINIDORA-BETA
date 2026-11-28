use minidora_domain::{MAX_REQUEST_ATTACHMENTS, RequestAttachment, ResourceBudget};
use std::path::Path;

pub(super) fn input_bytes(attachments: &[RequestAttachment]) -> usize {
    attachments.iter().fold(0, |bytes, file| {
        bytes
            .saturating_add(file.name.len())
            .saturating_add(file.payload_bytes())
    })
}

pub(super) fn stage(
    attachments: &mut Vec<RequestAttachment>,
    path: &Path,
    max_input_bytes: usize,
) -> Result<(), String> {
    if attachments.len() == MAX_REQUEST_ATTACHMENTS {
        return Err(format!("添付は最大{MAX_REQUEST_ATTACHMENTS}ファイルです。"));
    }
    if let Some(name) = path
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_lowercase)
        && attachments
            .iter()
            .any(|file| file.name.to_lowercase() == name)
    {
        return Err("同じ名前のファイルを複数添付できません。".into());
    }
    let remaining = max_input_bytes
        .min(ResourceBudget::development().max_input_bytes)
        .saturating_sub(input_bytes(attachments));
    let attachment = minidora_engine::snapshot_local_attachment(path, remaining)
        .map_err(|error| error.to_string())?;
    attachments.push(attachment);
    Ok(())
}
