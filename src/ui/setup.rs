use dioxus::prelude::*;

use crate::audio::devices;
use crate::ui::components::{Card, StatusDot};

#[component]
pub fn SetupWizard(on_ready: EventHandler<()>) -> Element {
    let mut blackhole = use_signal(devices::is_blackhole_installed);
    let mut permission = use_signal(devices::has_screen_recording_permission);

    let recheck = move |_| {
        blackhole.set(devices::is_blackhole_installed());
        permission.set(devices::has_screen_recording_permission());
    };

    let bh_ok = blackhole();
    let perm_ok = permission();
    let all_ready = bh_ok && perm_ok;

    rsx! {
        Card {
            div { class: "space-y-2",
                h2 { class: "text-base font-semibold text-zinc-900 dark:text-zinc-100",
                    "First-time setup"
                }
                p { class: "text-sm leading-6 text-zinc-600 dark:text-zinc-400",
                    "music-mic needs two things to work:"
                }
                ul { class: "list-disc pl-5 text-sm leading-6 text-zinc-600 dark:text-zinc-400 space-y-1",
                    li {
                        span { class: "font-medium text-zinc-700 dark:text-zinc-300", "BlackHole 2ch" }
                        " — a virtual audio device we publish the mixed mic into."
                    }
                    li {
                        span { class: "font-medium text-zinc-700 dark:text-zinc-300", "Screen Recording permission" }
                        " — so we can tap the audio of the app you want to share."
                    }
                }
            }

            ol { class: "mt-5 space-y-3",
                CheckRow {
                    ok: bh_ok,
                    title: "BlackHole 2ch installed",
                    detail: rsx! {
                        p { class: "mb-2", "Install via Homebrew:" }
                        pre {
                            class: "rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 \
                                    font-mono text-xs text-zinc-800 select-text \
                                    dark:border-zinc-800 dark:bg-zinc-950 dark:text-zinc-200",
                            "brew install blackhole-2ch"
                        }
                        p { class: "mt-2 text-xs text-zinc-500 dark:text-zinc-400",
                            span { class: "font-semibold text-zinc-700 dark:text-zinc-300", "Restart your Mac" }
                            " after installing — macOS only registers new audio devices on boot. Then click \"Re-check\"."
                        }
                    }
                }
                CheckRow {
                    ok: perm_ok,
                    title: "Screen Recording permission granted",
                    detail: rsx! {
                        p { "macOS requires Screen Recording permission for any app "
                            "that captures system audio (even audio-only)."
                        }
                        p { class: "mt-2",
                            "Open: "
                            span { class: "font-medium text-zinc-700 dark:text-zinc-300",
                                "System Settings → Privacy & Security → Screen & System Audio Recording"
                            }
                            ", and enable music-mic. You may need to relaunch after granting."
                        }
                    }
                }
            }

            div { class: "mt-6 flex justify-end gap-2",
                Button { variant: ButtonVariant::Ghost, onclick: recheck, "Re-check" }
                Button {
                    variant: ButtonVariant::Primary,
                    disabled: !all_ready,
                    onclick: move |_| on_ready.call(()),
                    "Continue"
                }
            }
        }
    }
}

#[component]
fn CheckRow(ok: bool, title: String, detail: Element) -> Element {
    let border = if ok {
        "border-emerald-500/30 bg-emerald-50/40 dark:border-emerald-400/20 dark:bg-emerald-400/5"
    } else {
        "border-amber-500/30 bg-amber-50/40 dark:border-amber-400/20 dark:bg-amber-400/5"
    };
    rsx! {
        li {
            class: "rounded-xl border px-4 py-3 {border}",
            div { class: "flex items-center gap-2.5",
                StatusDot { ok: ok }
                span { class: "text-sm font-medium text-zinc-900 dark:text-zinc-100",
                    "{title}"
                }
            }
            if !ok {
                div { class: "mt-3 text-sm leading-6 text-zinc-600 dark:text-zinc-400",
                    {detail}
                }
            }
        }
    }
}

// ------------------------------------------------------------------
// Button — re-used by setup + main view. Local to the ui module.
// ------------------------------------------------------------------

#[derive(PartialEq, Clone, Copy)]
pub enum ButtonVariant {
    Primary,
    Ghost,
    Danger,
}

#[component]
pub fn Button(
    variant: ButtonVariant,
    #[props(default = false)] disabled: bool,
    onclick: EventHandler<MouseEvent>,
    children: Element,
) -> Element {
    let base = "inline-flex items-center justify-center rounded-lg px-3.5 py-2 text-sm font-medium \
                transition active:translate-y-px disabled:cursor-not-allowed disabled:opacity-50 \
                focus:outline-none focus-visible:ring-2 focus-visible:ring-brand-500/40";
    let variant_cls = match variant {
        ButtonVariant::Primary =>
            "bg-brand-500 text-white shadow-sm hover:bg-brand-600",
        ButtonVariant::Ghost =>
            "bg-zinc-100 text-zinc-800 hover:bg-zinc-200 \
             dark:bg-zinc-800 dark:text-zinc-100 dark:hover:bg-zinc-700",
        ButtonVariant::Danger =>
            "bg-rose-500 text-white shadow-sm hover:bg-rose-600",
    };
    rsx! {
        button {
            r#type: "button",
            class: "{base} {variant_cls}",
            disabled: disabled,
            onclick: move |e| onclick.call(e),
            {children}
        }
    }
}
