// These are used only by the macOS-only probe builders below; gate the imports
// to match so non-macOS builds (e.g. Linux CI) don't flag them as unused.
#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

fn main() {
    #[cfg(target_os = "macos")]
    {
        build_probe("meeting_probe");
        build_probe("mic_probe");
    }
    tauri_build::build()
}

/// The Rust target triples of the per-architecture Swift builds, paired with
/// the `swiftc -target` architecture name.
#[cfg(target_os = "macos")]
const ARCHES: [(&str, &str); 2] = [
    ("aarch64-apple-darwin", "arm64"),
    ("x86_64-apple-darwin", "x86_64"),
];

/// Lowest macOS the probes are compiled for.
#[cfg(target_os = "macos")]
const MACOS_MIN: &str = "11.0";

/// Compiles the Swift helper `helpers/<name>.swift` — the EventKit
/// meeting-detector (design spec §8, `meeting_probe`) or the CoreAudio
/// microphone-in-use probe (design spec §4.5, `mic_probe`; a device-property
/// read, not audio capture, so it needs NO microphone TCC permission) — and
/// ad-hoc signs it so its TCC grant sticks across runs.
///
/// The probes ship inside the app bundle as Tauri sidecars
/// (`bundle.externalBin` in `tauri.macos.conf.json`), which requires a
/// `binaries/<name>-<target-triple>` file for every triple the app is built
/// for. Cargo is invoked once per architecture even for a universal build, and
/// the bundler then wants `<name>-universal-apple-darwin`, so all three are
/// produced here on every macOS build. The app locates the probe next to its
/// own executable at runtime (`quiet_os::probe_path`); no build-machine path is
/// baked in. Fails loudly if the toolchain or a compile/sign/lipo step is
/// unavailable — a broken helper must not silently degrade the quiet rules.
#[cfg(target_os = "macos")]
fn build_probe(name: &str) {
    let source = format!("helpers/{name}.swift");
    println!("cargo:rerun-if-changed={source}");

    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let binaries = Path::new("binaries");
    fs::create_dir_all(binaries).expect("the sidecar binaries directory is created");

    let mut slices = Vec::new();
    for (triple, arch) in ARCHES {
        let slice = out_dir.join(format!("{name}-{arch}"));
        run(
            Command::new("swiftc")
                .arg(&source)
                .args([
                    "-O",
                    "-target",
                    &format!("{arch}-apple-macos{MACOS_MIN}"),
                    "-o",
                ])
                .arg(&slice),
            &format!("compiling {source} for {triple}"),
        );
        let dest = binaries.join(format!("{name}-{triple}"));
        fs::copy(&slice, &dest).expect("the per-arch probe is copied into binaries/");
        sign(&dest);
        slices.push(slice);
    }

    let universal = binaries.join(format!("{name}-universal-apple-darwin"));
    run(
        Command::new("lipo")
            .arg("-create")
            .args(&slices)
            .arg("-output")
            .arg(&universal),
        &format!("lipo-ing {name} into a universal binary"),
    );
    sign(&universal);
}

#[cfg(target_os = "macos")]
fn sign(binary: &Path) {
    run(
        Command::new("codesign").args(["-s", "-", "-f"]).arg(binary),
        &format!("ad-hoc signing {}", binary.display()),
    );
}

#[cfg(target_os = "macos")]
fn run(command: &mut Command, what: &str) {
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("could not start the tool for {what}: {e}"));
    assert!(status.success(), "{what} failed");
}
