#!/usr/bin/env python3
"""Compare standalone BVH4 node math on the CPU, Radeon and GeForce.

The control covers finite nonzero ray directions, zero box growth and
closest-first child sorting. Supplying --serialized adds the root BOX16 or
BOX32 from a user-owned Frosty serialized BLAS to the synthetic inputs.
The output is a mathematical comparison, not a native AMD-instruction oracle.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain, find_dxc
from project_tuxbox.host import Host

INVALID = 0xffffffff


def words(data):
    return list(struct.unpack("<" + "I" * (len(data) // 4), data))


def box_node(bounds, box16):
    data = struct.pack("<4I", 9, 18, 27, INVALID)
    for bound in bounds:
        data += struct.pack("<6e" if box16 else "<6f", *bound)
    return words(data.ljust(128, b"\0"))


def box_reference(node, box16, origin, inverse, extent, sort_children):
    raw = struct.pack("<32I", *node)
    results = []
    for child in range(4):
        bound = struct.unpack_from("<6e" if box16 else "<6f", raw,
                                   16 + child * (12 if box16 else 24))
        near = max(0.0, *(min((bound[i] - origin[i]) * inverse[i],
                               (bound[i + 3] - origin[i]) * inverse[i]) for i in range(3)))
        far = min(extent, *(max((bound[i] - origin[i]) * inverse[i],
                                (bound[i + 3] - origin[i]) * inverse[i]) for i in range(3)))
        if node[child] != INVALID and near <= far:
            results.append((near, child, node[child]))
    if sort_children:
        results.sort()
    else:
        hits = {row[1]: row[2] for row in results}
        return [hits.get(i, INVALID) for i in range(4)]
    return [row[2] for row in results] + [INVALID] * (4 - len(results))


def subtract(a, b):
    return tuple(x - y for x, y in zip(a, b))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0])


def dot(a, b):
    return sum(x * y for x, y in zip(a, b))


def triangle_reference(origin, direction, a, b, c):
    e1, e2, v = subtract(b, a), subtract(c, a), subtract(origin, a)
    p, q = cross(direction, e2), cross(v, e1)
    determinant = dot(e1, p)
    t, i, j = dot(e2, q), dot(v, p), dot(direction, q)
    if not determinant or t / determinant < 0 or i / determinant < 0 or j / determinant < 0 or (i + j) / determinant > 1:
        return [math.inf, 1.0, i, j]
    return [t, determinant, i, j]


def vector(value):
    return "float3(" + ", ".join(f"{v:.9f}" for v in value) + ")"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proton", type=Path, required=True)
    parser.add_argument("--prefix-amd", type=Path, required=True)
    parser.add_argument("--prefix-nvidia", type=Path, required=True)
    parser.add_argument("--amd-vulkan", type=Path, help="optional RADV ICD JSON")
    parser.add_argument("--serialized", type=Path, help="optional user-supplied serialized Xbox BLAS")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    bounds = [
        (-2, -2, 2, 2, 2, 4),
        (-3, -3, 5, 3, 3, 8),
        (10, 10, 10, 12, 12, 12),
        (-1, -1, 0.5, 1, 1, 1),
    ]
    inputs = [(box_node(bounds, kind), kind, "synthetic") for kind in (True, False)]
    source_hash = None
    retail_triangle = None
    if args.serialized:
        data = args.serialized.read_bytes()
        source_hash = hashlib.sha256(data).hexdigest()
        body = data[64:]
        pointer = struct.unpack_from("<I", body, 48)[0]
        if pointer & 7 not in (4, 5):
            raise ValueError("serialized root is not BOX16 or BOX32")
        offset = (pointer >> 3) * 64
        size = 64 if pointer & 7 == 4 else 128
        raw = body[offset:offset + size]
        if len(raw) != size:
            raise ValueError("serialized root extends outside the input")
        inputs.append((words(raw.ljust(128, b"\0")), pointer & 7 == 4, "serialized root"))
        leaf_offset = struct.unpack_from("<I", body, 52)[0]
        if leaf_offset + 64 > len(body):
            raise ValueError("serialized leaf extends outside the input")
        positions = struct.unpack_from("<9f", body, leaf_offset)
        retail_triangle = tuple(tuple(positions[3 * i:3 * i + 3]) for i in range(3))
    boxes = []
    for node, box16, label in inputs:
        for origin, inverse, extent, sorting in [
            ((0.25, 0.25, -4), (1, 1, 1), 100, True),
            ((0, 0, -4), (0.25, 0.25, 1), 100, True),
            ((100, 100, -4), (1, 1, 1), 100, True),
            ((0, 0, -4), (0.25, 0.25, 1), 5, True),
            ((0, 0, 12), (-0.25, -0.25, -1), 100, True),
            ((0, 0, 3), (0.25, 0.25, 1), 100, True),
            ((0, 0, -4), (0.25, 0.25, 1), 100, False),
        ]:
            boxes.append((node, box16, label, origin, inverse, extent, sorting))
    triangles = [
        ((0.25, 0.25, -1), (0, 0, 1), (0, 0, 0), (1, 0, 0), (0, 1, 0)),
        ((0.25, 0.25, 1), (0, 0, -1), (0, 0, 0), (1, 0, 0), (0, 1, 0)),
        ((2, 2, -1), (0, 0, 1), (0, 0, 0), (1, 0, 0), (0, 1, 0)),
        ((0.25, 0.25, 1), (0, 0, 1), (0, 0, 0), (1, 0, 0), (0, 1, 0)),
        ((0.25, 0.25, -1), (0, 0, 1), (0, 0, 0), (0, 1, 0), (1, 0, 0)),
        ((0.5, 0.5, 0), (0, 0, 1), (0, 0, 0), (1, 0, 0), (0, 1, 0)),
        ((0.25, 0.25, -1), (1, 0, 0), (0, 0, 0), (1, 0, 0), (0, 1, 0)),
        ((0.25, 0.25, -1), (0, 0, 1), (0, 0, 0), (1, 0, 0), (2, 0, 0)),
    ]
    if retail_triangle:
        center = tuple(sum(vertex[axis] for vertex in retail_triangle) / 3 for axis in range(3))
        origin = (center[0], center[1], center[2] - 16.0)
        triangles.append((origin, (0, 0, 1), *retail_triangle))
    expected = []
    shader = (ROOT / "tools/reference/bvh-node/node.hlsl").read_text()
    shader += "\nRWBuffer<uint> output : register(u0);\n[numthreads(1,1,1)] void main() {\n"
    for node, box16, label, origin, inverse, extent, sorting in boxes:
        values = ", ".join(f"0x{w:08x}u" for w in node)
        index = len(expected) * 4
        shader += f"{{ uint node[32] = {{ {values} }}; uint4 r = tb_bvh_boxes(node, {'true' if box16 else 'false'}, {vector(origin)}, {vector(inverse)}, {extent}.0, {'true' if sorting else 'false'});"
        shader += "".join(f" output[{index + i}] = r[{i}];" for i in range(4)) + " }\n"
        expected.append(("box", label, box_reference(node, box16, origin, inverse, extent, sorting)))
    for case in triangles:
        index = len(expected) * 4
        leaf = words(struct.pack("<9f", *(v for vertex in case[2:] for v in vertex)).ljust(128, b"\0"))
        values = ", ".join(f"0x{word:08x}u" for word in leaf)
        shader += f"{{ uint node[32] = {{ {values} }}; float4 r = tb_bvh_triangle_leaf(node, {vector(case[0])}, {vector(case[1])});"
        shader += "".join(f" output[{index + i}] = asuint(r[{i}]);" for i in range(4)) + " }\n"
        expected.append(("triangle", "synthetic", triangle_reference(*case)))
    shader += "}\n"
    if len(expected) * 4 > 192:
        raise ValueError("control exceeds the guarded UAV")
    report = {"scope": "Finite-input BVH node math compared with CPU calculations; no native instruction oracle",
              "serialized_sha256": source_hash, "box_cases": len(boxes),
              "triangle_cases": len(triangles), "adapters": {}}
    with TemporaryDirectory(prefix="tuxbox-bvh-node-") as directory:
        work = Path(directory)
        hlsl, cso = work / "node.hlsl", work / "node.cso"
        hlsl.write_text(shader)
        dxc = Path(find_dxc(None))
        subprocess.run([dxc, "-T", "cs_6_0", "-E", "main", hlsl, "-Fo", cso], check=True)
        subprocess.run([dxc.with_name("dxv"), cso], check=True)
        report["hlsl_sha256"] = hashlib.sha256(shader.encode()).hexdigest()
        report["dxil_sha256"] = hashlib.sha256(cso.read_bytes()).hexdigest()
        toolchain = Toolchain()
        obj = toolchain.compile(ROOT / "tools/reference/independent-shaders/compute.cpp",
                                work, f"/I{ROOT / 'runtime/d3d12'}")
        exe = work / "node.exe"
        toolchain.link(exe, obj, "d3d12.lib", "dxgi.lib", "kernel32.lib")
        (work / "cases.txt").write_text("node 0 uint 1\n")
        for name, prefix in (("AMD", args.prefix_amd), ("NVIDIA", args.prefix_nvidia)):
            os.environ.update(VKD3D_FILTER_DEVICE_NAME=name, DXVK_FILTER_DEVICE_NAME=name)
            if name == "AMD" and args.amd_vulkan:
                os.environ["VK_DRIVER_FILES"] = str(args.amd_vulkan)
            elif name == "NVIDIA":
                os.environ.pop("VK_DRIVER_FILES", None)
            result = Host(None, str(args.proton), prefix).run(exe, [], timeout=45, capture=True)
            if result.returncode:
                raise RuntimeError(f"{name} GPU control failed: {result.returncode}")
            receipt = (work / "result.txt").read_text()
            adapter = next((line.removeprefix("adapter=") for line in receipt.splitlines()
                            if line.startswith("adapter=")), None)
            vendor = "1002:" if name == "AMD" else "10de:"
            if adapter is None or not adapter.lower().startswith(vendor):
                raise ValueError(f"{name} selected unexpected adapter {adapter!r}")
            actual = words((work / "node.bin").read_bytes())
            base = 96
            for case_index, (kind, label, want) in enumerate(expected):
                got = actual[base + case_index * 4:base + case_index * 4 + 4]
                if kind == "triangle":
                    got = [struct.unpack("<f", struct.pack("<I", v))[0] for v in got]
                    if not all(math.isclose(a, b, rel_tol=1e-6, abs_tol=1e-6) for a, b in zip(got, want)):
                        raise ValueError(f"{name} {label} {kind} {case_index}: {got} != {want}")
                elif got != want:
                    raise ValueError(f"{name} {label} {kind} {case_index}: {got} != {want}")
            end = base + len(expected) * 4
            if any(value != 0x76540000 + i for i, value in enumerate(actual) if i < base or i >= end):
                raise ValueError(f"{name} output guard changed")
            report["adapters"][name] = {"adapter": adapter, "exit": result.returncode, "cases": len(expected),
                                         "guard_words": len(actual) - len(expected) * 4}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
