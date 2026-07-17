use anyhow::Result;
use mp3lame_encoder::{Bitrate, Builder, FlushNoGap, MonoPcm, Quality};
use std::path::Path;

pub fn encode_wav_to_mp3(wav_path: &Path, mp3_path: &Path) -> Result<()> {
    let reader = hound::WavReader::open(wav_path)?;
    let spec = reader.spec();

    let samples: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .collect::<std::result::Result<_, _>>()?,
        hound::SampleFormat::Int => reader
            .into_samples::<i32>()
            .map(|s| s.map(|v| v as f32 / i32::MAX as f32))
            .collect::<std::result::Result<_, _>>()?,
    };

    encode_samples_to_mp3(&samples, spec.sample_rate, mp3_path)
}

pub fn encode_samples_to_mp3(samples: &[f32], _sample_rate: u32, mp3_path: &Path) -> Result<()> {
    let i16_samples: Vec<i16> = samples
        .iter()
        .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
        .collect();

    let mut encoder = Builder::new()
        .expect("Create LAME builder")
        .with_num_channels(1)
        .expect("set channels")
        .with_sample_rate(16000)
        .expect("set sample rate")
        .with_brate(Bitrate::Kbps64)
        .expect("set bitrate")
        .with_quality(Quality::Best)
        .expect("set quality")
        .build()
        .expect("build encoder");

    let input = MonoPcm(i16_samples.as_slice());
    let mut mp3_buf = Vec::new();
    mp3_buf.reserve(mp3lame_encoder::max_required_buffer_size(i16_samples.len()));

    let encoded_size = encoder
        .encode(input, mp3_buf.spare_capacity_mut())
        .expect("encode");
    unsafe {
        mp3_buf.set_len(mp3_buf.len().wrapping_add(encoded_size));
    }

    let flush_size = encoder
        .flush::<FlushNoGap>(mp3_buf.spare_capacity_mut())
        .expect("flush");
    unsafe {
        mp3_buf.set_len(mp3_buf.len().wrapping_add(flush_size));
    }

    std::fs::write(mp3_path, &mp3_buf)?;
    Ok(())
}
