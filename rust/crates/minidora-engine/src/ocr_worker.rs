//! Run fixed Tesseract Legacy Japanese OCR on a bounded grayscale bitmap.
use minidora_domain::Result;
use std::{path::Path, time::Instant};

pub(crate) const READER_VERSION: &str = "tesseract-5.5.3.20260724-legacy-jpn-psm6-tsv-v1";

pub(crate) fn configured() -> bool {
    #[cfg(windows)]
    {
        windows::configured()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn configure_image_ocr(root: &Path) -> Result<()> {
    #[cfg(windows)]
    {
        windows::configure(root)
    }
    #[cfg(not(windows))]
    {
        let _ = root;
        Err(minidora_domain::failure(
            "IMAGE_OCR_UNAVAILABLE",
            "画像のローカル文字認識は現在Windowsで利用できます。",
        ))
    }
}

pub(crate) fn recognize(pgm: &[u8], deadline: Instant) -> Result<String> {
    #[cfg(windows)]
    {
        windows::recognize(pgm, deadline)
    }
    #[cfg(not(windows))]
    {
        let _ = (pgm, deadline);
        Err(minidora_domain::failure(
            "IMAGE_OCR_UNAVAILABLE",
            "画像のローカル文字認識は現在Windowsで利用できます。",
        ))
    }
}

#[cfg(windows)]
mod windows {
    use crate::durability::{
        is_link_or_reparse_point, keeps_local_target_name, local_target_and_ancestors,
        validate_local_target_path,
    };
    use minidora_domain::{Error, Result, digest, failure};
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

    const PROGRAM: &str = "bin/tesseract.exe";
    // Only explicitly verified release assets may be selected at startup.
    const ASSETS: &[(&str, u64, &str)] = &[
        (
            "bin/libarchive-13.dll",
            972210,
            "ddabddfdc98e1e1342ce4a7069fb4004de6c870d7bc9f4e95ef243dd18468e97",
        ),
        (
            "bin/libb2-1.dll",
            34653,
            "77bab532b5421d6cdddb96c3ae9a4bde144eb481ccaf243336c9f8c690a50987",
        ),
        (
            "bin/libbrotlicommon.dll",
            143429,
            "ca7bc06ead80259971badf2cd0bfc688db9154dad65f139f49a38b4be9976f97",
        ),
        (
            "bin/libbrotlidec.dll",
            62461,
            "a142dc6cfa77cb907028aa9426f486f3d8de37855e8db2ae2894147908121881",
        ),
        (
            "bin/libbz2-1.dll",
            100964,
            "b79cd3a7102e359ef6324e98966a67939e58ec2b651ae2847cae84a4dfa453f7",
        ),
        (
            "bin/libcurl-4.dll",
            1226963,
            "3d302f9690e810fdfd950254e2f0663d0ff768737f350c602a4ed3a06e9b6b2b",
        ),
        (
            "bin/libdeflate.dll",
            99228,
            "3f3fc3a73bc3623dc93b37bb321c484d3731d93006fd01c0495d15711826eb44",
        ),
        (
            "bin/libexpat-1.dll",
            237603,
            "92ae866c6c58dd2d8366aa0e6230ba8d434d5306e8d8c3a08b7e61750fc16153",
        ),
        (
            "bin/libgcc_s_seh-1.dll",
            954773,
            "172cfdc9d83703ec0a0ca2cee08f6cf2a166b801717ee51a56b3a63b36bc2f83",
        ),
        (
            "bin/libgif-7.dll",
            45777,
            "5b3d837ccaab84683dd93fbaddf2d98e1a9f90e0ddb8ea73dc70157adb4d26f1",
        ),
        (
            "bin/libiconv-2.dll",
            1143148,
            "7a282a854e01be726c6cccfe46f548c716aa45b3014818468253aaa4efbcd067",
        ),
        (
            "bin/libidn2-0.dll",
            247081,
            "9880b52651a0247640a2531d777380f7c35fac7efa7e3979b265ddcb78db8642",
        ),
        (
            "bin/libintl-8.dll",
            298731,
            "0537c3dd2378218508ebe3cc416d72a99ee2d24ae1c5525e23458f32544ef861",
        ),
        (
            "bin/libjbig-0.dll",
            62083,
            "275a827ba6839d47cfdbe059db1b1b403a92ff26e2b76a29362915487868bef2",
        ),
        (
            "bin/libjpeg-8.dll",
            967968,
            "b564d34adaca83e73af0e2231719dd0198ea6cd074c64b2ba85bfd7182f27db7",
        ),
        (
            "bin/libleptonica-6.dll",
            2850600,
            "1869b44e3d46fd830620b042477e5d60a950779582f38f60291547efb27792f1",
        ),
        (
            "bin/libLerc.dll",
            804265,
            "6887fb5aeabf382a835a4cb693025e8d48d9c90e82ca4eae5d2d8ffb70c6ca9a",
        ),
        (
            "bin/liblz4.dll",
            158649,
            "35f917274bca8f19677ba66f1b3cc3c83568c3249fc43215fc16e76439c8e856",
        ),
        (
            "bin/liblzma-5.dll",
            191836,
            "dc9edb9bd83b0371fe68d96cb3e70cd17b7a5aba958326a18f8210cb812432e9",
        ),
        (
            "bin/libopenjp2-7.dll",
            519082,
            "123587086cb1a25d2a338fb9b7c1d6bda10bf9062625b4f08c6a92b3d9cafe47",
        ),
        (
            "bin/libpng16-16.dll",
            264269,
            "3737838ee8d6b893df7bdabf7439775371a57dce7e17ee6b35586023c64bd9ab",
        ),
        (
            "bin/libpsl-5.dll",
            105304,
            "b31fc70eda598a39b65fe2736d815518db0652d641346f75eac40c2a0499b058",
        ),
        (
            "bin/libsharpyuv-0.dll",
            53757,
            "ab0c9f61dc69b6bcdff1011e215f8d5fd6e5096aba9be3386ab87f217fbbeffe",
        ),
        (
            "bin/libssh2-1.dll",
            271318,
            "ee7fe3e749859334c99e9261689481dd879811122598d22585421f95d29fc7ae",
        ),
        (
            "bin/libstdc++-6.dll",
            26390974,
            "5fe5d4d8f6636eb6f2dc55be99dc5dc8a6c40db1abdf592e84480d1e6985abd0",
        ),
        (
            "bin/libtesseract-5.dll",
            3501048,
            "54d54528b453ce3a5ba79487687f8df43e7b194b9ea97beba74200453fe1fb46",
        ),
        (
            "bin/libtiff-6.dll",
            625178,
            "47fe69165bf8e211aaa84224f51f145dc894e330fe7ca167f7c99419e0c8c894",
        ),
        (
            "bin/libunistring-5.dll",
            2251328,
            "b91e2850d3668d4931056a8f4d35783b8ec3eea3735485108a936a072cd16071",
        ),
        (
            "bin/libwebp-7.dll",
            749724,
            "c6c21a10f8cfe13270832e64640b82709e1d87b956b5f15af435a6c25acd6bbf",
        ),
        (
            "bin/libwebpmux-3.dll",
            82858,
            "2c8a2e9d7901ec65330f31f9989c2a07ef0506e903b485fcd8b241a6bab9b88f",
        ),
        (
            "bin/libwinpthread-1.dll",
            65955,
            "6a661d5846d80a91394dbb9b2dab87ba3cc705eac80ad45fe73677bff70cd6d2",
        ),
        (
            "bin/libzstd.dll",
            1218273,
            "b95c223a9548a9ecf51377c962e0bc8f0c51eb0c6f67a296dbc885996f0dd40d",
        ),
        (
            "bin/tesseract.exe",
            1867784,
            "c66f0f12ed76f6aa455dac97684bbc86756d6a732380bee09122454cfda3f420",
        ),
        (
            "bin/zlib1.dll",
            128488,
            "93e9243a44c29200eeacaf9658efe2558581770e4b11ca4b500e18e424a6e3b5",
        ),
        (
            "tessdata/jpn.traineddata",
            35659159,
            "6f416b902d129d8cc28e99c33244034b1cf52549e8560f6320b06d317852159a",
        ),
    ];
    const FILE_SHARE_READ: u32 = 1;
    const FILE_SHARE_WRITE: u32 = 2;
    const FILE_READ_ATTRIBUTES: u32 = 0x80;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    const MAX_PGM_BYTES: usize = 2_048 * 2_048 + 64;
    const MAX_TSV_BYTES: u64 = 1_000_000;
    const MAX_ERROR_BYTES: u64 = 64 * 1_024;
    const MEMORY_LIMIT: u64 = 256 * 1_024 * 1_024;
    const WORKER_TIME: Duration = Duration::from_secs(12);
    const POLL: Duration = Duration::from_millis(5);

    struct Runtime {
        root: PathBuf,
        // Hold the verified files and their directory names while the
        // configured worker may be launched.
        _assets: Vec<File>,
        _directories: Vec<File>,
    }

    static RUNTIME: OnceLock<Runtime> = OnceLock::new();

    pub(super) fn configured() -> bool {
        RUNTIME.get().is_some()
    }

    fn invalid_asset(name: &str) -> Error {
        failure(
            "IMAGE_OCR_ASSET_INVALID",
            format!("画像の文字認識に使う固定資産「{name}」を照合できません。"),
        )
    }

    fn work_failed() -> Error {
        failure(
            "IMAGE_OCR_INTERRUPTED",
            "画像の文字認識を正常に完了できませんでした。",
        )
    }

    fn timed_out() -> Error {
        failure(
            "IMAGE_OCR_DEADLINE",
            "画像の文字認識が依頼の期限を超えました。",
        )
    }

    fn output_limit() -> Error {
        failure(
            "IMAGE_OCR_OUTPUT_LIMIT",
            "画像の文字認識結果が上限を超えました。",
        )
    }

    fn scratch_failed() -> Error {
        failure(
            "IMAGE_OCR_TEMP_INVALID",
            "画像の文字認識に使う一時ファイルを用意できません。",
        )
    }

    fn same_root(runtime: &Runtime, root: &Path) -> Result<()> {
        if runtime.root != root {
            return Err(failure(
                "IMAGE_OCR_CONFIGURATION",
                "このプロセスでは既に別の画像文字認識資産を設定しています。",
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
            return Err(io::Error::other("not an ordinary asset directory"));
        }
        Ok(file)
    }

    fn hold_asset(path: &Path, len: u64, checksum: &str) -> io::Result<File> {
        let file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let info = file.metadata()?;
        if !info.is_file() || is_link_or_reparse_point(&info) || info.len() != len {
            return Err(io::Error::other("unexpected asset file metadata"));
        }
        let mut input = (&file).take(len + 1);
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
        if observed != len || format!("{:x}", hash.finalize()) != checksum {
            return Err(io::Error::other("asset file content differs"));
        }
        Ok(file)
    }

    pub(super) fn configure(root: &Path) -> Result<()> {
        let executable = root.join(PROGRAM);
        validate_local_target_path(&executable).map_err(|_| invalid_asset(PROGRAM))?;
        if let Some(runtime) = RUNTIME.get() {
            return same_root(runtime, root);
        }
        let mut seen = BTreeSet::new();
        let mut directories = Vec::new();
        let mut assets = Vec::with_capacity(ASSETS.len());
        for &(name, len, hash) in ASSETS {
            let path = root.join(name);
            for ancestor in local_target_and_ancestors(&path).filter(|entry| *entry != path) {
                if seen.insert(ancestor.to_owned()) {
                    directories.push(hold_directory(ancestor).map_err(|_| invalid_asset(name))?);
                }
            }
            let asset = hold_asset(&path, len, hash).map_err(|_| invalid_asset(name))?;
            if !keeps_local_target_name(&path).map_err(|_| invalid_asset(name))? {
                return Err(invalid_asset(name));
            }
            assets.push(asset);
        }
        let runtime = Runtime {
            root: root.to_owned(),
            _assets: assets,
            _directories: directories,
        };
        match RUNTIME.set(runtime) {
            Ok(()) => Ok(()),
            Err(unused) => same_root(RUNTIME.get().ok_or_else(work_failed)?, &unused.root),
        }
    }

    pub(super) fn recognize(pgm: &[u8], deadline: Instant) -> Result<String> {
        let runtime = RUNTIME.get().ok_or_else(|| {
            failure(
                "IMAGE_OCR_UNCONFIGURED",
                "固定の画像文字認識資産を起動時に指定してください。",
            )
        })?;
        let deadline = deadline.min(Instant::now() + WORKER_TIME);
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        if pgm.len() > MAX_PGM_BYTES || !pgm.starts_with(b"P5\n") {
            return Err(failure(
                "IMAGE_OCR_INPUT_INVALID",
                "制限内のP5画像を照合できません。",
            ));
        }
        // Native Windows builds may use narrow filenames. Only the
        // temporary normalized pixels are exposed to this program.
        let scratch = tempfile::tempdir().map_err(|_| scratch_failed())?;
        let result = recognize_temporary(runtime, scratch.path(), pgm, deadline);
        scratch.close().map_err(|_| {
            failure(
                "IMAGE_OCR_TEMP_CLEANUP",
                "画像の文字認識に使った一時ファイルを閉じられませんでした。",
            )
        })?;
        result
    }

    fn recognize_temporary(
        runtime: &Runtime,
        directory: &Path,
        pgm: &[u8],
        deadline: Instant,
    ) -> Result<String> {
        let path = directory.join("image.pgm");
        validate_local_target_path(&path).map_err(|_| scratch_failed())?;
        let filename = path
            .to_str()
            .filter(|name| name.is_ascii())
            .ok_or_else(|| {
                failure(
                    "IMAGE_OCR_TEMP_PATH",
                    "画像の文字認識には一時フォルダーのASCII絶対パスが必要です。",
                )
            })?;
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|_| scratch_failed())?;
        staged.write_all(pgm).map_err(|_| scratch_failed())?;
        staged.flush().map_err(|_| scratch_failed())?;
        drop(staged);
        let _input =
            hold_asset(&path, pgm.len() as u64, &digest(pgm)).map_err(|_| scratch_failed())?;
        run(runtime, filename, deadline)
    }

    fn capture(pipe: impl Read, limit: u64, exceeded: &AtomicBool) -> io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        pipe.take(limit + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > limit {
            exceeded.store(true, Ordering::Relaxed);
            return Err(io::Error::other("recognizer output exceeded its limit"));
        }
        Ok(bytes)
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
                return Err(timed_out());
            }
            if let Some(status) = child.try_wait().map_err(|_| work_failed())? {
                return Ok(status);
            }
            thread::sleep(POLL);
        }
    }

    fn reap_after_job_close(child: &mut Child) -> Result<()> {
        let _ = child.kill();
        let end = Instant::now() + Duration::from_millis(750);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) if Instant::now() < end => thread::sleep(POLL),
                _ => {
                    return Err(failure(
                        "IMAGE_OCR_TERMINATION_UNCONFIRMED",
                        "画像の文字認識プロセスの終了を確認できませんでした。",
                    ));
                }
            }
        }
    }

    fn run(runtime: &Runtime, filename: &str, deadline: Instant) -> Result<String> {
        let system_root = std::env::var_os("SystemRoot")
            .filter(|root| {
                let root = Path::new(root);
                root.is_absolute() && root.join("System32").is_dir()
            })
            .ok_or_else(work_failed)?;
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        let mut command = Command::new(runtime.root.join(PROGRAM));
        command
            .current_dir(&runtime.root)
            .env_clear()
            .env("SystemRoot", system_root)
            .env("OMP_THREAD_LIMIT", "1")
            .args([
                filename,
                "stdout",
                "--tessdata-dir",
                "tessdata",
                "-l",
                "jpn",
                "--oem",
                "0",
                "--psm",
                "6",
                "-c",
                "tessedit_create_tsv=1",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        let (mut child, guard) = job_limits::Limits::new()
            .process_memory(MEMORY_LIMIT)
            .spawn(&mut command)
            .map_err(|_| work_failed())?;
        let exceeded = Arc::new(AtomicBool::new(false));
        let stdout = child.stdout.take().expect("configured stdout pipe");
        let stderr = child.stderr.take().expect("configured stderr pipe");
        let pipes = (|| -> io::Result<_> {
            let stdout_limit = Arc::clone(&exceeded);
            let stdout = thread::Builder::new()
                .spawn(move || capture(stdout, MAX_TSV_BYTES, &stdout_limit))?;
            let stderr_limit = Arc::clone(&exceeded);
            let stderr = thread::Builder::new()
                .spawn(move || capture(stderr, MAX_ERROR_BYTES, &stderr_limit))?;
            Ok((stdout, stderr))
        })();
        let (stdout, stderr) = match pipes {
            Ok(pipes) => pipes,
            Err(_) => {
                drop(guard);
                reap_after_job_close(&mut child)?;
                return Err(work_failed());
            }
        };
        let status = wait_for_exit(&mut child, deadline, &exceeded);
        drop(guard);
        let status = match status {
            Ok(status) => status,
            Err(error) => {
                reap_after_job_close(&mut child)?;
                return Err(error);
            }
        };
        while !stdout.is_finished() || !stderr.is_finished() {
            if Instant::now() >= deadline {
                return Err(timed_out());
            }
            thread::sleep(POLL);
        }
        if exceeded.load(Ordering::Relaxed) {
            return Err(output_limit());
        }
        if !status.success() {
            return Err(work_failed());
        }
        let output = stdout
            .join()
            .map_err(|_| work_failed())?
            .map_err(|_| work_failed())?;
        stderr
            .join()
            .map_err(|_| work_failed())?
            .map_err(|_| work_failed())?;
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        String::from_utf8(output).map_err(|_| work_failed())
    }
}
