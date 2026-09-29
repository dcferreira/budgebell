//! Locates the bundled Swift helper probes at runtime. The probes are Tauri
//! sidecars (`bundle.externalBin` in `tauri.macos.conf.json`): the bundler
//! strips the `-<target-triple>` suffix and places them next to the app
//! executable (`Budgebell.app/Contents/MacOS/`), and `tauri-build` copies them
//! next to the binary in `target/{debug,release}/` for `cargo run`/dev. Either
//! way, a probe lives beside the running executable — never at a build-machine
//! path baked in at compile time, which would not exist on a user's Mac.

use std::path::{Path, PathBuf};

/// The path of the sidecar probe `name` given the directory holding the app
/// executable.
pub fn sidecar_path(exe_dir: &Path, name: &str) -> PathBuf {
    exe_dir.join(name)
}

/// Resolves the sidecar probe `name` next to the currently running executable.
#[cfg(target_os = "macos")]
pub fn resolve(name: &str) -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let exe_dir = exe.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "the app executable has no parent directory",
        )
    })?;
    Ok(sidecar_path(exe_dir, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_sits_beside_the_executable_without_a_triple_suffix() {
        let dir = Path::new("/Applications/Budgebell.app/Contents/MacOS");
        assert_eq!(
            sidecar_path(dir, "meeting_probe"),
            PathBuf::from("/Applications/Budgebell.app/Contents/MacOS/meeting_probe")
        );
    }

    #[test]
    fn probes_resolve_to_distinct_paths_in_the_same_dir() {
        let dir = Path::new("/x");
        assert_ne!(
            sidecar_path(dir, "mic_probe"),
            sidecar_path(dir, "meeting_probe")
        );
        assert_eq!(sidecar_path(dir, "mic_probe").parent(), Some(dir));
    }
}
