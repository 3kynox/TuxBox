#!/usr/bin/env python3
"""Compare the TuxBox AAC transform with FFmpeg on six independent audio channels."""
import argparse
from array import array
import hashlib
import json
import math
from pathlib import Path
import shutil
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest


def adts_input(video):
    if video:
        args = ["-i", str(video), "-map", "0:a:0", "-frames:a", "96", "-c:a", "copy"]
    else:
        tones = "|".join(f"0.1*sin(2*PI*{hz}*t)" for hz in (191, 233, 307, 61, 379, 461))
        args = ["-f", "lavfi", "-i", f"aevalsrc={tones}:s=48000:d=0.5:c=5.1",
                "-c:a", "aac", "-profile:a", "aac_low", "-b:a", "384k"]
    return subprocess.check_output(["ffmpeg", "-v", "error", *args, "-f", "adts", "pipe:1"])


def write_packets(data, dest):
    pos = count = 0
    with dest.open("wb") as stream:
        while pos < len(data):
            h = data[pos:pos + 7]
            if (len(h) != 7 or h[0] != 0xff or h[1] & 0xf6 != 0xf0 or
                    h[2] >> 2 != 0x13 or ((h[2] & 1) << 2 | h[3] >> 6) != 6 or h[6] & 3):
                raise ValueError("Expected one raw AAC-LC access unit per 48 kHz 5.1 ADTS frame")
            size = (h[3] & 3) << 11 | h[4] << 3 | h[5] >> 5
            header = 7 if h[1] & 1 else 9
            if size <= header or size > len(data) - pos:
                raise ValueError("Invalid ADTS frame length")
            stream.write(struct.pack("<I", size - header))
            stream.write(data[pos + header:pos + size])
            pos += size
            count += 1
    if not count:
        raise ValueError("No AAC access units")
    return count


def compare(data, raw, info, count):
    oracle = subprocess.check_output(["ffmpeg", "-v", "error", "-f", "aac", "-i", "pipe:0",
                                      "-f", "f32le", "pipe:1"], input=data)
    expected = array("f")
    expected.frombytes(oracle)
    if info["float"] == "1" and info["bits"] == "32":
        actual = array("f")
        actual.frombytes(raw)
    elif info["float"] == "0" and info["bits"] == "16":
        samples = array("h")
        samples.frombytes(raw)
        actual = array("f", (x / 32768 for x in samples))
    else:
        raise ValueError("Unexpected PCM format")
    if (int(info["access_units"]) != count or len(raw) != int(info["pcm_bytes"]) or len(actual) != count * 1024 * 6 or
            len(actual) != len(expected) or int(info["rate"]) != 48000 or int(info["channels"]) != 6):
        raise ValueError("Decoded PCM dimensions differ from the reference")
    if not all(math.isfinite(x) for x in actual) or not all(math.isfinite(x) for x in expected):
        raise ValueError("Invalid floating-point PCM")
    error = max(abs(x-y) for x, y in zip(actual, expected))
    similarity = []
    for channel in range(6):
        got, want = actual[channel::6], expected[channel::6]
        norm = math.sqrt(sum(x*x for x in got) * sum(x*x for x in want))
        if not norm:
            raise ValueError(f"Silent channel {channel} cannot verify channel order")
        similarity.append(sum(x*y for x, y in zip(got, want)) / norm)
    if error >= 0.0001 or min(similarity) < 0.99999:
        raise ValueError(f"PCM mismatch: max error {error}, per-channel similarity {similarity}")
    return {"frames": count * 1024, "pcm_max_absolute_error": error, "channel_cosine_similarity": similarity,
            "pcm_sha256": hashlib.sha256(raw).hexdigest(), "reference_float_sha256": hashlib.sha256(oracle).hexdigest()}


def read_receipt(path):
    return dict(line.split("=", 1) for line in path.read_text().splitlines()) if path.exists() else {}


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--proton", required=True, type=Path)
    ap.add_argument("--prefix", required=True, type=Path)
    ap.add_argument("--output", required=True, type=Path)
    ap.add_argument("--build", type=Path, default=ROOT / "build/aac", help="output of python build.py aac")
    ap.add_argument("--video", type=Path, help="read the first 96 packets of a 48 kHz 5.1 AAC-LC track")
    args = ap.parse_args()
    host = Host("proton", args.proton, args.prefix)
    data = adts_input(args.video)
    report = {"host": host.version, "input": "supplied video" if args.video else "six generated tones",
              "adts_sha256": hashlib.sha256(data).hexdigest()}
    with TemporaryDirectory(dir=ROOT / "build", prefix="aac-check-") as temp:
        stage = Path(temp)
        shutil.copy2(args.build / "aac.exe", stage / "aac.exe")
        count = write_packets(data, stage / "access-units.bin")
        # Run the prefix's registered decoder first, before staging the project DLL.
        control = host.run(stage / "aac.exe", ["decode", "control.txt"], capture=True, timeout=90)
        report["registered_control"] = {"exit": control.returncode, "receipt": read_receipt(stage / "control.txt")}
        shutil.copy2(args.build / "MSAudDecMFT.dll", stage / "MSAudDecMFT.dll")
        report.update(dll_sha256=digest(stage / "MSAudDecMFT.dll"), guest_sha256=digest(stage / "aac.exe"))
        # Wine may prefer its built-in server even with an absolute LoadLibrary path.
        host.env["WINEDLLOVERRIDES"] = ";".join(filter(None, (host.env.get("WINEDLLOVERRIDES"), "msauddecmft=n")))
        run = host.run(stage / "aac.exe", ["decode-local", "result.txt"], capture=True, timeout=90)
        info = read_receipt(stage / "result.txt")
        report.update(exit=run.returncode, receipt=info, access_units=count, passed=False)
        if run.returncode:
            sys.stderr.buffer.write(run.stdout[-1500:])
        else:
            report.update(compare(data, (stage / "audio.pcm").read_bytes(), info, count), passed=True)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print("AAC PCM comparison passed." if report["passed"] else f"AAC comparison failed: {info.get('error', 'no guest result')}")
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
