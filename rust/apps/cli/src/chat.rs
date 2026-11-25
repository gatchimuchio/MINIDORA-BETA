use super::{Options, execute_request};
use minidora_domain::{
    CaseRevision, DeliveryReceipt, EffectPhase, Intent, RequestEnvelope, ResultRecord,
};
use minidora_engine::{Engine, input};
use std::collections::BTreeSet;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const HELP: &str = "\
日本語で入力し、Enterで送信します。\n\
/help               この案内を表示\n\
/exit               対話を終了\n\
/case ID            同じ利用者の会話を選択\n\
/new ID             未使用の会話IDを選択\n\
/history [結果ID]   現在の会話の最新結果と履歴、または指定結果を表示\n\
/history page N    以前の結果を1ページ20件ずつ表示\n\
/context 結果ID     現在の会話の保存済み結果を次の一発話で参照\n\
/context off        次の参照指定を解除\n\
                    参照先を指定し直せないときは発話を送信しません\n\
/status             エンジン、提供能力、会話の状態と採用された模型の版を表示\n\
/paste              複数行を入力。単独の /end 行でまとめて送信\n\
/net [on|off]       通信設定を確認・変更。起動時の --allow-read-host の範囲内\n\
/output PATH        次の一発話の成果を指定先へ保存。起動時の --allow-write の範囲内\n\
/output off         次の保存指定を解除\n\
                    選択に失敗した場合は、選び直すか解除するまで通常の発話を送信しません\n\
/reconcile          現在の会話の保存を照合。確定済みなら受領記録を表示\n\
/reconcile-effects  取り消した会話に残る作用を照合\n\
未確定の会話を取り消すには「中止」と入力してください。\n\
再開するには、同じ --db、--actor、--case で chat を起動してください。";
const MAX_CASE_ID_BYTES: usize = 256;
const MAX_PATH_ARGUMENT_BYTES: usize = 4_096;
const HISTORY_PAGE_SIZE: usize = 20;
const UNRESOLVED_CONTEXT: &str =
    "参照先が未確定です。/context 結果ID で選び直すか、/context off で解除してください。";
const UNRESOLVED_OUTPUT: &str = "保存先が未確定です。/output PATH で許可した保存先を選び直すか、/output off で解除してください。";

enum Line {
    Input(Vec<u8>),
    TooLong { output_command: bool },
    End,
}

fn starts_output_command(bytes: &[u8]) -> bool {
    let Some(argument) = bytes.strip_prefix(b"/output") else {
        return false;
    };
    // Only the command separator matters. The path itself may be invalid UTF-8.
    let prefix = std::str::from_utf8(argument).unwrap_or_else(|error| {
        std::str::from_utf8(&argument[..error.valid_up_to()]).expect("valid UTF-8 prefix")
    });
    prefix.starts_with(char::is_whitespace)
}

fn line_body(bytes: &[u8]) -> &[u8] {
    match bytes.strip_suffix(b"\n") {
        Some(body) => body.strip_suffix(b"\r").unwrap_or(body),
        None => bytes,
    }
}

fn read_line(reader: &mut impl BufRead, max_bytes: usize) -> io::Result<Line> {
    let mut bytes = Vec::new();
    // Leave room for CRLF framing without charging it against the turn.
    let cap = u64::try_from(max_bytes)
        .unwrap_or(u64::MAX)
        .saturating_add(2);
    if (&mut *reader).take(cap).read_until(b'\n', &mut bytes)? == 0 {
        return Ok(Line::End);
    }
    if line_body(&bytes).len() > max_bytes {
        if !bytes.ends_with(b"\n") {
            reader.skip_until(b'\n')?;
        }
        return Ok(Line::TooLong {
            output_command: starts_output_command(&bytes),
        });
    }
    Ok(Line::Input(bytes))
}

fn read_paste(reader: &mut impl BufRead, max_bytes: usize) -> io::Result<Line> {
    let mut pasted = Vec::new();
    let mut too_long = false;
    loop {
        match read_line(reader, max_bytes.max(b"/end".len()))? {
            Line::End => return Ok(Line::End),
            Line::TooLong { .. } => too_long = true,
            Line::Input(line) if line_body(&line) == b"/end" => {
                // Preserve all internal line endings; the last ends the frame.
                pasted.truncate(line_body(&pasted).len());
                return Ok(if too_long || pasted.len() > max_bytes {
                    Line::TooLong {
                        output_command: false,
                    }
                } else {
                    Line::Input(pasted)
                });
            }
            Line::Input(line) => {
                if !too_long {
                    let remaining = max_bytes.saturating_add(2).saturating_sub(pasted.len());
                    if line.len() > remaining {
                        too_long = true;
                    } else {
                        pasted.extend(line);
                    }
                }
            }
        }
    }
}

fn show_result(result: &ResultRecord) -> io::Result<()> {
    if !result.text.is_empty() {
        writeln!(io::stdout().lock(), "{}", result.text)?;
    }
    if let Some(reason) = &result.reason {
        match reason.split_once(": ") {
            Some((code, message)) if message == result.text => eprintln!("{code}"),
            _ => eprintln!("{reason}"),
        }
    }
    for receipt in &result.receipts {
        show_receipt(receipt);
    }
    Ok(())
}

pub(super) fn show_receipt(receipt: &DeliveryReceipt) {
    show_effect_record(
        receipt.phase,
        &receipt.target,
        &receipt.action_id,
        receipt.reason.as_deref(),
    );
}

fn show_effect_record(phase: EffectPhase, target: &str, action_id: &str, reason: Option<&str>) {
    let phase = match phase {
        EffectPhase::Proposed => "提案",
        EffectPhase::Authorized => "許可済み",
        EffectPhase::Prepared => "準備済み",
        EffectPhase::Attempted => "試行済み",
        EffectPhase::Confirmed => "確認済み",
        EffectPhase::Failed => "失敗",
        EffectPhase::Unknown => "結果未確定",
        EffectPhase::Cancelled => "取消済み",
    };
    let reason = reason.map_or_else(String::new, |reason| format!(" / 理由 {reason:?}"));
    let _ = writeln!(
        io::stderr().lock(),
        "作用記録 {phase} / 対象 {target:?} / 作用ID {action_id}{reason}"
    );
}

fn saved_result<'a>(case: &'a CaseRevision, result_id: &str) -> Option<&'a ResultRecord> {
    case.result
        .as_ref()
        .filter(|result| result.id == result_id)
        .or_else(|| case.prior_results.get(result_id))
}

fn show_history(engine: &Engine, options: &Options, selection: &str) -> Result<(), String> {
    let Some((case, order)) = engine
        .case_with_history_order(&options.actor, &options.case_id)
        .map_err(|error| error.to_string())?
    else {
        eprintln!("この会話には保存済みの結果がありません。");
        return Ok(());
    };
    let page = if selection.is_empty() {
        if let Some(result) = &case.result {
            eprintln!(
                "会話 {id} の最新結果ID: {result_id}",
                id = case.case_id,
                result_id = result.id
            );
            show_result(result).map_err(|error| error.to_string())?;
        }
        1
    } else if selection == "page" || selection.starts_with("page ") {
        let Ok(page) = selection["page".len()..].trim().parse::<usize>() else {
            eprintln!("/history page の後に1以上のページ番号を指定してください。");
            return Ok(());
        };
        page
    } else {
        match saved_result(&case, selection) {
            Some(result) => show_result(result).map_err(|error| error.to_string())?,
            None => eprintln!("この会話に該当する結果IDがありません。"),
        }
        return Ok(());
    };

    let count = order.len();
    let pages = count.div_ceil(HISTORY_PAGE_SIZE);
    if pages == 0 && selection.is_empty() {
        return Ok(());
    }
    if page == 0 || page > pages {
        eprintln!("そのページには以前の結果がありません（全{pages}ページ）。");
        return Ok(());
    }
    eprintln!("以前の結果ID（{page}/{pages}ページ、全{count}件）:");
    for id in order
        .iter()
        .skip((page - 1) * HISTORY_PAGE_SIZE)
        .take(HISTORY_PAGE_SIZE)
    {
        let result = &case.prior_results[id];
        let message = result
            .text
            .lines()
            .find(|line| !line.trim().is_empty())
            .or(result.reason.as_deref())
            .unwrap_or_default();
        let mut characters = message.chars();
        let mut preview: String = characters.by_ref().take(48).collect();
        if characters.next().is_some() {
            preview.push('…');
        }
        eprintln!("{id} [{:?}] {preview:?}", result.lifecycle);
    }
    if page < pages {
        eprintln!("続きは /history page {}", page + 1);
    }
    Ok(())
}

fn show_status(
    engine: &Engine,
    options: &Options,
    context_unresolved: bool,
    output_unresolved: bool,
) -> Result<(), String> {
    let status = engine
        .case_status(&options.actor, &options.case_id)
        .map_err(|error| error.to_string())?;
    let info = status.engine;
    eprintln!(
        "{} {} / API {} / schema {} / {} {}",
        info.package,
        info.version,
        info.api_version,
        info.schema_version,
        info.target_os,
        info.target_arch
    );
    for capability in info.capabilities {
        eprintln!("{}: {}", capability.id, capability.scope);
    }
    if let (Some(revision), Some(lifecycle)) = (status.case_revision, status.case_lifecycle) {
        eprintln!(
            "会話 {:?} / 保存版 {revision} / 会話状態 {lifecycle:?}",
            status.case_id
        );
    } else {
        eprintln!("会話 {:?} / 保存版なし", status.case_id);
    }
    if let Some(result) = status.result {
        eprintln!("最新結果 {} ({:?})", result.id, result.lifecycle);
        if let Some(reason) = result.reason {
            eprintln!("{reason}");
        }
        eprintln!(
            "model_revision={}",
            result.model_revision.as_deref().unwrap_or("未記録")
        );
    } else {
        eprintln!("この会話には保存済みの結果がありません。");
    }
    if context_unresolved {
        eprintln!("{UNRESOLVED_CONTEXT}");
    } else if let Some(result_id) = &options.context_ref {
        eprintln!("次の発話の参照先: {result_id}");
    }
    if output_unresolved {
        eprintln!("{UNRESOLVED_OUTPUT}");
    }
    Ok(())
}

fn is_cancellation(options: &Options, text: &str) -> bool {
    // A cancellation does not require a new output action. Use the same
    // interpretation as Core without registering an input or changing a case.
    let mut probe = RequestEnvelope::new(&options.case_id, "cancel-check", &options.actor, text);
    probe.budget = options.budget.clone();
    input::seal(&probe, None).is_ok_and(|bundle| matches!(bundle.meaning.intent, Intent::Cancel))
}

pub(super) fn run(engine: &mut Engine, mut options: Options) -> Result<u8, String> {
    if options.case_id.len() > MAX_CASE_ID_BYTES {
        return Err(format!(
            "会話IDは{MAX_CASE_ID_BYTES}バイト以内で指定してください。"
        ));
    }
    engine
        .latest(&options.actor, &options.case_id)
        .map_err(|error| error.to_string())?;
    let stdin = io::stdin();
    let interactive = stdin.is_terminal();
    let mut reader = stdin.lock();
    let offline = BTreeSet::new();
    let mut network_enabled = !options.read_hosts.is_empty();
    let mut context_unresolved = false;
    let mut output_unresolved = false;
    let started = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?;
    let input_prefix = format!("chat-{:x}-{:x}", started.as_nanos(), std::process::id());
    let mut turn = 0u64;
    if interactive {
        eprintln!(
            "MINIDORA — 会話: {} / 保存先: {}\n操作の案内: /help",
            options.case_id,
            options.db.display()
        );
    }
    loop {
        if interactive {
            eprint!("> ");
            io::stderr().flush().map_err(|error| error.to_string())?;
        }
        let max_bytes = options.budget.max_input_bytes;
        let line = read_line(
            &mut reader,
            max_bytes.max(MAX_PATH_ARGUMENT_BYTES + b"/output ".len()),
        )
        .map_err(|error| error.to_string())?;
        let mut bytes = match line {
            Line::End => break,
            Line::TooLong { output_command } => {
                if output_command {
                    options.output = None;
                    output_unresolved = true;
                }
                eprintln!("入力が上限を超えたため送信しませんでした。");
                continue;
            }
            Line::Input(bytes) => bytes,
        };
        bytes.truncate(line_body(&bytes).len());
        let mut text = match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(error) => {
                if starts_output_command(error.as_bytes()) {
                    options.output = None;
                    output_unresolved = true;
                }
                eprintln!("UTF-8ではない入力を送信しませんでした。");
                continue;
            }
        };
        if text.starts_with('/') {
            let (command, argument) = text
                .split_once(char::is_whitespace)
                .map_or((text.as_str(), ""), |(command, argument)| {
                    (command, argument.trim())
                });
            match (command, argument) {
                ("/exit", "") => break,
                ("/help", "") => {
                    eprintln!("{HELP}");
                    continue;
                }
                ("/case" | "/new", "") => {
                    eprintln!("会話IDを指定してください。");
                    continue;
                }
                ("/case" | "/new", id) if id.len() > MAX_CASE_ID_BYTES => {
                    eprintln!("会話IDは{MAX_CASE_ID_BYTES}バイト以内で指定してください。");
                    continue;
                }
                ("/case" | "/new", id) => {
                    match engine.latest(&options.actor, id) {
                        Ok(Some(_)) if command == "/new" => {
                            eprintln!("使用済みの会話IDです。再開するには /case を使ってください。")
                        }
                        Ok(_) => {
                            if options.case_id != id {
                                options.context_ref = None;
                                context_unresolved = false;
                                output_unresolved = false;
                            }
                            options.case_id = id.to_owned();
                            options.output = None;
                            eprintln!("会話: {}", options.case_id);
                        }
                        Err(error) => eprintln!("会話を選択できませんでした。{error}"),
                    }
                    continue;
                }
                ("/history", id) => {
                    if let Err(error) = show_history(engine, &options, id) {
                        eprintln!("履歴を読み込めませんでした。{error}");
                    }
                    continue;
                }
                ("/context", "off") => {
                    options.context_ref = None;
                    context_unresolved = false;
                    eprintln!("次の発話の参照指定を解除しました。");
                    continue;
                }
                ("/context", id) => {
                    options.context_ref = None;
                    context_unresolved = true;
                    if id.len() != 64
                        || !id
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                    {
                        eprintln!(
                            "/context の後に /history に表示された結果IDか off を入力してください。"
                        );
                        continue;
                    }
                    match engine.latest(&options.actor, &options.case_id) {
                        Ok(Some(case)) if saved_result(&case, id).is_some() => {
                            options.context_ref = Some(id.into());
                            context_unresolved = false;
                            eprintln!("次の発話の参照先: {id}");
                        }
                        Ok(_) => eprintln!("この会話に該当する結果IDがありません。"),
                        Err(error) => eprintln!("参照先を選択できませんでした。{error}"),
                    }
                    continue;
                }
                ("/status", "") => {
                    if let Err(error) =
                        show_status(engine, &options, context_unresolved, output_unresolved)
                    {
                        eprintln!("状態を読み込めませんでした。{error}");
                    }
                    continue;
                }
                ("/reconcile", "") => {
                    match engine.reconcile(&options.actor, &options.case_id) {
                        Ok(result) => {
                            eprintln!("照合後の結果ID: {} ({:?})", result.id, result.lifecycle);
                            show_result(&result).map_err(|error| error.to_string())?;
                        }
                        Err(error) => eprintln!("保存結果を照合できませんでした。{error}"),
                    }
                    continue;
                }
                ("/reconcile-effects", "") => {
                    match engine.reconcile_cancelled_effects(&options.actor, &options.case_id) {
                        Ok(actions) if actions.is_empty() => {
                            eprintln!("この会話に保存済みの作用はありません。");
                        }
                        Ok(actions) => {
                            for action in actions {
                                show_effect_record(
                                    action.phase,
                                    &action.target,
                                    &action.action_id,
                                    action
                                        .receipt
                                        .as_ref()
                                        .and_then(|receipt| receipt.reason.as_deref()),
                                );
                            }
                        }
                        Err(error) => eprintln!("作用を照合できませんでした。{error}"),
                    }
                    continue;
                }
                ("/net", "" | "on" | "off") => {
                    if !argument.is_empty() {
                        network_enabled = argument == "on" && !options.read_hosts.is_empty();
                    }
                    if network_enabled {
                        let hosts = options
                            .read_hosts
                            .iter()
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(", ");
                        eprintln!("通信許可先: {hosts}");
                    } else {
                        eprintln!(
                            "通信はオフです。許可先は起動時の --allow-read-host で指定できます。"
                        );
                    }
                    continue;
                }
                ("/output", "off") => {
                    options.output = None;
                    output_unresolved = false;
                    eprintln!("次の発話の保存指定を解除しました。");
                    continue;
                }
                ("/output", "") => {
                    if output_unresolved {
                        eprintln!("{UNRESOLVED_OUTPUT}");
                    } else if let Some(path) = &options.output {
                        eprintln!("次の発話の保存先: {}", path.display());
                    } else {
                        eprintln!("保存先を指定するには /output PATH と入力してください。");
                    }
                    continue;
                }
                ("/output", path) => {
                    let path = PathBuf::from(path);
                    if options.write_paths.contains(&path) {
                        eprintln!("次の発話の保存先: {}", path.display());
                        options.output = Some(path);
                        output_unresolved = false;
                    } else {
                        options.output = None;
                        output_unresolved = true;
                        eprintln!(
                            "WRITE_PERMISSION_MISSING: 起動時に許可した保存先を指定してください。"
                        );
                    }
                    continue;
                }
                ("/paste", "") => {
                    if interactive {
                        eprintln!("複数行を入力し、最後に新しい行で /end と入力してください。");
                    }
                    match read_paste(&mut reader, max_bytes).map_err(|error| error.to_string())? {
                        Line::End => {
                            eprintln!("/end がないため送信しませんでした。");
                            break;
                        }
                        Line::TooLong { .. } => {
                            eprintln!("入力が上限を超えたため送信しませんでした。");
                            continue;
                        }
                        Line::Input(pasted) => {
                            let Ok(request) = String::from_utf8(pasted) else {
                                eprintln!("UTF-8ではない入力を送信しませんでした。");
                                continue;
                            };
                            text = request;
                        }
                    }
                }
                _ => {
                    eprintln!("操作を確認するには /help と入力してください。");
                    continue;
                }
            }
        }
        if text.trim().is_empty() {
            continue;
        }
        if text.len() > max_bytes {
            eprintln!("入力が上限を超えたため送信しませんでした。");
            continue;
        }
        if context_unresolved {
            eprintln!("入力を送信していません。{UNRESOLVED_CONTEXT}");
            continue;
        }
        if output_unresolved && !is_cancellation(&options, &text) {
            eprintln!("入力を送信していません。{UNRESOLVED_OUTPUT}");
            continue;
        }
        let read_hosts = if network_enabled {
            &options.read_hosts
        } else {
            &offline
        };
        // A rejected attempt can register an input without advancing the case.
        // A fresh turn, including cancel, needs its own input identity.
        turn = turn
            .checked_add(1)
            .ok_or("対話の発話数が上限に達しました。")?;
        options.input_id = Some(format!("{input_prefix}-{turn:x}"));
        let result = execute_request(engine, &options, &text, read_hosts);
        options.context_ref = None;
        options.output = None;
        match result {
            Ok(result) => show_result(&result).map_err(|error| error.to_string())?,
            Err(error) => eprintln!("入力を処理できませんでした。{error}"),
        }
    }
    Ok(0)
}
