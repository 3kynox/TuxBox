#!/usr/bin/env python3
"""Dispatch Frosty's packaged Nanite InstanceCull and check its compacted queues and counters."""
import argparse
import json
from pathlib import Path
import shutil
import struct
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain
from check_frosty_shaders import GLOBAL_SHA, containers, decompress, global_archive
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest
from project_tuxbox.runtime_shaders import shader_session
from project_tuxbox.shader_inventory import decoder
from project_tuxbox.shader_prep import compiler_tools


PROGRAMS = {
    3425: ("b07d956ff61846c8e25f0de1fd0f72d3d72a4910fca9599d990b401d34fc7637", 1),
    3444: ("06444185d9d7411b6a4cdf41ce1bd9a549ed6ab66add5a04b06a6209874a760d", 2),
    3452: ("85fe16be29d6ba9dae77cfc7fb2885b99ed3acb35ce0550e0b7464a90c361c61", 1),
    3473: ("cbe4d099f555f59bd1c3c5c2667918dcf5e01340cf7fbc4cb23f9374f15ffec9", 2),
}
SLOTS = 128
GUARD = 0xa5c3e749
QUEUE_BASE = 32
INITIAL_HEAD = 3
WIDTH = HEIGHT = 9


def save(path, words):
    path.write_bytes(struct.pack(f"<{len(words)}I", *words))


def load(path):
    return [word for word, in struct.iter_unpack("<I", path.read_bytes())]


def f32(number):
    return struct.unpack("<I", struct.pack("<f", number))[0]


def prepare_case(path, visible, outside, *, instances=64, groups=1, capacity=128, stats=False, image_words=2, occluders=(), protected_depth=None):
    path.mkdir()
    cb = [0] * 64
    cb[:5] = [instances, capacity, 64, 0, SLOTS]
    cb[8] = QUEUE_BASE << 4
    cb[10] = 64
    cb[11] = int(stats)
    save(path / "b0", cb)
    # One primitive, with its local center at zero and extent 1/8 on each axis.
    # Its instance stream stores metadata, a packed unit transform and translation in separate columns.
    primitive = [0] * (38 * 4)
    primitive[0] = 0x4000 if occluders else 0
    primitive[27 * 4:27 * 4 + 3] = [f32(.125)] * 3
    save(path / "t0", primitive)
    scene = [0] * (SLOTS * 3 * 4)
    for index in range(SLOTS):
        scene[4 * index] = 0 if index in visible or index in outside or index in occluders else 0xfffff
        transform = 4 * (SLOTS + index)
        scene[transform:transform + 4] = [0x80008000, 0x4000, 0x80018001, 0x008e8001]
        scene[4 * (2 * SLOTS + index)] = f32(4 if index in outside else 0)
    save(path / "t1", scene)
    save(path / "t2", [0] * 16)
    view = [0] * 192
    # Orthographic X and Y, fixed Z=0.75 and W=1. A center at X=4 falls outside.
    view[48] = view[53] = view[63] = f32(1)
    view[62] = f32(.75)
    view[162:164] = [WIDTH, HEIGHT]
    view[78:80] = [f32(.375), f32(.5)]
    view[92:95] = [f32(-.125), f32(-.125), f32(-1)]
    save(path / "t3", view)
    # Each packed impostor sample has the same material ID (0x21) and coverage.
    save(path / "t4", [0x7f217f21] * (12 * 1024) if occluders else [0] * 16)
    pixels = []
    for i in range(WIDTH * HEIGHT):
        if image_words == 2:
            pixels.append(i)
        pixels.append(f32(.25))
    center = ((HEIGHT // 2) * WIDTH + WIDTH // 2) * image_words
    if protected_depth is not None:
        pixels[center:center + image_words] = ([0xffffffff] if image_words == 2 else []) + [f32(protected_depth)]
    save(path / "u0", pixels)
    if occluders and protected_depth is None:
        # All selected impostors project to the middle texel at depth .375 / .5.
        # Unsigned 64-bit max breaks equal-depth ties with the larger instance ID.
        pixels[center:center + image_words] = ([0x800000a1 + (max(occluders) << 8)] if image_words == 2 else []) + [f32(.75)]
    queue = [GUARD] * (2 * SLOTS + 16)
    counters = [GUARD] * 32
    counters[4:6] = [INITIAL_HEAD, 19]
    stats_buffer = [GUARD] * 32
    stats_buffer[4] = 43
    for i, words in enumerate((queue, counters, stats_buffer), 1):
        save(path / f"u{i}", words)
    active = sorted(i for i in visible if i < min(instances, groups * 64))
    return dict(groups=groups, active=active, capacity=capacity, stats=stats, pixels=pixels)


def verify_case(path, case):
    queue, counters, stats = (load(path / f"u{i}") for i in range(1, 4))
    observed, read_counts, read_stats = (load(path / f"out{i}") for i in range(1, 4))
    if len(observed) != len(queue):
        raise ValueError(f"{path.name} truncated queue readback")
    active = case["active"]
    counters[4] += len(active)
    counters[5] += len(active)
    if case["stats"]:
        stats[4] += len(active)
    if counters != read_counts or stats != read_stats:
        raise ValueError(f"{path.name} counter mismatch: expected {counters[4:6]}, {stats[4]}, "
                         f"got {read_counts[4:6]}, {read_stats[4]}")
    stored = min(len(active), max(0, case["capacity"] - INITIAL_HEAD))
    # Atomic reservations may order entire groups differently. Within a group the
    # source shader uses each visible lane's rank, so its records must remain consecutive.
    first = QUEUE_BASE // 4 + 2 * INITIAL_HEAD
    ids = [word >> 3 for word in observed[first:first + 2 * stored:2]]
    groups = [i // 64 for i in ids]
    order = list(dict.fromkeys(groups))
    expected = [i for group in order for i in active if i // 64 == group][:stored]
    if len(expected) != stored or ids != expected or (stored == len(active) and sorted(ids) != active):
        raise ValueError(f"{path.name} compacted instance IDs: expected visible {active}, got {ids}")
    for offset, index in enumerate(ids):
        queue[first + 2 * offset:first + 2 * offset + 2] = [(index << 3) | 1, 0]
    if observed != queue:
        diff = next((i, a, b) for i, (a, b) in enumerate(zip(queue, observed)) if a != b)
        raise ValueError(f"{path.name} queue mismatch at word {diff[0]}: {diff[1]:08x} != {diff[2]:08x}")
    pixels = load(path / "out0")
    if pixels != case["pixels"]:
        diff = [(i, hex(a), hex(b)) for i, (a, b) in enumerate(zip(case["pixels"], pixels)) if a != b]
        raise ValueError(f"{path.name} visibility image mismatches: {diff[:12]}")
    changed = sum(a != b for a, b in zip(pixels, load(path / "u0")))
    words = len(pixels) // (WIDTH * HEIGHT)
    middle = ((HEIGHT // 2) * WIDTH + WIDTH // 2) * words
    return dict(checked_texels=WIDTH * HEIGHT, changed_visibility_words=changed,
                center_words=[f"{value:08x}" for value in pixels[middle:middle + words]],
                groups=case["groups"], visible=len(active), queue_ids=ids, counters=read_counts[4:6],
                stats_counter=read_stats[4], checked_words=len(queue) + len(counters) + len(stats))


def dispatch(original, host, adapter, trial, entry):
    fingerprint, words = PROGRAMS[entry]
    source = trial / "instance.dxbc"
    source.write_bytes(original)
    if digest(source) != fingerprint:
        raise ValueError("Unexpected source for Frosty InstanceCull")
    cases = {}

    def add(name, visible, outside, **options):
        cases[name] = prepare_case(trial / name, visible, outside, image_words=words, **options)

    add("empty", [], [])
    add("clip", [], [2, 34])
    add("upper", [37], [35], stats=True)
    add("sparse", [3, 7, 31, 32, 47, 63], [4, 35])
    add("capacity", [3, 15, 36, 47, 61], [4], capacity=5, stats=True)
    add("two-groups", [3, 34, 61, 64, 95, 106, 122], [2, 98], instances=107, groups=2, stats=True)
    add("visibility", [], [], occluders=[0, 37], stats=True)
    add("visibility-guard", [], [], occluders=[37], protected_depth=.875)
    if words == 2:
        add("visibility-tie", [], [], occluders=[37], protected_depth=.75)
    (trial / "image.txt").write_text(f"{WIDTH} {HEIGHT} {words}\n")
    (trial / "cases.txt").write_text("".join(f"{name} {case['groups']}\n" for name, case in cases.items()))
    exe = trial / "cull.exe"
    cache = trial / "temp/tuxbox_shader_cache"
    compiled = []
    with shader_session(cache, host.env, compiled.append):
        session = cache / host.env["TUXBOX_SHADER_SESSION"]
        result = host.run(exe, ["--adapter", adapter, "--report", "result.txt"], capture=True, timeout=150)
        receipt = trial / "result.txt"
        line = receipt.read_text().strip() if receipt.exists() else ""
        if result.returncode or line != f"instance-cull PASS pci={adapter} cases={len(cases)}":
            sys.stderr.buffer.write(result.stdout[-4000:])
            raise ValueError(f"InstanceCull GPU test failed: {line}, compilation: {compiled}")
        if compiled != [f"Compiled Xbox shader {fingerprint}"]:
            raise ValueError(f"Unexpected runtime compilation: {compiled}")
        dxil = digest(session / (fingerprint + ".cso"))
    return dict(entry=entry, visibility_format="R32G32_UINT" if words == 2 else "R32_UINT",
                source_sha256=fingerprint, dxil_sha256=dxil,
                cases={name: verify_case(trial / name, case) for name, case in cases.items()})


def run(mount, host, adapter, provider, trial, selected):
    payload, entries = global_archive(mount)
    decode = decoder()
    build = Toolchain()
    obj = build.compile(ROOT / "tools/reference/instance-cull/guest.cpp", trial, "/W4", "/WX",
                        f"/I{ROOT / 'runtime/d3d12'}")
    exe = trial / "cull.exe"
    build.link(exe, obj, str(ROOT / "build/d3d12-smoke/d3d12_xs.lib"), "dxgi.lib", "kernel32.lib")
    runs = []
    for entry in selected:
        work = trial / str(entry)
        work.mkdir()
        shutil.copyfile(provider, work / "d3d12_xs.dll")
        shutil.copyfile(exe, work / exe.name)
        ((_, original, _),) = containers(decompress(payload, entries[entry], decode))
        runs.append(dispatch(original, host, adapter, work, entry))
    return dict(host=host.version, adapter=adapter, global_archive_sha256=GLOBAL_SHA,
                name="InstanceCull", guest_sha256=digest(exe), provider_sha256=digest(provider),
                compiler_sha256=digest(compiler_tools()[0]), programs=runs,
                scope="Original compute shader and root signature at Xbox device slot 62. The fixtures check frustum "
                      "culling, wave allocation, clipped queue writes, optional statistics and impostor visibility atomics. "
                      "Every dispatch reads back the full visibility image, queue and both counter buffers.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mount", type=Path)
    parser.add_argument("--entry", type=int, choices=sorted(PROGRAMS), help="test one shader (default: all four)")
    parser.add_argument("--adapter", required=True)
    parser.add_argument("--proton", type=Path)
    parser.add_argument("--prefix", type=Path)
    parser.add_argument("--provider", type=Path, default=ROOT / "build/d3d12-smoke/d3d12_xs.dll")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with TemporaryDirectory(prefix="instance-cull-", dir=ROOT / "build") as tmp:
        result = run(args.mount, Host("proton", args.proton, args.prefix), args.adapter.lower(),
                     args.provider, Path(tmp), [args.entry] if args.entry else sorted(PROGRAMS))
    args.output.parent.mkdir(exist_ok=True, parents=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
