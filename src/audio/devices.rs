use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};

fn device_display_name(device: &cpal::Device) -> String {
    // cpal 0.17 deprecated `name()` in favor of `description()`/`id()`.
    // Description is what users see in System Settings, which is what we want
    // both for matching "BlackHole 2ch" and for showing in the picker.
    device
        .description()
        .map(|d| d.name().to_owned())
        .unwrap_or_else(|_| "<unknown>".into())
}

pub const BLACKHOLE_DEVICE_NAME: &str = "BlackHole 2ch";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputDevice {
    pub name: String,
    pub default_sample_rate: u32,
    pub default_channels: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputDevice {
    pub name: String,
    pub default_sample_rate: u32,
    pub default_channels: u16,
    pub is_blackhole: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturableApp {
    pub bundle_id: String,
    pub name: String,
    pub pid: i32,
}

pub fn list_input_devices() -> Result<Vec<InputDevice>> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    for device in host
        .input_devices()
        .context("enumerating cpal input devices")?
    {
        let name = device_display_name(&device);
        let cfg = match device.default_input_config() {
            Ok(c) => c,
            Err(_) => continue, // device exists but has no usable input config
        };
        out.push(InputDevice {
            name,
            default_sample_rate: cfg.sample_rate(),
            default_channels: cfg.channels(),
        });
    }
    Ok(out)
}

pub fn list_output_devices() -> Result<Vec<OutputDevice>> {
    let host = cpal::default_host();
    let mut out = Vec::new();
    for device in host
        .output_devices()
        .context("enumerating cpal output devices")?
    {
        let name = device_display_name(&device);
        let cfg = match device.default_output_config() {
            Ok(c) => c,
            Err(_) => continue,
        };
        let is_blackhole = name.eq_ignore_ascii_case(BLACKHOLE_DEVICE_NAME)
            || name.to_lowercase().contains("blackhole");
        out.push(OutputDevice {
            name,
            default_sample_rate: cfg.sample_rate(),
            default_channels: cfg.channels(),
            is_blackhole,
        });
    }
    Ok(out)
}

pub fn is_blackhole_installed() -> bool {
    list_output_devices()
        .map(|devs| devs.iter().any(|d| d.is_blackhole))
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
pub fn list_capturable_apps() -> Result<Vec<CapturableApp>> {
    use screencapturekit::prelude::*;

    let content = SCShareableContent::get()
        .map_err(|e| anyhow::anyhow!("SCShareableContent::get failed: {e:?}"))?;
    let windows = content.windows();
    let mut out: Vec<CapturableApp> = content
        .applications()
        .iter()
        .filter(|a| !a.bundle_identifier().is_empty())
        // Restrict to apps that own at least one on-screen window — these are
        // the ones whose audio the user is plausibly going to capture.
        .filter(|a| {
            windows.iter().any(|w| {
                w.is_on_screen()
                    && w.owning_application()
                        .is_some_and(|oa| oa.process_id() == a.process_id())
            })
        })
        .map(|a| CapturableApp {
            bundle_id: a.bundle_identifier(),
            name: a.application_name(),
            pid: a.process_id(),
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.bundle_id == b.bundle_id);
    Ok(out)
}

#[cfg(not(target_os = "macos"))]
pub fn list_capturable_apps() -> Result<Vec<CapturableApp>> {
    Ok(Vec::new())
}

/// Probe whether Screen Recording permission has been granted to this process.
///
/// `SCShareableContent::get` returns an error if the user has not granted the
/// Screen & System Audio Recording entitlement in System Settings → Privacy &
/// Security. We treat any failure here as "not granted" — the setup wizard
/// surfaces the System Settings deeplink in that case.
#[cfg(target_os = "macos")]
pub fn has_screen_recording_permission() -> bool {
    use screencapturekit::prelude::*;
    SCShareableContent::get().is_ok()
}

#[cfg(not(target_os = "macos"))]
pub fn has_screen_recording_permission() -> bool {
    true
}
