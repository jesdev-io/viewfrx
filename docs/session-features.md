# Folder sessions and channel views

Opening files/folders replaces the current session. Refresh rescans those roots. No catalog or settings are restored or saved. Caches are temporary unless an explicit `--cache DIR` artifact directory is requested. Legacy `--library` is ignored; old catalogs are left untouched. CLI `import` validates/emits roots and `list` without a path lists the working directory.

Stereo sources have independent Left / Right / Both selectors for waveform and spectrogram. Plot order is wave L, wave R, spec L, spec R; mono sources have one plot of each kind. Double-click a channel selector to restore Both. Channel switching uses already prepared data, not UI-thread decoding.

CLI `waveform FILE` includes `channel_peaks` and signed per-channel `dc_offset` (linear amplitude). `spectrogram FILE --channel 0` selects Left; `--channel 1` selects Right. `--stereo` returns two independent results in L/R order. Without either option, the legacy power-averaged spectrum remains available. All routes share core analysis/cache code; channel identity is part of the spectral cache key.
