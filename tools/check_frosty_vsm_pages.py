#!/usr/bin/env python3
"""Read back Frosty's physical shadow page allocation and initialization shaders."""
import argparse
import json
from pathlib import Path
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from frosty_shadow_instances import shadow_instances, shadow_clusters
from check_frosty_instance_cull import scene_inputs, f32
from build import Toolchain
from frosty_vsm_packing import compact_views, free_pages, unordered_wave_ranges
from check_frosty_shaders import containers, decoder, decompress, global_archive
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest
from project_tuxbox.shader_prep import compiler_tools

GUARD = 0xdeadbeef


def compact_feedback(active, count, capacity, valid_limit):
    # One guest wave reserves a contiguous range before checking the
    # referenced material index. Invalid indices still consume a slot.
    cb = [0] * 64
    cb[12], cb[6], cb[18] = count, capacity, valid_limit
    records = [word for i in range(64) for word in (0x12340000 + i, 0x40 + i)]
    indices = list(range(64))
    hits = [0] * 64
    for lane in active:
        hits[lane] = 7 + lane
    seed = [[GUARD] * 160]
    seed[0][0] = 2
    expected = [seed[0].copy()]
    selected = [i for i in active if i < count]
    expected[0][0] += len(selected)
    for rank, lane in enumerate(selected):
        dst = 3 + rank
        index = indices[lane]
        if dst < capacity and index < valid_limit:
            expected[0][2 * dst] = records[2 * index]
            expected[0][2 * dst + 1] = (hits[lane] << 16) | records[2 * index + 1]
    return cb, [records, indices, hits], seed, [8], 4, 1, 1, expected, {
        "selected": len(selected), "count": count,
        "capacity": capacity, "valid_limit": valid_limit,
    }


def cull_voxel(active, count, small):
    cb = [0] * 64
    cb[8] = count
    cb[19] = 0x3f000000  # Minimum radius 0.5.
    source = [0] * (128 * 4)
    for lane in range(64):
        center = 0 if lane in active else 0x42c80000  # 0.0 or 100.0.
        extent = 0 if lane in small else 0x3f800000  # 0.0 or 1.0.
        source[8 * lane:8 * lane + 3] = [center] * 3
        source[8 * lane + 4:8 * lane + 7] = [extent] * 3
    selected = [lane for lane in active if lane < count and lane not in small]
    seed = [[5, GUARD], [GUARD] * 80]
    expected = [row.copy() for row in seed]
    expected[0][0] += len(selected)
    expected[1][5:5 + len(selected)] = selected
    return cb, source, seed, [4, 4], 16, 1, 1, expected, {
        "count": count, "selected": len(selected),
    }


def command_space(counts):
    cb = [0] * 64
    cb[0] = len(counts)
    source = [0] * 320
    for index, count in enumerate(counts):
        source[5 * index + 1] = count
    seed = [[GUARD] * 80 for _ in range(3)]
    seed[1][0] = 13
    expected = [array.copy() for array in seed]
    offset = 13
    for index, count in enumerate(counts):
        expected[0][index] = expected[2][index] = offset if count else 0
        offset += count
    expected[1][0] = offset
    return cb, source, seed, [4, 4, 4], 4, 1, 1, expected, {
        "commands": len(counts), "instances": sum(counts),
    }


def card_update(count):
    constants = [0] * 64
    constants[24] = 0x3f800000
    constants[26] = count
    sources = [[0] * 512 for _ in range(4)]
    seed = [[0] * 4096 for _ in range(6)]
    for card in range(count):
        # Each card occupies five 16-byte records. These fields give it
        # two units of work, which the shader adds to two statistics counters.
        seed[0][card * 20 + 2] = 0x43000000
        seed[0][card * 20 + 3] = 0x3f800000
    expected = [data.copy() for data in seed]
    expected[5][1] = expected[5][3] = 2 * count
    return ([constants, [0] * 272], sources, seed, [16, 4, 4, 4, 4, 4],
            4, 1, 1, expected, {"cards": count, "expected_work": 2 * count})


def words(path, data):
    path.write_bytes(struct.pack(f"<{len(data)}I", *data))


def allocation(entry, count, layer):
    # All requested lanes belong to a single guest wave. Its pop order is
    # deterministic even if the host uses smaller physical subgroups.
    offset = layer * 21845
    size = offset + 260
    requests, flags, mapping = [0] * size, [0] * size, [GUARD] * size
    physical = [GUARD] * 160
    free = [(7 * i + 3) % 73 for i in range(32)] + [GUARD] * 16
    free[37] = count
    active = [2, 9, 31, 32, 35, 56, 63]
    for i in [*active, 47]:
        requests[offset+i] = 8 if i & 1 else 1
    flags[offset+47] = 0x1234
    cb = [0] * 64
    cb[35], cb[38], cb[39] = 37, 7, 3
    seed = [flags, mapping, physical, free]
    if entry == 1235:
        seed.insert(0, [13, GUARD])
    expected = [s.copy() for s in seed]
    mapped, table, meta, pool = expected[-4:]
    pool[37] = (count - len(active)) & 0xffffffff
    if entry == 1235:
        expected[0][0] += len(active)
    for j, lane in enumerate(active[:count]):
        pid = free[count-j-1]
        f = 7 | (requests[offset+lane] & 8)
        mapped[offset+lane] = meta[2*pid] = f
        table[offset+lane] = 0x08000000 | ((pid >> 3) << 10) | (pid & 7)
    return (cb, requests, seed, [4, 4, 8, 4] if entry == 1224 else [4, 4, 4, 8, 4],
            4, 1, layer+1, expected, {"requested": len(active), "available": count, "allocated": min(count, len(active)), "layer": layer})


def selection(entry, count, extra):
    # The second metadata word is deliberately unrelated to its page flags.
    active_flags = {2: 3, 9: 5, 31: 7, 32: 3, 35: 1, 47: 6, 56: 5, 63: 7, 64: 5}
    metadata = [word for i in range(256) for word in (active_flags.get(i, 0), 7)]
    cb = [0] * 64
    cb[35:38] = [count, int(extra), 256]
    hzb = entry == 1215
    merge = entry in (1219, 1230)
    stats = {1219: 12, 1236: 13}.get(entry)
    base = [i for i in range(count) if active_flags.get(i, 0) & 1 and (hzb or active_flags[i] & 6)]
    second = [256+i for i in base if extra and not merge and (hzb or active_flags[i] & 4)]
    indirect = [32, GUARD, GUARD, GUARD, 17, GUARD] if hzb else [32, GUARD, GUARD, GUARD]
    seed = [indirect, [GUARD] * 20]
    if stats is not None:
        seed.insert(0, [GUARD] * stats + [17, GUARD])
    out = [a.copy() for a in seed]
    selected = base + second
    out[-2][0] += 16 * len(selected)
    if hzb:
        out[-2][4] += len(selected)
    out[-1][2:2+len(selected)] = selected
    if stats is not None:
        out[0][stats] += len(selected)
    return cb, metadata, seed, [4] * len(seed), 8, 1, 1, out, {
        "pages": count, "selected": len(base), "additional_pages": len(second), "initial_indirect_x": 32}


def visibility_instance(visible, occluders, protected=None, post=False, stats_variant=False):
    # Exercise the packaged shader with an actual depth texture and visibility UAVs.
    with TemporaryDirectory(dir=ROOT / "build") as tmp:
        scene = Path(tmp) / "scene"
        scene_inputs(scene, visible, [3], instances=64, capacity=128, stats=True,
                     image_words=1, view_centers=(0,), hzb_depth=0, hzb_views=(0,),
                     occluders=occluders, protected_depth=protected)
        def read(name):
            data = (scene / name).read_bytes()
            return list(struct.unpack(f"<{len(data) // 4}I", data))
        cb = read("b0")
        srv = [read(name) for name in ("t0", "t1", "t2", "hzb", "t3", "t4")]
        srv[3] = srv[3][:64]
    depth = f32(protected if protected is not None else .25)
    seed = [[0x77] * (9 * 9 * 4), [depth] * 81,
            [0xa5c3e749] * 512, [0] * 64, [0xa5c3e749] * 512, [0] * 22]
    if post:
        srv += [[word for lane in visible for word in (lane, 0)] or [0, 0],
                [1, 1, 1, len(visible)] + [0xa5c3e749] * 4]
        seed = seed[:2] + [[0xa5c3e749] * 512, [0xa5c3e749] * 11]
        seed[3][9:11] = [3, 29]
    if stats_variant:
        seed += [[0xa5c3e749] * 14]
        seed[-1][8] = 43
    expected = [row.copy() for row in seed]
    if stats_variant and post:
        expected[-1][7] = len(visible)
        expected[-1][8] = 43 + (0 if occluders else len(visible))
    if stats_variant and not post and not occluders:
        expected[-1][4] = 0xa5c3e749 + len(visible)
    if post and not occluders:
        start = (32 << 4) // 8 // 4 + 128 * 2 + 3 * 3
        for i, lane in enumerate(visible):
            expected[2][start + i * 3:start + i * 3 + 3] = [(lane << 3) | 5, 0, 255]
        expected[3][9:11] = [3 + len(visible), 29 + len(visible)]
    elif not post and not occluders:
        expected[4][8:16] = [9, 0, 249, 0, 257, 0, 505, 0]
        expected[5][4:6] = [4, 4]
    elif protected is None:
        expected[0][160:164] = [0x800000a1 + (occluders[0] << 8), 0, 0, 0]
        expected[1][40] = f32(.75)
    # CopyTextureRegion has 256-byte row alignment; padding is unspecified.
    for i, width in ((0, 36), (1, 9)):
        expected[i] = [v for y in range(9) for v in
                       (expected[i][y * width:(y + 1) * width] + [0] * (64 - width))]
    return cb, srv, seed, (([16, 4, 4, 44] if post else [16, 4, 8, 4, 4, 44]) + ([56] if stats_variant else [])), 16, 1, 1, expected, {
        "visible": visible, "occluders": occluders, "protected": protected}


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("mount", type=Path)
    p.add_argument("--prefix", type=Path)
    p.add_argument("--proton", type=Path)
    p.add_argument("--xshader", type=Path)
    p.add_argument("--entry", type=int, choices=(1016, 1019, 1213, 1215, 1219, 1223, 1224, 1228, 1230, 1235, 1236, 1243, 1257, 1793, 2138, 3396, 3397, 3398, 3399, 3403, 3412, 3422, 3430, 3432, 3438, 3449, 3451, 3454, 3456, 3457, 3458, 3470, 3477),
                   help="run the controls for one archive entry")
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    host = Host(None, args.proton, args.prefix)
    compiler, dxc, dxv = compiler_tools()
    compiler = args.xshader.resolve() if args.xshader else compiler
    data, entries = global_archive(args.mount)
    cases, libraries = {}, {}
    for name, active, count, small in (
        ("empty", [], 64, []),
        ("sparse", [0, 2, 31, 32, 33, 63], 64, []),
        ("partial", [0, 2, 31, 32, 33, 63], 33, []),
        ("small", [0, 2, 31, 32, 33, 63], 64, [2, 32]),
        ("full", list(range(64)), 64, []),
    ):
        cases["voxel_" + name] = (2138, cull_voxel(active, count, small))
    for name, active, count, capacity, valid_limit in (
        ("empty", [], 64, 80, 64),
        ("sparse", [0, 2, 31, 32, 33, 63], 64, 80, 64),
        ("partial", [0, 2, 31, 32, 33, 63], 33, 80, 64),
        ("capacity", [0, 2, 31, 32, 33, 63], 64, 6, 64),
        ("indices", [0, 2, 31, 32, 33, 63], 64, 80, 33),
        ("full", list(range(64)), 64, 80, 64),
    ):
        cases["feedback_" + name] = (1016, compact_feedback(active, count, capacity, valid_limit))
    for name, counts in {
        "empty": [],
        "one": [3],
        "sparse": [3 if i in (0, 2, 31, 32, 33, 63) else 0 for i in range(64)],
        "full": [i % 5 + 1 for i in range(64)],
        "partial": [i % 3 for i in range(37)],
    }.items():
        cases["commands_" + name] = (1019, command_space(counts))
    for count, start, mode in ((449, 5, "mixed"), (449, 0, "all"), (317, 3, "none"), (0, 0, "none")):
        cases[f"pack_{count}_{mode}"] = (1223, free_pages(count, start, mode))
    for count, group in ((75, 0), (75, 1), (75, -1), (0, -1)):
        cases[f"views_{count}_{group}"] = (3477, compact_views(count, group))
    for entry in (1243, 1257):
        for count in (0, 1, 31, 32, 33, 64):
            cases[f"cards_{entry}_{count}"] = (entry, card_update(count))
    for e in (1224, 1235):
        for count, layer in [(10, 0), (3, 0), (0, 0), (8, 1)]:
            cases[f"alloc_{e}_{count}_{layer}"] = (e, allocation(e, count, layer))
    for e in (1213, 1215, 1219, 1230, 1236):
        for count, extra in [(64, False), (64, True), (35, True), (0, True)]:
            cases[f"init_{e}_{count}_{int(extra)}"] = (e, selection(e, count, extra))
    for mode in ("zero", "disabled", "empty_views", "missing_pages", "visible", "shadow_cache", "shadow_blocked"):
        cases["instance_" + mode] = (3403, shadow_instances(mode))
    for mode in ("empty", "visible", "missing", "hierarchy"):
        cases["cluster_" + mode] = (1793, shadow_clusters(mode))
    for name, visible, occluders, protected in (
        ("empty", [1, 31, 32, 63], [], None),
        *((f"lane{lane}", [lane], [lane], None) for lane in (1, 31, 32, 63)),
        ("pair", [1, 32], [1, 32], None),
        ("full", [1, 31, 32, 63], [1, 31, 32, 63], None),
        ("protected", [1, 32], [1, 32], 1.0),
    ):
        cases["visibility_" + name] = (3396, visibility_instance(visible, occluders, protected))
        cases["visibility_post_" + name] = (3397, visibility_instance(visible, occluders, protected, post=True))
        for entry in (3398, 3399, 3412, 3422, 3430, 3432, 3438, 3449, 3451, 3454, 3456, 3457, 3458, 3470):
            cases[f"visibility_{entry}_" + name] = (entry, visibility_instance(
                visible, occluders, protected, post=entry not in (3399, 3438, 3430, 3449, 3451, 3456, 3458), stats_variant=entry in (3398, 3399, 3412, 3422, 3430, 3456, 3458, 3470)))
    if args.entry is not None:
        cases = {name: case for name, case in cases.items() if case[0] == args.entry}
        if not cases:
            p.error(f"no GPU fixture for archive entry {args.entry}")
    with TemporaryDirectory(dir=ROOT / "build") as tmp:
        work = Path(tmp)
        unpack = decoder()
        for entry in sorted({entry for entry, _ in cases.values()}):
            native, source, profile, dxil = (work / f"{entry}.{ext}" for ext in ("dxbc", "hlsl", "profile", "dxil"))
            ((_, cooked, _),) = containers(decompress(data, entries[entry], unpack))
            native.write_bytes(cooked)
            subprocess.run([compiler, native, "--hlsl-out", source, "--profile-out", profile], check=True)
            subprocess.run([dxc, "-E", "main", "-T", profile.read_text().strip(), "-O3", "-Fo", dxil, source], check=True)
            subprocess.run([dxv, dxil], check=True, stdout=subprocess.DEVNULL)
            libraries[entry] = {"source_sha256": digest(native), "dxil_sha256": digest(dxil)}
        manifest = []
        for name, (entry, (cb, srv, buffers, strides, srv_stride, gx, gy, _, _)) in cases.items():
            manifest.append(" ".join(map(str, [name, entry, gx, gy, srv_stride, len(strides), *strides])))
            if entry in (1243, 1257, 1793, 3403):
                for i, constants in enumerate(cb):
                    words(work / f"{name}.b{i}", constants)
            else:
                words(work / (name+".cbv"), cb)
            if entry in (1016, 1243, 1257, 1793, 3477, 3396, 3397, 3398, 3399, 3403, 3412, 3422, 3430, 3432, 3438, 3449, 3451, 3454, 3456, 3457, 3458, 3470):
                for i, records in enumerate(srv):
                    words(work / f"{name}.t{i}", records)
            else:
                words(work / (name+".srv"), srv)
            for i, initial in enumerate(buffers):
                words(work / f"{name}.u{i}", initial)
        (work / "vsm.inputs").write_text("\n".join(manifest)+"\n")
        toolchain = Toolchain()
        obj = toolchain.compile(ROOT / "tools/reference/page-list/mappings.cpp", work, "/W4", "/WX", f"/I{ROOT / 'runtime/d3d12'}")
        exe = work / "vsm.exe"
        toolchain.link(exe, obj, "d3d12.lib", "dxgi.lib", "kernel32.lib")
        run = host.run(exe, [], timeout=60, capture=True)
        info = dict(line.split("=", 1) for line in (work / "vsm.txt").read_text().splitlines())
        results = {}
        passed = run.returncode == 0
        for name, (entry, case) in cases.items():
            expected, metadata = case[-2:]
            diffs = []
            for i, array in enumerate(expected):
                path = work / f"{name}.out{i}.bin"
                actual = list(struct.unpack(f"<{len(array)}I", path.read_bytes())) if path.is_file() else []
                if len(array) != len(actual):
                    diffs.append([i, "missing readback"])
                else:
                    if entry in (3396, 3397, 3398, 3399, 3412, 3422, 3430, 3432, 3438, 3449, 3451, 3454, 3456, 3457, 3458, 3470) and i < 2:
                        width = 36 if i == 0 else 9
                        for y in range(9):
                            actual[y * 64 + width:(y + 1) * 64] = [0] * (64 - width)
                        if i == 0 and len(metadata["occluders"]) > 1 and metadata["protected"] is None:
                            # Equal-depth atomics race: whichever lane wins owns the pixel.
                            winners = {0x800000a1 + (lane << 8) for lane in metadata["occluders"]}
                            if actual[272] in winners:
                                array = array.copy()
                                array[272] = actual[272]
                    if entry == 1223:
                        actual = unordered_wave_ranges(actual, array, metadata["initial"], metadata["selected_pages"])
                    diffs.extend([i, j, x, y] for j,(x,y) in enumerate(zip(array,actual)) if x != y)
            results[name] = {**metadata, "checked_words": sum(map(len,expected)), "mismatches": len(diffs), "first_mismatches": diffs[:4]}
            passed &= not diffs
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps({"gpu":info, "host":host.version, "exit":run.returncode, "libraries":libraries, "cases":results},indent=2)+"\n")
        if not passed:
            sys.stderr.buffer.write(run.stdout[-1500:])
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
