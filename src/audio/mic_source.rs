//! Microphone source via cpal.
//!
//! Opens the chosen input device with its default config, converts whatever
//! native format/rate/layout to f32 stereo @ 48 kHz interleaved, optionally
//! runs voice isolation, and pushes the result into the mic ring.
//!
//! All scratch buffers are owned by `MicState` and reused — there are no
//! per-callback heap allocations after warmup.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream, StreamConfig};
use rtrb::Producer;
use rubato::{FftFixedInOut, Resampler};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info, warn};

use super::ring::push_or_drop;
use super::types::{PIPELINE_CHANNELS, PIPELINE_SAMPLE_RATE};
use super::voice_processing::VoiceProcessor;

pub struct MicSource {
    _stream: Stream,
}

pub fn start(
    device_name: &str,
    producer: Producer<f32>,
    voice_processing: Arc<AtomicBool>,
) -> Result<MicSource> {
    let host = cpal::default_host();
    let device = host
        .input_devices()
        .context("enumerating cpal input devices")?
        .find(|d| {
            d.description()
                .map(|desc| desc.name() == device_name)
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("input device {device_name:?} not found"))?;

    let supported_cfg = device
        .default_input_config()
        .context("getting default input config")?;
    let sample_format = supported_cfg.sample_format();
    let cfg: StreamConfig = supported_cfg.into();
    let in_channels = cfg.channels as usize;
    let in_rate = cfg.sample_rate;

    info!(
        device = device_name,
        sample_format = ?sample_format,
        in_rate,
        in_channels,
        "opening mic stream"
    );

    let mut state = MicState::new(producer, in_rate, in_channels, voice_processing)?;

    let err_fn = |e| warn!("mic stream error: {e}");
    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &cfg,
            move |data: &[f32], _| state.push_f32(data),
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &cfg,
            move |data: &[i16], _| state.push_int(data),
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &cfg,
            move |data: &[u16], _| state.push_int(data),
            err_fn,
            None,
        ),
        other => {
            return Err(anyhow!(
                "unsupported mic sample format: {other:?} (only f32/i16/u16 are wired up)"
            ));
        }
    }
    .context("building input stream")?;

    stream.play().context("starting input stream")?;

    Ok(MicSource { _stream: stream })
}

/// All state and scratch buffers for the mic callback. Sized so that the
/// callback never needs to allocate after warmup.
struct MicState {
    producer: Producer<f32>,
    in_channels: usize,

    resampler: Option<FftFixedInOut<f32>>,
    /// Per-channel input chunk supplied to the resampler. One Vec per
    /// channel, sized to `input_frames_max()`.
    resampler_in: Vec<Vec<f32>>,
    /// Per-channel output buffer the resampler writes into.
    resampler_out: Vec<Vec<f32>>,
    /// Carry buffers for samples received but not yet handed to the
    /// resampler — one Vec per channel.
    carry_l: Vec<f32>,
    carry_r: Vec<f32>,

    /// Reusable interleaved scratch we push into the ring.
    interleaved: Vec<f32>,
    /// f32 scratch for i16/u16 → f32 conversion.
    format_scratch: Vec<f32>,

    /// Optional voice isolation step. Always present; gated at runtime by the
    /// shared `vp_enabled` atomic.
    voice_processor: VoiceProcessor,
    vp_enabled: Arc<AtomicBool>,
}

impl MicState {
    fn new(
        producer: Producer<f32>,
        in_rate: u32,
        in_channels: usize,
        vp_enabled: Arc<AtomicBool>,
    ) -> Result<Self> {
        // 1024-frame FFT chunks ≈ 21 ms @ 48 kHz — a sane resampler block.
        let chunk_size = 1024;
        let resampler = if in_rate == PIPELINE_SAMPLE_RATE {
            None
        } else {
            Some(
                FftFixedInOut::<f32>::new(
                    in_rate as usize,
                    PIPELINE_SAMPLE_RATE as usize,
                    chunk_size,
                    PIPELINE_CHANNELS as usize,
                )
                .context("creating mic resampler")?,
            )
        };

        let nchan = PIPELINE_CHANNELS as usize;
        let (resampler_in, resampler_out, carry_cap) = if let Some(r) = &resampler {
            (
                (0..nchan).map(|_| vec![0.0; r.input_frames_max()]).collect(),
                (0..nchan).map(|_| vec![0.0; r.output_frames_max()]).collect(),
                r.input_frames_max() * 2,
            )
        } else {
            (Vec::new(), Vec::new(), 0)
        };

        Ok(Self {
            producer,
            in_channels,
            resampler,
            resampler_in,
            resampler_out,
            carry_l: Vec::with_capacity(carry_cap),
            carry_r: Vec::with_capacity(carry_cap),
            // Pre-size the interleave / format scratch for a comfortable
            // upper bound. Resize is no-op once capacity stabilizes.
            interleaved: vec![0.0; 4096 * nchan],
            format_scratch: vec![0.0; 4096 * 4],
            voice_processor: VoiceProcessor::new(),
            vp_enabled,
        })
    }

    /// Push a block of native-format integer samples by routing through
    /// `format_scratch` (so the conversion doesn't allocate).
    fn push_int<T: Sample + cpal::SizedSample>(&mut self, data: &[T])
    where
        T: cpal::Sample,
        T: dasp_sample::ToSample<f32>,
    {
        if data.len() > self.format_scratch.len() {
            self.format_scratch.resize(data.len(), 0.0);
        }
        for (i, s) in data.iter().enumerate() {
            self.format_scratch[i] = s.to_sample::<f32>();
        }
        // Borrow the scratch read-only for push_f32. Take a copy of the len
        // up front so we don't keep a borrow across the &mut self call.
        let n = data.len();
        let scratch_ptr = self.format_scratch.as_ptr();
        // SAFETY: scratch_ptr is valid for the next n elements; push_f32
        // only mutates other fields of `self`, never the format_scratch
        // backing buffer.
        let view = unsafe { std::slice::from_raw_parts(scratch_ptr, n) };
        self.push_f32(view);
    }

    /// Push a block of interleaved f32 samples (in_channels per frame).
    fn push_f32(&mut self, data: &[f32]) {
        if data.is_empty() {
            return;
        }
        let frames = data.len() / self.in_channels;
        if frames == 0 {
            return;
        }

        if self.resampler.is_some() {
            // Resampling path: append to per-channel carry, then drain as many
            // full blocks as we have.
            self.carry_l.reserve(frames);
            self.carry_r.reserve(frames);
            let nc = self.in_channels;
            for f in 0..frames {
                let base = f * nc;
                let l = data[base];
                let r = if nc >= 2 { data[base + 1] } else { l };
                self.carry_l.push(l);
                self.carry_r.push(r);
            }
            self.drain_resampler();
        } else {
            // Pass-through path: write direct into the interleaved scratch
            // and push.
            let nc = self.in_channels;
            let need = frames * 2;
            if self.interleaved.len() < need {
                self.interleaved.resize(need, 0.0);
            }
            for f in 0..frames {
                let base = f * nc;
                let l = data[base];
                let r = if nc >= 2 { data[base + 1] } else { l };
                self.interleaved[2 * f] = l;
                self.interleaved[2 * f + 1] = r;
            }
            let pushed = self.apply_voice_processing(need);
            let view = &self.interleaved[..pushed];
            if !view.is_empty() {
                let n = push_or_drop(&mut self.producer, view);
                if n < view.len() {
                    debug!("mic ring overrun: dropped {}", view.len() - n);
                }
            }
        }
    }

    fn drain_resampler(&mut self) {
        loop {
            // Read needed-frames each iteration so we don't hold a &mut to the
            // resampler across the apply_voice_processing call below.
            let needed = match self.resampler.as_ref() {
                Some(r) => r.input_frames_next(),
                None => return,
            };
            if self.carry_l.len() < needed {
                return;
            }

            self.resampler_in[0][..needed].copy_from_slice(&self.carry_l[..needed]);
            self.resampler_in[1][..needed].copy_from_slice(&self.carry_r[..needed]);
            self.carry_l.drain(0..needed);
            self.carry_r.drain(0..needed);

            // Scoped resampler borrow — released before we call &mut-self methods.
            let out_n = {
                let resampler = self.resampler.as_mut().expect("present");
                let in_refs: [&[f32]; 2] = [
                    &self.resampler_in[0][..needed],
                    &self.resampler_in[1][..needed],
                ];
                match resampler.process_into_buffer(
                    &in_refs,
                    self.resampler_out.as_mut_slice(),
                    None,
                ) {
                    Ok((_, out_frames)) => out_frames,
                    Err(e) => {
                        warn!("rubato error: {e}");
                        return;
                    }
                }
            };

            let need = out_n * 2;
            if self.interleaved.len() < need {
                self.interleaved.resize(need, 0.0);
            }
            for f in 0..out_n {
                self.interleaved[2 * f] = self.resampler_out[0][f];
                self.interleaved[2 * f + 1] = self.resampler_out[1][f];
            }
            let pushed = self.apply_voice_processing(need);
            let view = &self.interleaved[..pushed];
            if !view.is_empty() {
                let n = push_or_drop(&mut self.producer, view);
                if n < view.len() {
                    debug!("mic ring overrun: dropped {}", view.len() - n);
                }
            }
        }
    }

    /// Run voice isolation in place over `interleaved[..len]` if enabled.
    /// Returns the number of samples now valid in `interleaved` — usually
    /// equal to `len`, but may be smaller during the first call after enable
    /// while the denoiser buffers up its first frame.
    fn apply_voice_processing(&mut self, len: usize) -> usize {
        if !self.vp_enabled.load(Ordering::Relaxed) {
            // Reset on the next enable so we don't replay stale samples.
            self.voice_processor.reset();
            return len;
        }
        self.voice_processor.process(&mut self.interleaved[..len])
    }
}
