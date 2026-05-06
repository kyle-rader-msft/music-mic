// Bake Swift runtime rpaths into our binaries.
//
// The `screencapturekit` crate links Swift code that depends on
// `libswift_Concurrency.dylib`. Its own `build.rs` emits the right rpaths via
// `cargo:rustc-link-arg=...`, but Cargo applies that flag only to the crate's
// own artifacts — not to downstream binaries that link the rlib. Without the
// rpath, our app dies at load time with:
//   dyld: Library not loaded: @rpath/libswift_Concurrency.dylib
//
// Fix: re-emit the rpaths here with `rustc-link-arg-bins` / `-examples` /
// `-tests`, which targets the consuming binary.

use std::process::Command;

fn main() {
    if !cfg!(target_os = "macos") {
        return;
    }

    let bin_targets = ["bins", "examples"];

    for target in bin_targets {
        emit(target, "-Wl,-rpath,/usr/lib/swift");
    }

    if let Ok(output) = Command::new("xcode-select").arg("-p").output() {
        if output.status.success() {
            let xcode_path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            for path_template in [
                "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift-5.5/macosx",
                "{}/Toolchains/XcodeDefault.xctoolchain/usr/lib/swift/macosx",
            ] {
                let rpath = path_template.replace("{}", &xcode_path);
                for target in bin_targets {
                    emit(target, &format!("-Wl,-rpath,{rpath}"));
                }
            }
        }
    }

    println!("cargo:rerun-if-env-changed=DEVELOPER_DIR");
    println!("cargo:rerun-if-changed=build.rs");
}

fn emit(target: &str, arg: &str) {
    println!("cargo:rustc-link-arg-{target}={arg}");
}
