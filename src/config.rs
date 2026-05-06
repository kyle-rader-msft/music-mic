//! Persist last-used selections so the user doesn't have to re-pick on relaunch.
//!
//! Lives at: `~/Library/Application Support/music-mic/config.json` on macOS,
//! falling back to the OS default config dir on others.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::warn;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    pub mic_device_name: Option<String>,
    pub system_app_bundle_id: Option<String>,
    pub mic_gain: Option<f32>,
    pub system_gain: Option<f32>,
    /// Whether voice isolation should run on the mic before mixing.
    /// `None` defaults to ON (the whole reason the feature exists).
    pub mic_voice_processing: Option<bool>,
}

const APP_DIR: &str = "music-mic";
const FILE: &str = "config.json";

fn config_path() -> Option<PathBuf> {
    let base = dirs::config_dir()?;
    Some(base.join(APP_DIR).join(FILE))
}

pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    match fs::read_to_string(&path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
            warn!("failed to parse {path:?}: {e}; using defaults");
            Config::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::default(),
        Err(e) => {
            warn!("failed to read {path:?}: {e}; using defaults");
            Config::default()
        }
    }
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path().context("no config dir available")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("creating {parent:?}"))?;
    }
    let s = serde_json::to_string_pretty(cfg).context("serializing config")?;
    fs::write(&path, s).with_context(|| format!("writing {path:?}"))?;
    Ok(())
}
