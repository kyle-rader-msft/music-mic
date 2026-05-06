//! Print the live device + app inventory and the health checks.
//!
//! Run: `cargo run --example devices`

use music_mic::audio::devices::{
    BLACKHOLE_DEVICE_NAME, has_screen_recording_permission, is_blackhole_installed,
    list_capturable_apps, list_input_devices, list_output_devices,
};

fn main() -> anyhow::Result<()> {
    println!("== health checks ==");
    println!("  BlackHole 2ch installed:        {}", is_blackhole_installed());
    println!(
        "  Screen Recording permission:    {}",
        has_screen_recording_permission()
    );

    println!("\n== input (microphone) devices ==");
    for d in list_input_devices()? {
        println!(
            "  - {} ({} Hz, {} ch)",
            d.name, d.default_sample_rate, d.default_channels
        );
    }

    println!("\n== output devices ==");
    for d in list_output_devices()? {
        let star = if d.is_blackhole { " ← target" } else { "" };
        println!(
            "  - {} ({} Hz, {} ch){}",
            d.name, d.default_sample_rate, d.default_channels, star
        );
    }
    println!(
        "  (looking for: \"{}\" — install via `brew install blackhole-2ch`)",
        BLACKHOLE_DEVICE_NAME
    );

    println!("\n== capturable apps (with on-screen windows) ==");
    match list_capturable_apps() {
        Ok(apps) => {
            for a in apps {
                println!("  - {:30} [{}]", a.name, a.bundle_id);
            }
        }
        Err(e) => println!("  (error: {e})"),
    }

    Ok(())
}
