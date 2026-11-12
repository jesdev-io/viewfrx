# Development

[Documentation index](README.md) · [Build instructions](installation.md#build-from-source) · [Architecture](architecture.md)

## CI and releases

GitHub Actions ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)) runs formatting, clippy, tests, and release builds on **Linux, Windows, and macOS** for pushes and pull requests to `main`/`dev`. A `v*` tag publishes portable binaries plus Linux `.deb`/AppImage, Windows MSI, and macOS DMG installers; GitHub also attaches source archives. Installer signing/notarization is not configured yet. See [CHANGELOG.md](../CHANGELOG.md). The `dev` branch is the integration branch; `main` tracks releases.

Only create a release on the owner’s explicit command. Keep ordinary work on `dev`, split commits by meaningful topic, and run tests before code changes.

## Test and measure

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --features spectrogram --locked -- -D warnings
cargo test --workspace --features spectrogram --locked
cargo test --workspace --no-default-features --locked
cargo build --release --workspace --features spectrogram --locked
./target/release/viewfrx benchmark '/path/to/recording.wav'
python3 scripts/check-performance.py
python3 scripts/check-performance.py --audio --spectrogram  # MUTED real-device smoke test
python3 scripts/check-gui.py --expect-spectrogram --demo --screenshot /tmp/viewfrx.png  # needs a display
```

Unit/integration tests do not require an audio device. WAV fixtures are generated in temporary directories; the tiny synthetic MP3 fixture is committed with its generation command. The performance script generates 60-second and 600-second WAVs and 10,000 directory entries, then removes them. FFmpeg is optional and enables an additional generated MP3 benchmark. The optional device test uses POSIX pipe polling, gain zero, and never deliberately plays the synthetic tone audibly.

[Measured baseline and caveats](performance.md). These tests establish a foundation—not proof that every codec, driver, or slow filesystem is glitch-free.
