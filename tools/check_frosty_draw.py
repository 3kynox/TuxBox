#!/usr/bin/env python3
"""Draw Frosty's two observed Slate shader pairs offscreen with controlled red texture, white vertices, and a blue target."""
import argparse
import ctypes
from ctypes.util import find_library
import json
from pathlib import Path
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain
from check_frosty_clear import fnv1a
from check_frosty_shaders import GLOBAL_SHA, containers, decompress, global_archive, require
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mount", type=Path)
    parser.add_argument("--prefix", type=Path, required=True, help="prefix containing the retail shader cache")
    parser.add_argument("--probe-prefix", type=Path, required=True)
    parser.add_argument("--proton", type=Path)
    parser.add_argument("--xshader", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    work_root = ROOT / "build/frosty-draw"
    work_root.mkdir(parents=True, exist_ok=True)
    host = Host(None, args.proton, args.probe_prefix)
    payload, entries = global_archive(args.mount)
    lz4 = ctypes.CDLL(find_library("lz4")).LZ4_decompress_safe
    lz4.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
    cache = args.prefix / "drive_c/users/steamuser/AppData/Local/Temp/tombox_shader_cache/by_hlsl"
    active = [(3643, 0x8e9e3b832c7dd2ae, "vs_6_0", "vertex_shader"),
              (785, 0x01dadaebf074d085, "ps_6_1", "pixel_a"),
              (792, 0xf6d1b9eafc4e1bd3, "ps_6_1", "pixel_b")]
    shaders = []
    with TemporaryDirectory(dir=work_root) as temp:
        work = Path(temp)
        blobs = {}
        for index, input_hash, profile, name in active:
            ((_, cooked, _),) = containers(decompress(payload, entries[index], lz4))
            require(fnv1a(cooked) == input_hash, f"Runtime shader hash mismatch at entry {index}")
            native, hlsl = work / "shader.dxbc", work / "shader.hlsl"
            native.write_bytes(cooked)
            subprocess.run([args.xshader.resolve(), native, "--hlsl-out", hlsl], stdout=subprocess.DEVNULL, check=True)
            p = fnv1a(profile.encode())
            key = fnv1a(hlsl.read_bytes()) ^ (((p << 17) | (p >> 47)) & 0xffffffffffffffff)
            cso = cache / f"{key:016x}.cso"
            blobs[name] = cso.read_bytes()
            if name == "vertex_shader":
                blobs["cooked_shader"] = cooked
            shaders.append(dict(entry=index, profile=profile, cooked_fnv1a64=f"{input_hash:016x}", cooked_sha256=digest(native),
                                hlsl_sha256=digest(hlsl), cache_file=cso.name, desktop_sha256=digest(cso)))
        (work / "draw_blobs.h").write_text("".join(
            f"const unsigned char {key}[] = {{" + ",".join(map(str, value)) + "};\n" for key, value in blobs.items()))
        tools = Toolchain()
        obj = tools.compile(ROOT / "tools/reference/draw/guest.cpp", work, "/W4", "/WX",
                            f"/I{work}", f"/I{ROOT / 'runtime/d3d12'}")
        exe = work / "draw.exe"
        tools.link(exe, obj, "d3d12.lib", "dxgi.lib", "kernel32.lib")
        run = host.run(exe, [], timeout=60, capture=True)
        receipt = work / "readback.txt"
        require(receipt.exists(), "No guest receipt")
        observed = dict(line.split("=", 1) for line in receipt.read_text().splitlines())
        result = dict(archive_sha256=GLOBAL_SHA, shaders=shaders, host=host.version, guest_sha256=digest(exe),
                      extent=[16, 16], scissor=[3, 2, 12, 11], vertex_count=3,
                      scope="Title-cached DXIL, original cooked root signature, controlled vertex, texture and raster state",
                      readback=observed, exit=run.returncode)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(result, indent=2) + "\n")
        require(not run.returncode and all(observed.get(f"entry_{entry}_drawn_red") == "81"
                                          and observed.get(f"entry_{entry}_guard_blue") == "175"
                                          for entry in (785, 792)), str(observed))


if __name__ == "__main__":
    main()
