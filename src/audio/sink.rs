//! Output sink to BlackHole 2ch.
//!
//! Opens the BlackHole 2ch output device with a stereo f32 stream, and in
//! the cpal output callback it:
//!   1. pops as many frames as fit from the mic + system source rings,
//!   2. applies per-source gain,
//!   3. sums them into the output buffer,
//!   4. updates per-source + master peak/RMS atomics for the UI.
//!
//! The callback is the single consumer for both source rings (rtrb is SPSC),
//! so this is correct lock-free.

use anyhow::{Context, Result, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use rtrb::Consumer;
use std::sync::atomic::AtomicU32;
use tracing::{info, warn};

use super::types::{EngineState, PIPELINE_CHANNELS, PIPELINE_SAMPLE_RATE, SourceState};
use std::sync::Arc;

pub struct Sink {
    _stream: Stream,
}

pub fn start(
    device_name: &str,
    mic: Consumer<f32>,
    system: Consumer<f32>,
    state: EngineState,
) -> Result<Sink> {
    let host = cpal::default_host();
    let device = host
        .output_devices()
        .context("enumerating cpal output devices")?
        .find(|d| {
            d.description()
                .map(|desc| desc.name() == device_name)
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow!("output device {device_name:?} not found"))?;

    // Pin the output to 48 kHz / stereo / f32 — that's our pipeline format,
    // and BlackHole 2ch is happy with it.
    let cfg = StreamConfig {
        channels: PIPELINE_CHANNELS,
        sample_rate: PIPELINE_SAMPLE_RATE,
        buffer_size: cpal::BufferSize::Default,
    };

    info!(device = device_name, ?cfg, "opening output stream");

    let mut callback = OutputCallback {
        mic,
        system,
        mic_state: state.mic.clone(),
        system_state: state.system.clone(),
        master_state: state.master.clone(),
        underruns: state.underruns.clone(),
        scratch_mic: Vec::new(),
        scratch_sys: Vec::new(),
    };

    let err_fn = |e| warn!("output stream error: {e}");
    let stream = device
        .build_output_stream(
            &cfg,
            move |out: &mut [f32], _info| callback.fill(out),
            err_fn,
            None,
        )
        .context("building output stream")?;
    stream.play().context("starting output stream")?;
    let _ = SampleFormat::F32; // cargo accidentally unused if I drop the import later
    Ok(Sink { _stream: stream })
}

struct OutputCallback {
    mic: Consumer<f32>,
    system: Consumer<f32>,
    mic_state: Arc<SourceState>,
    system_state: Arc<SourceState>,
    master_state: Arc<SourceState>,
    underruns: Arc<AtomicU32>,
    scratch_mic: Vec<f32>,
    scratch_sys: Vec<f32>,
}

impl OutputCallback {
    fn fill(&mut self, out: &mut [f32]) {
        let n = out.len();
        if n == 0 {
            return;
        }

        // Resize scratch buffers to match request.
        if self.scratch_mic.len() != n {
            self.scratch_mic.resize(n, 0.0);
            self.scratch_sys.resize(n, 0.0);
        } else {
            self.scratch_mic.fill(0.0);
            self.scratch_sys.fill(0.0);
        }

        // Pull as many samples as available from each ring; pad rest with 0.
        let mic_avail = self.mic.slots();
        let sys_avail = self.system.slots();
        let mic_take = mic_avail.min(n);
        let sys_take = sys_avail.min(n);

        if mic_take == 0 && sys_take == 0 {
            self.underruns
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }

        if mic_take > 0 {
            if let Ok(chunk) = self.mic.read_chunk(mic_take) {
                let (a, b) = chunk.as_slices();
                self.scratch_mic[..a.len()].copy_from_slice(a);
                self.scratch_mic[a.len()..a.len() + b.len()].copy_from_slice(b);
                chunk.commit_all();
            }
        }
        if sys_take > 0 {
            if let Ok(chunk) = self.system.read_chunk(sys_take) {
                let (a, b) = chunk.as_slices();
                self.scratch_sys[..a.len()].copy_from_slice(a);
                self.scratch_sys[a.len()..a.len() + b.len()].copy_from_slice(b);
                chunk.commit_all();
            }
        }

        // Apply gains, sum, soft-clip, and write to out. Track peaks/RMS.
        let mic_gain = self.mic_state.gain();
        let sys_gain = self.system_state.gain();
        let mut mic_peak = 0.0_f32;
        let mut mic_sumsq = 0.0_f64;
        let mut sys_peak = 0.0_f32;
        let mut sys_sumsq = 0.0_f64;
        let mut mas_peak = 0.0_f32;
        let mut mas_sumsq = 0.0_f64;

        for i in 0..n {
            let m = self.scratch_mic[i] * mic_gain;
            let s = self.scratch_sys[i] * sys_gain;
            let mixed = soft_clip(m + s);
            out[i] = mixed;

            let am = m.abs();
            let as_ = s.abs();
            let amx = mixed.abs();
            if am > mic_peak {
                mic_peak = am;
            }
            if as_ > sys_peak {
                sys_peak = as_;
            }
            if amx > mas_peak {
                mas_peak = amx;
            }
            mic_sumsq += (m as f64) * (m as f64);
            sys_sumsq += (s as f64) * (s as f64);
            mas_sumsq += (mixed as f64) * (mixed as f64);
        }

        let denom = n as f64;
        self.mic_state
            .store_levels(mic_peak, (mic_sumsq / denom).sqrt() as f32);
        self.system_state
            .store_levels(sys_peak, (sys_sumsq / denom).sqrt() as f32);
        self.master_state
            .store_levels(mas_peak, (mas_sumsq / denom).sqrt() as f32);
    }
}

/// Cheap soft-clip to keep the mix from hard-clipping when both sources are
/// hot. tanh is well-behaved and effectively transparent below ~0.5.
#[inline]
fn soft_clip(x: f32) -> f32 {
    // Approximate tanh — fast and good enough at audio rates.
    let x2 = x * x;
    let num = x * (27.0 + x2);
    let den = 27.0 + 9.0 * x2;
    (num / den).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::soft_clip;

    #[test]
    fn soft_clip_passes_silence_and_quiet_unchanged() {
        assert_eq!(soft_clip(0.0), 0.0);
        // Below ~0.3 the difference vs. identity should be tiny.
        for x in [0.05, 0.1, 0.2, -0.2] {
            let y = soft_clip(x);
            assert!((y - x).abs() < 0.005, "x={x} y={y}");
        }
    }

    #[test]
    fn soft_clip_bounds_extreme_values() {
        for x in [1.5, 2.0, 5.0, 100.0, -1.5, -2.0, -100.0] {
            let y = soft_clip(x);
            assert!(y.abs() <= 1.0, "x={x} y={y} out of [-1,1]");
        }
        // Monotonic at the edges.
        assert!(soft_clip(2.0) > soft_clip(1.0));
        assert!(soft_clip(-2.0) < soft_clip(-1.0));
    }
}
