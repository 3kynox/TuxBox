#!/usr/bin/env python3
"""Validate Frosty's BLAS prebuild and command-list build against desktop D3D12.

Select a Proton distribution and optional Xbox provider. Both runs build two
triangle geometries on the same device as their TLAS and inline ray query.
--prebuild-only tests just the provider's sizing and uses a desktop build.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain, find_dxc
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--provider", type=Path)
    parser.add_argument("--proton", type=Path)
    parser.add_argument("--prefix", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--prebuild-only", action="store_true")
    args = parser.parse_args()
    if args.prebuild_only and not args.provider:
        parser.error("--prebuild-only requires --provider")
    host = Host(None, args.proton, args.prefix)
    src = ROOT / "tools/reference/blas"
    build = ROOT / "build/frosty-blas"
    build.mkdir(exist_ok=True, parents=True)
    with TemporaryDirectory(dir=build) as tmp:
        work = Path(tmp)
        subprocess.run([find_dxc(None), "-T", "cs_6_5", "-E", "main", "-O3", "-Fh", "query_dxil.h",
                        "-Vn", "query_dxil", str(src / "query.hlsl")], cwd=work, check=True)
        toolchain = Toolchain()
        obj = toolchain.compile(src / "guest.cpp", work, "/W4", "/WX", f"/I{work}", f"/I{ROOT / 'runtime/d3d12'}")
        exe = work / "blas.exe"
        toolchain.link(exe, obj, "d3d12.lib", "dxgi.lib", "kernel32.lib")
        result = {"guest_sha256": digest(exe), "host": host.version, "runs": {}}
        modes = ["desktop"]
        if args.provider:
            shutil.copyfile(args.provider, work / "d3d12_xs.dll")
            result["d3d12_xs_sha256"] = digest(work / "d3d12_xs.dll")
            modes.append("xbox-prebuild" if args.prebuild_only else "xbox")
        for mode in modes:
            receipt = work / (mode + ".txt")
            run = host.run(exe, [mode, receipt.name], timeout=60, capture=True)
            observation = dict(line.split("=", 1) for line in receipt.read_text().splitlines()) if receipt.exists() else {}
            if not observation or run.returncode:
                sys.stderr.buffer.write(run.stdout[-4096:])
            result["runs"][mode] = {"exit": run.returncode, **observation}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return int(any(run["exit"] or run.get("ray_query_results") != "70010,70120,80010,80120,dead"
                   for run in result["runs"].values()))


if __name__ == "__main__":
    sys.exit(main())
