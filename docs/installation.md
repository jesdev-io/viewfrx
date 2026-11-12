# Installation

[Documentation index](README.md) · [Downloads](https://github.com/jesdev-io/viewfrx/releases/latest)

## Install a release

Download the archive for your operating system from the [Releases page](https://github.com/jesdev-io/viewfrx/releases/latest):

- Linux: `viewfrx-ubuntu-latest.tar.gz`
- macOS: `viewfrx-macos-latest.tar.gz`
- Windows: `viewfrx-windows-latest.zip`

Archives contain the `viewfrx` CLI, `viewfrx-gui` desktop app, this README, and the license. Extract the archive and run the binary directly (on Linux, the GUI requires system OpenGL/X11 or Wayland libraries and desktop portal support for file dialogs). Releases also include a Linux `.deb` (Debian/Ubuntu) and AppImage, a Windows `.msi`, and a macOS `.dmg`. The `.deb` declares its ALSA runtime dependency; graphics, desktop portal, and audio-device libraries still come from the host system. The MSI and DMG are currently unsigned, so Windows/macOS may show security warnings; signing/notarization can be added when credentials are available. Verify downloads with `SHA256SUMS` when available. Releases are built by GitHub Actions for Linux, Windows, and macOS; source archives are also attached automatically.

## Build from source

Rust 1.89+ is declared. GitHub Actions builds and tests on Linux, Windows, and macOS; device/driver behavior can still vary by platform.

```sh
# Debian/Ubuntu native build dependencies
sudo apt install build-essential pkg-config libasound2-dev
cargo build --release --locked
./target/release/viewfrx --help
```

The CLI binary is `target/release/viewfrx` (`viewfrx.exe` on Windows). Plain `cargo build`/`cargo run` still target the CLI only; build the whole workspace or select the GUI explicitly. Python and FFmpeg are optional test tools, not application runtime dependencies.

### Desktop app

```sh
cargo run --release -p viewfrx-gui --features spectrogram -- /path/to/recordings
cargo build --release --workspace --features spectrogram --locked
```

The `spectrogram` feature enables spectral analysis; omit it for a waveform-only build.
