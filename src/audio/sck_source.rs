//! ScreenCaptureKit-based system audio source.
//!
//! Captures audio for a single application (filtered by bundle id) and pushes
//! f32 stereo interleaved L,R,L,R, ... samples at 48 kHz into the system
//! ring buffer.
//!
//! The audio handler runs on a Swift dispatch queue and we treat it as a
//! real-time path: no per-callback heap allocations once warmed up. The
//! interleave scratch lives behind a Mutex alongside the producer so the
//! callback grabs both with a single uncontended lock.

use anyhow::{Result, anyhow};
use rtrb::Producer;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, warn};

use super::ring::push_or_drop;
use super::types::{PIPELINE_CHANNELS, PIPELINE_SAMPLE_RATE};

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use parking_lot::Mutex;
    use screencapturekit::prelude::*;

    pub struct SckSource {
        stream: Option<SCStream>,
        // Hold strong refs so the handler keeps living while the stream runs.
        _inner: Arc<Mutex<HandlerInner>>,
        shutting_down: Arc<AtomicBool>,
    }

    struct HandlerInner {
        producer: Producer<f32>,
        // Reusable interleave scratch — grown on first call, never shrunk.
        scratch: Vec<f32>,
    }

    struct AudioHandler {
        inner: Arc<Mutex<HandlerInner>>,
        shutting_down: Arc<AtomicBool>,
    }

    impl SCStreamOutputTrait for AudioHandler {
        fn did_output_sample_buffer(&self, sample: CMSampleBuffer, of_type: SCStreamOutputType) {
            if of_type != SCStreamOutputType::Audio {
                return;
            }
            if self.shutting_down.load(Ordering::Relaxed) {
                return;
            }

            let Some(buffer_list) = sample.audio_buffer_list() else {
                return;
            };

            let frame_count = sample.num_samples();
            if frame_count == 0 {
                return;
            }
            let num_buffers = buffer_list.num_buffers();
            if num_buffers == 0 {
                return;
            }

            // ScreenCaptureKit emits Linear PCM Float32. With channel_count(2)
            // the typical layout is non-interleaved: two buffers, one per
            // channel. Older paths or mono sources may give us interleaved
            // (one buffer, number_channels >= 2) or single-channel (one
            // buffer, number_channels == 1) — handle all three.
            let mut inner = self.inner.lock();
            let HandlerInner { producer, scratch } = &mut *inner;
            ensure_capacity(scratch, frame_count * 2);

            let written_frames = if num_buffers == 1 {
                let Some(buf) = buffer_list.get(0) else { return };
                let nc = buf.number_channels.max(1);
                let samples = bytes_as_f32(buf.data());
                if nc == 1 {
                    fill_mono(&mut scratch[..], samples, frame_count)
                } else {
                    fill_interleaved(&mut scratch[..], samples, nc as usize, frame_count)
                }
            } else {
                let l_buf = buffer_list.get(0);
                let r_buf = buffer_list.get(1);
                if let (Some(l), Some(r)) = (l_buf, r_buf) {
                    let l_samples = bytes_as_f32(l.data());
                    let r_samples = bytes_as_f32(r.data());
                    fill_planar(&mut scratch[..], l_samples, r_samples, frame_count)
                } else {
                    0
                }
            };

            // Only push the samples we actually wrote — pushing the full
            // scratch length when the channel branch wrote less would inject
            // a periodic pulse of silence into the mix.
            let valid = &scratch[..written_frames * 2];
            if valid.is_empty() {
                return;
            }
            let written = push_or_drop(producer, valid);
            if written < valid.len() {
                debug!(
                    "system audio ring overrun: dropped {} of {} samples",
                    valid.len() - written,
                    valid.len()
                );
            }
        }
    }

    /// Resize a scratch buffer to at least `n` samples without ever shrinking.
    /// After warmup the capacity stabilizes and there are zero allocations.
    #[inline]
    fn ensure_capacity(buf: &mut Vec<f32>, n: usize) {
        if buf.len() < n {
            buf.resize(n, 0.0);
        }
    }

    /// Returns the number of stereo frames written.
    #[inline]
    fn fill_mono(out: &mut [f32], src: &[f32], frame_count: usize) -> usize {
        let n = src.len().min(frame_count);
        for i in 0..n {
            let s = src[i];
            out[2 * i] = s;
            out[2 * i + 1] = s;
        }
        n
    }

    #[inline]
    fn fill_interleaved(out: &mut [f32], src: &[f32], nc: usize, frame_count: usize) -> usize {
        let usable = src.len() / nc.max(1);
        let n = usable.min(frame_count);
        for i in 0..n {
            let base = i * nc;
            let l = src[base];
            let r = if nc >= 2 { src[base + 1] } else { l };
            out[2 * i] = l;
            out[2 * i + 1] = r;
        }
        n
    }

    #[inline]
    fn fill_planar(out: &mut [f32], l: &[f32], r: &[f32], frame_count: usize) -> usize {
        let n = l.len().min(r.len()).min(frame_count);
        for i in 0..n {
            out[2 * i] = l[i];
            out[2 * i + 1] = r[i];
        }
        n
    }

    #[allow(clippy::cast_ptr_alignment)]
    fn bytes_as_f32(b: &[u8]) -> &[f32] {
        // SCK guarantees 4-byte alignment for its float buffers; sanity check
        // is debug-only.
        debug_assert!(b.as_ptr().align_offset(std::mem::align_of::<f32>()) == 0);
        let len = b.len() / std::mem::size_of::<f32>();
        unsafe { std::slice::from_raw_parts(b.as_ptr().cast::<f32>(), len) }
    }

    pub fn start(bundle_id: &str, producer: Producer<f32>) -> Result<SckSource> {
        let content = SCShareableContent::get()
            .map_err(|e| anyhow!("SCShareableContent::get failed: {e:?}"))?;

        let apps = content.applications();
        let app = apps
            .iter()
            .find(|a| a.bundle_identifier() == bundle_id)
            .ok_or_else(|| anyhow!("application bundle id {bundle_id:?} not found"))?;
        let display = content
            .displays()
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("no displays available"))?;

        let filter = SCContentFilter::create()
            .with_display(&display)
            .with_including_applications(&[app], &[])
            .build();

        let config = SCStreamConfiguration::new()
            .with_captures_audio(true)
            .with_sample_rate(PIPELINE_SAMPLE_RATE as i32)
            .with_channel_count(PIPELINE_CHANNELS as i32);

        // Pre-size the scratch for a comfortable upper bound — SCK typically
        // hands us ~480-frame buffers, but headroom avoids first-call growth.
        let inner = Arc::new(Mutex::new(HandlerInner {
            producer,
            scratch: vec![0.0; 4096 * PIPELINE_CHANNELS as usize],
        }));
        let shutting_down = Arc::new(AtomicBool::new(false));

        let handler = AudioHandler {
            inner: inner.clone(),
            shutting_down: shutting_down.clone(),
        };

        let mut stream = SCStream::new(&filter, &config);
        stream.add_output_handler(handler, SCStreamOutputType::Audio);
        stream
            .start_capture()
            .map_err(|e| anyhow!("SCStream::start_capture failed: {e:?}"))?;

        Ok(SckSource {
            stream: Some(stream),
            _inner: inner,
            shutting_down,
        })
    }

    impl Drop for SckSource {
        fn drop(&mut self) {
            self.shutting_down.store(true, Ordering::Relaxed);
            if let Some(stream) = self.stream.take() {
                if let Err(e) = stream.stop_capture() {
                    warn!("SCStream::stop_capture failed: {e:?}");
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::{SckSource, start};

#[cfg(not(target_os = "macos"))]
pub struct SckSource;

#[cfg(not(target_os = "macos"))]
pub fn start(_bundle_id: &str, _producer: Producer<f32>) -> Result<SckSource> {
    Err(anyhow!("system audio capture is only supported on macOS"))
}

// Avoid unused-import errors on non-macos builds.
#[cfg(not(target_os = "macos"))]
#[allow(dead_code)]
fn _silence_warns() {
    let _ = (PIPELINE_CHANNELS, PIPELINE_SAMPLE_RATE);
}
