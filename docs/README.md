# viewfrx documentation

[Back to viewfrx](../README.md) · [Downloads](https://github.com/jesdev-io/viewfrx/releases/latest)

## Using viewfrx

- [Installation](installation.md) — downloads, platform requirements, and source builds.
- [Desktop guide](desktop-guide.md) — browsing, playback, plots, shortcuts, and saving annotations.
- [CLI reference](cli.md) — command examples, output conventions, and the JSON-lines control session.
- [Current limitations](limitations.md) — supported formats, platform caveats, and analysis limits.
- [Changelog](../CHANGELOG.md) — changes by version.

## Specifications and implementation

- [Project brief](../goal.md) — the owner's goals; leave this file untouched.
- [Architecture](architecture.md) — crate boundaries, realtime audio contracts, workers, and caching.
- [Analysis conventions](analysis.md) — levels, normalization, waveform and spectral analysis.
- [GUI design, tests, and limitations](gui.md) — UI behavior and verification boundaries.
- [Folder sessions and channel views](session-features.md) — stateless sessions and stereo behavior.
- [Performance](performance.md) — measured baselines and caveats.
- [References and branding](references.md) — prior art and visual sources.
- [Roadmap](roadmap.md) — remaining work, not a list of shipped features.

## Contributing

See [Development](development.md) for test commands, CI, and release policy, and the [AI use policy](../AI_POLICY.md) for AI-assisted contributions. Production tuning constants live in each crate's `src/config.rs`; GUI colors live in `theme.rs`.
