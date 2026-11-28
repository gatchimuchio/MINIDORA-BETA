//! Run the pinned Japanese Julius GMM-HMM recognizer on one bounded WAV.
use minidora_domain::{Result, failure};
use std::{path::Path, time::Instant};

pub(crate) const MODEL_ID: &str =
    "julius.dictation-kit.gmm-hmm.1ceb4dec245ef482918ca33c55c71d383dce145e";

pub(crate) struct Recognition {
    pub text: String,
}

pub fn configure_japanese_recognition(root: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows::configure(root)
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Err(unavailable())
    }
}

pub(crate) fn recognize(wave_16k_pcm16: &[u8], deadline: Instant) -> Result<Recognition> {
    #[cfg(windows)]
    {
        windows::recognize(wave_16k_pcm16, deadline)
    }
    #[cfg(not(windows))]
    {
        let _ = (wave_16k_pcm16, deadline);
        Err(unavailable())
    }
}

#[cfg(not(windows))]
fn unavailable() -> minidora_domain::Error {
    failure(
        "SPEECH_INPUT_UNAVAILABLE",
        "日本語のローカル音声認識は現在Windowsで利用できます。",
    )
}

#[cfg(windows)]
mod windows {
    use super::{Recognition, Result, failure};
    use crate::durability::{
        is_link_or_reparse_point, keeps_local_target_name, local_target_and_ancestors,
        validate_local_target_path,
    };
    use minidora_domain::digest;
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeSet,
        fs::{File, OpenOptions},
        io::{self, Read, Write},
        os::windows::{fs::OpenOptionsExt, process::CommandExt},
        path::{Path, PathBuf},
        process::{Child, Command, ExitStatus, Stdio},
        sync::{
            Arc, OnceLock,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };

    const PROGRAM: &str = "bin/windows/julius.exe";
    // Hashes of the original Windows executable, configurations and GMM/N-gram
    // assets at the dictation-kit commit identified by MODEL_ID.
    const ASSETS: [(&str, u64, &str); 7] = [
        (
            PROGRAM,
            1_421_066,
            "797f6fd618bc72032952be130577ec480fce34bc946be58ca1286794fd257438",
        ),
        (
            "main.jconf",
            15_881,
            "0a62282d4da65fbe6862c6f30345ba8e40931ff9f76ab275416f8b0fb9b10335",
        ),
        (
            "am-gmm.jconf",
            316,
            "ef2eea000ef55ca0105318c20ebc56ee1cb9fc259a6906e1b9b665394dbac2ab",
        ),
        (
            "model/phone_m/jnas-tri-3k16-gid.binhmm",
            11_287_607,
            "5f427fa11189a4d49de9a9ef51e8b1159971ee1cf5e3656f42081cf69dbf0c98",
        ),
        (
            "model/phone_m/logicalTri-3k16-gid.bin",
            1_213_799,
            "bb4673040fd2691b53dea5bc411dead26732336a5c997fb0f86f93818bbfac66",
        ),
        (
            "model/lang_m/bccwj.60k.bingram",
            95_946_803,
            "76e0c8a675d63531f8cb1c3c22214d5c2e36c295c8f175624475e4efe0281228",
        ),
        (
            "model/lang_m/bccwj.60k.htkdic",
            2_691_767,
            "4ffa0555e815d70419bba551ab44c44d2c5153a0d83fe147b1336401a6175c55",
        ),
    ];
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const MAX_SECONDS: usize = 10;
    const SAMPLE_RATE: u32 = 16_000;
    const MAX_STDOUT: usize = 256 * 1_024;
    const MAX_STDERR: usize = 64 * 1_024;
    const MAX_TEXT: usize = 2_048;
    const POLL: Duration = Duration::from_millis(5);

    struct Runtime {
        root: PathBuf,
        // Keep verified files and directory names stable for the process lifetime.
        _assets: Vec<File>,
        _directories: Vec<File>,
    }

    static RUNTIME: OnceLock<Runtime> = OnceLock::new();

    fn invalid_asset(name: &str) -> minidora_domain::Error {
        failure(
            "SPEECH_INPUT_ASSET_INVALID",
            format!("日本語音声認識の固定資産「{name}」を照合できません。"),
        )
    }

    fn worker_failed() -> minidora_domain::Error {
        failure(
            "SPEECH_INPUT_RECOGNITION_FAILED",
            "日本語の音声認識を正常に完了できませんでした。",
        )
    }

    fn timeout() -> minidora_domain::Error {
        failure(
            "SPEECH_INPUT_DEADLINE",
            "日本語の音声認識が依頼の期限を超えました。",
        )
    }

    fn output_limit() -> minidora_domain::Error {
        failure(
            "SPEECH_INPUT_RECOGNITION_LIMIT",
            "音声認識の出力が上限を超えました。",
        )
    }

    fn same_root(existing: &Runtime, root: &Path) -> Result<()> {
        if existing.root != root {
            return Err(failure(
                "SPEECH_INPUT_CONFIGURATION",
                "このプロセスでは既に別の音声認識資産を設定しています。",
            ));
        }
        Ok(())
    }

    fn hold_directory(path: &Path) -> io::Result<File> {
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        let info = file.metadata()?;
        if !info.is_dir() || is_link_or_reparse_point(&info) {
            return Err(io::Error::other(
                "asset directory is not an ordinary directory",
            ));
        }
        Ok(file)
    }

    fn hold_asset(path: &Path, expected_bytes: u64, checksum: &str) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let info = file.metadata()?;
        if !info.is_file() || is_link_or_reparse_point(&info) || info.len() != expected_bytes {
            return Err(io::Error::other("asset metadata differs"));
        }
        let mut input = (&file).take(expected_bytes + 1);
        let mut hash = Sha256::new();
        let mut observed = 0;
        let mut buffer = [0; 64 * 1_024];
        loop {
            let count = input.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            observed += count as u64;
            hash.update(&buffer[..count]);
        }
        if observed != expected_bytes || format!("{:x}", hash.finalize()) != checksum {
            return Err(io::Error::other("asset content differs"));
        }
        Ok(file)
    }

    pub(super) fn configure(root: &Path) -> Result<()> {
        let exe = root.join(PROGRAM);
        validate_local_target_path(&exe).map_err(|_| invalid_asset(PROGRAM))?;
        if let Some(runtime) = RUNTIME.get() {
            return same_root(runtime, root);
        }
        let mut seen = BTreeSet::new();
        let mut directories = Vec::new();
        let mut assets = Vec::new();
        for (name, size, checksum) in ASSETS {
            let path = root.join(name);
            for parent in local_target_and_ancestors(&path).filter(|entry| *entry != path) {
                if seen.insert(parent.to_owned()) {
                    directories.push(hold_directory(parent).map_err(|_| invalid_asset(name))?);
                }
            }
            let file = hold_asset(&path, size, checksum).map_err(|_| invalid_asset(name))?;
            if !keeps_local_target_name(&path).map_err(|_| invalid_asset(name))? {
                return Err(invalid_asset(name));
            }
            assets.push(file);
        }
        let runtime = Runtime {
            root: root.to_owned(),
            _assets: assets,
            _directories: directories,
        };
        match RUNTIME.set(runtime) {
            Ok(()) => Ok(()),
            Err(unused) => same_root(RUNTIME.get().ok_or_else(worker_failed)?, &unused.root),
        }
    }

    fn check_wave(wave: &[u8]) -> Result<()> {
        let invalid = || {
            failure(
                "SPEECH_INPUT_FORMAT",
                "16 kHz・16 bit・モノラルで10秒以内のPCM WAVを指定してください。",
            )
        };
        let facts = crate::output::inspect_wave(wave).map_err(|_| invalid())?;
        if facts.sample_rate != SAMPLE_RATE
            || facts.frame_count > SAMPLE_RATE as usize * MAX_SECONDS
            || wave.len().checked_sub(facts.data_bytes) != Some(44)
            || wave.get(36..40) != Some(b"data")
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn capture(mut pipe: impl Read, limit: usize, exceeded: &AtomicBool) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        let mut buffer = [0; 8 * 1_024];
        loop {
            let count = pipe.read(&mut buffer)?;
            if count == 0 {
                return Ok(bytes);
            }
            if count > limit - bytes.len() {
                exceeded.store(true, Ordering::Relaxed);
                return Err(io::Error::other("recognizer output exceeded its limit"));
            }
            bytes.extend_from_slice(&buffer[..count]);
        }
    }

    fn wait_for_exit(
        child: &mut Child,
        deadline: Instant,
        exceeded: &AtomicBool,
    ) -> Result<ExitStatus> {
        loop {
            if exceeded.load(Ordering::Relaxed) {
                return Err(output_limit());
            }
            if Instant::now() >= deadline {
                return Err(timeout());
            }
            if let Some(exit) = child.try_wait().map_err(|_| worker_failed())? {
                return Ok(exit);
            }
            thread::sleep(POLL);
        }
    }

    fn terminate(child: &mut Child) -> Result<()> {
        let _ = child.kill();
        let reap_deadline = Instant::now() + Duration::from_millis(750);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if Instant::now() < reap_deadline => thread::sleep(POLL),
                _ => {
                    return Err(failure(
                        "SPEECH_INPUT_TERMINATION_UNCONFIRMED",
                        "音声認識プロセスの終了を確認できませんでした。",
                    ));
                }
            }
        }
    }

    fn completed_text(stdout: &[u8]) -> Result<Recognition> {
        let output = std::str::from_utf8(stdout).map_err(|_| worker_failed())?;
        let mut text = String::new();
        let mut seen_sentence = false;
        for line in output.lines() {
            let Some(sentence) = line.trim_start().strip_prefix("sentence1:") else {
                continue;
            };
            if std::mem::replace(&mut seen_sentence, true) {
                return Err(failure(
                    "SPEECH_INPUT_MULTIPLE_UTTERANCES",
                    "1回に1つの発話を含むWAVを指定してください。",
                ));
            }
            for token in sentence.split_whitespace() {
                if matches!(token, "<s>" | "</s>" | "<sp>") {
                    continue;
                }
                if token.chars().any(char::is_control) {
                    return Err(worker_failed());
                }
                if token.len() > MAX_TEXT - text.len() {
                    return Err(output_limit());
                }
                text.push_str(token);
            }
        }
        if text.is_empty() {
            return Err(failure(
                "SPEECH_INPUT_NO_RESULT",
                "音声から確定した発話を取得できませんでした。",
            ));
        }
        Ok(Recognition { text })
    }

    pub(super) fn recognize(wave: &[u8], deadline: Instant) -> Result<Recognition> {
        let runtime = RUNTIME.get().ok_or_else(|| {
            failure(
                "SPEECH_INPUT_UNCONFIGURED",
                "日本語の固定音声認識資産を起動時に指定してください。",
            )
        })?;
        let deadline = deadline.min(Instant::now() + Duration::from_secs(30));
        check_wave(wave)?;
        if Instant::now() >= deadline {
            return Err(timeout());
        }

        // The pinned MinGW binary reads piped waveform bytes in text mode.
        // Its file input uses binary mode. Only the temporary filename goes
        // over stdin, and the original attachment is never opened by Julius.
        let scratch = tempfile::tempdir().map_err(|_| temp_failed())?;
        let result = recognize_temporary(runtime, scratch.path(), wave, deadline);
        scratch.close().map_err(|_| {
            failure(
                "SPEECH_INPUT_TEMP_CLEANUP",
                "音声認識の一時ファイルを閉じられませんでした。",
            )
        })?;
        result
    }

    fn temp_failed() -> minidora_domain::Error {
        failure(
            "SPEECH_INPUT_TEMP_INVALID",
            "音声認識用の一時ファイルを用意できませんでした。",
        )
    }

    fn recognize_temporary(
        runtime: &Runtime,
        directory: &Path,
        wave: &[u8],
        deadline: Instant,
    ) -> Result<Recognition> {
        let path = directory.join("utterance.wav");
        validate_local_target_path(&path).map_err(|_| temp_failed())?;
        let filename = path
            .to_str()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| {
                failure(
                    "SPEECH_INPUT_TEMP_PATH",
                    "音声認識には一時フォルダーのASCII絶対パスが必要です。",
                )
            })?;
        let mut copy = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| temp_failed())?;
        copy.write_all(wave).map_err(|_| temp_failed())?;
        copy.flush().map_err(|_| temp_failed())?;
        drop(copy);
        let _wave =
            hold_asset(&path, wave.len() as u64, &digest(wave)).map_err(|_| temp_failed())?;
        run(runtime, format!("{filename}\r\n").into_bytes(), deadline)
    }

    fn run(runtime: &Runtime, input: Vec<u8>, deadline: Instant) -> Result<Recognition> {
        let system_root = std::env::var_os("SystemRoot")
            .filter(|path| {
                let path = Path::new(path);
                path.is_absolute() && path.join("System32").is_dir()
            })
            .ok_or_else(worker_failed)?;
        if Instant::now() >= deadline {
            return Err(timeout());
        }

        let mut child = Command::new(runtime.root.join(PROGRAM))
            .current_dir(&runtime.root)
            .env_clear()
            .env("SystemRoot", system_root)
            .args([
                "-C",
                "main.jconf",
                "-C",
                "am-gmm.jconf",
                "-input",
                "file",
                "-fvad",
                "-1",
                "-rejectshort",
                "250",
                "-quiet",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|_| worker_failed())?;

        let exceeded = Arc::new(AtomicBool::new(false));
        let started = (|| -> io::Result<_> {
            let stdout = child.stdout.take().expect("configured stdout pipe");
            let stdout_exceeded = Arc::clone(&exceeded);
            let stdout = thread::Builder::new()
                .spawn(move || capture(stdout, MAX_STDOUT, &stdout_exceeded))?;
            let stderr = child.stderr.take().expect("configured stderr pipe");
            let stderr_exceeded = Arc::clone(&exceeded);
            let stderr = thread::Builder::new()
                .spawn(move || capture(stderr, MAX_STDERR, &stderr_exceeded))?;
            let mut stdin = child.stdin.take().expect("configured stdin pipe");

            let writer = thread::Builder::new().spawn(move || stdin.write_all(&input))?;
            Ok((stdout, stderr, writer))
        })();
        let (stdout, stderr, writer) = match started {
            Ok(pipes) => pipes,
            Err(_) => {
                terminate(&mut child)?;
                return Err(worker_failed());
            }
        };
        let exit = match wait_for_exit(&mut child, deadline, &exceeded) {
            Ok(exit) => exit,
            Err(error) => {
                terminate(&mut child)?;
                return Err(error);
            }
        };
        while !stdout.is_finished() || !stderr.is_finished() || !writer.is_finished() {
            if Instant::now() >= deadline {
                return Err(timeout());
            }
            thread::sleep(POLL);
        }
        if exceeded.load(Ordering::Relaxed) {
            return Err(output_limit());
        }
        if !exit.success() {
            return Err(worker_failed());
        }
        let output = stdout
            .join()
            .map_err(|_| worker_failed())?
            .map_err(|_| worker_failed())?;
        stderr
            .join()
            .map_err(|_| worker_failed())?
            .map_err(|_| worker_failed())?;
        writer
            .join()
            .map_err(|_| worker_failed())?
            .map_err(|_| worker_failed())?;
        if Instant::now() >= deadline {
            return Err(timeout());
        }
        completed_text(&output)
    }
}
