#!/usr/bin/env python3
"""Check the two post-WakeUp material pipelines on a private Proton GPU device."""
import argparse
import ctypes
from ctypes.util import find_library
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import find_dxc
from frosty_gpu_guest import run_gpu_guest
from check_frosty_shaders import ENTRY, containers
from project_tuxbox.inspect import digest

MATERIAL_START = 0x19AB8C6
SHADERS = {
    523: "ff94f1d5a7f37f24346ef9bfa15cad12e2504b43b3176b535e4b56937204e614",
    6113: "4952ebb81b8868e66c39f8f1323501d9c9affec2f54fb6fcec21472933b46035",
    31750: "3ca45139817062553ab3bb8cb6f429211dc7c944458312cce8b64c872959f33d",
    31788: "bfee6e102ab504ca659652189db6ad1d4e0790971745c3919dc2111076ef35cd",
}
PIXELS = {
    "b75039a494495b07": "7e616f5862bb9b5bcda11503d1e30f5b32d5f6d04412a0e52f7e4b1f2bb531d3",
    "43ba45d6b7d30fbd": "095e3aad4f03fa0b445ac06b1d5566a1cec65800b149fe8336615ca20f315736",
}


def extract(mount, destination):
    decompress = ctypes.CDLL(find_library("lz4")).LZ4_decompress_safe
    decompress.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    with (mount / "Frosty/Content/Paks/Frosty-XSX.pak").open("rb") as pak:
        pak.seek(MATERIAL_START - 53)
        header = pak.read(53)
        if struct.unpack_from("<QQQI", header) != (0, 340714881, 340714881, 0):
            raise ValueError("This package differs from the Frosty material archive")
        if header[28:48].hex() != "e8002cb8598b46079448866de370bc780df6144c" or pak.read(4) != b"\x02\0\0\0":
            raise ValueError("Unexpected material archive identity or version")
        for width in (20, 20, 16, ENTRY.size, 16, 4):
            count, = struct.unpack("<I", pak.read(4))
            if width == ENTRY.size:
                if count != 38831:
                    raise ValueError("Unexpected material shader count")
                entries = list(ENTRY.iter_unpack(pak.read(count * width)))
            else:
                pak.seek(count * width, 1)
        start = pak.tell()
        for index, fingerprint in SHADERS.items():
            offset, stored, length, _ = entries[index]
            pak.seek(start + offset)
            source = pak.read(stored)
            if stored != length:
                target = ctypes.create_string_buffer(length)
                if decompress(source, target, stored, length) != length:
                    raise ValueError(f"Material entry {index} could not be decompressed")
                source = target.raw
            ((_, native, _),) = containers(source)
            if hashlib.sha256(native).hexdigest() != fingerprint:
                raise ValueError(f"Material entry {index} differs")
            (destination / f"mat{index}.dxbc").write_bytes(native)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ("mount", "provider", "proton", "prefix", "shader-cache", "compiler-dir", "work", "output"):
        p.add_argument("--" + name, type=Path, required=True)
    args = p.parse_args()
    work, proton = args.work.resolve(), args.proton.resolve()
    work.mkdir(parents=True, exist_ok=True)
    sources = ROOT / "tools/reference/chase-link"
    with TemporaryDirectory(dir=work) as temporary:
        trial = Path(temporary)
        extract(args.mount, trial)
        for name, fingerprint in PIXELS.items():
            code = args.shader_cache / (name + ".cso")
            if digest(code) != fingerprint:
                raise ValueError(f"Missing matching compiled retail pixel shader {name}")
            shutil.copyfile(code, trial / code.name)
        for variant in (0, 1):
            subprocess.run([find_dxc(None), "-D", f"EXTRA={variant}", "-T", "vs_6_0", "-E", "main",
                            "-Fo", trial / f"control{variant}.cso", sources / "control.hlsl"], check=True)
        exe, status, observed = run_gpu_guest(sources / "guest.cpp", "chase-probe.exe", trial, work, proton,
                                              args.prefix, args.provider, args.compiler_dir)
        expected = {"pair_0_slot_86": "passed", "pair_0_depth_written": "0",
                    "pair_1_slot_86": "passed", "pair_1_depth_written": "64"}
        if status or observed != expected:
            raise ValueError(f"GPU control returned {status}: {observed}. See {work / 'gpu.log'}")
        adapters = [line.split("Found device: ", 1)[1] for line in (work / "gpu.log").read_text().splitlines()
                    if "Found device: " in line]
        receipt = dict(
            proton=(proton / "version").read_text().strip(), enumerated_adapters=adapters, guest_sha256=digest(exe),
            provider_sha256=digest(args.provider), patch_sha256=digest(ROOT / "tools/reference/tombox-chase-stage-linkage.patch"),
            title_observed="After WakeUp, frame 295, depth-only two-stage triangle pipeline, November 21 11:23:39",
            material_archive_offset=hex(MATERIAL_START), material_container_sha256=SHADERS,
            compiled_pixel_sha256=PIXELS, root="Original embedded RTS0 with 16 tables, IA input layout allowed",
            pairs=[dict(vertex=6113, pixel=523, equivalent_pixel=4141, previous_position_register=6,
                        linked_position_register=4), dict(vertex=31788, pixel=31750, previous_position_register=7,
                        linked_position_register=5)], guest_create_pipeline_slot=86, original_slot_results=observed,
            depth_control="Controlled nondegenerate triangle; real captured pixel shaders. Clear depth 1, triangle depth 0.25, 8 by 8.",
            remaining="In the original vertex translation the merged-stage lane count in s3 starts at zero. EXEC suppresses the translated LDS vertex exports, and DXC folds SV_Position to zero.",
        )
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")


if __name__ == "__main__":
    main()
