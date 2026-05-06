use dioxus::prelude::*;

/// Horizontal level meter with a gradient RMS bar and a thin peak marker.
#[component]
pub fn Meter(peak: f32, rms: f32) -> Element {
    let peak_pct = (peak.clamp(0.0, 1.0) * 100.0).round() as i32;
    let rms_pct = (rms.clamp(0.0, 1.0) * 100.0).round() as i32;
    rsx! {
        div {
            class: "relative h-1.5 rounded-full overflow-hidden bg-zinc-200 dark:bg-zinc-800",
            div {
                class: "absolute inset-y-0 left-0 rounded-full bg-gradient-to-r from-emerald-500 via-amber-400 to-rose-500 transition-[width] duration-75 ease-linear",
                style: "width: {rms_pct}%;",
            }
            div {
                class: "absolute inset-y-0 w-0.5 bg-zinc-900/85 dark:bg-zinc-100/90 transition-[left] duration-75 ease-linear",
                style: "left: {peak_pct}%;",
            }
        }
    }
}

/// 0–200% gain slider with a numeric readout. Calls `on_change` with a 0–2 float.
#[component]
pub fn GainSlider(value: f32, on_change: EventHandler<f32>) -> Element {
    let pct = (value * 100.0).round() as i32;
    rsx! {
        div { class: "flex items-center gap-3",
            input {
                r#type: "range",
                min: "0",
                max: "200",
                step: "1",
                value: "{pct}",
                class: "flex-1 h-1 cursor-pointer",
                oninput: move |e| {
                    if let Ok(n) = e.value().parse::<f32>() {
                        on_change.call(n / 100.0);
                    }
                },
            }
            span {
                class: "w-12 text-right text-xs tabular-nums text-zinc-500 dark:text-zinc-400",
                "{pct}%"
            }
        }
    }
}

/// Reusable card surface — cards are the primary visual container.
#[component]
pub fn Card(label: Option<String>, children: Element) -> Element {
    rsx! {
        section {
            class: "rounded-2xl border border-zinc-200 bg-white p-5 shadow-sm \
                    dark:border-zinc-800 dark:bg-zinc-900/60 dark:shadow-none",
            if let Some(label) = label {
                div {
                    class: "mb-3 text-[11px] font-semibold uppercase tracking-wider \
                            text-zinc-500 dark:text-zinc-400",
                    "{label}"
                }
            }
            {children}
        }
    }
}

/// Compact iOS-style toggle. Calls `on_toggle` with the new state.
#[component]
pub fn Switch(
    checked: bool,
    label: String,
    #[props(default = None)] hint: Option<String>,
    on_toggle: EventHandler<bool>,
) -> Element {
    let track = if checked {
        "bg-brand-500"
    } else {
        "bg-zinc-300 dark:bg-zinc-700"
    };
    let knob = if checked { "translate-x-4" } else { "translate-x-0.5" };
    rsx! {
        label {
            class: "flex items-center justify-between gap-3 cursor-pointer select-none",
            div { class: "min-w-0",
                div { class: "text-sm font-medium text-zinc-800 dark:text-zinc-100", "{label}" }
                if let Some(h) = hint.as_ref() {
                    div { class: "mt-0.5 text-xs text-zinc-500 dark:text-zinc-400", "{h}" }
                }
            }
            span {
                role: "switch",
                "aria-checked": "{checked}",
                onclick: move |_| on_toggle.call(!checked),
                class: "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors {track}",
                span {
                    class: "inline-block h-4 w-4 rounded-full bg-white shadow transition-transform {knob}",
                }
            }
        }
    }
}

/// Pill-shaped status dot used in the wizard.
#[component]
pub fn StatusDot(ok: bool) -> Element {
    let cls = if ok {
        "bg-emerald-500 text-white"
    } else {
        "bg-zinc-300 text-zinc-600 dark:bg-zinc-700 dark:text-zinc-300"
    };
    let glyph = if ok { "✓" } else { "•" };
    rsx! {
        span {
            class: "inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-[11px] font-bold {cls}",
            "{glyph}"
        }
    }
}
