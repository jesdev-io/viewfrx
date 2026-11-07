#!/usr/bin/env python3
"""Reproducible local baseline. No third-party Python packages; temp fixtures only.
--audio opts into a MUTED real-device session (default output or --device).
Numbers are observations, not universal latency guarantees. Hardware smoke is POSIX.
"""
import argparse
import json
import math
import os
from pathlib import Path
import platform
import selectors
import shutil
import statistics
import struct
import subprocess
import tempfile
import time
import wave

ROOT = Path(__file__).resolve().parent.parent


def fixture(path, seconds):
    block = b"".join(struct.pack("<hh", int(1600 * math.sin(2 * math.pi * 440 * i / 48000)),
                                int(1600 * math.sin(2 * math.pi * 660 * i / 48000))) for i in range(48000))
    with wave.open(str(path), "wb") as out:
        out.setparams((2, 2, 48000, 0, "NONE", "not compressed"))
        for _ in range(seconds):
            out.writeframesraw(block)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--audio", action="store_true")
    parser.add_argument("--spectrogram", action="store_true", help="Benchmark the optional spectrogram feature")
    parser.add_argument("--device", type=int)
    parser.add_argument("--binary", type=Path, default=ROOT / "target/release/frxview")
    args = parser.parse_args()
    binary = str(args.binary.resolve())
    report = {"platform": platform.platform(), "cpu": platform.processor(),
              "note": "Local warm filesystem; cold/warm refer to waveform analysis cache, not OS page cache."}
    with tempfile.TemporaryDirectory(prefix="frxview-bench-") as temporary:
        tmp = Path(temporary)
        short, long = tmp / "60s.wav", tmp / "600s.wav"
        fixture(short, 60)
        fixture(long, 600)

        def run(*command):
            result = subprocess.run([binary, *map(str, command)], capture_output=True, text=True, timeout=120, check=True)
            return json.loads(result.stdout)

        report["decode"] = {"60s_wav": run("benchmark", short), "600s_wav": run("benchmark", long)}
        if shutil.which("ffmpeg"):
            mp3 = tmp / "60s.mp3"
            subprocess.run(["ffmpeg", "-v", "error", "-i", str(short), "-codec:a", "libmp3lame", "-q:a", "3", str(mp3)], check=True, timeout=60)
            report["decode"]["60s_mp3"] = run("benchmark", mp3)
            assert report["decode"]["60s_mp3"]["frames"] == 60 * 48000
            report["mp3_metadata"] = run("inspect", mp3)
        for label in ["cold", "warm"]:
            start = time.perf_counter()
            data = run("waveform", long, "--cache", tmp / "cache")
            report[f"waveform_{label}"] = {"ms": (time.perf_counter() - start) * 1000,
                                           "cache_hit": data["cache_hit"], "bins": len(data["waveform"]["peaks"])}
            assert data["cache_hit"] == (label == "warm")
        if args.spectrogram:
            for label in ["cold", "warm"]:
                start = time.perf_counter()
                data = run("spectrogram", long, "--cache", tmp / "spectral-cache")
                spectral = data["spectrogram"]
                report[f"spectrogram_{label}"] = {"ms": (time.perf_counter() - start) * 1000,
                    "cache_hit": data["cache_hit"], "columns": spectral["columns"], "frequency_bins": spectral["frequency_bins"]}
                assert data["cache_hit"] == (label == "warm")
                assert len(spectral["dbfs"]) == spectral["columns"] * spectral["frequency_bins"]
                assert all(math.isfinite(v) for v in spectral["dbfs"])
        listing = tmp / "listing"
        listing.mkdir()
        for i in range(10000):
            (listing / f"{i}.wav").touch()
        start = time.perf_counter()
        result = subprocess.run([binary, "list", str(listing)], capture_output=True, text=True, check=True, timeout=30)
        report["list_10000"] = {"ms": (time.perf_counter() - start) * 1000, "entries": len(result.stdout.splitlines())}
        assert report["list_10000"]["entries"] == 10000
        report["binary_bytes"] = os.path.getsize(binary)
        gui = ROOT / "target/release/frxview-gui"
        if gui.exists():
            report["gui_binary_bytes"] = os.path.getsize(gui)
        if Path("/usr/bin/time").exists():
            report["peak_rss_kib"] = {}
            for label, cmd in [("benchmark", ["benchmark", str(long)]),
                               ("waveform", ["waveform", str(long), "--cache", str(tmp / "rss-cache")]),
                               ("spectrogram", ["spectrogram", str(long), "--cache", str(tmp / "rss-spec")])]:
                usage = tmp / "rss.txt"
                subprocess.run(["/usr/bin/time", "-f", "%M", "-o", str(usage), binary, *cmd], stdout=subprocess.DEVNULL, check=True, timeout=120)
                report["peak_rss_kib"][label] = int(usage.read_text().strip())
        if args.audio:
            command = [binary, "session"]
            if args.device is not None:
                command += ["--device", str(args.device)]
            # stderr file avoids a full stderr pipe stalling audio-device startup.
            with (tmp / "session.stderr").open("w+") as errors:
                process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors, text=True, bufsize=1)
                selector = selectors.DefaultSelector()
                selector.register(process.stdout, selectors.EVENT_READ)

                def receive():
                    if not selector.select(timeout=5):
                        raise TimeoutError("session did not respond within five seconds")
                    line = process.stdout.readline()
                    if not line:
                        errors.seek(0)
                        raise RuntimeError(errors.read())
                    return json.loads(line)

                def request(op, **fields):
                    process.stdin.write(json.dumps({"op": op, **fields}) + "\n")
                    process.stdin.flush()
                    return receive()

                def wait(predicate, after_generation=None):
                    deadline = time.monotonic() + 5
                    while time.monotonic() < deadline:
                        status = request("status")
                        if predicate(status):
                            return status
                        if status.get("error") and (after_generation is None or status["generation"] > after_generation):
                            raise AssertionError(status)
                        time.sleep(0.001)
                    raise TimeoutError(status)

                try:
                    assert receive()["event"] == "ready"
                    assert request("gain", value=0)["ok"]  # NEVER emit the synthetic test tone.
                    latencies = {"switch_ms": [], "seek_ms": []}
                    for path in [short, long, short, long, short]:
                        generation = request("status")["generation"]
                        start = time.perf_counter()
                        assert request("play", path=str(path))["ok"]
                        wait(lambda s: s["rendered_generation"] > generation)
                        latencies["switch_ms"].append((time.perf_counter() - start) * 1000)
                    for seconds in [1, 25, 2, 40, 5]:
                        generation = request("status")["generation"]
                        start = time.perf_counter()
                        assert request("seek", seconds=seconds)["ok"]
                        wait(lambda s: s["rendered_generation"] > generation)
                        latencies["seek_ms"].append((time.perf_counter() - start) * 1000)
                    request("pause")
                    time.sleep(0.05)
                    paused = request("status")
                    time.sleep(0.05)
                    assert request("status")["position_seconds"] == paused["position_seconds"]
                    assert request("rate", value=2)["ok"]
                    request("resume")
                    wait(lambda s: s["rate"] == 2)
                    assert request("rate", value=0)["ok"] is False
                    normalization = run("normalization", short, "--cache", tmp / "cache")
                    request("normalize", path=str(short), peak=normalization["source_peak"])
                    normalized = wait(lambda s: s["normalization_enabled"])
                    assert abs(normalized["normalization_gain"] - normalization["gain"]) < 1e-5
                    assert normalized["gain"] == 0, "normalization must not override mute"
                    assert normalized["rms_dbfs"] == [None, None]
                    generation = normalized["generation"]
                    request("play_normalized", path=str(short), peak=normalization["source_peak"])
                    wait(lambda s: s["rendered_generation"] > generation and s["normalization_enabled"])
                    request("normalize", path=str(short), peak=None)
                    wait(lambda s: not s["normalization_enabled"])
                    report["normalization"] = normalization
                    if (tmp / "60s.mp3").exists():
                        previous = request("status")["generation"]
                        request("play", path=str(tmp / "60s.mp3"))
                        wait(lambda s: s["rendered_generation"] > previous)
                        previous = request("status")["generation"]
                        request("seek", seconds=20)
                        wait(lambda s: s["rendered_generation"] > previous)
                    # Exercise natural EOF on a real callback, not only offline rendering.
                    previous = request("status")["generation"]
                    request("play", path=str(ROOT / "crates/frx-core/tests/fixtures/tone.mp3"))
                    wait(lambda s: s["generation"] > previous and s["state"] == "ended")
                    request("stop")
                    wait(lambda s: s["state"] == "stopped")
                    request("play", path=str(tmp / "missing.wav"))
                    failed = wait(lambda s: s["state"] == "error")
                    request("play", path=str(short))
                    wait(lambda s: s["state"] == "playing", after_generation=failed["generation"])
                    time.sleep(0.1)
                    before = request("status")["underrun_callbacks"]
                    time.sleep(1.0)
                    final_status = request("status")
                    steady_underruns = final_status["underrun_callbacks"] - before
                    assert steady_underruns == 0, final_status
                    final_status["path"] = "<temporary fixture>/60s.wav"
                    report["audio"] = {**latencies, "steady_1s_underruns": steady_underruns, "final_status": final_status,
                                       "note": "Muted CPAL stream; command-to-first-rendered-generation, includes polling, excludes DAC latency."}
                    for name, values in latencies.items():
                        report["audio"][name + "_median"] = statistics.median(values)
                    request("quit")
                    process.wait(timeout=5)
                    assert process.returncode == 0
                finally:
                    selector.close()
                    if process.poll() is None:
                        process.kill()
                    process.wait()
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
