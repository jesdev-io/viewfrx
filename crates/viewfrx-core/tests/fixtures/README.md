# Generated fixture

`tone.mp3`: synthetic 440 Hz, 0.25 second, stereo MP3, generated locally (not a third-party recording):

```sh
ffmpeg -v error -f lavfi -i 'sine=frequency=440:sample_rate=44100:duration=0.25' \
  -ac 2 -codec:a libmp3lame -q:a 4 -map_metadata -1 tone.mp3
```

Retained so MP3 probe/decode/seek regression tests do not require FFmpeg or network access. Encoder delay/padding are described by the MP3 gapless metadata.
