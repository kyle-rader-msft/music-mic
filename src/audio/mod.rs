pub mod devices;
pub mod mic_source;
pub mod ring;
pub mod sck_source;
pub mod sink;
pub mod types;
pub mod voice_processing;

use crossbeam_channel::{Receiver, Sender, unbounded};
use rtrb::RingBuffer;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tracing::{error, info, warn};

pub use types::{EngineState, PIPELINE_CHANNELS, PIPELINE_SAMPLE_RATE, SourceId, SourceState};

use mic_source::MicSource;
use sck_source::SckSource;
use sink::Sink;

/// Selection of inputs and the BlackHole output.
#[derive(Clone, Debug, Default)]
pub struct EngineSelection {
    pub mic_device_name: Option<String>,
    pub system_app_bundle_id: Option<String>,
    pub output_device_name: Option<String>, // expected: "BlackHole 2ch"
}

#[derive(Debug)]
pub enum EngineCommand {
    Start(EngineSelection),
    Stop,
    SetGain { source: SourceId, gain: f32 },
    Shutdown,
}

pub struct AudioEngine {
    cmd_tx: Sender<EngineCommand>,
    state: EngineState,
    join: Option<JoinHandle<()>>,
}

impl AudioEngine {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = unbounded::<EngineCommand>();
        let state = EngineState::new();
        let state_clone = state.clone();

        let join = thread::Builder::new()
            .name("music-mic-engine".into())
            .spawn(move || run_engine(cmd_rx, state_clone))
            .expect("failed to spawn audio engine thread");

        Self {
            cmd_tx,
            state,
            join: Some(join),
        }
    }

    pub fn state(&self) -> EngineState {
        self.state.clone()
    }

    pub fn send(&self, cmd: EngineCommand) {
        if let Err(e) = self.cmd_tx.send(cmd) {
            warn!("failed to send engine command: {e}");
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(EngineCommand::Shutdown);
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// Holds the live source/sink handles. Dropping this stops everything.
struct LiveStreams {
    _mic: MicSource,
    _system: SckSource,
    _sink: Sink,
}

fn run_engine(cmd_rx: Receiver<EngineCommand>, state: EngineState) {
    info!("audio engine thread started");
    let mut live: Option<LiveStreams> = None;

    loop {
        match cmd_rx.recv_timeout(Duration::from_millis(250)) {
            Ok(EngineCommand::Shutdown) => {
                info!("audio engine shutting down");
                break;
            }
            Ok(EngineCommand::Start(sel)) => {
                if live.is_some() {
                    info!("Start received while running — restarting");
                    live = None;
                }
                match start_streams(&sel, &state) {
                    Ok(streams) => {
                        info!("audio streams started");
                        state.mic.active.store(true, std::sync::atomic::Ordering::Relaxed);
                        state.system.active.store(true, std::sync::atomic::Ordering::Relaxed);
                        live = Some(streams);
                    }
                    Err(e) => {
                        error!("failed to start streams: {e:#}");
                    }
                }
            }
            Ok(EngineCommand::Stop) => {
                info!("Stop received");
                live = None;
                state.mic.active.store(false, std::sync::atomic::Ordering::Relaxed);
                state.system.active.store(false, std::sync::atomic::Ordering::Relaxed);
                // Zero the meters so the UI immediately reflects the silence.
                state.mic.store_levels(0.0, 0.0);
                state.system.store_levels(0.0, 0.0);
                state.master.store_levels(0.0, 0.0);
            }
            Ok(EngineCommand::SetGain { source, gain }) => match source {
                SourceId::Mic => state.mic.set_gain(gain),
                SourceId::System => state.system.set_gain(gain),
            },
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    info!("audio engine thread exited");
}

fn start_streams(sel: &EngineSelection, state: &EngineState) -> anyhow::Result<LiveStreams> {
    let mic_name = sel
        .mic_device_name
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no microphone device selected"))?;
    let app_bundle = sel
        .system_app_bundle_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no system audio app selected"))?;
    let out_name = sel
        .output_device_name
        .as_deref()
        .unwrap_or(devices::BLACKHOLE_DEVICE_NAME);

    // 200 ms of stereo @ 48 kHz of headroom per ring.
    const RING_CAPACITY: usize = (PIPELINE_SAMPLE_RATE as usize / 5) * PIPELINE_CHANNELS as usize;
    let (mic_prod, mic_cons) = RingBuffer::<f32>::new(RING_CAPACITY);
    let (sys_prod, sys_cons) = RingBuffer::<f32>::new(RING_CAPACITY);

    let mic = mic_source::start(mic_name, mic_prod, state.mic_voice_processing.clone())?;
    let system = sck_source::start(app_bundle, sys_prod)?;
    let sink = sink::start(out_name, mic_cons, sys_cons, state.clone())?;

    Ok(LiveStreams {
        _mic: mic,
        _system: system,
        _sink: sink,
    })
}
