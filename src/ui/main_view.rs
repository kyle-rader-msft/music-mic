use dioxus::prelude::*;
use std::time::Duration;
use tracing::warn;

use crate::audio::devices::{self, BLACKHOLE_DEVICE_NAME};
use crate::audio::{AudioEngine, EngineCommand, EngineSelection, EngineState, SourceId};
use crate::config;
use crate::ui::components::{Card, GainSlider, Meter, Switch};
use crate::ui::setup::{Button, ButtonVariant};
use std::sync::atomic::Ordering;

#[component]
pub fn MainView(engine: Signal<AudioEngine>) -> Element {
    let inputs = use_signal(|| devices::list_input_devices().unwrap_or_default());
    let apps = use_signal(|| devices::list_capturable_apps().unwrap_or_default());
    let outputs = use_signal(|| devices::list_output_devices().unwrap_or_default());

    let saved = use_signal(config::load);

    let mut selected_mic = use_signal(|| {
        saved
            .read()
            .mic_device_name
            .clone()
            .or_else(|| inputs.read().first().map(|d| d.name.clone()))
    });
    let mut selected_app = use_signal(|| saved.read().system_app_bundle_id.clone());
    let mut mic_gain = use_signal(|| saved.read().mic_gain.unwrap_or(1.0));
    let mut sys_gain = use_signal(|| saved.read().system_gain.unwrap_or(1.0));
    // Voice processing toggle. Default: ON. Apply restored value to the
    // engine atomic immediately so the audio thread sees it.
    let mut mic_voice = use_signal(|| saved.read().mic_voice_processing.unwrap_or(true));
    {
        let st = engine.read().state();
        st.mic_voice_processing.store(mic_voice(), Ordering::Relaxed);
    }
    let mut is_running = use_signal(|| false);

    let mut mic_lvl = use_signal(|| (0.0_f32, 0.0_f32));
    let mut sys_lvl = use_signal(|| (0.0_f32, 0.0_f32));
    let mut mas_lvl = use_signal(|| (0.0_f32, 0.0_f32));

    // Apply restored gain immediately so a Start picks it up.
    {
        let eng = engine.read();
        eng.send(EngineCommand::SetGain { source: SourceId::Mic, gain: mic_gain() });
        eng.send(EngineCommand::SetGain { source: SourceId::System, gain: sys_gain() });
    }

    let persist = move || {
        let cfg = config::Config {
            mic_device_name: selected_mic.read().clone(),
            system_app_bundle_id: selected_app.read().clone(),
            mic_gain: Some(mic_gain()),
            system_gain: Some(sys_gain()),
            mic_voice_processing: Some(mic_voice()),
        };
        if let Err(e) = config::save(&cfg) {
            warn!("failed to save config: {e:#}");
        }
    };

    // Poll engine atomics into UI signals at ~30 Hz.
    let state: EngineState = engine.read().state();
    use_future(move || {
        let state = state.clone();
        async move {
            loop {
                tokio::time::sleep(Duration::from_millis(33)).await;
                mic_lvl.set((state.mic.peak(), state.mic.rms()));
                sys_lvl.set((state.system.peak(), state.system.rms()));
                mas_lvl.set((state.master.peak(), state.master.rms()));
            }
        }
    });

    let blackhole_output = outputs
        .read()
        .iter()
        .find(|d| d.is_blackhole)
        .map(|d| d.name.clone());

    let can_start = selected_mic.read().is_some()
        && selected_app.read().is_some()
        && blackhole_output.is_some();

    let on_start = {
        let blackhole_output = blackhole_output.clone();
        move |_| {
            let sel = EngineSelection {
                mic_device_name: selected_mic.read().clone(),
                system_app_bundle_id: selected_app.read().clone(),
                output_device_name: blackhole_output.clone(),
            };
            engine.read().send(EngineCommand::Start(sel));
            is_running.set(true);
        }
    };

    let on_stop = move |_| {
        engine.read().send(EngineCommand::Stop);
        is_running.set(false);
    };

    rsx! {
        div { class: "space-y-4",
            // ---- Microphone ----
            SourceCard {
                title: "Microphone",
                meter_peak: mic_lvl().0,
                meter_rms: mic_lvl().1,
                gain: mic_gain(),
                on_gain: move |g| {
                    mic_gain.set(g);
                    engine.read().send(EngineCommand::SetGain { source: SourceId::Mic, gain: g });
                    persist();
                },
                picker: rsx! {
                    Select {
                        value: selected_mic.read().clone().unwrap_or_default(),
                        placeholder: "— select microphone —",
                        on_change: move |v: String| {
                            selected_mic.set(if v.is_empty() { None } else { Some(v) });
                            persist();
                        },
                        options: inputs.read().iter().map(|d| {
                            (d.name.clone(), format!("{} · {} Hz", d.name, d.default_sample_rate))
                        }).collect(),
                    }
                },
                footer: Some(rsx! {
                    Switch {
                        checked: mic_voice(),
                        label: "Voice isolation",
                        hint: Some("Cleans the mic before mixing — leave Teams' isolation off so music isn't suppressed.".to_string()),
                        on_toggle: move |on: bool| {
                            mic_voice.set(on);
                            engine.read().state().mic_voice_processing.store(on, Ordering::Relaxed);
                            persist();
                        },
                    }
                }),
            }

            // ---- App audio ----
            SourceCard {
                title: "App audio",
                meter_peak: sys_lvl().0,
                meter_rms: sys_lvl().1,
                gain: sys_gain(),
                on_gain: move |g| {
                    sys_gain.set(g);
                    engine.read().send(EngineCommand::SetGain { source: SourceId::System, gain: g });
                    persist();
                },
                picker: rsx! {
                    Select {
                        value: selected_app.read().clone().unwrap_or_default(),
                        placeholder: "— select app —",
                        on_change: move |v: String| {
                            selected_app.set(if v.is_empty() { None } else { Some(v) });
                            persist();
                        },
                        options: apps.read().iter().map(|a| {
                            (a.bundle_id.clone(), a.name.clone())
                        }).collect(),
                    }
                },
            }

            // ---- Output → BlackHole ----
            Card { label: "Output → virtual mic".to_string(),
                if let Some(name) = blackhole_output.as_ref() {
                    div { class: "flex items-center justify-between",
                        div {
                            div { class: "font-medium text-zinc-900 dark:text-zinc-100", "{name}" }
                            div { class: "text-xs text-zinc-500 dark:text-zinc-400 mt-0.5",
                                "48 kHz · stereo · f32"
                            }
                        }
                        span {
                            class: "rounded-full bg-emerald-500/10 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wider text-emerald-700 dark:text-emerald-400",
                            "ready"
                        }
                    }
                } else {
                    div { class: "text-sm text-amber-700 dark:text-amber-400",
                        "No BlackHole device found — install via "
                        code {
                            class: "font-mono text-xs rounded bg-amber-100 dark:bg-amber-400/10 px-1.5 py-0.5",
                            "brew install blackhole-2ch"
                        }
                        " then restart."
                    }
                }
                div { class: "mt-3", Meter { peak: mas_lvl().0, rms: mas_lvl().1 } }
            }

            // ---- Control + footer ----
            div { class: "flex flex-col items-center gap-3 pt-1",
                if is_running() {
                    Button { variant: ButtonVariant::Danger, onclick: on_stop, "Stop" }
                } else {
                    Button {
                        variant: ButtonVariant::Primary,
                        disabled: !can_start,
                        onclick: on_start,
                        "Start mixing"
                    }
                }
                p { class: "text-center text-xs leading-6 text-zinc-500 dark:text-zinc-400 max-w-md",
                    "In Teams: Settings → Devices → Microphone → "
                    span { class: "font-medium text-zinc-700 dark:text-zinc-300",
                        "{BLACKHOLE_DEVICE_NAME}"
                    }
                    ". Keep your speakers as the playback device so you can still hear your music."
                }
            }
        }
    }
}

#[component]
fn SourceCard(
    title: String,
    meter_peak: f32,
    meter_rms: f32,
    gain: f32,
    on_gain: EventHandler<f32>,
    picker: Element,
    #[props(default = None)] footer: Option<Element>,
) -> Element {
    rsx! {
        Card { label: title,
            div { class: "space-y-3",
                {picker}
                Meter { peak: meter_peak, rms: meter_rms }
                GainSlider { value: gain, on_change: on_gain }
                if let Some(f) = footer {
                    div {
                        class: "mt-1 pt-3 border-t border-zinc-200/70 dark:border-zinc-800/70",
                        {f}
                    }
                }
            }
        }
    }
}

/// Themed `<select>` — shared between mic + app pickers.
#[component]
fn Select(
    value: String,
    placeholder: String,
    on_change: EventHandler<String>,
    options: Vec<(String, String)>,
) -> Element {
    rsx! {
        select {
            value: value.clone(),
            onchange: move |e| on_change.call(e.value()),
            class: "w-full appearance-none rounded-lg border bg-white px-3 py-2 text-sm \
                    text-zinc-900 shadow-sm transition \
                    focus:outline-none focus:ring-2 focus:ring-brand-500/30 \
                    border-zinc-200 hover:border-zinc-300 \
                    dark:border-zinc-800 dark:bg-zinc-950 dark:text-zinc-100 \
                    dark:hover:border-zinc-700",
            option { value: "", "{placeholder}" }
            for (val, label) in options.iter() {
                option { value: "{val}", "{label}" }
            }
        }
    }
}
