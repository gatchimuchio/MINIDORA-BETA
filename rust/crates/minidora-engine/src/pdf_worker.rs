//! Fixed PDF decoder. Normal Windows requests give the child only a PDF byte
//! snapshot on stdin, after Windows has applied its memory limit.
use minidora_domain::{Result, failure};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[cfg(windows)]
pub(crate) const MAX_PAGES: usize = 64;
#[cfg(windows)]
pub(crate) const MAX_TEXT_BYTES: usize = 256_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Page {
    pub number: u32,
    pub text: String,
    pub state: PageState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum PageState {
    Text,
    ImageWithoutText,
    NoExtractableText,
}

#[cfg(not(windows))]
pub(crate) fn read(_bytes: &[u8], _deadline: Instant) -> Result<Vec<Page>> {
    Err(unavailable())
}

#[cfg(not(windows))]
pub(crate) fn rasterize(_bytes: &[u8], _number: u32, _deadline: Instant) -> Result<Vec<u8>> {
    Err(raster_unavailable())
}

fn unavailable() -> minidora_domain::Error {
    failure(
        "PDF_READER_UNAVAILABLE",
        "PDFの文字を読むには、Windows版のミニドラを起動してください。",
    )
}

fn raster_unavailable() -> minidora_domain::Error {
    failure(
        "PDF_RASTER_UNAVAILABLE",
        "PDFのページ画像を作るには、対応するWindows版のミニドラを起動してください。",
    )
}

#[cfg(windows)]
mod windows {
    use super::*;
    use minidora_domain::{Error, MAX_PDF_ATTACHMENT_BYTES, digest};
    use serde::{Deserialize, Serialize};
    use std::io::{Read, Write};
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;
    use std::time::Duration;

    const WORKER_ARG: &str = "--minidora-internal-pdf-reader-v1";
    const FRAME: &[u8] = b"MINIDORA-PDF-v1\0";
    const RASTER_FRAME: &[u8; FRAME.len()] = b"MINIDORA-PNG-v1\0";
    const MEMORY_BYTES: u64 = 128 * 1024 * 1024;
    const REPLY_BYTES: u64 = 1_600_000;
    const WORKER_TIME: Duration = Duration::from_secs(8);
    const RASTER_MEMORY_BYTES: u64 = 256 * 1024 * 1024;
    const MAX_RASTER_BYTES: usize = crate::pdf_raster::MAX_PNG_BYTES;
    const MAX_RASTER_HEADER: usize = 4_096;
    const RASTER_REPLY_BYTES: u64 = (4 + MAX_RASTER_HEADER + MAX_RASTER_BYTES) as u64;
    const RASTER_WORKER_TIME: Duration = Duration::from_secs(10);
    const RASTER_MAX_SIDE: u32 = 2_048;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    static EXECUTABLE: OnceLock<PathBuf> = OnceLock::new();

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct Reply {
        input_sha256: String,
        result: std::result::Result<Vec<Page>, (String, String)>,
    }

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct RasterReply {
        input_sha256: String,
        page_number: u32,
        result: std::result::Result<RasterMeta, (String, String)>,
    }

    #[derive(Deserialize, Serialize)]
    #[serde(deny_unknown_fields)]
    struct RasterMeta {
        bytes: usize,
        sha256: String,
    }

    #[derive(Clone, Copy)]
    enum Mode {
        Text,
        Raster(u32),
    }

    impl Mode {
        fn limits(self) -> (u64, u64, Duration) {
            match self {
                Self::Text => (MEMORY_BYTES, REPLY_BYTES, WORKER_TIME),
                Self::Raster(_) => (RASTER_MEMORY_BYTES, RASTER_REPLY_BYTES, RASTER_WORKER_TIME),
            }
        }

        fn unavailable(self) -> Error {
            match self {
                Self::Text => unavailable(),
                Self::Raster(_) => raster_unavailable(),
            }
        }

        fn timeout(self) -> Error {
            match self {
                Self::Text => failure("PDF_DEADLINE", "PDFの読取りが制限時間を超えました。"),
                Self::Raster(_) => failure(
                    "PDF_RASTER_DEADLINE",
                    "PDFのページ画像の作成が制限時間を超えました。",
                ),
            }
        }

        fn invalid_worker(self) -> Error {
            match self {
                Self::Text => failure(
                    "PDF_READER_INTERRUPTED",
                    "PDFの読取り処理を完了できませんでした。",
                ),
                Self::Raster(_) => failure(
                    "PDF_RASTER_INTERRUPTED",
                    "PDFから作成したページ画像を確認できませんでした。",
                ),
            }
        }
    }

    /// Local frontend startup binds its own trusted executable. No request field
    /// selects an executable, argument, environment, or filesystem path.
    pub fn configure_pdf_worker(executable: PathBuf) -> Result<()> {
        if std::env::current_exe()? != executable {
            return Err(unavailable());
        }
        match EXECUTABLE.set(executable) {
            Ok(()) => Ok(()),
            Err(executable) if EXECUTABLE.get() == Some(&executable) => Ok(()),
            Err(_) => Err(unavailable()),
        }
    }

    pub fn run_pdf_worker_stdio() -> Result<()> {
        let mut input = std::io::stdin().lock();
        let mut frame = [0; FRAME.len()];
        input.read_exact(&mut frame)?;
        let mode = if frame == FRAME {
            Mode::Text
        } else if frame == *RASTER_FRAME {
            let mut number = [0; 4];
            input.read_exact(&mut number)?;
            Mode::Raster(u32::from_le_bytes(number))
        } else {
            return Err(Mode::Text.invalid_worker());
        };
        let mut bytes = Vec::new();
        input
            .take(MAX_PDF_ATTACHMENT_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_PDF_ATTACHMENT_BYTES {
            return Err(mode.invalid_worker());
        }
        if let Mode::Raster(number) = mode {
            return emit_raster_reply(&bytes, number);
        }
        let result =
            crate::pdf_text::read(&bytes, Instant::now() + WORKER_TIME).map_err(
                |error| match error {
                    Error::Contract { code, message } => (code, message),
                    _ => ("PDF_INVALID".into(), "PDFを解析できません。".into()),
                },
            );
        let reply = Reply {
            input_sha256: digest(&bytes),
            result,
        };
        let bytes = serde_json::to_vec(&reply)?;
        if bytes.len() as u64 > REPLY_BYTES {
            return Err(mode.invalid_worker());
        }
        std::io::stdout().lock().write_all(&bytes)?;
        Ok(())
    }

    fn emit_raster_reply(bytes: &[u8], number: u32) -> Result<()> {
        let mut png = Vec::new();
        let result = match validate_page_number(number)
            .and_then(|()| crate::pdf_raster::rasterize(bytes, number))
        {
            Ok(data) if data.len() > MAX_RASTER_BYTES => Err((
                "PDF_RASTER_LIMIT".into(),
                "PDFのページ画像のバイト数が上限を超えました。".into(),
            )),
            Ok(data) if valid_png(&data) => {
                let meta = RasterMeta {
                    bytes: data.len(),
                    sha256: digest(&data),
                };
                png = data;
                Ok(meta)
            }
            Ok(_) => Err(wire_error(Mode::Raster(number).invalid_worker())),
            Err(error) => Err(wire_error(error)),
        };
        let header = serde_json::to_vec(&RasterReply {
            input_sha256: digest(bytes),
            page_number: number,
            result,
        })?;
        if header.len() > MAX_RASTER_HEADER {
            return Err(Mode::Raster(number).invalid_worker());
        }
        let mut output = std::io::stdout().lock();
        output.write_all(&(header.len() as u32).to_le_bytes())?;
        output.write_all(&header)?;
        output.write_all(&png)?;
        Ok(())
    }

    fn wire_error(error: Error) -> (String, String) {
        match error {
            Error::Contract { code, message } => (code, message),
            _ => (
                "PDF_RASTER_INTERRUPTED".into(),
                "PDFのページ画像を作れませんでした。".into(),
            ),
        }
    }

    fn validate_page_number(number: u32) -> Result<()> {
        if (1..=MAX_PAGES as u32).contains(&number) {
            Ok(())
        } else {
            Err(failure(
                "PDF_RASTER_PAGE",
                "画像化するPDFのページ番号が範囲外です。",
            ))
        }
    }

    fn valid_png(bytes: &[u8]) -> bool {
        // The PNG decoder downstream checks the complete image. In the worker
        // protocol already refuse the wrong container or an oversized raster.
        let Some(header) = bytes.get(..33) else {
            return false;
        };
        let width = u32::from_be_bytes(header[16..20].try_into().expect("PNG width"));
        let height = u32::from_be_bytes(header[20..24].try_into().expect("PNG height"));
        header.starts_with(b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR")
            && (1..=RASTER_MAX_SIDE).contains(&width)
            && (1..=RASTER_MAX_SIDE).contains(&height)
    }

    fn exchange(bytes: &[u8], deadline: Instant, mode: Mode) -> Result<Vec<u8>> {
        let (memory_bytes, reply_bytes, worker_time) = mode.limits();
        let deadline = deadline.min(Instant::now() + worker_time);
        if deadline <= Instant::now() {
            return Err(mode.timeout());
        }
        if bytes.len() > MAX_PDF_ATTACHMENT_BYTES {
            return Err(failure(
                "PDF_RESOURCE_LIMIT",
                "PDFのサイズが上限を超えています。",
            ));
        }
        let exe = EXECUTABLE.get().ok_or_else(|| mode.unavailable())?;
        let mut command = Command::new(exe);
        command
            .arg(WORKER_ARG)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW);
        if matches!(mode, Mode::Raster(_)) {
            // WinRT locates the in-box PDF components through SystemRoot.
            let system_root = std::env::var_os("SystemRoot")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute() && path.join("System32").is_dir())
                .ok_or_else(|| mode.unavailable())?;
            command.env("SystemRoot", system_root);
        }
        let (mut child, guard) = job_limits::Limits::new()
            .process_memory(memory_bytes)
            .spawn(&mut command)
            .map_err(|_| mode.unavailable())?;
        let mut input = child.stdin.take().expect("PDF worker stdin pipe");
        let output = child.stdout.take().expect("PDF worker stdout pipe");
        let byte_reply = std::thread::scope(|scope| -> Result<Vec<u8>> {
            let writer = scope.spawn(move || -> std::io::Result<()> {
                match mode {
                    Mode::Text => input.write_all(FRAME)?,
                    Mode::Raster(number) => {
                        input.write_all(RASTER_FRAME)?;
                        input.write_all(&number.to_le_bytes())?;
                    }
                }
                input.write_all(bytes)
            });
            let reader = scope.spawn(move || -> std::io::Result<Vec<u8>> {
                let mut reply = Vec::new();
                output.take(reply_bytes + 1).read_to_end(&mut reply)?;
                Ok(reply)
            });
            let observed = loop {
                match child.try_wait() {
                    Ok(Some(status)) if status.success() => break Ok(()),
                    Ok(Some(_)) | Err(_) => break Err(mode.invalid_worker()),
                    Ok(None) if Instant::now() >= deadline => break Err(mode.timeout()),
                    Ok(None) => std::thread::sleep(
                        Duration::from_millis(5)
                            .min(deadline.saturating_duration_since(Instant::now())),
                    ),
                }
            };
            // Also seals the pipes on timeout or a failed observation before
            // the scoped stdin/stdout threads are joined.
            drop(guard);
            child.wait().map_err(|_| mode.invalid_worker())?;
            observed?;
            writer
                .join()
                .map_err(|_| mode.invalid_worker())?
                .map_err(|_| mode.invalid_worker())?;
            reader
                .join()
                .map_err(|_| mode.invalid_worker())?
                .map_err(|_| mode.invalid_worker())
        })?;
        if Instant::now() >= deadline {
            return Err(mode.timeout());
        }
        if byte_reply.len() as u64 > reply_bytes {
            return Err(mode.invalid_worker());
        }
        Ok(byte_reply)
    }

    pub(crate) fn read(bytes: &[u8], deadline: Instant) -> Result<Vec<Page>> {
        let byte_reply = exchange(bytes, deadline, Mode::Text)?;
        let reply: Reply =
            serde_json::from_slice(&byte_reply).map_err(|_| Mode::Text.invalid_worker())?;
        if reply.input_sha256 != digest(bytes) {
            return Err(Mode::Text.invalid_worker());
        }
        let pages = reply
            .result
            .map_err(|(code, message)| failure(&code, message))?;
        if pages.is_empty()
            || pages.len() > MAX_PAGES
            || pages.iter().map(|page| page.text.len()).sum::<usize>() > MAX_TEXT_BYTES
            || pages.iter().enumerate().any(|(index, page)| {
                page.number as usize != index + 1
                    || (page.state == PageState::Text) == page.text.trim().is_empty()
            })
        {
            return Err(Mode::Text.invalid_worker());
        }
        Ok(pages)
    }

    pub(crate) fn rasterize(bytes: &[u8], number: u32, deadline: Instant) -> Result<Vec<u8>> {
        validate_page_number(number)?;
        let mode = Mode::Raster(number);
        let mut reply_bytes = exchange(bytes, deadline, mode)?;
        let [l0, l1, l2, l3, rest @ ..] = reply_bytes.as_slice() else {
            return Err(mode.invalid_worker());
        };
        let header_len = u32::from_le_bytes([*l0, *l1, *l2, *l3]) as usize;
        if header_len > MAX_RASTER_HEADER {
            return Err(mode.invalid_worker());
        }
        let (header, png) = rest
            .split_at_checked(header_len)
            .ok_or_else(|| mode.invalid_worker())?;
        let reply: RasterReply =
            serde_json::from_slice(header).map_err(|_| mode.invalid_worker())?;
        if reply.input_sha256 != digest(bytes) || reply.page_number != number {
            return Err(mode.invalid_worker());
        }
        match reply.result {
            Ok(meta)
                if meta.bytes == png.len()
                    && meta.bytes <= MAX_RASTER_BYTES
                    && valid_png(png)
                    && meta.sha256 == digest(png) =>
            {
                if Instant::now() >= deadline {
                    return Err(mode.timeout());
                }
                reply_bytes.drain(..4 + header_len);
                Ok(reply_bytes)
            }
            Err((code, message)) if png.is_empty() => Err(failure(&code, message)),
            _ => Err(mode.invalid_worker()),
        }
    }
}

#[cfg(windows)]
pub use windows::{configure_pdf_worker, run_pdf_worker_stdio};
#[cfg(windows)]
pub(crate) use windows::{rasterize, read};
