//! Microphone capture on a dedicated thread (section 7.2). Captures at the
//! device rate into a pre-allocated buffer, converts to mono, and resamples
//! to 16 kHz 16-bit PCM on stop. Audio is never written to disk (FR-GEM-07).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, SizedSample};
use rubato::{FftFixedIn, Resampler};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

pub const TARGET_RATE: u32 = 16_000;
pub const MAX_SECONDS: u32 = 120;
pub const MIN_MS: u128 = 300;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AudioEvent {
    Level(f32),
    /// The 120 s limit was reached (FR-DIC-02).
    Full,
    DeviceLost,
}

pub struct Recording {
    pub pcm16k: Vec<i16>,
    pub native: Vec<f32>,
    pub native_rate: u32,
    pub duration: Duration,
}

impl Recording {
    pub fn too_short(&self) -> bool {
        self.duration.as_millis() < MIN_MS
    }

    /// 16 kHz mono 16-bit PCM wrapped as WAV (FR-DIC-03).
    pub fn wav(&self) -> Vec<u8> {
        wav_bytes(&self.pcm16k, TARGET_RATE)
    }
}

pub fn wav_bytes(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("No microphone was found")]
    NoDevice,
    #[error("The microphone could not be opened. Windows may be blocking microphone access.")]
    Open(String),
    #[error("The microphone was disconnected")]
    DeviceLost,
    #[error("Not recording")]
    NotRecording,
}

enum Cmd {
    Start {
        device: Option<String>,
        events: UnboundedSender<AudioEvent>,
        reply: oneshot::Sender<Result<(), AudioError>>,
    },
    Stop {
        reply: oneshot::Sender<Result<Recording, AudioError>>,
    },
}

#[derive(Clone)]
pub struct Recorder {
    tx: std_mpsc::Sender<Cmd>,
    recording: Arc<AtomicBool>,
}

struct Shared {
    buf: Vec<f32>,
    cap: usize,
    lost: bool,
    full_sent: bool,
    sum_sq: f32,
    n: usize,
}

impl Recorder {
    pub fn spawn() -> Recorder {
        let (tx, rx) = std_mpsc::channel::<Cmd>();
        let recording = Arc::new(AtomicBool::new(false));
        let flag = recording.clone();
        std::thread::Builder::new()
            .name("wisp-audio".into())
            .spawn(move || audio_thread(rx, flag))
            .expect("spawn audio");
        Recorder { tx, recording }
    }

    pub fn is_recording(&self) -> bool {
        self.recording.load(Ordering::SeqCst)
    }

    pub async fn start(
        &self,
        device: Option<String>,
        events: UnboundedSender<AudioEvent>,
    ) -> Result<(), AudioError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Start {
                device,
                events,
                reply,
            })
            .map_err(|_| AudioError::NotRecording)?;
        rx.await.map_err(|_| AudioError::NotRecording)?
    }

    pub async fn stop(&self) -> Result<Recording, AudioError> {
        let (reply, rx) = oneshot::channel();
        self.tx
            .send(Cmd::Stop { reply })
            .map_err(|_| AudioError::NotRecording)?;
        rx.await.map_err(|_| AudioError::NotRecording)?
    }
}

fn find_device(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    if let Some(n) = name {
        if let Ok(mut devs) = host.input_devices() {
            #[allow(deprecated)]
            if let Some(d) = devs.find(|d| d.name().map(|x| x == n).unwrap_or(false)) {
                return Some(d);
            }
        }
    }
    host.default_input_device()
}

/// Input device names for Settings (FR-ISL-10).
pub fn input_devices() -> Vec<String> {
    #[allow(deprecated)]
    cpal::default_host()
        .input_devices()
        .map(|d| d.filter_map(|x| x.name().ok()).collect())
        .unwrap_or_default()
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    shared: Arc<Mutex<Shared>>,
    events: UnboundedSender<AudioEvent>,
) -> Result<cpal::Stream, cpal::BuildStreamError>
where
    T: SizedSample + cpal::Sample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels.max(1) as usize;
    let level_every = (config.sample_rate.0 / 30).max(1) as usize;
    let ev2 = events.clone();
    let shared_err = shared.clone();
    device.build_input_stream(
        config,
        move |data: &[T], _| {
            let Ok(mut s) = shared.lock() else { return };
            for frame in data.chunks(channels) {
                let mut acc = 0.0f32;
                for x in frame {
                    acc += <f32 as cpal::FromSample<T>>::from_sample_(*x);
                }
                let m = acc / frame.len() as f32;
                if s.buf.len() < s.cap {
                    s.buf.push(m);
                } else if !s.full_sent {
                    s.full_sent = true;
                    let _ = events.send(AudioEvent::Full);
                }
                s.sum_sq += m * m;
                s.n += 1;
                if s.n >= level_every {
                    let rms = (s.sum_sq / s.n as f32).sqrt();
                    s.sum_sq = 0.0;
                    s.n = 0;
                    let _ = events.send(AudioEvent::Level((rms * 4.0).sqrt().min(1.0)));
                }
            }
        },
        move |err| {
            tracing::warn!(error = %err, "audio stream error");
            if let Ok(mut s) = shared_err.lock() {
                s.lost = true;
            }
            let _ = ev2.send(AudioEvent::DeviceLost);
        },
        None,
    )
}

fn audio_thread(rx: std_mpsc::Receiver<Cmd>, flag: Arc<AtomicBool>) {
    let mut active: Option<(cpal::Stream, Arc<Mutex<Shared>>, u32, Instant)> = None;
    while let Ok(cmd) = rx.recv() {
        match cmd {
            Cmd::Start {
                device,
                events,
                reply,
            } => {
                active = None;
                let res = (|| {
                    let dev = find_device(device.as_deref()).ok_or(AudioError::NoDevice)?;
                    let sup = dev
                        .default_input_config()
                        .map_err(|e| AudioError::Open(e.to_string()))?;
                    let rate = sup.sample_rate().0;
                    let cap = (rate * MAX_SECONDS) as usize;
                    let shared = Arc::new(Mutex::new(Shared {
                        buf: Vec::with_capacity(cap),
                        cap,
                        lost: false,
                        full_sent: false,
                        sum_sq: 0.0,
                        n: 0,
                    }));
                    let cfg: cpal::StreamConfig = sup.config();
                    let stream = match sup.sample_format() {
                        SampleFormat::F32 => {
                            build_stream::<f32>(&dev, &cfg, shared.clone(), events)
                        }
                        SampleFormat::I16 => {
                            build_stream::<i16>(&dev, &cfg, shared.clone(), events)
                        }
                        SampleFormat::U16 => {
                            build_stream::<u16>(&dev, &cfg, shared.clone(), events)
                        }
                        SampleFormat::I32 => {
                            build_stream::<i32>(&dev, &cfg, shared.clone(), events)
                        }
                        SampleFormat::I8 => build_stream::<i8>(&dev, &cfg, shared.clone(), events),
                        SampleFormat::U8 => build_stream::<u8>(&dev, &cfg, shared.clone(), events),
                        SampleFormat::F64 => {
                            build_stream::<f64>(&dev, &cfg, shared.clone(), events)
                        }
                        other => {
                            return Err(AudioError::Open(format!(
                                "unsupported sample format {other:?}"
                            )))
                        }
                    }
                    .map_err(|e| AudioError::Open(e.to_string()))?;
                    stream.play().map_err(|e| AudioError::Open(e.to_string()))?;
                    Ok((stream, shared, rate))
                })();
                match res {
                    Ok((stream, shared, rate)) => {
                        active = Some((stream, shared, rate, Instant::now()));
                        flag.store(true, Ordering::SeqCst);
                        let _ = reply.send(Ok(()));
                    }
                    Err(e) => {
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Cmd::Stop { reply } => {
                flag.store(false, Ordering::SeqCst);
                let Some((stream, shared, rate, started)) = active.take() else {
                    let _ = reply.send(Err(AudioError::NotRecording));
                    continue;
                };
                drop(stream);
                let (native, lost) = {
                    let mut s = shared.lock().unwrap_or_else(|p| p.into_inner());
                    (std::mem::take(&mut s.buf), s.lost)
                };
                if lost {
                    let _ = reply.send(Err(AudioError::DeviceLost));
                    continue;
                }
                let duration = started
                    .elapsed()
                    .min(Duration::from_secs(MAX_SECONDS as u64));
                let pcm16k = to_pcm16k(&native, rate);
                let _ = reply.send(Ok(Recording {
                    pcm16k,
                    native,
                    native_rate: rate,
                    duration,
                }));
            }
        }
    }
}

/// Resamples mono f32 at `rate` to 16 kHz i16.
pub fn to_pcm16k(native: &[f32], rate: u32) -> Vec<i16> {
    let to_i16 = |x: f32| (x.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
    if native.is_empty() {
        return vec![];
    }
    if rate == TARGET_RATE {
        return native.iter().map(|x| to_i16(*x)).collect();
    }
    let chunk = 1024;
    let Ok(mut rs) = FftFixedIn::<f32>::new(rate as usize, TARGET_RATE as usize, chunk, 2, 1)
    else {
        return linear_resample(native, rate, TARGET_RATE)
            .into_iter()
            .map(to_i16)
            .collect();
    };
    let mut out: Vec<f32> =
        Vec::with_capacity(native.len() * TARGET_RATE as usize / rate as usize + chunk);
    let mut pos = 0;
    while native.len() - pos >= rs.input_frames_next() {
        let n = rs.input_frames_next();
        match rs.process(&[&native[pos..pos + n]], None) {
            Ok(o) => out.extend_from_slice(&o[0]),
            Err(_) => break,
        }
        pos += n;
    }
    if pos < native.len() {
        if let Ok(o) = rs.process_partial(Some(&[&native[pos..]]), None) {
            out.extend_from_slice(&o[0]);
        }
    }
    if let Ok(o) = rs.process_partial::<&[f32]>(None, None) {
        out.extend_from_slice(&o[0]);
    }
    // Trim the resampler delay so the length matches the input duration.
    let expected = (native.len() as u64 * TARGET_RATE as u64 / rate as u64) as usize;
    let delay = rs.output_delay();
    let start = delay.min(out.len());
    let end = (start + expected).min(out.len());
    out[start..end].iter().map(|x| to_i16(*x)).collect()
}

pub fn linear_resample(x: &[f32], from: u32, to: u32) -> Vec<f32> {
    if x.is_empty() || from == to {
        return x.to_vec();
    }
    let n = (x.len() as u64 * to as u64 / from as u64) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 * from as f64 / to as f64;
            let j = t.floor() as usize;
            let f = (t - j as f64) as f32;
            let a = x[j.min(x.len() - 1)];
            let b = x[(j + 1).min(x.len() - 1)];
            a + (b - a) * f
        })
        .collect()
}

/// Plays mono samples on the default output device (mic test, FR-SET-06). Blocking.
pub fn play(samples: &[f32], rate: u32) -> anyhow::Result<()> {
    let host = cpal::default_host();
    let dev = host
        .default_output_device()
        .ok_or_else(|| anyhow::anyhow!("no speaker found"))?;
    let sup = dev.default_output_config()?;
    let out_rate = sup.sample_rate().0;
    let channels = sup.channels() as usize;
    let data = Arc::new(linear_resample(samples, rate, out_rate));
    let len = data.len();
    let pos = Arc::new(Mutex::new(0usize));
    let cfg: cpal::StreamConfig = sup.config();
    let (d2, p2) = (data.clone(), pos.clone());
    let stream = match sup.sample_format() {
        SampleFormat::F32 => dev.build_output_stream(
            &cfg,
            move |out: &mut [f32], _| {
                let mut p = p2.lock().unwrap();
                for frame in out.chunks_mut(channels) {
                    let v = d2.get(*p).copied().unwrap_or(0.0);
                    *p += 1;
                    for s in frame.iter_mut() {
                        *s = v;
                    }
                }
            },
            |e| tracing::warn!(error = %e, "playback error"),
            None,
        )?,
        SampleFormat::I16 => dev.build_output_stream(
            &cfg,
            move |out: &mut [i16], _| {
                let mut p = p2.lock().unwrap();
                for frame in out.chunks_mut(channels) {
                    let v = d2.get(*p).copied().unwrap_or(0.0);
                    *p += 1;
                    for s in frame.iter_mut() {
                        *s = (v.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
                    }
                }
            },
            |e| tracing::warn!(error = %e, "playback error"),
            None,
        )?,
        other => anyhow::bail!("unsupported output format {other:?}"),
    };
    stream.play()?;
    let secs = len as f64 / out_rate as f64;
    std::thread::sleep(Duration::from_secs_f64(secs + 0.2));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header() {
        let w = wav_bytes(&[0, 1, -1], 16000);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), 16000);
        assert_eq!(u32::from_le_bytes([w[40], w[41], w[42], w[43]]), 6);
        assert_eq!(w.len(), 50);
    }

    #[test]
    fn resample_48k_to_16k_keeps_duration() {
        let one_sec: Vec<f32> = (0..48_000)
            .map(|i| ((i as f32) * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.5)
            .collect();
        let out = to_pcm16k(&one_sec, 48_000);
        assert!((out.len() as i64 - 16_000).abs() <= 16, "got {}", out.len());
        assert!(out.iter().any(|s| s.abs() > 8000));
    }

    #[test]
    fn same_rate_passthrough() {
        let out = to_pcm16k(&[0.5, -0.5], 16_000);
        assert_eq!(out.len(), 2);
    }
}
