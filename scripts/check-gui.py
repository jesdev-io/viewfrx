#!/usr/bin/env python3
"""Native, MUTED GUI smoke test. Needs a graphical display (or xvfb-run on Linux).
Uses a temporary catalog/cache; does not touch the user's library or play audio.
"""
import argparse
import json
import math
import random
import wave
from pathlib import Path
import struct
import subprocess
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent

# Synthetic visual-fixture configuration, unrelated to application DSP defaults.
DEMO_RATE = 48000
DEMO_SECONDS = 6
DEMO_START_HZ = 300
DEMO_SWEEP_HZ_PER_SECOND = 2200
DEMO_RIGHT_HZ = 660
DEMO_LEVEL = 0.03
DEMO_IMPULSE_TIME = 3.3
DEMO_IMPULSE_WIDTH = 0.025


def demo_fixture(path):
    rng = random.Random(0)
    pcm = bytearray()
    for i in range(DEMO_RATE * DEMO_SECONDS):
        t = i / DEMO_RATE
        envelope = 0.25 + 0.75 * math.sin(math.pi * t / DEMO_SECONDS) ** 2
        phase = 2 * math.pi * (DEMO_START_HZ * t + DEMO_SWEEP_HZ_PER_SECOND * t * t / 2)
        burst = DEMO_LEVEL * math.exp(-((t - DEMO_IMPULSE_TIME) / DEMO_IMPULSE_WIDTH) ** 2) * rng.uniform(-1, 1)
        left = DEMO_LEVEL * envelope * math.sin(phase) + burst
        right = DEMO_LEVEL / 2 * envelope * math.sin(2 * math.pi * DEMO_RIGHT_HZ * t)
        pcm.extend(struct.pack("<hh", int(left * 32767), int(right * 32767)))
    with wave.open(str(path), "wb") as audio:
        audio.setparams((2, 2, DEMO_RATE, 0, "NONE", "not compressed"))
        audio.writeframes(pcm)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/viewviewfrx-gui")
    parser.add_argument("--screenshot", type=Path, help="Keep the rendered PNG at this path")
    parser.add_argument("--entries", type=int, default=10000)
    parser.add_argument("--demo", action="store_true", help="Use a generated stereo chirp/burst for the screenshot")
    parser.add_argument("--expect-spectrogram", action="store_true", help="Require the optional spectrogram feature and completed texture")
    args = parser.parse_args()
    if not 1 <= args.entries <= 100000:
        parser.error("entries must be between 1 and 100000")
    with tempfile.TemporaryDirectory(prefix="viewviewfrx-gui-") as temporary:
        work = Path(temporary)
        sounds = work / "sounds"
        sounds.mkdir()
        fixture = ROOT / "crates/viewfrx-core/tests/fixtures/tone.mp3"
        if args.demo:
            demo_fixture(sounds / "00000 - Stereo chirp.wav")
        else:
            (sounds / "00000 - Synthetic test tone.mp3").write_bytes(fixture.read_bytes())
        # Candidate discovery does not decode every file: verify that at scale.
        for i in range(1, args.entries):
            (sounds / f"{i:05d} - Unselected placeholder.wav").touch()
        screenshot = args.screenshot.resolve() if args.screenshot else work / "rendered.png"
        start = time.perf_counter()
        result = subprocess.run([
            str(args.binary.resolve()), str(sounds), "--muted", "--library", str(work / "library.json"),
            "--cache", str(work / "cache"), "--smoke-seconds", "3", "--screenshot", str(screenshot),
            *(["--spectrogram"] if args.expect_spectrogram else []),
        ], cwd=ROOT, text=True, capture_output=True, timeout=25)
        if result.returncode:
            raise RuntimeError(f"GUI exit {result.returncode}: {result.stderr}")
        report = json.loads(result.stdout)
        assert report["library_candidates"] == args.entries, report
        assert not report["scanning"] and not report["analyzing"], report
        assert report["waveform_bins"] > 0, report
        if args.expect_spectrogram:
            assert report["spectrogram_feature"] and report["spectrogram_ready"], report
        png = screenshot.read_bytes()
        assert png[:8] == b"\x89PNG\r\n\x1a\n", "Expected a real screenshot PNG"
        width, height = struct.unpack(">II", png[16:24])
        assert width >= 900 and height >= 560, (width, height)
        catalog = json.loads((work / "library.json").read_text())
        assert catalog["roots"] == [str(sounds.resolve())]
        assert list((work / "cache").glob("viewfrx-waveform-*.json")), "Selected waveform never completed"
        print(json.dumps({"ok": True, "candidate_files": args.entries, "rendered_size": [width, height], "ui": report,
                          "total_seconds_including_3s_hold": round(time.perf_counter() - start, 3),
                          "screenshot": str(screenshot) if args.screenshot else "validated; temporary",
                          "stderr_lines": len(result.stderr.splitlines()),
                          "note": "Native rendering/import/analysis smoke only. Keyboard and waveform-click routing are tested separately without audio hardware."}, indent=2))


if __name__ == "__main__":
    main()
