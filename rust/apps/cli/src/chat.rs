use super::{Options, RequestInput, attachment, execute_request, microphone};
use minidora_domain::{
    CaseBranch, CaseRevision, DeliveryReceipt, EffectPhase, Intent, PurposeProfile, PurposeTask,
    PurposeTools, PurposeUse, RequestAttachment, RequestEnvelope, ResultRecord,
};
use minidora_engine::{CapabilityPermission, Engine, input};
use std::collections::BTreeSet;
use std::io::{self, BufRead, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const HELP: &str = "\
依頼を入力し、Enterで送信します。\n\
/help               この案内を表示\n\
/exit               対話を終了\n\
/language [ja|en]   テキスト依頼の応答言語を確認・変更。音声入力は日本語\n\
/case ID            同じ利用者の会話を選択\n\
/new ID             未使用の会話IDを選択\n\
/fork ID [結果ID]   現在の会話の確定結果から、未使用のIDへ分岐\n\
                    結果IDを省くときは最新の確定結果が必要です\n\
                    子の会話で「分岐元の回答を2行で要約して」と頼めます\n\
                    「分岐元の回答の「旧」を「新」に置き換えて」とも頼めます\n\
                    選択に失敗した場合は、会話を選び直すまで発話を送信しません\n\
/history [結果ID]   現在の会話の最新結果と履歴、または指定結果を表示\n\
/history page N    以前の結果を1ページ20件ずつ表示\n\
/history inherited [page N]  分岐時に引き継いだ履歴を表示\n\
/context 結果ID     現在の会話の保存済み結果を次の一発話で参照\n\
/context off        次の参照指定を解除\n\
                    選択に失敗した場合は、選び直すか解除するまで通常の発話を送信しません\n\
/status             エンジン、提供能力、会話の状態と採用された模型の版を表示\n\
/paste              複数行を入力。単独の /end 行でまとめて送信\n\
/listen PATH        --asr-assets で起動し、WAVの絶対パスを一発話として送信\n\
/record [秒数]      Windowsの端末で、既定のマイクから録音して一時保持\n\
                    --asr-assets で起動。秒数を省くと最大5秒\n\
/record send        保留中の録音を現在の会話に送信\n\
/record off         保留中の録音を破棄\n\
/attach PATH        WindowsのPDF、XLSX、PNG、JPEG、またはUTF-8の .csv、.txt、.md を次の一発話に添付\n\
                    複数回指定できます。添付資料を出典付きで要約して、と頼めます\n\
                    添付資料から「締切」が書かれた箇所を抜き出して、とも頼めます\n\
/attach off         次の発話の添付をすべて解除\n\
/profile            現在選択中の用途を表示\n\
/profile save NAME summarize [設定]  /attach で選んだ資料を使う要約の用途を保存\n\
/profile save NAME find QUERY [設定]  同じ方法で語句検索の用途を保存\n\
                    設定は +save、+web=HOST、+wasm。各操作には現在の許可も必要です\n\
                    検索語句に空白があるときは二重引用符で囲みます\n\
/profile use NAME   利用者の用途を選び、「用途の資料を確認して」で実行\n\
/profile off        用途の選択を解除\n\
/net [on|off]       通信設定を確認・変更。起動時の --allow-read-host の範囲内\n\
                    指定を読み取れない場合は通信だけをオフにします\n\
/output PATH        次の一発話の成果を指定先へ保存。起動時の --allow-write の範囲内\n\
                    --speech-assets で起動し、/output <絶対パス.wav> を設定して\n\
                    「前の回答を音声ファイルにして」と入力するとWAVを保存\n\
                    PNGは図形の配置や、添付画像の範囲を指定して保存できます\n\
/output off         次の保存指定を解除\n\
                    選択に失敗した場合は、選び直すか解除するまで通常の発話を送信しません\n\
/allow-exec         次の発話1件のWebAssembly隔離実行を許可。起動時に --wasm-node が必要\n\
                    保存先などを先に設定し、依頼または /paste の直前に入力します\n\
/reconcile          現在の会話の保存を照合。確定済みなら受領記録を表示\n\
/reconcile-effects  取り消した会話に残る作用を照合\n\
未確定の会話を取り消すには「中止」と入力してください。\n\
再開するには、同じ --db、--actor、--case で chat を起動してください。";
fn unquote_path(path: &str) -> &str {
    path.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or(path)
}

const MAX_CASE_ID_BYTES: usize = 256;
const MAX_PATH_ARGUMENT_BYTES: usize = 4_096;
const HISTORY_PAGE_SIZE: usize = 20;
const UNRESOLVED_CASE: &str =
    "会話先が未確定です。/case ID または /new ID で会話を選び直してください。";
const UNRESOLVED_CONTEXT: &str =
    "参照先が未確定です。/context 結果ID で選び直すか、/context off で解除してください。";
const UNRESOLVED_OUTPUT: &str = "保存先が未確定です。/output PATH で許可した保存先を選び直すか、/output off で解除してください。";
const UNRESOLVED_PROFILE: &str =
    "用途が未確定です。/profile use NAME で選び直すか、/profile off で解除してください。";
const PROFILE_USAGE: &str = "/profile の書式は /help で表示できます。";
const RECORDING_PENDING: &str =
    "保留中の録音があります。/record send で送信するか、/record off で破棄してください。";

enum SettingCommand {
    Case,
    Context,
    Output,
    Network,
    Attachment,
    Profile,
}

enum Line {
    Input(Vec<u8>),
    TooLong(Option<SettingCommand>),
    End,
}

fn leading_setting_command(bytes: &[u8]) -> Option<SettingCommand> {
    let (command, argument) = if let Some(argument) = bytes.strip_prefix(b"/net") {
        (SettingCommand::Network, argument)
    } else if let Some(argument) = bytes.strip_prefix(b"/context") {
        (SettingCommand::Context, argument)
    } else if let Some(argument) = bytes.strip_prefix(b"/attach") {
        (SettingCommand::Attachment, argument)
    } else if let Some(argument) = bytes.strip_prefix(b"/profile") {
        (SettingCommand::Profile, argument)
    } else if let Some(argument) = bytes
        .strip_prefix(b"/case")
        .or_else(|| bytes.strip_prefix(b"/new"))
        .or_else(|| bytes.strip_prefix(b"/fork"))
    {
        (SettingCommand::Case, argument)
    } else {
        (SettingCommand::Output, bytes.strip_prefix(b"/output")?)
    };
    // Classify the command boundary without requiring a decodable argument.
    argument
        .utf8_chunks()
        .next()
        .is_some_and(|prefix| prefix.valid().starts_with(char::is_whitespace))
        .then_some(command)
}

fn disable_network_after_bad_setting(network_enabled: &mut bool) {
    *network_enabled = false;
    eprintln!(
        "通信設定を読み取れなかったため、通信をオフにしました。再開するには /net on を入力してください。"
    );
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
        return Ok(Line::TooLong(leading_setting_command(&bytes)));
    }
    Ok(Line::Input(bytes))
}

fn read_paste(reader: &mut impl BufRead, max_bytes: usize) -> io::Result<Line> {
    let mut pasted = Vec::new();
    let mut too_long = false;
    loop {
        match read_line(reader, max_bytes.max(b"/end".len()))? {
            Line::End => return Ok(Line::End),
            Line::TooLong(_) => too_long = true,
            Line::Input(line) if line_body(&line) == b"/end" => {
                // Preserve all internal line endings; the last ends the frame.
                pasted.truncate(line_body(&pasted).len());
                return Ok(if too_long || pasted.len() > max_bytes {
                    Line::TooLong(None)
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

fn show_result(engine: &Engine, options: &Options, result: &ResultRecord) -> io::Result<()> {
    let summary = match engine.csv_chart_summary(&options.actor, &options.case_id, result) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("グラフの要約を読み込めませんでした。{error}");
            None
        }
    };
    let text = summary.as_deref().unwrap_or(&result.text);
    if !text.is_empty() {
        writeln!(io::stdout().lock(), "{text}")?;
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
    let (kind, argument) = selection
        .split_once(char::is_whitespace)
        .unwrap_or((selection, ""));
    if kind == "inherited" {
        return show_inherited_history(engine, options, argument.trim());
    }
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
            show_result(engine, options, result).map_err(|error| error.to_string())?;
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
            Some(result) => {
                show_result(engine, options, result).map_err(|error| error.to_string())?
            }
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
        let preview = history_preview(message);
        eprintln!("{id} [{:?}] {preview:?}", result.lifecycle);
    }
    if page < pages {
        eprintln!("続きは /history page {}", page + 1);
    }
    Ok(())
}

fn history_preview(text: &str) -> String {
    let mut characters = text.chars();
    let mut preview: String = characters.by_ref().take(48).collect();
    if characters.next().is_some() {
        preview.push('…');
    }
    preview
}

fn show_inherited_history(
    engine: &Engine,
    options: &Options,
    argument: &str,
) -> Result<(), String> {
    let page = if argument.is_empty() {
        Some(1)
    } else {
        argument
            .split_once(char::is_whitespace)
            .filter(|(kind, _)| *kind == "page")
            .and_then(|(_, page)| page.trim().parse::<usize>().ok())
    };
    let Some((page, offset)) = page.and_then(|page| {
        page.checked_sub(1)
            .and_then(|zero_based| zero_based.checked_mul(HISTORY_PAGE_SIZE))
            .map(|offset| (page, offset))
    }) else {
        eprintln!("/history inherited page の後に1以上のページ番号を指定してください。");
        return Ok(());
    };
    let entries = engine
        .branch_history(&options.actor, &options.case_id, offset, HISTORY_PAGE_SIZE)
        .map_err(|error| error.to_string())?;
    if entries.is_empty() {
        eprintln!("そのページに引き継いだ結果はありません。");
        return Ok(());
    }
    eprintln!("分岐時に引き継いだ履歴（{page}ページ）");
    for entry in &entries {
        let input = history_preview(&entry.input_text);
        let output = history_preview(&entry.text);
        eprintln!(
            "会話 {:?} / 結果ID {}\n  入力 {input:?}\n  回答 {output:?}",
            entry.source_case_id, entry.result_id
        );
    }
    if entries.len() == HISTORY_PAGE_SIZE {
        eprintln!("続きは /history inherited page {}", page + 1);
    }
    Ok(())
}

fn show_branch_source(branch: &CaseBranch) {
    eprintln!(
        "分岐元の会話 {:?} / 結果ID {} / 保存版 {}",
        branch.source_case_id, branch.source_result_id, branch.source_revision
    );
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
        match capability.permission {
            Some(CapabilityPermission::ExactHttpsHost { host }) => {
                eprintln!("  必要許可 HTTPSホスト {host}（完全一致）");
            }
            Some(CapabilityPermission::ExactLocalWritePath) => {
                eprintln!("  必要許可 ローカル保存先のパス（完全一致）");
            }
            Some(CapabilityPermission::ExplicitLocalWave) => {
                eprintln!("  必要操作 /listen でローカルWAVを1件指定");
            }
            Some(CapabilityPermission::LocalWasmExecution) => {
                eprintln!("  必要許可 隔離実行（発話ごとに /allow-exec）");
            }
            None => {}
        }
    }
    if let (Some(revision), Some(lifecycle)) = (status.case_revision, status.case_lifecycle) {
        eprintln!(
            "会話 {:?} / 保存版 {revision} / 会話状態 {lifecycle:?}",
            status.case_id
        );
    } else {
        eprintln!("会話 {:?} / 保存版なし", status.case_id);
    }
    if let Some(branch) = engine
        .case_branch(&options.actor, &options.case_id)
        .map_err(|error| error.to_string())?
    {
        show_branch_source(&branch);
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

enum ProfileSelection {
    Off,
    Active(String),
    Unresolved,
}

impl ProfileSelection {
    fn show(&self) {
        match self {
            Self::Off => eprintln!("用途を選択していません。"),
            Self::Active(name) => eprintln!("現在の用途: {name:?}"),
            Self::Unresolved => eprintln!("{UNRESOLVED_PROFILE}"),
        }
    }

    fn request(&self) -> Option<PurposeUse> {
        let Self::Active(name) = self else {
            return None;
        };
        Some(PurposeUse {
            name: name.clone(),
            binding: None,
        })
    }
}

fn profile_argument(input: &str) -> Option<(&str, &str)> {
    let input = input.trim_start();
    let (value, rest) = if let Some(quoted) = input.strip_prefix('"') {
        let (value, rest) = quoted.split_once('"')?;
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return None;
        }
        (value, rest.trim_start())
    } else {
        let end = input.find(char::is_whitespace).unwrap_or(input.len());
        let (value, rest) = input.split_at(end);
        if value.contains('"') {
            return None;
        }
        (value, rest.trim_start())
    };
    (!value.trim().is_empty()).then_some((value, rest))
}

fn parse_profile(arguments: &str, attachments: &[RequestAttachment]) -> Option<PurposeProfile> {
    let (name, rest) = profile_argument(arguments)?;
    let (task, mut flags) = profile_argument(rest)?;
    let task = match task {
        "summarize" => PurposeTask::Summarize,
        "find" => {
            let (query, rest) = profile_argument(flags)?;
            flags = rest;
            PurposeTask::Find {
                query: query.to_owned(),
            }
        }
        _ => return None,
    };
    let mut tools = PurposeTools::default();
    for flag in flags.split_whitespace() {
        match flag {
            "+save" => tools.write_files = true,
            "+wasm" => tools.wasm_execution = true,
            _ => {
                let host = flag.strip_prefix("+web=").filter(|host| !host.is_empty())?;
                tools.read_hosts.insert(host.to_owned());
            }
        }
    }
    Some(PurposeProfile {
        name: name.to_owned(),
        task,
        attachments: attachments.to_vec(),
        tools,
    })
}

fn profile_command(
    engine: &Engine,
    actor: &str,
    argument: &str,
    selected: &mut ProfileSelection,
    attachments: &mut Vec<RequestAttachment>,
) {
    let updated = match profile_argument(argument) {
        None if argument.is_empty() => {
            selected.show();
            return;
        }
        Some(("off", "")) => {
            eprintln!("用途の選択を解除しました。");
            Ok(ProfileSelection::Off)
        }
        Some(("use", arguments)) => match profile_argument(arguments) {
            Some((name, "")) => engine
                .purpose_profile(actor, name)
                .map_err(|error| error.to_string())
                .and_then(|entry| {
                    if entry.is_some() {
                        Ok(ProfileSelection::Active(name.to_owned()))
                    } else {
                        Err(format!(
                            "この利用者には用途 {name:?} が保存されていません。"
                        ))
                    }
                }),
            _ => Err(PROFILE_USAGE.into()),
        },
        Some(("save", arguments)) => match parse_profile(arguments, attachments) {
            Some(profile) => match engine.save_purpose_profile(actor, &profile) {
                Ok(reference) => {
                    attachments.clear();
                    eprintln!("用途 {:?} を保存しました。版 {reference}", profile.name);
                    return;
                }
                Err(error) => Err(error.to_string()),
            },
            None => Err(PROFILE_USAGE.into()),
        },
        _ => Err(PROFILE_USAGE.into()),
    };
    match updated {
        Ok(profile) => {
            *selected = profile;
            if matches!(selected, ProfileSelection::Active(_)) {
                selected.show();
            }
        }
        Err(error) => {
            *selected = ProfileSelection::Unresolved;
            eprintln!("{error}\n{UNRESOLVED_PROFILE}");
        }
    }
}

fn is_cancellation(options: &Options, text: &str) -> bool {
    // Cancellation needs neither a selected result nor a new output action.
    // Use Core's interpretation without registering an input or changing a case.
    let mut probe = RequestEnvelope::new(&options.case_id, "cancel-check", &options.actor, text);
    probe.budget = options.budget.clone();
    input::seal(&probe, None).is_ok_and(|bundle| matches!(bundle.meaning.intent, Intent::Cancel))
}

fn check_case_selection(engine: &Engine, actor: &str, id: &str, new: bool) -> Result<(), String> {
    if id.is_empty() {
        return Err("会話IDを指定してください。".into());
    }
    if id.len() > MAX_CASE_ID_BYTES {
        return Err(format!(
            "会話IDは{MAX_CASE_ID_BYTES}バイト以内で指定してください。"
        ));
    }
    let registered = if new {
        engine.case_registered(actor, id)
    } else {
        engine.latest(actor, id).map(|case| case.is_some())
    };
    match registered {
        Ok(true) if new => {
            Err("使用済みの会話IDです。再開するには /case を使ってください。".into())
        }
        Ok(_) => Ok(()),
        Err(error) => Err(format!("会話を選択できませんでした。{error}")),
    }
}

fn fork_case(engine: &Engine, options: &Options, argument: &str) -> Result<CaseBranch, String> {
    const USAGE: &str =
        "/fork の後に未使用の会話IDと、必要なら /history の結果IDを指定してください。";
    let mut arguments = argument.split_whitespace();
    let id = arguments.next().ok_or(USAGE)?;
    let result_id = arguments.next();
    if arguments.next().is_some()
        || result_id.is_some_and(|id| {
            id.len() != 64
                || !id
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        })
    {
        return Err(USAGE.into());
    }
    if id.len() > MAX_CASE_ID_BYTES {
        return Err(format!(
            "会話IDは{MAX_CASE_ID_BYTES}バイト以内で指定してください。"
        ));
    }
    engine
        .fork_case(&options.actor, &options.case_id, id, result_id)
        .map_err(|error| format!("会話を分岐できませんでした。{error}"))
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
    let mut case_unresolved = false;
    let mut context_unresolved = false;
    let mut output_unresolved = false;
    let mut profile = ProfileSelection::Off;
    let mut next_execution_allowed = false;
    let mut attachments = Vec::new();
    let mut staged_recording = None;
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
        // Even an invalid input or a command consumes a staged execution
        // grant. A single /paste block is still one input.
        let execution_allowed = std::mem::take(&mut next_execution_allowed);
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
        let decoded = match line {
            Line::End => break,
            Line::TooLong(command) => Err((command, "入力が上限を超えたため送信しませんでした。")),
            Line::Input(mut bytes) => {
                bytes.truncate(line_body(&bytes).len());
                String::from_utf8(bytes).map_err(|error| {
                    (
                        leading_setting_command(error.as_bytes()),
                        "UTF-8ではない入力を送信しませんでした。",
                    )
                })
            }
        };
        let mut text = match decoded {
            Ok(text) => text,
            Err((command, message)) => {
                eprintln!("{message}");
                attachments.clear();
                match command {
                    Some(SettingCommand::Case) => {
                        if staged_recording.take().is_some() {
                            eprintln!("保留中の録音を破棄しました。");
                        }
                        case_unresolved = true;
                        eprintln!("{UNRESOLVED_CASE}");
                    }
                    Some(SettingCommand::Context | SettingCommand::Output) if case_unresolved => {
                        eprintln!("{UNRESOLVED_CASE}");
                    }
                    Some(SettingCommand::Context) => {
                        options.context_ref = None;
                        context_unresolved = true;
                    }
                    Some(SettingCommand::Output) => {
                        options.output = None;
                        output_unresolved = true;
                    }
                    Some(SettingCommand::Network) => {
                        disable_network_after_bad_setting(&mut network_enabled);
                    }
                    Some(SettingCommand::Attachment) => {
                        eprintln!("次の発話の添付を解除しました。");
                    }
                    Some(SettingCommand::Profile) => {
                        profile = ProfileSelection::Unresolved;
                        eprintln!("{UNRESOLVED_PROFILE}");
                    }
                    None => {}
                }
                continue;
            }
        };
        let mut speech_path = None;
        let mut send_recording = false;
        if text.starts_with('/') {
            let (command, argument) = text
                .split_once(char::is_whitespace)
                .map_or((text.as_str(), ""), |(command, argument)| {
                    (command, argument.trim())
                });
            if matches!(command, "/case" | "/new" | "/fork") && staged_recording.take().is_some() {
                eprintln!("保留中の録音を破棄しました。");
            }
            if case_unresolved
                && matches!(
                    command,
                    "/history"
                        | "/context"
                        | "/fork"
                        | "/output"
                        | "/attach"
                        | "/listen"
                        | "/allow-exec"
                        | "/reconcile"
                        | "/reconcile-effects"
                )
            {
                eprintln!("{UNRESOLVED_CASE}");
                continue;
            }
            match (command, argument) {
                ("/exit", "") => break,
                ("/help", "") => {
                    eprintln!("{HELP}");
                    continue;
                }
                ("/language", locale) => {
                    match locale {
                        "" => eprintln!("応答言語: {}", options.locale),
                        "ja" | "en" => {
                            options.locale = locale.into();
                            eprintln!("応答言語: {}", options.locale);
                        }
                        _ => eprintln!(
                            "応答言語を指定するには /language ja または /language en を入力してください。"
                        ),
                    }
                    continue;
                }
                ("/case" | "/new", id) => {
                    attachments.clear();
                    match check_case_selection(engine, &options.actor, id, command == "/new") {
                        Ok(()) => {
                            // Reselecting the former case never revives staged
                            // targets, but it does not fix earlier setting errors.
                            if case_unresolved || options.case_id != id {
                                options.context_ref = None;
                            }
                            if options.case_id != id {
                                context_unresolved = false;
                                output_unresolved = false;
                            }
                            options.case_id = id.to_owned();
                            options.output = None;
                            case_unresolved = false;
                            eprintln!("会話: {}", options.case_id);
                        }
                        Err(error) => {
                            case_unresolved = true;
                            eprintln!("{error}\n{UNRESOLVED_CASE}");
                        }
                    }
                    continue;
                }
                ("/fork", argument) => {
                    attachments.clear();
                    match fork_case(engine, &options, argument) {
                        Ok(branch) => {
                            eprintln!("会話 {:?} を作成しました。", branch.case_id);
                            show_branch_source(&branch);
                            options.case_id = branch.case_id;
                            options.context_ref = None;
                            options.output = None;
                            context_unresolved = false;
                            output_unresolved = false;
                            network_enabled = false;
                            profile = ProfileSelection::Off;
                        }
                        Err(error) => {
                            case_unresolved = true;
                            eprintln!("{error}\n{UNRESOLVED_CASE}");
                        }
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
                    if case_unresolved {
                        eprintln!(
                            "最後に選択した会話: {:?}\n{UNRESOLVED_CASE}",
                            options.case_id
                        );
                        continue;
                    }
                    if let Err(error) =
                        show_status(engine, &options, context_unresolved, output_unresolved)
                    {
                        eprintln!("状態を読み込めませんでした。{error}");
                    }
                    profile.show();
                    continue;
                }
                ("/reconcile", "") => {
                    match engine.reconcile(&options.actor, &options.case_id) {
                        Ok(result) => {
                            eprintln!("照合後の結果ID: {} ({:?})", result.id, result.lifecycle);
                            show_result(engine, &options, &result)
                                .map_err(|error| error.to_string())?;
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
                ("/net", _) => {
                    disable_network_after_bad_setting(&mut network_enabled);
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
                    let path = PathBuf::from(unquote_path(path));
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
                ("/attach", "off") => {
                    attachments.clear();
                    eprintln!("次の発話の添付を解除しました。");
                    continue;
                }
                ("/attach", "") => {
                    attachments.clear();
                    eprintln!("添付を指定するには /attach PATH と入力してください。");
                    continue;
                }
                ("/attach", path) => {
                    let path = unquote_path(path);
                    match attachment::stage(&mut attachments, std::path::Path::new(path), max_bytes)
                    {
                        Ok(()) => {
                            let file = attachments.last().expect("staged attachment");
                            eprintln!("次の発話の添付: {}", file.name);
                        }
                        Err(error) => {
                            attachments.clear();
                            eprintln!("添付をすべて解除しました。{error}");
                        }
                    }
                    continue;
                }
                ("/profile", argument) => {
                    profile_command(
                        engine,
                        &options.actor,
                        argument,
                        &mut profile,
                        &mut attachments,
                    );
                    continue;
                }
                ("/listen", path) => {
                    if staged_recording.is_some() {
                        eprintln!("{RECORDING_PENDING}");
                        continue;
                    }
                    let path = path
                        .strip_prefix('"')
                        .and_then(|rest| rest.strip_suffix('"'))
                        .unwrap_or(path);
                    if path.is_empty() || path.len() > MAX_PATH_ARGUMENT_BYTES {
                        eprintln!("音声を送信するには /listen WAVの絶対パス と入力してください。");
                        continue;
                    }
                    speech_path = Some(PathBuf::from(path));
                }
                ("/record", "off") => {
                    if staged_recording.take().is_some() {
                        eprintln!("保留中の録音を破棄しました。");
                    } else {
                        eprintln!("保留中の録音はありません。");
                    }
                    continue;
                }
                ("/record", argument) => {
                    let send = argument == "send";
                    if send && staged_recording.is_none() {
                        eprintln!("保留中の録音はありません。/record で録音してください。");
                        continue;
                    }
                    if !send && staged_recording.is_some() {
                        eprintln!("{RECORDING_PENDING}");
                        continue;
                    }
                    if !microphone::AVAILABLE || !interactive {
                        eprintln!(
                            "マイク録音にはWindowsの対話端末が必要です。WAVは /listen PATH で送信できます。"
                        );
                        continue;
                    }
                    if options.asr_assets.is_none() {
                        eprintln!(
                            "マイク録音を使うには --asr-assets で音声認識資産を指定して起動してください。"
                        );
                        continue;
                    }
                    let unresolved = if case_unresolved {
                        Some(UNRESOLVED_CASE)
                    } else if context_unresolved {
                        Some(UNRESOLVED_CONTEXT)
                    } else if output_unresolved {
                        Some(UNRESOLVED_OUTPUT)
                    } else if matches!(profile, ProfileSelection::Unresolved) {
                        Some(UNRESOLVED_PROFILE)
                    } else {
                        None
                    };
                    if let Some(reason) = unresolved {
                        eprintln!("{reason}");
                        continue;
                    }
                    if !attachments.is_empty() || !matches!(profile, ProfileSelection::Off) {
                        eprintln!(
                            "録音は単独で扱います。資料は /attach off、用途は /profile off で選択を解除してください。"
                        );
                        continue;
                    }
                    if send {
                        send_recording = true;
                    } else {
                        let maximum = microphone::max_seconds(max_bytes);
                        if maximum == 0 {
                            eprintln!("現在の入力上限では1秒の録音を保持できません。");
                            continue;
                        }
                        let seconds = if argument.is_empty() {
                            Some(maximum.min(5))
                        } else {
                            argument
                                .parse::<u32>()
                                .ok()
                                .filter(|seconds| (1..=maximum).contains(seconds))
                        };
                        let Some(seconds) = seconds else {
                            eprintln!("/record の秒数には1〜{maximum}の整数を指定してください。");
                            continue;
                        };
                        eprintln!(
                            "既定のマイクで最大{seconds}秒録音します。録音は送信せず、この端末で保留します。"
                        );
                        io::stderr().flush().map_err(|error| error.to_string())?;
                        match microphone::record(seconds) {
                            Ok(wave) => {
                                staged_recording = Some(wave);
                                eprintln!("{RECORDING_PENDING}");
                            }
                            Err(error) => eprintln!("録音できませんでした。{error}"),
                        }
                        continue;
                    }
                }
                ("/allow-exec", "") => {
                    if options.wasm_node.is_none() {
                        eprintln!(
                            "隔離実行を使うには起動時に --wasm-node ABSOLUTE_PATH を指定してください。"
                        );
                    } else if context_unresolved || output_unresolved {
                        eprintln!(
                            "{}",
                            if context_unresolved {
                                UNRESOLVED_CONTEXT
                            } else {
                                UNRESOLVED_OUTPUT
                            }
                        );
                    } else {
                        next_execution_allowed = true;
                        eprintln!("次の発話1件のWebAssembly隔離実行を許可しました。");
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
                        Line::TooLong(_) => {
                            attachments.clear();
                            eprintln!("入力が上限を超えたため送信しませんでした。");
                            continue;
                        }
                        Line::Input(pasted) => {
                            let Ok(request) = String::from_utf8(pasted) else {
                                attachments.clear();
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
        let audio_turn = send_recording || speech_path.is_some();
        if !audio_turn && text.trim().is_empty() {
            continue;
        }
        let cancelled = !audio_turn
            && (execution_allowed
                || context_unresolved
                || output_unresolved
                || !matches!(profile, ProfileSelection::Off)
                || !attachments.is_empty()
                || staged_recording.is_some())
            && is_cancellation(&options, &text);
        if !send_recording && staged_recording.is_some() {
            if !cancelled {
                eprintln!("{RECORDING_PENDING}");
                continue;
            }
            staged_recording = None;
            eprintln!("保留中の録音を破棄しました。");
        }
        let mut turn_attachments = std::mem::take(&mut attachments);
        if speech_path.is_some() && !turn_attachments.is_empty() {
            turn_attachments.clear();
            eprintln!("音声発話を選択したため、次の発話用の添付指定を解除しました。");
        }
        let profile_unresolved = matches!(profile, ProfileSelection::Unresolved);
        if cancelled {
            turn_attachments.clear();
        }
        if !audio_turn
            && text
                .len()
                .saturating_add(attachment::input_bytes(&turn_attachments))
                > max_bytes
        {
            eprintln!("入力が上限を超えたため送信しませんでした。");
            continue;
        }
        if case_unresolved {
            eprintln!("入力を送信していません。{UNRESOLVED_CASE}");
            continue;
        }
        if context_unresolved || output_unresolved || profile_unresolved {
            if !cancelled {
                let reason = if context_unresolved {
                    UNRESOLVED_CONTEXT
                } else if output_unresolved {
                    UNRESOLVED_OUTPUT
                } else {
                    UNRESOLVED_PROFILE
                };
                eprintln!("入力を送信していません。{reason}");
                continue;
            }
            // Keep the unresolved state. The cancellation itself must not
            // carry the previous turn's one-shot context or output selection.
            options.context_ref = None;
            options.output = None;
        }
        let read_hosts = if network_enabled && !send_recording {
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
        let allow_wasm_execution = execution_allowed && !cancelled && !send_recording;
        let recording = send_recording.then(|| {
            staged_recording
                .take()
                .expect("recording checked before admitting the turn")
        });
        let input = match recording.as_deref() {
            Some(wave) => RequestInput::SpeechBytes(wave),
            None => speech_path
                .as_deref()
                .map_or(RequestInput::Text(&text), RequestInput::Speech),
        };
        let purpose = if cancelled || send_recording {
            None
        } else {
            profile.request()
        };
        let result = execute_request(
            engine,
            &options,
            input,
            read_hosts,
            allow_wasm_execution,
            turn_attachments,
            purpose,
        );
        options.context_ref = None;
        options.output = None;
        match result {
            Ok(result) => {
                show_result(engine, &options, &result).map_err(|error| error.to_string())?
            }
            Err(error) => eprintln!("入力を処理できませんでした。{error}"),
        }
    }
    Ok(0)
}
