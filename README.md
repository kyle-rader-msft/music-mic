# music-mic

A small macOS desktop app that mixes a microphone with the audio of a chosen
application (e.g. Spotify) and exposes the result as a virtual microphone — so
you can share music in Microsoft Teams without "share screen + include audio."

Built with [Dioxus](https://dioxuslabs.com/) (UI), [cpal](https://github.com/RustAudio/cpal)
(microphone + output), [ScreenCaptureKit](https://developer.apple.com/documentation/screencapturekit)
(per-app system audio), [BlackHole 2ch](https://existential.audio/blackhole/) (virtual
mic device), and [nnnoiseless](https://github.com/jneem/nnnoiseless) / RNNoise
(voice isolation on the mic side).

![macOS 13+](https://img.shields.io/badge/macOS-13%2B-blue)
![Rust](https://img.shields.io/badge/Rust-1.86%2B-orange)
![License: MIT](https://img.shields.io/badge/License-MIT-green)

## Why

Microsoft Teams' "share desktop audio" forces you to share your whole screen.
Setting your mic to a virtual device that already contains music + voice
sidesteps that — Teams just sees a normal mic input.

The catch: Teams' voice isolation will cancel music as background noise.
This app applies its own RNNoise-based voice isolation to the mic *before*
mixing, so you can leave Teams' isolation off and keep both clean voice
*and* music.

## Architecture

```
 ScreenCaptureKit ── per-app audio ──┐
   (Spotify, etc.)                   │
                                     ├──▶ mixer ──▶ cpal output ──▶ BlackHole 2ch
                                     │   (gain +                       │
 cpal input ── mic ──▶ RNNoise ──────┘    soft-clip                    ▼
   (any device)        (optional)         + meters)                  Teams
                                                                  (mic input)
```

Internally everything is f32, stereo, interleaved, 48 kHz. Audio threads
write to lock-free SPSC ring buffers ([`rtrb`](https://github.com/mgeier/rtrb))
and never allocate after warmup; the mixer lives inside the cpal output
callback so there's no extra hop. The Dioxus UI polls atomic level
counters at ~30 Hz — it never sits on the audio path.

## Requirements

- macOS 13 (Ventura) or later — uses ScreenCaptureKit for audio loopback.
- [Xcode](https://developer.apple.com/xcode/) installed (the build links Swift
  code from the `screencapturekit` crate, which needs the Swift toolchain).
- [BlackHole 2ch](https://existential.audio/blackhole/) — installed once via
  Homebrew. Acts as the virtual mic.
- [Rust](https://www.rust-lang.org/) 1.86+ (edition 2024).
- [Node](https://nodejs.org/) 18+ — only used to build the Tailwind CSS bundle.
  The compiled `assets/main.css` is checked in, so you only need Node if you're
  changing styles.

## Quickstart

```bash
# 1. Install BlackHole (one-time)
brew install blackhole-2ch

# 2. Restart your Mac so macOS registers the new audio device.

# 3. Build + run
git clone https://github.com/kyle-rader-msft/music-mic
cd music-mic
cargo run --release        # release matters — debug builds make denoising ~10× slower
```

On first launch the app's setup wizard checks for BlackHole and asks you to
grant **System Settings → Privacy & Security → Screen & System Audio
Recording**. After that, pick your microphone + the app whose audio you want
to share + click "Start mixing."

In Teams: **Settings → Devices → Microphone → BlackHole 2ch**, and
**leave Teams' voice isolation off** (Standard mode). music-mic does the
voice cleanup itself, on the mic-only signal, so the music isn't suppressed.

## Working on the UI

```bash
npm install              # one-time
npm run watch:css        # rebuilds assets/main.css on .rs changes
cargo run                # in another terminal
```

The Tailwind input is `src/input.css`; classes are scanned out of `src/**/*.rs`
by Tailwind v4's auto-content discovery. Light + dark themes track
`prefers-color-scheme` automatically — no JS toggle.

## Project layout

```
src/
  main.rs                # Dioxus entry, tracing init
  lib.rs                 # exposes audio + config for the example bin
  config.rs              # ~/Library/Application Support/music-mic/config.json
  audio/
    mod.rs               # AudioEngine: control thread + Start/Stop/SetGain
    types.rs             # f32/48k/stereo pipeline, atomic Levels
    devices.rs           # cpal in/out + SCK app enumeration, health checks
    ring.rs              # rtrb push helper
    mic_source.rs        # cpal mic → resample → voice-isolate → ring
    sck_source.rs        # ScreenCaptureKit per-app audio → ring
    voice_processing.rs  # RNNoise (nnnoiseless) wrapper
    sink.rs              # cpal → BlackHole; mixer + soft-clip + meters
  ui/
    mod.rs, setup.rs, main_view.rs, components.rs
  input.css              # Tailwind v4 input
build.rs                 # bakes Swift Concurrency rpaths into binaries
examples/devices.rs      # standalone enumerator for diagnosing setups
```

A small `examples/devices.rs` prints the live device + app inventory and the
health checks — handy when debugging without launching the GUI:

```bash
cargo run --example devices
```

## Status

This is a side project, used personally; YMMV. Things I'd build next if it
gets enough mileage:

- Replace the Dioxus + WebView shell with something lighter (the audio engine
  itself is fine in WebView, but startup is slower than it needs to be).
- Optionally use Apple's `AUVoiceProcessingIO` audio unit instead of RNNoise
  for higher-quality voice isolation on macOS.
- Code signing + notarization + a `.dmg` so it's a one-double-click install.

## Acknowledgements

- [BlackHole](https://github.com/ExistentialAudio/BlackHole) — Devin Roth's
  virtual audio driver. None of this would work without it.
- [nnnoiseless](https://github.com/jneem/nnnoiseless) — Joe Neeman's pure-Rust
  port of Xiph's [RNNoise](https://github.com/xiph/rnnoise).
- [screencapturekit-rs](https://github.com/doom-fish/screencapturekit-rs) —
  safe Rust bindings for Apple's ScreenCaptureKit.

## License

[MIT](LICENSE) © Kyle Rader.

The compiled binary, when distributed, links against several BSD-3-Clause
crates (notably `nnnoiseless`); the BSD attribution requirements are
satisfied by Cargo's bundled license metadata in the resulting binary.
