//! Pre-mix channel loudness metering.
//!
//! The recorder mixes mic + system loopback into one stream before the ASR
//! ever sees it, so channel identity is gone downstream. This meter runs at
//! the one point where both blocks still exist separately (the mic callback,
//! just before summing) and folds them into ~half-second
//! [`ChannelWindow`]s. The speaker pipeline later asks "was this diarized
//! cluster's time mic-dominant?" to attribute clusters to the user.
//!
//! Timing is derived from consumed sample counts, exactly like the WAV and
//! transcript timelines, so windows and segments share a clock.

use embral_engine::speakers::ChannelWindow;

const SAMPLE_RATE: u64 = 16_000;
/// Nominal window length. Real windows close on the first block boundary at
/// or past this, so they can run a block long — times stay sample-exact.
const WINDOW_SAMPLES: u64 = SAMPLE_RATE / 2;

#[derive(Default)]
pub struct ChannelMeter {
    windows: Vec<ChannelWindow>,
    window_start: u64,
    mic_sq: f64,
    loop_sq: f64,
    n: u64,
}

impl ChannelMeter {
    /// Consume one mic block and the loopback samples mixed against it.
    /// `lb` may be shorter than `mic` (loopback buffer ran dry) — the missing
    /// tail is silence, which contributes nothing to the sum of squares.
    pub fn push_block(&mut self, mic: &[f32], lb: &[f32]) {
        self.mic_sq += mic.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>();
        self.loop_sq += lb.iter().map(|s| (*s as f64) * (*s as f64)).sum::<f64>();
        self.n += mic.len() as u64;
        if self.n >= WINDOW_SAMPLES {
            self.close_window();
        }
    }

    /// Close any partial window and hand back the full timeline.
    pub fn finish(mut self) -> Vec<ChannelWindow> {
        if self.n > 0 {
            self.close_window();
        }
        self.windows
    }

    fn close_window(&mut self) {
        let end = self.window_start + self.n;
        self.windows.push(ChannelWindow {
            start: self.window_start as f64 / SAMPLE_RATE as f64,
            end: end as f64 / SAMPLE_RATE as f64,
            mic_rms: (self.mic_sq / self.n as f64).sqrt() as f32,
            loop_rms: (self.loop_sq / self.n as f64).sqrt() as f32,
        });
        self.window_start = end;
        self.mic_sq = 0.0;
        self.loop_sq = 0.0;
        self.n = 0;
    }
}

/// Frequency bands for the live spectrum meter, log-spaced across the
/// vocal fundamental range. The frontend renders one stationary bar per
/// band.
pub const LEVEL_BANDS: usize = 24;
const BAND_LOW_HZ: f32 = 85.0;
const BAND_HIGH_HZ: f32 = 500.0;

/// The band center frequencies (log-spaced low→high).
pub fn band_frequencies() -> [f32; LEVEL_BANDS] {
    let ratio = BAND_HIGH_HZ / BAND_LOW_HZ;
    std::array::from_fn(|i| {
        BAND_LOW_HZ * ratio.powf(i as f32 / (LEVEL_BANDS - 1) as f32)
    })
}

/// Normalized single-frequency magnitude (Goertzel) of a sample slice —
/// a tiny filterbank beats pulling in an FFT for two dozen bands.
fn goertzel_magnitude(samples: &[f32], freq: f32) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let w = 2.0 * std::f32::consts::PI * freq / SAMPLE_RATE as f32;
    let coeff = 2.0 * w.cos();
    let (mut s_prev, mut s_prev2) = (0.0f32, 0.0f32);
    for &x in samples {
        let s = x + coeff * s_prev - s_prev2;
        s_prev2 = s_prev;
        s_prev = s;
    }
    let power =
        (s_prev2 * s_prev2 + s_prev * s_prev - coeff * s_prev * s_prev2).max(0.0);
    2.0 * power.sqrt() / samples.len() as f32
}

/// Live spectrum tap for the recording view's meter: folds the same pre-mix
/// blocks into ~100 ms slices and hands each slice's per-band mic/system
/// magnitudes to a callback (the Tauri event emitter). Sample-counted like
/// everything else, so a paused stream (no blocks) emits nothing.
pub struct LevelTap {
    cb: Box<dyn Fn(&[f32], &[f32]) + Send>,
    mic_buf: Vec<f32>,
    loop_buf: Vec<f32>,
}

/// ~100 ms at 16 kHz → 10 slices/second.
const LEVEL_SLICE_SAMPLES: usize = SAMPLE_RATE as usize / 10;

impl LevelTap {
    pub fn new(cb: Box<dyn Fn(&[f32], &[f32]) + Send>) -> Self {
        Self {
            cb,
            mic_buf: Vec::new(),
            loop_buf: Vec::new(),
        }
    }

    /// Consume one mic block and the loopback samples mixed against it
    /// (`lb` may be shorter — the missing tail is silence).
    pub fn push_block(&mut self, mic: &[f32], lb: &[f32]) {
        self.mic_buf.extend_from_slice(mic);
        self.loop_buf.extend_from_slice(lb);
        // Keep the two channels sample-aligned.
        self.loop_buf.resize(self.mic_buf.len(), 0.0);

        while self.mic_buf.len() >= LEVEL_SLICE_SAMPLES {
            let mic_slice: Vec<f32> = self.mic_buf.drain(..LEVEL_SLICE_SAMPLES).collect();
            let loop_slice: Vec<f32> = self.loop_buf.drain(..LEVEL_SLICE_SAMPLES).collect();
            let freqs = band_frequencies();
            let mic_bands: Vec<f32> =
                freqs.iter().map(|&f| goertzel_magnitude(&mic_slice, f)).collect();
            let loop_bands: Vec<f32> =
                freqs.iter().map(|&f| goertzel_magnitude(&loop_slice, f)).collect();
            (self.cb)(&mic_bands, &loop_bands);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn level_tap_emits_band_spectra_per_slice() {
        let seen: Arc<Mutex<Vec<(Vec<f32>, Vec<f32>)>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = seen.clone();
        let mut tap = LevelTap::new(Box::new(move |m, l| {
            sink.lock().unwrap().push((m.to_vec(), l.to_vec()))
        }));

        // One second: a 440 Hz tone on the mic, silence on the loopback
        // (shorter blocks — ran dry — pad as silence).
        let mic: Vec<f32> = (0..1024)
            .map(|i| (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 16_000.0).sin() * 0.5)
            .collect();
        for _ in 0..16 {
            tap.push_block(&mic, &[]);
        }
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 10, "10 Hz for one second of samples");
        let (mic_bands, loop_bands) = &seen[0];
        assert_eq!(mic_bands.len(), LEVEL_BANDS);
        assert_eq!(loop_bands.len(), LEVEL_BANDS);
        // The loudest mic band sits nearest 440 Hz; the loopback is silent.
        let freqs = band_frequencies();
        let loudest = mic_bands
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(i, _)| i)
            .unwrap();
        assert!(
            (freqs[loudest] - 440.0).abs() < 150.0,
            "loudest band at {} Hz",
            freqs[loudest]
        );
        assert!(loop_bands.iter().all(|&v| v < 1e-6));
    }

    #[test]
    fn level_tap_holds_partial_slices() {
        let count = Arc::new(Mutex::new(0usize));
        let sink = count.clone();
        let mut tap = LevelTap::new(Box::new(move |_, _| *sink.lock().unwrap() += 1));
        tap.push_block(&vec![0.3f32; 1000], &[]);
        assert_eq!(*count.lock().unwrap(), 0, "under one slice → nothing");
        tap.push_block(&vec![0.3f32; 1000], &[]);
        assert_eq!(*count.lock().unwrap(), 1);
    }

    #[test]
    fn windows_are_contiguous_and_sample_timed() {
        let mut m = ChannelMeter::default();
        // 1024-sample blocks (the resampler's real chunk size) for 2 seconds.
        let block = vec![0.5f32; 1024];
        for _ in 0..(32_000 / 1024 + 1) {
            m.push_block(&block, &block);
        }
        let windows = m.finish();
        assert!(windows.len() >= 4);
        assert_eq!(windows[0].start, 0.0);
        for pair in windows.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "no gaps or overlap");
        }
        // Constant 0.5 amplitude → RMS 0.5 on both channels.
        assert!((windows[0].mic_rms - 0.5).abs() < 1e-6);
        assert!((windows[0].loop_rms - 0.5).abs() < 1e-6);
        // Nominal length respected within one block.
        let len = windows[0].end - windows[0].start;
        assert!((0.5..0.6).contains(&len), "window length {len}");
    }

    #[test]
    fn short_loopback_block_reads_as_silence() {
        let mut m = ChannelMeter::default();
        let mic = vec![0.4f32; 8000];
        m.push_block(&mic, &mic[..2000]); // loopback ran dry after a quarter
        let windows = m.finish();
        assert_eq!(windows.len(), 1);
        assert!((windows[0].mic_rms - 0.4).abs() < 1e-6);
        // Quarter of the energy → half the RMS.
        assert!((windows[0].loop_rms - 0.2).abs() < 1e-6);
    }

    #[test]
    fn trailing_partial_window_is_flushed() {
        let mut m = ChannelMeter::default();
        m.push_block(&vec![0.1f32; 3000], &[]);
        let windows = m.finish();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].end, 3000.0 / 16_000.0);
        assert_eq!(windows[0].loop_rms, 0.0);
    }

    #[test]
    fn empty_meter_yields_no_windows() {
        assert!(ChannelMeter::default().finish().is_empty());
    }
}
