//! Push-to-talk: record the microphone with cpal, write a WAV, transcribe it with whisper-cli.

use crate::herdr::Runner;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The capture rate. The MacBook microphone does not offer 16 kHz; whisper-cli resamples itself.
pub const RATE: u32 = 48_000;

/// How long one whisper-cli run may take. The design measured an 18 s cold start for the base
/// model on a 60 s clip; 90 s leaves room for a slower machine without letting a hung run wedge the daemon.
pub const WHISPER_TIMEOUT: Duration = Duration::from_secs(90);

pub struct Recorder { stream: cpal::Stream, buf: Arc<Mutex<Vec<f32>>> }

impl Recorder {
    /// Starts capturing mono f32 from the default input, keeping at most `max_seconds`.
    pub fn start(max_seconds: u64) -> Result<Self, String> {
        let dev = cpal::default_host().default_input_device().ok_or("no input device")?;
        let cfg = cpal::StreamConfig { channels: 1, sample_rate: RATE, buffer_size: cpal::BufferSize::Default };
        let max = max_seconds as usize * RATE as usize;
        // Reserve for a minute at most: max_seconds may be an hour, and a reserve that size per hold is waste.
        let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::with_capacity(max.min(RATE as usize * 60))));
        let sink = buf.clone();
        let stream = dev.build_input_stream(
            cfg,
            move |data: &[f32], _| append_capped(&mut sink.lock().unwrap(), data, max),
            |e| crate::logging::event(format_args!("herd-gamepad: recorder: {e}")),
            None,
        ).map_err(|e| format!("input stream: {e}"))?;
        stream.play().map_err(|e| format!("play: {e}"))?;
        Ok(Recorder { stream, buf })
    }

    pub fn stop(self) -> Vec<f32> {
        let Recorder { stream, buf } = self;
        drop(stream);
        let v = std::mem::take(&mut *buf.lock().unwrap());
        v
    }
}

/// Appends `data` to `buf` without letting it grow past `max` samples.
pub fn append_capped(buf: &mut Vec<f32>, data: &[f32], max: usize) {
    let room = max.saturating_sub(buf.len());
    buf.extend_from_slice(&data[..data.len().min(room)]);
}

pub fn long_enough(samples: usize) -> bool { samples >= RATE as usize * 3 / 10 }

#[cfg(test)]
pub fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    write_wav_cancellable(path, samples, || false)
}

pub fn write_wav_cancellable(path: &Path, samples: &[f32], cancelled: impl Fn() -> bool) -> Result<(), String> {
    if cancelled() { return Err("recording export cancelled".into()); }
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for chunk in samples.chunks(1024) {
        if cancelled() { return Err("recording export cancelled".into()); }
        for s in chunk { w.write_sample((s * i16::MAX as f32) as i16).map_err(|e| e.to_string())?; }
    }
    w.finalize().map_err(|e| e.to_string())
}

pub fn transcribe(r: &impl Runner, model: &Path, wav: &Path, language: &str) -> Result<String, String> {
    let out = r.run(&["whisper-cli", "-m", &model.to_string_lossy(), "-f", &wav.to_string_lossy(), "-nt", "-np", "-l", language])?;
    Ok(clean(&out))
}

/// whisper-cli's stdout as one line of words, without bracketed markers such as [BLANK_AUDIO] or (typing).
pub fn clean(raw: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
