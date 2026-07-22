use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    #[cfg(target_os = "macos")]
    build_meeting_probe();
    tauri_build::build()
}

/// Compiles the EventKit meeting-detector helper (design spec §8) and ad-hoc
/// signs it so its calendar TCC grant sticks across runs. The compiled path is
/// exported as `MEETING_PROBE_PATH` for the calendar probe to invoke. Fails
/// loudly if the toolchain or the compile/sign step is unavailable — a broken
/// helper must not silently degrade the meeting-aware pause.
#[cfg(target_os = "macos")]
fn build_meeting_probe() {
    let source = "helpers/meeting_probe.swift";
    println!("cargo:rerun-if-changed={source}");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let binary = out_dir.join("meeting_probe");

    let compile = Command::new("swiftc")
        .args([source, "-O", "-o"])
        .arg(&binary)
        .status()
        .expect("swiftc is available to compile the meeting probe");
    assert!(compile.success(), "compiling {source} failed");

    let sign = Command::new("codesign")
        .args(["-s", "-", "-f"])
        .arg(&binary)
        .status()
        .expect("codesign is available to ad-hoc sign the meeting probe");
    assert!(sign.success(), "ad-hoc signing the meeting probe failed");

    println!("cargo:rustc-env=MEETING_PROBE_PATH={}", binary.display());
}
