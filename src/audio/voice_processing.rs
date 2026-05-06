//! Voice isolation (RNNoise) applied to the mic input *before* mixing.
//!
//! Why: when we mix the mic with system audio and feed the sum to Teams,
//! Teams' own voice isolation will treat the music as background noise and
//! attenuate it. Disabling Teams' voice isolation preserves the music but
//! also removes the cleanup it would have applied to the mic. Doing the
//! cleanup ourselves on the mic-only signal sidesteps the trade-off.
//!
//! `nnnoiseless` is a pure-Rust port of Xiph's RNNoise. It expects:
//! - 48 kHz, mono, f32 (we run two instances for L/R)
//! - Samples in the ±32768 range (16-bit PCM as float — RNNoise convention)
//! - 480-sample frames (10 ms)
//!
//! Internal latency is one frame (≈ 10 ms). The processor buffers samples
//! that don't fill a frame, so [`process`] may return fewer samples than it
//! consumed during warmup.

use nnnoiseless::DenoiseState;

const FRAME: usize = 480;

/// Working values for the input-side carry / output-side queue. Sized so
/// that warmup pushes never reallocate.
const QUEUE_CAP: usize = FRAME * 8; // 80 ms — well above any sane callback size

pub struct VoiceProcessor {
    denoise_l: Box<DenoiseState<'static>>,
    denoise_r: Box<DenoiseState<'static>>,

    // Per-channel input carry — samples accumulated until we have a full FRAME.
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    // Per-channel output queue — denoised samples awaiting drain.
    out_l: Vec<f32>,
    out_r: Vec<f32>,

    // Reusable per-frame scratch.
    frame_in: [f32; FRAME],
    frame_out: [f32; FRAME],
}

impl VoiceProcessor {
    pub fn new() -> Self {
        Self {
            denoise_l: DenoiseState::new(),
            denoise_r: DenoiseState::new(),
            in_l: Vec::with_capacity(QUEUE_CAP),
            in_r: Vec::with_capacity(QUEUE_CAP),
            out_l: Vec::with_capacity(QUEUE_CAP),
            out_r: Vec::with_capacity(QUEUE_CAP),
            frame_in: [0.0; FRAME],
            frame_out: [0.0; FRAME],
        }
    }

    /// Denoise interleaved stereo `inout` in place.
    ///
    /// The slice is treated as input on entry and overwritten with denoised
    /// output on exit. Returns the number of *interleaved samples* (i.e.
    /// frames × 2) that were filled. The first call after construction will
    /// typically return fewer samples than the slice length because the
    /// processor needs FRAME samples queued before it can emit anything;
    /// subsequent calls converge to input-len-out = input-len-in.
    pub fn process(&mut self, inout: &mut [f32]) -> usize {
        let frames = inout.len() / 2;
        if frames == 0 {
            return 0;
        }

        // Append input (scaled to RNNoise's ±32768 convention) to per-channel
        // carry. This is the only place input samples enter the processor.
        self.in_l.reserve(frames);
        self.in_r.reserve(frames);
        for f in 0..frames {
            self.in_l.push(inout[2 * f] * 32768.0);
            self.in_r.push(inout[2 * f + 1] * 32768.0);
        }

        // Process every full FRAME we can.
        while self.in_l.len() >= FRAME && self.in_r.len() >= FRAME {
            self.frame_in.copy_from_slice(&self.in_l[..FRAME]);
            self.denoise_l
                .process_frame(&mut self.frame_out, &self.frame_in);
            self.out_l.extend_from_slice(&self.frame_out);
            self.in_l.drain(0..FRAME);

            self.frame_in.copy_from_slice(&self.in_r[..FRAME]);
            self.denoise_r
                .process_frame(&mut self.frame_out, &self.frame_in);
            self.out_r.extend_from_slice(&self.frame_out);
            self.in_r.drain(0..FRAME);
        }

        // Drain as many denoised frames as the caller has space for, scaling
        // back to ±1.0.
        let avail = self.out_l.len().min(self.out_r.len()).min(frames);
        for f in 0..avail {
            inout[2 * f] = self.out_l[f] * (1.0 / 32768.0);
            inout[2 * f + 1] = self.out_r[f] * (1.0 / 32768.0);
        }
        self.out_l.drain(0..avail);
        self.out_r.drain(0..avail);

        avail * 2
    }

    /// Drop everything buffered. Call when toggling off after on, so when the
    /// user toggles back on we don't replay stale samples.
    pub fn reset(&mut self) {
        self.in_l.clear();
        self.in_r.clear();
        self.out_l.clear();
        self.out_r.clear();
    }
}

impl Default for VoiceProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_in_silence_out_after_warmup() {
        let mut vp = VoiceProcessor::new();
        let mut buf = vec![0.0_f32; FRAME * 2 * 4]; // 4 frames of stereo silence
        let _ = vp.process(&mut buf);
        // After warmup a follow-up call should return all the samples we passed.
        let mut buf2 = vec![0.0_f32; FRAME * 2 * 2];
        let n = vp.process(&mut buf2);
        assert_eq!(n, buf2.len());
        // RNNoise on silence should still be ~silence.
        for s in buf2.iter() {
            assert!(s.abs() < 0.01, "non-silent output on silent input: {s}");
        }
    }
}
