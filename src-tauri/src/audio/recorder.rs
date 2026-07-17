use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SampleRate, StreamConfig};
use embral_engine::speakers::ChannelWindow;
use hound::{WavSpec, WavWriter};
use rubato::{FftFixedInOut, Resampler};
use std::collections::VecDeque;
use std::io::BufWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use super::meter::{ChannelMeter, LevelTap};

const TARGET_SAMPLE_RATE: u32 = 16000;
const RESAMPLE_CHUNK: usize = 1024;

/// How far the loopback (system-audio) buffer may run ahead of the mic before
/// the oldest samples are dropped. The mic is the master clock; the loopback
/// stream fills a buffer that the mic drains and mixes. Bounds added latency
/// and unbounded growth from clock drift between the two capture devices —
/// fires rarely at realistic drift, and the discontinuity when it does is small
/// and bounded.
const MAX_LOOPBACK_LAG_SECS: usize = 2;

/// What a capture stream does with each block of resampled 16 kHz mono output.
///
/// The two device streams (mic + WASAPI loopback) run on independent clocks and
/// fire callbacks at unrelated times, so we can't just append both into one
/// file — doing so interleaves ~64 ms chunks from each source and yields a
/// doubled-length, garbled recording. Instead the mic acts as the master clock:
/// input devices deliver callbacks continuously (silence samples included),
/// whereas WASAPI loopback goes quiet — no callbacks — when nothing is playing.
#[derive(Clone)]
enum MixSink {
    /// Mic — the master clock. Owns the WAV writer and the transcription
    /// channel. For each resampled block it pulls an equal number of loopback
    /// samples (silence-padded when the loopback buffer is short), sums them,
    /// and writes the single mixed stream out.
    Primary {
        wav_writer: Arc<Mutex<Option<WavWriter<BufWriter<std::fs::File>>>>>,
        audio_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<f32>>>,
        loopback: Arc<Mutex<VecDeque<f32>>>,
        /// Pre-mix per-channel loudness timeline — the only place mic and
        /// loopback still exist separately (feeds "you" attribution).
        meter: Arc<Mutex<ChannelMeter>>,
        /// Live ~10 Hz spectrum tap for the recording view's meter.
        level: Option<Arc<Mutex<LevelTap>>>,
    },
    /// Loopback — pushes resampled samples into the shared buffer for the
    /// primary to drain and mix. Produces no WAV / transcription output itself.
    Secondary { loopback: Arc<Mutex<VecDeque<f32>>> },
    /// Plain accumulation into a buffer — used by short one-off captures
    /// (voice-reference enrollment), no mixing, no file.
    Buffer { out: Arc<Mutex<Vec<f32>>> },
    /// Stream blocks straight into a channel — dictation's mic-only live
    /// path (no WAV, no loopback). Dropping the stream drops this sender,
    /// which is how the consumer learns the capture ended.
    Tx {
        tx: tokio::sync::mpsc::UnboundedSender<Vec<f32>>,
    },
}

impl MixSink {
    /// Consume one block of this stream's resampled 16 kHz mono samples.
    fn consume(&self, resampled: &[f32]) {
        match self {
            MixSink::Buffer { out } => {
                out.lock().unwrap().extend_from_slice(resampled);
            }
            MixSink::Tx { tx } => {
                let _ = tx.send(resampled.to_vec());
            }
            MixSink::Secondary { loopback } => {
                let mut buf = loopback.lock().unwrap();
                buf.extend(resampled.iter().copied());
                // Cap latency / unbounded growth from device-clock drift.
                let cap = TARGET_SAMPLE_RATE as usize * MAX_LOOPBACK_LAG_SECS;
                if buf.len() > cap {
                    let excess = buf.len() - cap;
                    buf.drain(..excess);
                }
            }
            MixSink::Primary {
                wav_writer,
                audio_tx,
                loopback,
                meter,
                level,
            } => {
                // Mix in buffered loopback (system) audio aligned sample-for-
                // sample. When the loopback buffer is short — system audio
                // silent, or no loopback device — the tail stays mic-only, so
                // the mixer degrades gracefully to mic-only capture.
                let mut mixed = resampled.to_vec();
                {
                    let mut buf = loopback.lock().unwrap();
                    let take = mixed.len().min(buf.len());
                    let lb_block: Vec<f32> = buf.drain(..take).collect();
                    if let Ok(mut m) = meter.lock() {
                        m.push_block(resampled, &lb_block);
                    }
                    if let Some(level) = level {
                        if let Ok(mut tap) = level.lock() {
                            tap.push_block(resampled, &lb_block);
                        }
                    }
                    for (slot, lb) in mixed.iter_mut().zip(lb_block) {
                        // Sum-and-clamp: keeps each source at full volume (only
                        // one is usually active), hard-clipping the rare moment
                        // both peak together rather than halving everything.
                        *slot = (*slot + lb).clamp(-1.0, 1.0);
                    }
                }

                if let Ok(mut guard) = wav_writer.lock() {
                    if let Some(w) = guard.as_mut() {
                        for &s in &mixed {
                            let _ = w.write_sample(s);
                        }
                    }
                }
                if let Some(tx) = audio_tx {
                    if let Err(e) = tx.send(mixed) {
                        tracing::error!("mix audio_tx send failed (channel closed?): {}", e);
                    }
                }
            }
        }
    }
}

pub struct Recorder {
    paused: Arc<AtomicBool>,
    wav_path: PathBuf,
    wav_writer: Arc<Mutex<Option<WavWriter<BufWriter<std::fs::File>>>>>,
    meter: Arc<Mutex<ChannelMeter>>,
    _mic_stream: cpal::Stream,
    _loopback_stream: Option<cpal::Stream>,
}

// SAFETY: cpal::Stream opts out of Send/Sync via a conservative cross-platform marker
// (NotSendSyncAcrossAllPlatforms). On the target platform (Windows WASAPI) cpal::Stream
// is legitimately Send. All mutable state in Recorder is guarded by Arc<Mutex<...>>,
// making concurrent use safe.
unsafe impl Send for Recorder {}
unsafe impl Sync for Recorder {}

/// Find a device by name among `devices`, or `None` to use the default.
/// A configured-but-missing device falls back to the default with a warning —
/// an unplugged USB mic must not break recording.
fn find_device(
    devices: impl Iterator<Item = cpal::Device>,
    preferred: Option<&str>,
    kind: &str,
) -> Option<cpal::Device> {
    let name = preferred?.trim();
    if name.is_empty() {
        return None;
    }
    for device in devices {
        if device.name().map(|n| n == name).unwrap_or(false) {
            tracing::info!("[{}] using configured device '{}'", kind, name);
            return Some(device);
        }
    }
    tracing::warn!(
        "[{}] configured device '{}' not found — falling back to system default",
        kind,
        name
    );
    None
}

impl Recorder {
    pub fn start(
        wav_path: PathBuf,
        audio_tx: Option<tokio::sync::mpsc::UnboundedSender<Vec<f32>>>,
        mic_device: Option<&str>,
        output_device: Option<&str>,
        level_cb: Option<Box<dyn Fn(&[f32], &[f32]) + Send>>,
    ) -> Result<Self> {
        let paused = Arc::new(AtomicBool::new(false));

        let spec = WavSpec {
            channels: 1,
            sample_rate: TARGET_SAMPLE_RATE,
            bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float,
        };

        if let Some(parent) = wav_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let writer = WavWriter::create(&wav_path, spec)?;
        let wav_writer = Arc::new(Mutex::new(Some(writer)));

        let host = cpal::default_host();

        // Shared sink for resampled 16 kHz loopback samples. The mic stream
        // (master clock) drains and mixes this; the loopback stream fills it.
        let loopback_buffer: Arc<Mutex<VecDeque<f32>>> = Arc::new(Mutex::new(VecDeque::new()));

        let mic_device = host
            .input_devices()
            .ok()
            .and_then(|devices| find_device(devices, mic_device, "mic"))
            .or_else(|| host.default_input_device())
            .ok_or_else(|| anyhow!("No default input device found"))?;
        let mic_name = mic_device
            .name()
            .unwrap_or_else(|_| "<unknown>".to_string());
        let mic_config = mic_device.default_input_config()?;
        tracing::info!(
            "Mic input: device='{}', sample_rate={} Hz, channels={}, format={:?}",
            mic_name,
            mic_config.sample_rate().0,
            mic_config.channels(),
            mic_config.sample_format()
        );
        let meter = Arc::new(Mutex::new(ChannelMeter::default()));
        let level = level_cb.map(|cb| Arc::new(Mutex::new(LevelTap::new(cb))));
        let mic_sink = MixSink::Primary {
            wav_writer: wav_writer.clone(),
            audio_tx: audio_tx.clone(),
            loopback: loopback_buffer.clone(),
            meter: meter.clone(),
            level,
        };
        let mic_stream = build_stream("mic", &mic_device, &mic_config, paused.clone(), mic_sink)?;
        mic_stream.play()?;
        tracing::info!("Mic input stream started");

        let loopback_stream =
            build_loopback_stream(&host, paused.clone(), loopback_buffer, output_device);

        Ok(Self {
            paused,
            wav_path,
            wav_writer,
            meter,
            _mic_stream: mic_stream,
            _loopback_stream: loopback_stream,
        })
    }

    pub fn pause(&self) {
        self.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.paused.store(false, Ordering::SeqCst);
    }

    /// Stop capturing; returns the WAV path and the pre-mix channel-loudness
    /// timeline (for "you" attribution in the speaker pipeline).
    pub fn stop(self) -> Result<(PathBuf, Vec<ChannelWindow>)> {
        drop(self._mic_stream);
        drop(self._loopback_stream);
        if let Ok(mut guard) = self.wav_writer.lock() {
            if let Some(writer) = guard.take() {
                writer.finalize()?;
            }
        }
        let windows = match self.meter.lock() {
            Ok(mut m) => std::mem::take(&mut *m).finish(),
            Err(_) => Vec::new(),
        };
        Ok((self.wav_path, windows))
    }
}

/// A live mic-only 16 kHz stream feeding a channel — dictation's capture.
/// Dropping it stops the stream and closes the channel.
pub struct MicStream {
    _stream: cpal::Stream,
}

// SAFETY: same justification as Recorder — on Windows WASAPI cpal::Stream is
// legitimately Send; no shared mutable state beyond the channel.
unsafe impl Send for MicStream {}

impl MicStream {
    pub fn start(
        mic_device: Option<&str>,
        tx: tokio::sync::mpsc::UnboundedSender<Vec<f32>>,
    ) -> Result<MicStream> {
        let host = cpal::default_host();
        let device = host
            .input_devices()
            .ok()
            .and_then(|devices| find_device(devices, mic_device, "dictation"))
            .or_else(|| host.default_input_device())
            .ok_or_else(|| anyhow!("No default input device found"))?;
        let config = device.default_input_config()?;
        let paused = Arc::new(AtomicBool::new(false));
        let stream = build_stream("dictation", &device, &config, paused, MixSink::Tx { tx })?;
        stream.play()?;
        Ok(MicStream { _stream: stream })
    }
}

/// Capture up to `secs` of mic-only 16 kHz mono audio, polling `cancel` to
/// stop early (returns what was captured so far). Blocking — call from a
/// blocking task. Used for voice-reference enrollment; system audio is
/// deliberately not mixed in.
pub fn capture_mic(
    mic_device: Option<&str>,
    secs: f32,
    cancel: Arc<AtomicBool>,
) -> Result<Vec<f32>> {
    let host = cpal::default_host();
    let device = host
        .input_devices()
        .ok()
        .and_then(|devices| find_device(devices, mic_device, "enroll"))
        .or_else(|| host.default_input_device())
        .ok_or_else(|| anyhow!("No default input device found"))?;
    let config = device.default_input_config()?;

    let out: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let sink = MixSink::Buffer { out: out.clone() };
    let paused = Arc::new(AtomicBool::new(false));
    let stream = build_stream("enroll", &device, &config, paused, sink)?;
    stream.play()?;

    let target = (secs * TARGET_SAMPLE_RATE as f32) as usize;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs_f32(secs + 3.0);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if cancel.load(Ordering::Acquire) {
            break;
        }
        if out.lock().unwrap().len() >= target {
            break;
        }
        // A dead device delivers no callbacks; don't hang the command forever.
        if std::time::Instant::now() > deadline {
            break;
        }
    }
    drop(stream);
    let mut samples = std::mem::take(&mut *out.lock().unwrap());
    samples.truncate(target);
    Ok(samples)
}

fn build_loopback_stream(
    host: &cpal::Host,
    paused: Arc<AtomicBool>,
    loopback_buffer: Arc<Mutex<VecDeque<f32>>>,
    preferred: Option<&str>,
) -> Option<cpal::Stream> {
    let device = host
        .output_devices()
        .ok()
        .and_then(|devices| find_device(devices, preferred, "loopback"))
        .or_else(|| host.default_output_device())?;
    let name = device.name().unwrap_or_else(|_| "<unknown>".to_string());
    let config = device.default_output_config().ok()?;
    tracing::info!(
        "Loopback output: device='{}', sample_rate={} Hz, channels={}, format={:?}",
        name,
        config.sample_rate().0,
        config.channels(),
        config.sample_format()
    );
    let sink = MixSink::Secondary {
        loopback: loopback_buffer,
    };
    match build_stream("loopback", &device, &config, paused, sink) {
        Ok(stream) => {
            if stream.play().is_ok() {
                tracing::info!("Loopback output stream started");
                Some(stream)
            } else {
                tracing::warn!("Loopback stream play() failed");
                None
            }
        }
        Err(e) => {
            tracing::warn!("Loopback stream unavailable: {}", e);
            None
        }
    }
}

fn build_stream(
    label: &'static str,
    device: &cpal::Device,
    config: &cpal::SupportedStreamConfig,
    paused: Arc<AtomicBool>,
    sink: MixSink,
) -> Result<cpal::Stream> {
    let channels = config.channels() as usize;
    let device_rate = config.sample_rate().0;
    let sample_format = config.sample_format();

    let resampler_inner = FftFixedInOut::<f32>::new(
        device_rate as usize,
        TARGET_SAMPLE_RATE as usize,
        RESAMPLE_CHUNK,
        1,
    )
    .map_err(|e| anyhow!("Failed to create resampler: {}", e))?;
    let required_input_frames = resampler_inner.input_frames_next();
    tracing::info!(
        "[{}] Resampler: {} Hz -> {} Hz, input_frames_per_chunk={}, output_frames_per_chunk={}",
        label,
        device_rate,
        TARGET_SAMPLE_RATE,
        required_input_frames,
        RESAMPLE_CHUNK
    );
    let resampler = Arc::new(Mutex::new(resampler_inner));
    let accumulator: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
    let callback_count = Arc::new(AtomicUsize::new(0));
    let chunk_count = Arc::new(AtomicUsize::new(0));

    let stream_config = StreamConfig {
        channels: config.channels(),
        sample_rate: SampleRate(device_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let error_cb = move |e: cpal::StreamError| {
        tracing::error!("[{}] Audio stream error: {}", label, e);
    };

    macro_rules! build_typed {
        ($ty:ty, $convert:expr) => {{
            let paused = paused.clone();
            let sink = sink.clone();
            let resampler = resampler.clone();
            let accumulator = accumulator.clone();
            let callback_count = callback_count.clone();
            let chunk_count = chunk_count.clone();

            device.build_input_stream(
                &stream_config,
                move |data: &[$ty], _| {
                    let cb_n = callback_count.fetch_add(1, Ordering::Relaxed);
                    if cb_n == 0 {
                        tracing::info!(
                            "[{}] First audio callback fired ({} samples, {} channels)",
                            label,
                            data.len(),
                            channels
                        );
                    }

                    if paused.load(Ordering::SeqCst) {
                        return;
                    }
                    let mono: Vec<f32> = data
                        .chunks(channels)
                        .map(|frame| {
                            let sum: f32 = frame.iter().map(|s| $convert(*s)).sum();
                            sum / channels as f32
                        })
                        .collect();

                    let max_amp = mono.iter().fold(0.0f32, |m, &s| m.max(s.abs()));

                    let mut acc = accumulator.lock().unwrap();
                    acc.extend_from_slice(&mono);

                    while acc.len() >= required_input_frames {
                        let chunk: Vec<f32> = acc.drain(..required_input_frames).collect();
                        let input = vec![chunk];
                        let mut rsp = resampler.lock().unwrap();
                        match rsp.process(&input, None) {
                            Ok(output) => {
                                let resampled = &output[0];
                                // Route to the WAV/transcription mix (mic) or
                                // the shared loopback buffer (loopback).
                                sink.consume(resampled);
                                let n = chunk_count.fetch_add(1, Ordering::Relaxed);
                                if n == 0 {
                                    tracing::info!(
                                        "[{}] First resampled chunk produced ({} samples, max_amp={:.4})",
                                        label,
                                        resampled.len(),
                                        max_amp
                                    );
                                }
                                // Every ~50 chunks (~3.2s of audio) emit a stats line
                                if (n + 1) % 50 == 0 {
                                    tracing::debug!(
                                        "[{}] Audio capture stats: {} chunks emitted, latest max_amp={:.4}",
                                        label,
                                        n + 1,
                                        max_amp
                                    );
                                }
                            }
                            Err(e) => tracing::error!("[{}] Resampler error: {}", label, e),
                        }
                    }
                },
                error_cb,
                None,
            )
        }};
    }

    let stream = match sample_format {
        SampleFormat::F32 => build_typed!(f32, |s: f32| s)?,
        SampleFormat::I16 => build_typed!(i16, |s: i16| s as f32 / i16::MAX as f32)?,
        SampleFormat::U16 => build_typed!(u16, |s: u16| (s as f32 / u16::MAX as f32) * 2.0 - 1.0)?,
        SampleFormat::I8 => build_typed!(i8, |s: i8| s as f32 / i8::MAX as f32)?,
        SampleFormat::I32 => build_typed!(i32, |s: i32| s as f32 / i32::MAX as f32)?,
        SampleFormat::F64 => build_typed!(f64, |s: f64| s as f32)?,
        fmt => return Err(anyhow!("Unsupported sample format: {:?}", fmt)),
    };

    Ok(stream)
}
