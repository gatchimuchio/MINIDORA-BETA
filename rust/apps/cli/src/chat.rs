use super::{Options, execute_request};
use minidora_domain::{CaseRevision, EffectPhase, ResultRecord};
use minidora_engine::Engine;
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
/history [結果ID]   現在の会話の最新結果と履歴ID、または指定結果を表示\n\
/context 結果ID     現在の会話の保存済み結果を次の一発話で参照\n\
/context off        次の参照指定を解除\n\
/status             エンジン、提供能力、現在の会話で採用された模型の版を表示\n\
/paste              複数行を入力。単独の /end 行でまとめて送信\n\
/net [on|off]       通信設定を確認・変更。起動時の --allow-read-host の範囲内\n\
/output PATH        次の一発話の成果を指定先へ保存。起動時の --allow-write の範囲内\n\
/output off         次の保存指定を解除\n\
/reconcile          現在の会話の保存を照合。確定済みなら受領記録を表示\n\
/reconcile-effects  取り消した会話に残る作用を照合\n\
未確定の会話を取り消すには「中止」と入力してください。\n\
再開するには、同じ --db、--actor、--case で chat を起動してください。";
const MAX_CASE_ID_BYTES: usize = 256;
const MAX_PATH_ARGUMENT_BYTES: usize = 4_096;

enum Line {
    Input(Vec<u8>),
    TooLong,
    End,
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
        return Ok(Line::TooLong);
    }
    Ok(Line::Input(bytes))
}

fn read_paste(reader: &mut impl BufRead, max_bytes: usize) -> io::Result<Line> {
    let mut pasted = Vec::new();
    let mut too_long = false;
    loop {
        match read_line(reader, max_bytes.max(b"/end".len()))? {
            Line::End => return Ok(Line::End),
            Line::TooLong => too_long = true,
            Line::Input(line) if line_body(&line) == b"/end" => {
                // Preserve all internal line endings; the last ends the frame.
                pasted.truncate(line_body(&pasted).len());
                return Ok(if too_long || pasted.len() > max_bytes {
                    Line::TooLong
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
        show_effect_record(receipt.phase, &receipt.target, &receipt.action_id);
    }
    Ok(())
}

fn show_effect_record(phase: EffectPhase, target: &str, action_id: &str) {
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
    eprintln!("作用記録 {phase} / 対象 {target:?} / 作用ID {action_id}");
}

fn saved_result<'a>(case: &'a CaseRevision, result_id: &str) -> Option<&'a ResultRecord> {
    case.result
        .as_ref()
        .filter(|result| result.id == result_id)
        .or_else(|| case.prior_results.get(result_id))
}

fn show_history(engine: &Engine, options: &Options, result_id: &str) -> Result<(), String> {
    let Some(case) = engine
        .latest(&options.actor, &options.case_id)
        .map_err(|error| error.to_string())?
    else {
        eprintln!("この会話には保存済みの結果がありません。");
        return Ok(());
    };
    if !result_id.is_empty() {
        match saved_result(&case, result_id) {
            Some(result) => show_result(result).map_err(|error| error.to_string())?,
            None => eprintln!("この会話に該当する結果IDがありません。"),
        }
        return Ok(());
    }
    if let Some(result) = &case.result {
        eprintln!(
            "会話 {id} の最新結果ID: {result_id}",
            id = case.case_id,
            result_id = result.id
        );
        show_result(result).map_err(|error| error.to_string())?;
    }
    if !case.prior_results.is_empty() {
        eprintln!(
            "以前の結果ID（全{}件、先頭20件まで）:",
            case.prior_results.len()
        );
        for id in case.prior_results.keys().take(20) {
            eprintln!("{id}");
        }
    }
    Ok(())
}

fn show_status(engine: &Engine, options: &Options) -> Result<(), String> {
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
    if let Some(result_id) = &options.context_ref {
        eprintln!("次の発話の参照先: {result_id}");
    }
    Ok(())
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
            Line::TooLong => {
                eprintln!("入力が上限を超えたため送信しませんでした。");
                continue;
            }
            Line::Input(bytes) => bytes,
        };
        bytes.truncate(line_body(&bytes).len());
        let Ok(mut text) = String::from_utf8(bytes) else {
            eprintln!("UTF-8ではない入力を送信しませんでした。");
            continue;
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
                    eprintln!("次の発話の参照指定を解除しました。");
                    continue;
                }
                ("/context", id)
                    if id.len() != 64
                        || !id
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) =>
                {
                    eprintln!(
                        "/context の後に /history に表示された結果IDか off を入力してください。"
                    );
                    continue;
                }
                ("/context", id) => {
                    match engine.latest(&options.actor, &options.case_id) {
                        Ok(Some(case)) if saved_result(&case, id).is_some() => {
                            options.context_ref = Some(id.into());
                            eprintln!("次の発話の参照先: {id}");
                        }
                        Ok(_) => eprintln!("この会話に該当する結果IDがありません。"),
                        Err(error) => eprintln!("参照先を選択できませんでした。{error}"),
                    }
                    continue;
                }
                ("/status", "") => {
                    if let Err(error) = show_status(engine, &options) {
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
                                show_effect_record(action.phase, &action.target, &action.action_id);
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
                    eprintln!("次の発話の保存指定を解除しました。");
                    continue;
                }
                ("/output", "") => {
                    if let Some(path) = &options.output {
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
                    } else {
                        options.output = None;
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
                        Line::TooLong => {
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
