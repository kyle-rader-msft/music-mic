mod components;
mod main_view;
mod setup;

use dioxus::prelude::*;

use crate::audio::{AudioEngine, devices};
use crate::ui::setup::{Button, ButtonVariant};

#[component]
pub fn App() -> Element {
    let engine = use_signal(AudioEngine::spawn);

    let initial_ready =
        devices::is_blackhole_installed() && devices::has_screen_recording_permission();
    let mut show_setup = use_signal(move || !initial_ready);

    rsx! {
        document::Stylesheet { href: asset!("/assets/main.css") }

        // Page surface — soft gradient in light, near-black in dark.
        main {
            class: "min-h-screen w-full bg-gradient-to-b from-zinc-50 to-zinc-100 \
                    text-zinc-900 \
                    dark:from-zinc-950 dark:to-zinc-900 dark:text-zinc-100",
            div { class: "mx-auto max-w-md px-5 pt-5 pb-6 space-y-4",
                Header {
                    show_setup: show_setup(),
                    on_toggle: move |_| show_setup.set(!show_setup()),
                }

                if show_setup() {
                    setup::SetupWizard {
                        on_ready: move |_| show_setup.set(false),
                    }
                } else {
                    main_view::MainView { engine: engine }
                }
            }
        }
    }
}

#[component]
fn Header(show_setup: bool, on_toggle: EventHandler<MouseEvent>) -> Element {
    rsx! {
        header { class: "flex items-start justify-between gap-3",
            div { class: "min-w-0",
                h1 {
                    class: "text-xl font-semibold tracking-tight text-zinc-900 dark:text-zinc-50",
                    "music-mic"
                }
                p { class: "mt-0.5 text-xs leading-snug text-zinc-600 dark:text-zinc-400",
                    "Mix your mic with any app's audio into a virtual mic for Teams."
                }
            }
            Button {
                variant: ButtonVariant::Ghost,
                onclick: on_toggle,
                if show_setup { "Skip setup" } else { "Setup" }
            }
        }
    }
}
