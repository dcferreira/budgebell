# Budgebell

Budgebell is a fully-local desktop app that nudges you through habits (movement drills first, general habits too), shows you what to do, and logs what you actually did.

It exists as a native, privacy-preserving replacement for an ad-hoc anti-sedentary setup: everything (habit definitions, media, and logs) stays on your machine, with no network calls, no telemetry, and no cloud.

## Install

Download the latest build for your platform from [GitHub Releases](https://github.com/dcferreira/budgebell/releases):

- **macOS:** the `.dmg` (Apple Silicon: `aarch64`, Intel: `x64`).
- **Linux:** the `.AppImage`, `.deb` or `.rpm`. On GNOME the tray icon needs the AppIndicator / StatusNotifierItem shell extension.
- **Windows:** the `.msi` or the NSIS `-setup.exe`.

Builds are **unsigned** for now, so your OS will warn on first launch:

- **macOS:** Gatekeeper will say the app is damaged or from an unidentified developer. After moving Budgebell to `/Applications`, clear the quarantine flag and open it again:

  ```sh
  xattr -dr com.apple.quarantine /Applications/Budgebell.app
  ```

- **Windows:** SmartScreen will show "Windows protected your PC". Click **More info**, then **Run anyway**.

## Development

The app is a [Tauri v2](https://v2.tauri.app/) (Rust) core with a Vite + Svelte 5 + TypeScript frontend. Everything runs locally with no network dependencies.

### Prerequisites

- **Rust** (via [rustup](https://rustup.rs/))
- **Node** with **[pnpm](https://pnpm.io/)** (`pnpm@11`, pinned in `package.json`)
- **Platform build dependencies for Tauri v2:**
  - **macOS:** Xcode Command Line Tools (`xcode-select --install`)
  - **Linux (Debian/Ubuntu):** `libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev` (see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for other distributions)

### Setup

```sh
pnpm install       # install frontend deps (Rust deps fetch on first build)
pnpm tauri dev     # run the desktop app (Vite on :1420 + Rust core)
```

### Gates

These are the same checks CI runs:

```sh
pnpm check                                          # type-check the frontend (svelte-check)
pnpm test                                           # frontend tests (vitest)
cargo fmt --check --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml     # Rust tests
```

## Licence

[MIT](LICENSE).
