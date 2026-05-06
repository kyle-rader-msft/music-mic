use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Internal pipeline format. Everything is f32, stereo, interleaved L,R,L,R...
pub const PIPELINE_SAMPLE_RATE: u32 = 48_000;
pub const PIPELINE_CHANNELS: u16 = 2;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SourceId {
    Mic,
    System,
}

/// Per-source live state shared between the audio engine and the UI.
///
/// Levels are stored as f32 bits in atomics so the audio threads never block
/// the UI and the UI never has to take a lock on the audio path. Gain is also
/// stored as f32 bits so the UI can change it without a mutex.
pub struct SourceState {
    pub id: SourceId,
    pub gain_bits: AtomicU32,
    pub peak_bits: AtomicU32,
    pub rms_bits: AtomicU32,
    pub active: AtomicBool,
}

impl SourceState {
    pub fn new(id: SourceId) -> Self {
        Self {
            id,
            gain_bits: AtomicU32::new(1.0_f32.to_bits()),
            peak_bits: AtomicU32::new(0),
            rms_bits: AtomicU32::new(0),
            active: AtomicBool::new(false),
        }
    }

    pub fn set_gain(&self, gain: f32) {
        self.gain_bits
            .store(gain.max(0.0).to_bits(), Ordering::Relaxed);
    }

    pub fn gain(&self) -> f32 {
        f32::from_bits(self.gain_bits.load(Ordering::Relaxed))
    }

    pub fn peak(&self) -> f32 {
        f32::from_bits(self.peak_bits.load(Ordering::Relaxed))
    }

    pub fn rms(&self) -> f32 {
        f32::from_bits(self.rms_bits.load(Ordering::Relaxed))
    }

    pub fn store_levels(&self, peak: f32, rms: f32) {
        self.peak_bits.store(peak.to_bits(), Ordering::Relaxed);
        self.rms_bits.store(rms.to_bits(), Ordering::Relaxed);
    }
}

/// Bundle of state the UI polls for live readouts.
#[derive(Clone)]
pub struct EngineState {
    pub mic: Arc<SourceState>,
    pub system: Arc<SourceState>,
    pub master: Arc<SourceState>,
    pub underruns: Arc<AtomicU32>,
    /// When true, voice isolation runs on the mic samples before they reach
    /// the mixer. Read on the audio thread; flipped from the UI.
    pub mic_voice_processing: Arc<AtomicBool>,
}

impl EngineState {
    pub fn new() -> Self {
        Self {
            mic: Arc::new(SourceState::new(SourceId::Mic)),
            system: Arc::new(SourceState::new(SourceId::System)),
            master: Arc::new(SourceState::new(SourceId::Mic)), // id unused for master
            underruns: Arc::new(AtomicU32::new(0)),
            // Default ON: this is the whole reason the user asked for the
            // feature — Teams' isolation is off, so ours needs to compensate.
            mic_voice_processing: Arc::new(AtomicBool::new(true)),
        }
    }
}

impl Default for EngineState {
    fn default() -> Self {
        Self::new()
    }
}
