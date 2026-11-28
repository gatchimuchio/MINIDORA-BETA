//! Local Japanese TTS using the pinned Open JTalk front-end and HTS vocoder.
use jbonsai::Engine as HtsEngine;
use jpreprocess::{DefaultTokenizer, JPreprocess, SystemDictionaryConfig};
use minidora_domain::{Error, Result, digest, failure};
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

pub(crate) const VOICE_ID: &str = "japanese.hts.nitech-jp-atr503-m001.v1.05";
const VOICE_BYTES: usize = 1_169_264;
const VOICE_SHA256: &str = "1329c5e4c0176dc037eef50a73712c60bede2b26ff4a1cba5008223608ca942f";
// These are the nine files of the jpreprocess 0.16.0 compiled NAIST-JDIC
// release (archive SHA-256 0455620677294140ed58c9b580000af21af4dea4d8aafc8b55f07d825a25935b).
const DICTIONARY_ASSETS: [(&str, usize, &str); 9] = [
    (
        "char_def.bin",
        2_356,
        "cd3dbb6fa448906590c2e5ef2600378e4bd074c4d1d6fc5c049d4fd8c2d2d556",
    ),
    (
        "dict.trie",
        10_516_788,
        "5b28b3493426619e8119d2bd5515f6fee60416b736faf4e5c91e0c33b8fd859f",
    ),
    (
        "dict.vals",
        7_889_010,
        "7fb308e1909be247779be0a4c81070416635c4ff0ab266d48a5e964baffd345f",
    ),
    (
        "dict.valsidx",
        2_710_692,
        "a019c29128cbbe45d437e965efb762111a0b665f1d50519c04d977b4b91f3a57",
    ),
    (
        "dict.words",
        16_314_738,
        "80786aaa073a2508a9749d29b1e196d756b44751b0434054f33e9a9a91583803",
    ),
    (
        "dict.wordsidx",
        3_155_608,
        "e6024e16879c7758daf960dc2c0e5733a6186c78cd26712eb7a55e4c3d70b240",
    ),
    (
        "matrix.mtx",
        3_792_264,
        "a64b58ac89b2ca21f3ae6c56ca772f8d4488cc636f47b821b4cca9c0c67c95d2",
    ),
    (
        "metadata.json",
        790,
        "f152abfd917ff07be957c4f1bebb4208bbb565e1b9b1bd3826a3d8a1e7633b2b",
    ),
    (
        "unk.bin",
        2_492,
        "1ff6b44ab3145cd8ac731b9cb0500804feae5fa257aee8bc78a65a8dc80bb454",
    ),
];
const MAX_TEXT_BYTES: usize = 1_024;
const MAX_TEXT_CHARS: usize = 300;
const MAX_LABELS: usize = 512;
const SAMPLE_RATE: usize = 48_000;
const FRAME_PERIOD: usize = 240;
const MAX_AUDIO_BYTES: usize = 15 * SAMPLE_RATE * 2;
const WAV_HEADER_BYTES: usize = 44;

struct Runtime {
    dictionary_path: PathBuf,
    voice_path: PathBuf,
    language: JPreprocess<DefaultTokenizer>,
    voice: HtsEngine,
    // Keep the verified files open for the lifetime of the process. The
    // Windows share mode below permits readers and excludes writes/deletion.
    _dictionary_files: Vec<File>,
}

static RUNTIME: OnceLock<Runtime> = OnceLock::new();

pub(crate) struct Synthesis {
    pub bytes: Vec<u8>,
    pub voice_id: String,
}

fn asset_error(name: &str) -> Error {
    failure(
        "SPEECH_ASSET_INVALID",
        format!("日本語音声の固定資産「{name}」を照合できません。"),
    )
}

fn configuration_error() -> Error {
    failure(
        "SPEECH_CONFIGURATION",
        "日本語音声には固定した辞書と音声モデルの絶対パスを指定してください。",
    )
}

fn synthesis_error() -> Error {
    failure(
        "SPEECH_SYNTHESIS_FAILED",
        "日本語の音声データを生成できませんでした。",
    )
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(failure(
            "SPEECH_DEADLINE",
            "音声生成が依頼の期限を超えました。",
        ));
    }
    Ok(())
}

fn open_asset(path: &Path, name: &str, len: usize, checksum: &str) -> Result<(File, Vec<u8>)> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(1); // FILE_SHARE_READ
    }
    let file = options.open(path).map_err(|_| asset_error(name))?;
    let metadata = file.metadata().map_err(|_| asset_error(name))?;
    if !metadata.is_file() || metadata.len() != len as u64 {
        return Err(asset_error(name));
    }
    let mut bytes = Vec::with_capacity(len);
    (&file)
        .take(len as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| asset_error(name))?;
    if bytes.len() != len || digest(&bytes) != checksum {
        return Err(asset_error(name));
    }
    Ok((file, bytes))
}

fn verify_same_runtime(runtime: &Runtime, dictionary: &Path, voice: &Path) -> Result<()> {
    if runtime.dictionary_path != dictionary || runtime.voice_path != voice {
        return Err(configuration_error());
    }
    Ok(())
}

/// Configure the local non-neural Japanese voice from trusted startup paths.
/// The runtime never downloads an asset, changes providers, or plays sound.
pub fn configure_japanese_speech(dictionary: &Path, voice_model: &Path) -> Result<()> {
    if !dictionary.is_absolute() || !voice_model.is_absolute() {
        return Err(configuration_error());
    }
    let dictionary_path = dictionary
        .canonicalize()
        .map_err(|_| asset_error("NAIST-JDIC"))?;
    let voice_path = voice_model
        .canonicalize()
        .map_err(|_| asset_error(VOICE_ID))?;
    if let Some(runtime) = RUNTIME.get() {
        return verify_same_runtime(runtime, &dictionary_path, &voice_path);
    }
    let (_, voice_bytes) = open_asset(&voice_path, VOICE_ID, VOICE_BYTES, VOICE_SHA256)?;
    let voice = HtsEngine::load_from_bytes([voice_bytes]).map_err(|_| asset_error(VOICE_ID))?;
    if voice.condition.get_sampling_frequency() != SAMPLE_RATE
        || voice.condition.get_fperiod() != FRAME_PERIOD
    {
        return Err(asset_error(VOICE_ID));
    }

    let mut dictionary_files = Vec::with_capacity(DICTIONARY_ASSETS.len());
    for (name, len, checksum) in DICTIONARY_ASSETS {
        let (file, _) = open_asset(&dictionary_path.join(name), name, len, checksum)?;
        dictionary_files.push(file);
    }
    let dictionary = SystemDictionaryConfig::File(dictionary_path.clone())
        .load()
        .map_err(|_| asset_error("NAIST-JDIC"))?;
    let runtime = Runtime {
        dictionary_path,
        voice_path,
        language: JPreprocess::with_dictionaries(dictionary, None),
        voice,
        _dictionary_files: dictionary_files,
    };
    match RUNTIME.set(runtime) {
        Ok(()) => Ok(()),
        Err(unused) => verify_same_runtime(
            RUNTIME.get().ok_or_else(configuration_error)?,
            &unused.dictionary_path,
            &unused.voice_path,
        ),
    }
}

fn write_wave_header(bytes: &mut [u8]) -> Result<()> {
    let data_len = u32::try_from(bytes.len() - WAV_HEADER_BYTES).map_err(|_| synthesis_error())?;
    bytes[..4].copy_from_slice(b"RIFF");
    bytes[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    bytes[8..16].copy_from_slice(b"WAVEfmt ");
    bytes[16..20].copy_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk
    bytes[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM format
    bytes[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    bytes[24..28].copy_from_slice(&(SAMPLE_RATE as u32).to_le_bytes());
    bytes[28..32].copy_from_slice(&(2 * SAMPLE_RATE as u32).to_le_bytes());
    bytes[32..34].copy_from_slice(&2u16.to_le_bytes()); // bytes per mono sample
    bytes[34..36].copy_from_slice(&16u16.to_le_bytes());
    bytes[36..40].copy_from_slice(b"data");
    bytes[40..44].copy_from_slice(&data_len.to_le_bytes());
    Ok(())
}

pub(crate) fn synthesize(text: &str, deadline: Instant) -> Result<Synthesis> {
    check_deadline(deadline)?;
    if text.len() > MAX_TEXT_BYTES
        || text.chars().count() > MAX_TEXT_CHARS
        || !text.chars().any(char::is_alphanumeric)
        || text
            .chars()
            .any(|ch| ch.is_control() && !matches!(ch, '\n' | '\r' | '\t'))
    {
        return Err(failure(
            "SPEECH_TEXT_LIMIT",
            "読み上げる文章が空か、文字数または対応する文字の範囲を外れています。",
        ));
    }
    let runtime = RUNTIME.get().ok_or_else(|| {
        failure(
            "SPEECH_UNCONFIGURED",
            "日本語音声の固定辞書と音声モデルを起動時に指定してください。",
        )
    })?;
    let labels = runtime
        .language
        .extract_fullcontext(text)
        .map_err(|_| synthesis_error())?;
    if !(3..=MAX_LABELS).contains(&labels.len()) {
        return Err(failure(
            "SPEECH_LABEL_LIMIT",
            "発音を表す音素列が利用できないか、長さの上限を超えました。",
        ));
    }
    check_deadline(deadline)?;
    let mut sequence = runtime
        .voice
        .generator(labels)
        .map_err(|_| synthesis_error())?;
    let mut frame = [0f64; FRAME_PERIOD];
    let mut bytes = vec![0u8; WAV_HEADER_BYTES];
    let mut has_signal = false;
    loop {
        check_deadline(deadline)?;
        let count = sequence.generate_step(&mut frame);
        if count == 0 {
            break;
        }
        if bytes.len() + 2 * count > WAV_HEADER_BYTES + MAX_AUDIO_BYTES {
            return Err(failure(
                "SPEECH_AUDIO_LIMIT",
                "生成する音声が15秒の上限を超えました。",
            ));
        }
        for &sample in frame.get(..count).ok_or_else(synthesis_error)? {
            if !sample.is_finite() {
                return Err(synthesis_error());
            }
            let sample = sample.clamp(i16::MIN as f64, i16::MAX as f64) as i16;
            has_signal |= sample != 0;
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    if !has_signal {
        return Err(synthesis_error());
    }
    write_wave_header(&mut bytes)?;
    check_deadline(deadline)?;
    Ok(Synthesis {
        bytes,
        voice_id: VOICE_ID.into(),
    })
}
