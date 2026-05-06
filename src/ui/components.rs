use dioxus::prelude::*;

/// Lower bound of the meter's dBFS window. Anything quieter pegs at 0% width.
const METER_MIN_DBFS: f32 = -60.0;

/// Convert a linear amplitude in `[0, 1+]` to a meter percentage on a
/// `METER_MIN_DBFS..0` dBFS scale.
///
/// A linear meter looks broken for normal voice/music: peaks land around
/// -10 dB (≈ 0.3 amplitude) which is only a third of the bar. A dB scale
/// matches what every pro audio meter does and makes meaningful movement
/// visible.
fn amp_to_db_pct(amp: f32) -> f32 {
    // log10(0) is -inf; clamp the floor so the math is well-defined.
    let amp = amp.max(1e-6);
    let db = 20.0 * amp.log10();
    let normalized = ((db - METER_MIN_DBFS) / -METER_MIN_DBFS).clamp(0.0, 1.0);
    normalized * 100.0
}

/// Horizontal level meter with a dB-scaled gradient RMS bar and a thin peak
/// marker.
///
/// The gradient lives on a fixed full-width layer that sits behind the track;
/// `clip-path` reveals only the portion up to the current RMS level. That way
/// the gradient stops correspond to *absolute* dB positions on the scale —
/// the bar's color genuinely indicates level (green = safe, amber = hot,
/// red = clipping) rather than always fading from green to red regardless of
/// loudness.
#[component]
pub fn Meter(peak: f32, rms: f32) -> Element {
    let peak_pct = amp_to_db_pct(peak);
    let rms_pct = amp_to_db_pct(rms);
    let clip_right = (100.0 - rms_pct).max(0.0);

    // Gradient stops are positioned on the dB axis: green safe zone holds
    // until ~ -12 dB (80% of the bar), amber takes over at -5 dB (~92%),
    // red bites only on the last ~3 dB.
    let gradient = "linear-gradient(to right, \
        #10b981 0%, #10b981 80%, \
        #fbbf24 92%, \
        #f43f5e 100%)";

    rsx! {
        div {
            class: "relative h-1.5 rounded-full overflow-hidden bg-zinc-200 dark:bg-zinc-800",
            div {
                class: "absolute inset-0 transition-[clip-path] duration-75 ease-linear",
                style: "background-image: {gradient}; clip-path: inset(0 {clip_right:.2}% 0 0);",
            }
            div {
                class: "absolute inset-y-0 w-0.5 bg-zinc-900/85 dark:bg-zinc-100/90 transition-[left] duration-75 ease-linear",
                style: "left: {peak_pct:.2}%;",
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.5
    }

    #[test]
    fn db_scale_endpoints_clamp() {
        // 0 dBFS → 100% (full scale).
        assert!(approx(amp_to_db_pct(1.0), 100.0));
        // < METER_MIN_DBFS → 0%.
        assert!(approx(amp_to_db_pct(0.0), 0.0));
        assert!(approx(amp_to_db_pct(1e-9), 0.0));
        // Above full scale clamps to 100%.
        assert!(approx(amp_to_db_pct(2.5), 100.0));
    }

    #[test]
    fn db_scale_speech_levels_are_visible() {
        // -10 dB ≈ 0.316 amplitude — typical speech peak. Should be deep into
        // the bar (>80%), unlike the linear scale's 32%.
        assert!(amp_to_db_pct(0.316) > 80.0);
        // -30 dB ≈ 0.0316 amplitude — soft speech RMS. Should still be
        // visible (~50% of bar) instead of vanishing at 3% on a linear scale.
        assert!(amp_to_db_pct(0.0316) > 45.0);
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
            class: "rounded-xl border border-zinc-200 bg-white px-4 py-3 shadow-sm \
                    dark:border-zinc-800 dark:bg-zinc-900/60 dark:shadow-none",
            if let Some(label) = label {
                div {
                    class: "mb-2 text-[11px] font-semibold uppercase tracking-wider \
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
