#!/usr/bin/env python3
"""Exercise the exact DXIL cached by TomBox for Frosty's first two ClearCS shaders.

python tools/check_frosty_clear.py MOUNT --prefix PROTON_PREFIX --xshader ../TomBox/target/release/xshader --output receipt.json

Select the prefix used to run Frosty. The test uses the original root signature
and verifies typed R32_FLOAT and R32G32B32A32_FLOAT UAVs on desktop D3D12.
"""
import argparse
import ctypes
from ctypes.util import find_library
import json
from pathlib import Path
import re
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain, find_dxc
from check_frosty_shaders import GLOBAL_SHA, containers, decompress, global_archive, require
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest


def fnv1a(data):
    value = 0xcbf29ce484222325
    for byte in data:
        value = (value ^ byte) * 0x100000001b3 & 0xffffffffffffffff
    return value


def extract_shaders(mount, xshader, work):
    payload, entries = global_archive(mount)
    lz4 = ctypes.CDLL(find_library("lz4")).LZ4_decompress_safe
    lz4.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    hlsl = work / "clear.hlsl"
    shaders = []
    for index, expected in ((402, 0x967bc3a70a86c7f4), (379, 0x97a201d874e27271)):
        blob = decompress(payload, entries[index], lz4)
        ((_, cooked, _),) = containers(blob)
        require(fnv1a(cooked) == expected, "Shader does not match the retail input")
        native = work / f"{index}.dxbc"
        native.write_bytes(cooked)
        report = subprocess.check_output([xshader, native, "--hlsl-out", hlsl], text=True)
        require('entry=Some("ClearCS")' in report and "consumed 148/148 bytes" in report, "Unexpected shader code")
        shaders.append({"index": index, "fnv1a64": f"{expected:016x}", "cooked_sha256": digest(native),
                        "hlsl_sha256": digest(hlsl)})
    require(shaders[0]["hlsl_sha256"] == shaders[1]["hlsl_sha256"], "ClearCS translations differ")
    return cooked, hlsl, shaders


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mount", type=Path)
    parser.add_argument("--prefix", type=Path, required=True)
    parser.add_argument("--probe-prefix", type=Path, help="use a separate prefix for the GPU readback guest")
    parser.add_argument("--proton", type=Path)
    parser.add_argument("--xshader", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    xshader = args.xshader.resolve()
    work_root = ROOT / "build/frosty-clear"
    work_root.mkdir(parents=True, exist_ok=True)
    host = Host(None, args.proton, args.probe_prefix or args.prefix)
    with TemporaryDirectory(dir=work_root) as temp:
        work = Path(temp)
        cooked, hlsl, shaders = extract_shaders(args.mount, xshader, work)
        # TomBox dxc::cache_path keys the original translated source and the DXC profile.
        profile_hash = fnv1a(b"cs_6_0")
        rotated = ((profile_hash << 17) | (profile_hash >> 47)) & 0xffffffffffffffff
        cache_name = f"{fnv1a(hlsl.read_bytes()) ^ rotated:016x}.cso"
        cache = args.prefix / "drive_c/users/steamuser/AppData/Local/Temp/tombox_shader_cache/by_hlsl" / cache_name
        original = cache.read_bytes()
        dump = subprocess.check_output([find_dxc(None), "-dumpbin", cache], text=True)
        require("NumThreads=(64,1,1)" in dump and "call void @dx.op.bufferStore.f32" in dump
                and re.search(r"UAV\s+f32\s+buf\s+U0\s+u0\s+1", dump), "Unexpected desktop ClearCS DXIL")
        header = work / "clear_blobs.h"
        header.write_text("".join(f"const unsigned char {name}[] = {{" + ",".join(map(str, value)) + "};\n"
                                  for name, value in (("cooked_shader", cooked), ("desktop_shader", original))))
        tools = Toolchain()
        obj = tools.compile(ROOT / "tools/reference/clear/guest.cpp", work, "/W4", "/WX",
                            f"/I{work}", f"/I{ROOT / 'runtime/d3d12'}")
        exe = work / "clear.exe"
        tools.link(exe, obj, "d3d12.lib", "dxgi.lib", "kernel32.lib")
        run = host.run(exe, [], timeout=60, capture=True)
        receipt = work / "readback.txt"
        require(receipt.exists(), "GPU guest exited without a receipt")
        observations = dict(line.split("=", 1) for line in receipt.read_text().splitlines())
        result = {"archive_sha256": GLOBAL_SHA, "inputs": shaders, "cache_name": cache_name, "dxil_sha256": digest(cache),
                  "numthreads": [64, 1, 1], "cbv_table": 1, "uav_table": 3, "start": 5, "end_exclusive": 102,
                  "unsigned_view_control": "Observed behavior of the cached float-typed DXIL on UINT views",
                  "elements_per_buffer": 140, "dispatch_groups": 2, "host": host.version, "guest_sha256": digest(exe),
                  "readback": observations, "exit": run.returncode}
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
        require(not run.returncode and observations.get("float_view_1_matched_words") == "140"
                and observations.get("float_view_4_matched_words") == "560", str(observations))


if __name__ == "__main__":
    main()
