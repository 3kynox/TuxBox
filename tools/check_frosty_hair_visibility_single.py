#!/usr/bin/env python3
"""Check single-sample hair visibility compaction with full GPU readbacks."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from frosty_gpu_guest import run_gpu_guest
from check_frosty_hair_visibility_compact import GUARD, words
from check_frosty_shaders import GLOBAL_SHA, containers, decoder, decompress, global_archive
from project_tuxbox.inspect import digest

SHADERS = {979: (4, "695a0af2716be84c7101edcb8970f48f40f2115a6f1e89b74c1f1d74f5dfafbb"),
           982: (8, "f5c3578c399acdb88eb90fd1fd852157a627056799680cb2cc325a84114db424")}


def inputs(work, case, height):
    cases = {"none": set(), "all": set(range(8 * height)),
             "sparse": {1, 7, 15, 24, 31} | ({32, 47, 60} if height == 8 else set()),
             "last": {8 * height - 1}, "depth": {1, 7, 15, 24, 31}}
    mask = cases[case]
    active = {7, 24} if case == "depth" else mask
    rejected = mask - active
    cb = [0] * 64
    cb[:4], cb[4], cb[8] = [8, 8, 0, 0], 512, 0x3f800000
    words(work / "cb.bin", cb)
    words(work / "t0.bin", [0x3f400000 if i in rejected else 0x3e800000 for i in range(64)])
    words(work / "t1.bin", [x for i in range(64) for x in ((i + 1 if i in mask else 0xffffffff), 0)])
    words(work / "t2.bin", [0x3f000000] * 64)
    words(work / "t3.bin", [0x3f000000] * 64)
    words(work / "t4.bin", [0] * 128)
    words(work / "t5.bin", [0] * 64)
    words(work / "t6.bin", [1])
    words(work / "t7.bin", [0, 0])
    words(work / "u0.bin", [5])
    words(work / "u1.bin", [GUARD] * 64)
    words(work / "u2.bin", [GUARD] * 1024)
    words(work / "u3.bin", [GUARD] * 1024)
    words(work / "u4.bin", [GUARD] * 64)
    return active


def verify(work, active, height):
    def read(slot):
        data = (work / f"out{slot}.bin").read_bytes()
        return struct.unpack(f"<{len(data) // 4}I", data)
    counter, index, record, coords, coverage = (read(i) for i in range(5))
    if counter[0] != 5 + len(active):
        raise ValueError(f"counter {counter[0]}")
    seen = set()
    for pixel in range(64):
        x, y = pixel % 8, pixel // 8
        packed, weight = index[y * 64 + x], coverage[y * 64 + x]
        if y >= height:
            if (packed, weight) != (GUARD, GUARD):
                raise ValueError(f"untouched pixel {pixel} changed")
        elif pixel not in active:
            if (packed, weight) != (0, 0):
                raise ValueError(f"empty pixel {pixel} changed")
        else:
            dst = packed & 0x3ffffff
            if packed >> 26 != 1 or not 5 <= dst < 5 + len(active) or dst in seen:
                raise ValueError(f"invalid compacted index at pixel {pixel}")
            seen.add(dst)
            if (record[dst * 2:dst * 2 + 2] != (0x7fffffff, pixel + 1)
                    or coords[dst * 2:dst * 2 + 2] != (x, y) or weight != 0x3f000000):
                raise ValueError(f"incorrect output at pixel {pixel}")
    for dst in [*range(5), *range(5 + len(active), 512)]:
        if record[dst * 2:dst * 2 + 2] != (GUARD, GUARD) or coords[dst * 2:dst * 2 + 2] != (GUARD, GUARD):
            raise ValueError(f"buffer guard overwritten at {dst}")
    return {"active_pixels": len(active), "image_pixels_checked": 64, "buffer_slots_checked": 512, "outputs_checked": 5}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("mount", "xshader", "provider", "proton", "prefix", "compiler-dir", "work", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    args = parser.parse_args()
    work, proton = args.work.resolve(), args.proton.resolve()
    work.mkdir(parents=True, exist_ok=True)
    archive, entries = global_archive(args.mount)
    os.environ["TUXBOX_XSHADER"] = str(args.xshader.resolve())
    results, adapters, guest_hash = {}, set(), None
    for entry, (height, expected) in SHADERS.items():
        ((_, native, _),) = containers(decompress(archive, entries[entry], decoder()))
        if hashlib.sha256(native).hexdigest() != expected:
            raise ValueError(f"unexpected global shader {entry}")
        cases = {}
        for name in ("none", "all", "sparse", "last", "depth"):
            with TemporaryDirectory(dir=work) as temp:
                trial = Path(temp)
                (trial / "retail.dxbc").write_bytes(native)
                active = inputs(trial, name, height)
                exe, status, actual = run_gpu_guest(ROOT / "tools/reference/hair-visibility-single/guest.cpp", "guest.exe",
                                                    trial, work, proton, args.prefix, args.provider, args.compiler_dir)
                if status or actual.get("status") != "ok":
                    raise RuntimeError(f"shader {entry} {name}: {status}, {actual}. See {work / 'gpu.log'}")
                guest_hash = digest(exe)
                adapters.update(line.split("Found device: ", 1)[1] for line in (work / "gpu.log").read_text().splitlines()
                                if "Found device: " in line)
                cases[name] = verify(trial, active, height)
        results[entry] = {"cooked_sha256": expected, "workgroup": [8, height, 1], "cases": cases}
    result = {"scope": "Two packaged single-sample hair visibility compaction shaders through TuxBox’s guest provider and runtime shader worker, with CPU-modeled GPU readback. Does not validate rendered hair.",
              "archive_sha256": GLOBAL_SHA, "adapters": sorted(adapters), "proton": (proton / "version").read_text().strip(),
              "xshader_sha256": digest(args.xshader), "provider_sha256": digest(args.provider), "guest_sha256": guest_hash, "shaders": results}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
