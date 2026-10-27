//! Push-to-talk: record the microphone with cpal, write a WAV, transcribe it with whisper-cli.

use crate::herdr::Runner;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The capture rate. The MacBook microphone does not offer 16 kHz; whisper-cli resamples itself.
pub const RATE: u32 = 48_000;

pub struct Recorder { stream: cpal::Stream, buf: Arc<Mutex<Vec<f32>>> }

impl Recorder {
    /// Starts capturing mono f32 from the default input, keeping at most `max_seconds`.
    pub fn start(max_seconds: u64) -> Result<Self, String> {
        let dev = cpal::default_host().default_input_device().ok_or("no input device")?;
        let cfg = cpal::StreamConfig { channels: 1, sample_rate: RATE, buffer_size: cpal::BufferSize::Default };
        let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let max = max_seconds as usize * RATE as usize;
        let sink = buf.clone();
        let stream = dev.build_input_stream(
            cfg,
            move |data: &[f32], _| {
                let mut v = sink.lock().unwrap();
                let room = max.saturating_sub(v.len());
                v.extend_from_slice(&data[..data.len().min(room)]);
            },
            |e| eprintln!("herd-gamepad: recorder: {e}"),
            None,
        ).map_err(|e| format!("input stream: {e}"))?;
        stream.play().map_err(|e| format!("play: {e}"))?;
        Ok(Recorder { stream, buf })
    }

    pub fn stop(self) -> Vec<f32> {
        let Recorder { stream, buf } = self;
        drop(stream);
        let v = buf.lock().unwrap().clone();
        v
    }
}

pub fn long_enough(samples: usize) -> bool { samples >= RATE as usize * 3 / 10 }

pub fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).map_err(|e| e.to_string())?;
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
