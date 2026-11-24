#!/usr/bin/env python3
"""Run the synthetic RDNA2 wave-guard fixture on a real wave32 GPU.

Build the emitter first:
    cargo build --manifest-path tools/shader-compiler/Cargo.toml
        --target x86_64-unknown-linux-gnu --example wave_guard
The existing D3D12 readback guest checks a guarded UAV view. No game data
is required. Only the translator's input and output are instrumented.
"""
import argparse
import json
import random
import struct
import subprocess
import sys
from pathlib import Path
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import find_dxc
from frosty_gpu_guest import run_gpu_guest
from project_tuxbox.inspect import digest


def instrument(hlsl):
    replacements = {
        "    v0 = gtid.x; v1 = gtid.y; v2 = gtid.z;": (
            "    uint input = output[gtid.x];\n"
            "    exec_lo = input >> 31;\n"
            "    v0 = input & 0x7fffffffu; v1 = gtid.y; v2 = gtid.z;"
        ),
        "    // 0x0044 s_cmp_eq_u32": "    uint first = s7;\n    // 0x0044 s_cmp_eq_u32",
        "    // 0x0094 s_endpgm": (
            "    output[32u + gtid.x] = first;\n"
            "    output[64u + gtid.x] = s7;\n"
            "    output[96u + gtid.x] = uint(j23);\n"
            "    output[128u + gtid.x] = exec_lo & 1u;\n"
            "    // 0x0094 s_endpgm"
        ),
    }
    if "[WaveSize(32)]" not in hlsl:
        raise ValueError("fixture must require wave32")
    for original, replacement in replacements.items():
        if hlsl.count(original) != 1:
            raise ValueError(f"unexpected fixture: {original}")
        hlsl = hlsl.replace(original, replacement)
    return "RWBuffer<uint> output : register(u0);\n" + hlsl


def cases():
    rng = random.Random(0x53434152)
    full = (1 << 32) - 1
    yield "full", full, list(range(32))
    yield "empty", 0, [123] * 32
    yield "odd", 0xaaaaaaaa, list(range(1, 33))
    yield "last", 1 << 31, [91] * 32
    yield "zero_sum", full, [0] * 32
    yield "wrap_zero", full, [0x40000000] * 4 + [0] * 28
    yield "wrap_nonzero", full, [0x7fffffff] * 32
    yield "lane_zero", 1, [17] * 32
    for i in range(24):
        yield f"random{i:02}", rng.getrandbits(32), [rng.getrandbits(31) for _ in range(32)]


def seed_and_expected(mask, values):
    seed = [0x76540000 + i for i in range(384)]
    active = [(mask >> lane) & 1 for lane in range(32)]
    seed[96:128] = [v | (a << 31) for v, a in zip(values, active)]
    total = sum(v for v, a in zip(values, active) if a) & 0xffffffff
    expected = seed.copy()
    expected[128:160] = [total] * 32
    second = (total + sum(active)) & 0xffffffff if total != 0 else total
    expected[160:192] = [second] * 32
    expected[192:224] = [int(total == 0)] * 32
    expected[224:256] = active
    return struct.pack("<384I", *seed), struct.pack("<384I", *expected)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("emitter", "provider", "proton", "prefix", "compiler-dir", "work", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--adapter", required=True)
    args = parser.parse_args()
    work = args.work.resolve()
    work.mkdir(parents=True, exist_ok=True)
    dxc = Path(find_dxc(None)).resolve()
    emitted = subprocess.check_output([args.emitter.resolve(), "32"], text=True, timeout=30)
    with TemporaryDirectory(dir=work) as tmp:
        trial = Path(tmp)
        source = trial / "guard.hlsl"
        source.write_text(instrument(emitted))
        blob = trial / "guard.cso"
        subprocess.run([dxc, "-T", "cs_6_6", "-E", "main", "-Fo", blob, source], check=True, timeout=30)
        subprocess.run([dxc.with_name("dxv"), blob], check=True, timeout=30)
        expected = {}
        for name, mask, values in cases():
            seed, expected[name] = seed_and_expected(mask, values)
            (trial / f"{name}.seed").write_bytes(seed)
            (trial / f"{name}.cso").hardlink_to(blob)
        (trial / "cases.txt").write_text("".join(f"{name} 0 uint 1\n" for name in expected))
        exe, status, fields = run_gpu_guest(
            ROOT / "tools/reference/independent-shaders/compute.cpp", "wave-guard.exe",
            trial, work, args.proton.resolve(), args.prefix, args.provider, args.compiler_dir,
        )
        rows = []
        for name, reference in expected.items():
            path = trial / f"{name}.bin"
            actual = path.read_bytes() if path.is_file() else b""
            rows.append(dict(case=name, matched=actual == reference,
                             readback_sha256=digest(path) if path.is_file() else None))
        passed = status == 0 and fields.get("adapter") == args.adapter and all(row["matched"] for row in rows)
        report = dict(
            passed=passed, scope="Synthetic native wave32 guarded reductions, instrumented inputs and outputs only",
            adapter=fields.get("adapter"), proton=(args.proton / "version").read_text().strip(),
            guest_exit=status, cases=len(rows), compared_words=len(rows)*384,
            result_words=len(rows)*128, input_and_guard_words=len(rows)*256,
            emitter_sha256=digest(args.emitter), hlsl_sha256=digest(source), dxil_sha256=digest(blob),
            dxc_sha256=digest(dxc), guest_sha256=digest(exe),
            d3d12_dlls={name: digest(trial / name) for name in ("d3d12.dll", "d3d12core.dll")},
            rows=rows,
        )
        if fields.get("error"):
            report["error"] = fields["error"]
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2) + "\n")
    if not passed:
        raise SystemExit(f"GPU readback failed: {args.output}")
    print(f"PASS: {report['cases']} cases, {report['compared_words']} words, adapter {report['adapter']}")


if __name__ == "__main__":
    main()
