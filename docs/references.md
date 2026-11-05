# Reference notes

Consulted 2026-11-05. These are design references, not benchmarks proving frxview's performance.

## ArtCraft identity and command parity

The relevant ArtCraft is the native creative-app suite at https://getartcraft.com/, specifically [PhotoCraft](https://github.com/storytold/photocraft), not the similarly named generative-media desktop project or the unrelated crates.io library.

The [primary README](https://raw.githubusercontent.com/storytold/photocraft/main/README.md), “Built for agents”, describes one registry used by UI, CLI, JSON control and MCP. This supports a shared typed application API and headless testing. We do not need its 500-command scope or dozens of crates. The [secondary overview](https://notes.nicolasdeville.com/apps/artcraft/) helped disambiguate the projects; architectural claims were checked against the primary README. No source code was copied.

## Audio browsers

- [Soundly local indexing](https://getsoundly.com/faq/how-can-i-index-private-sound-effects-in-my-local-soundly-library/): indexes local/external/network files without uploading them. Reference for browsing in place rather than importing/copying audio into a managed vault. Offline drives require explicit missing-file behavior.
- [Resonic FAQ](https://resonic.at/faq): folder navigation directly accesses the filesystem, and recursive scanning can be expensive. Avoid making a whole-library scan a prerequisite for auditioning. Do not infer proprietary caching internals from marketing claims.
- [Audacity spectrogram manual](https://manual.audacityteam.org/man/spectrogram_view.html): spectrograms complement waveforms; window size trades time resolution for frequency resolution. Make analysis optional/configurable rather than blocking playback.
- [CPAL](https://docs.rs/cpal/0.16.0/cpal/): cross-platform host/device/stream abstraction; default device configuration is not necessarily f32 or stereo. Handle supported sample formats explicitly and surface stream errors.
- [Symphonia](https://github.com/pdeljanov/Symphonia): Rust decoding/probing. Keep packet decoding on the worker; expose failures rather than turning every decoder error into EOF.

## JESDEV branding inventory

Source: `../jesdev/jesdev-io-website/docs/assets/stylesheets/jesdev.css` and `mkdocs.yml`.

| Token | Value |
|---|---|
| Background | `#0f1014` |
| Panel / raised panel | `#181a21` / `#222631` |
| Foreground / muted | `#edf1f5` / `#b8bec8` |
| Primary accent | `#ffb300` |
| Secondary orange | `#ff8000` |
| Purple | `#a170d9` |
| Burgundy | `#980825` |

Selected site logo: `docs/assets/images/cropped-jesdev-logo-complex.webp`; additional vector wordmark: `docs/assets/images/jesdev_logo_text.drawio-1.svg`. Dark compact panels, strong headings, amber interaction accents; code font stack uses SourceCode and system monospace fallbacks. Do not port the website's large spacing, shadows or hero typography into a dense audio browser. Assets have not yet been copied; bundle them with provenance when implementing the GUI.
