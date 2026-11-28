//! Capture a bounded microphone utterance without giving it a filesystem target.
pub(super) const AVAILABLE: bool = cfg!(windows);

const SAMPLE_RATE: u32 = 16_000;
const BYTES_PER_FRAME: usize = 2;
const BYTES_PER_SECOND: usize = SAMPLE_RATE as usize * BYTES_PER_FRAME;
const MAX_SECONDS: u32 = 10;
const WAVE_HEADER_BYTES: usize = 44;
const MAX_RECOGNIZED_TEXT_BYTES: usize = 2_048;

pub(super) fn max_seconds(max_input_bytes: usize) -> u32 {
    (max_input_bytes.saturating_sub(WAVE_HEADER_BYTES + MAX_RECOGNIZED_TEXT_BYTES)
        / BYTES_PER_SECOND)
        .min(MAX_SECONDS as usize) as u32
}

pub(super) fn record(seconds: u32) -> Result<Vec<u8>, String> {
    if !(1..=MAX_SECONDS).contains(&seconds) {
        return Err(format!(
            "録音する秒数を1〜{MAX_SECONDS}の整数で指定してください。"
        ));
    }
    #[cfg(windows)]
    {
        let worker = std::thread::Builder::new()
            .name("minidora-microphone".into())
            .spawn(move || windows::record_pcm(seconds))
            .map_err(|_| "録音用のスレッドを開始できませんでした。".to_owned())?;
        worker
            .join()
            .map_err(|_| "録音用のスレッドが予期せず停止しました。".to_owned())?
            .map(wave_from_pcm)
    }
    #[cfg(not(windows))]
    {
        Err("マイクでの録音はWindowsで利用できます。".into())
    }
}

#[cfg(any(windows, test))]
fn wave_from_pcm(pcm: Vec<u8>) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let mut wave = Vec::with_capacity(WAVE_HEADER_BYTES + pcm.len());
    wave.extend_from_slice(b"RIFF");
    wave.extend_from_slice(&(36 + data_len).to_le_bytes());
    wave.extend_from_slice(b"WAVEfmt ");
    wave.extend_from_slice(&16u32.to_le_bytes());
    wave.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wave.extend_from_slice(&1u16.to_le_bytes()); // Mono
    wave.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wave.extend_from_slice(&(BYTES_PER_SECOND as u32).to_le_bytes());
    wave.extend_from_slice(&(BYTES_PER_FRAME as u16).to_le_bytes());
    wave.extend_from_slice(&16u16.to_le_bytes());
    wave.extend_from_slice(b"data");
    wave.extend_from_slice(&data_len.to_le_bytes());
    wave.extend(pcm);
    wave
}

#[cfg(windows)]
mod windows {
    use super::{BYTES_PER_FRAME, BYTES_PER_SECOND, MAX_SECONDS, SAMPLE_RATE};
    use std::time::{Duration, Instant};
    use wasapi::{
        AudioCaptureClient, AudioClient, DeviceEnumerator, Direction, Handle, SPEAKER_FRONT_CENTER,
        SampleType, StreamMode, WaveFormat,
    };

    struct ComApartment;

    impl Drop for ComApartment {
        fn drop(&mut self) {
            wasapi::deinitialize();
        }
    }

    struct RunningCapture<'a> {
        client: &'a AudioClient,
        running: bool,
    }

    impl RunningCapture<'_> {
        fn stop(mut self) -> Result<(), String> {
            self.client
                .stop_stream()
                .map_err(|error| format!("マイクの録音を停止できませんでした。({error})"))?;
            self.running = false;
            Ok(())
        }
    }

    impl Drop for RunningCapture<'_> {
        fn drop(&mut self) {
            if self.running {
                let _ = self.client.stop_stream();
            }
        }
    }

    pub(super) fn record_pcm(seconds: u32) -> Result<Vec<u8>, String> {
        wasapi::initialize_mta()
            .ok()
            .map_err(|error| format!("Windowsの録音機能を初期化できませんでした。({error})"))?;
        let _apartment = ComApartment;
        let devices = DeviceEnumerator::new()
            .map_err(|error| format!("録音デバイスを確認できませんでした。({error})"))?;
        let microphone = devices
            .get_default_device(Direction::Capture)
            .map_err(|error| format!("既定のマイクを取得できませんでした。({error})"))?;
        let mut client = microphone
            .get_iaudioclient()
            .map_err(|error| format!("既定のマイクを開けませんでした。({error})"))?;
        let format = WaveFormat::new(
            16,
            16,
            SampleType::Int,
            SAMPLE_RATE as usize,
            1,
            Some(SPEAKER_FRONT_CENTER),
        );
        let (_, minimum_period) = client
            .get_device_period()
            .map_err(|error| format!("マイクの処理周期を確認できませんでした。({error})"))?;
        client
            .initialize_client(
                &format,
                Direction::Capture,
                StreamMode::EventsShared {
                    autoconvert: true,
                    buffer_duration_hns: minimum_period,
                },
                None,
            )
            .map_err(|error| format!("16 kHzのモノラル録音を開始できません。({error})"))?;
        let buffer_frames = client
            .get_buffer_size()
            .map_err(|error| format!("マイクのバッファーを確認できませんでした。({error})"))?;
        capture_pcm(&client, seconds, buffer_frames)
    }

    fn capture_pcm(
        client: &AudioClient,
        seconds: u32,
        buffer_frames: u32,
    ) -> Result<Vec<u8>, String> {
        let event = client
            .set_get_eventhandle()
            .map_err(|error| format!("マイクの通知を登録できませんでした。({error})"))?;
        let capture = client
            .get_audiocaptureclient()
            .map_err(|error| format!("マイクの取得経路を初期化できませんでした。({error})"))?;
        if !(1..=SAMPLE_RATE * MAX_SECONDS).contains(&buffer_frames) {
            return Err("マイクのバッファーが録音の上限を超えています。".into());
        }
        let mut packet = vec![0; buffer_frames as usize * BYTES_PER_FRAME];
        client
            .start_stream()
            .map_err(|error| format!("マイクの録音を開始できませんでした。({error})"))?;
        let running = RunningCapture {
            client,
            running: true,
        };
        let count = seconds as usize * BYTES_PER_SECOND;
        let deadline = Instant::now() + Duration::from_secs(u64::from(seconds) + 1);
        let recorded = read_packets(&capture, &event, &mut packet, count, deadline);
        running.stop().map_err(|error| match &recorded {
            Ok(_) => error,
            Err(read_error) => format!("{read_error} {error}"),
        })?;
        recorded
    }

    fn read_packets(
        capture: &AudioCaptureClient,
        event: &Handle,
        packet: &mut [u8],
        byte_count: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>, String> {
        let timeout = || "制限時間までに指定した長さの録音を取得できませんでした。".to_owned();
        let mut pcm = Vec::with_capacity(byte_count);
        while pcm.len() < byte_count {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(timeout());
            }
            let next = capture
                .get_next_packet_size()
                .map_err(|error| format!("録音データの長さを取得できませんでした。({error})"))?
                .ok_or_else(|| "既定のマイクは共有モードの録音に対応していません。".to_owned())?;
            if next == 0 {
                let wait_ms = u32::try_from(remaining.as_millis())
                    .unwrap_or(u32::MAX)
                    .max(1);
                event.wait_for_event(wait_ms).map_err(|_| timeout())?;
                continue;
            }
            if next as usize > packet.len() / BYTES_PER_FRAME {
                return Err("録音データがマイクのバッファー上限を超えました。".into());
            }
            let (frames, info) = capture
                .read_from_device(packet)
                .map_err(|error| format!("録音データを取得できませんでした。({error})"))?;
            if frames != next || (!pcm.is_empty() && info.flags.data_discontinuity) {
                return Err("録音の一部が途切れたため発話を送信できません。".into());
            }
            let length = (frames as usize * BYTES_PER_FRAME).min(byte_count - pcm.len());
            pcm.extend_from_slice(&packet[..length]);
        }
        Ok(pcm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn microphone_duration_reserves_wave_header_and_recognized_text() {
        let one_second = WAVE_HEADER_BYTES + MAX_RECOGNIZED_TEXT_BYTES + BYTES_PER_SECOND;
        assert_eq!(max_seconds(0), 0);
        assert_eq!(max_seconds(one_second - 1), 0);
        assert_eq!(max_seconds(one_second), 1);
        assert_eq!(max_seconds(256_000), 7);
        assert_eq!(max_seconds(usize::MAX), 10);
    }

    #[test]
    fn microphone_wave_keeps_exact_pcm_bytes() {
        let pcm = vec![0xff, 0x7f, 0, 0x80];
        let wave = wave_from_pcm(pcm.clone());
        assert_eq!(wave.len(), WAVE_HEADER_BYTES + pcm.len());
        assert_eq!(&wave[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(wave[4..8].try_into().unwrap()), 40);
        assert_eq!(&wave[8..16], b"WAVEfmt ");
        assert_eq!(&wave[16..24], &[16, 0, 0, 0, 1, 0, 1, 0]);
        assert_eq!(&wave[24..28], &SAMPLE_RATE.to_le_bytes());
        assert_eq!(&wave[28..32], &(BYTES_PER_SECOND as u32).to_le_bytes());
        assert_eq!(&wave[32..36], &[2, 0, 16, 0]);
        assert_eq!(&wave[36..44], b"data\x04\x00\x00\x00");
        assert_eq!(&wave[WAVE_HEADER_BYTES..], pcm);
    }
}
