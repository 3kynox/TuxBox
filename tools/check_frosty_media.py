#!/usr/bin/env python3
"""Build Frosty's Media Foundation reset bridge and verify its graphics-device association.

Supply TomBox's mfplat.dll, the Proton distribution and a dedicated prefix.
--video adds a first-frame byte comparison to host FFmpeg's NV12 decode.
The three DLLs in --work can also be staged beside the title executable.
"""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

import pefile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest


def named_exports(path):
    image = pefile.PE(str(path), fast_load=True)
    try:
        image.parse_data_directories(directories=[pefile.DIRECTORY_ENTRY["IMAGE_DIRECTORY_ENTRY_EXPORT"]])
        return {item.name.decode("ascii"): item.ordinal for item in image.DIRECTORY_ENTRY_EXPORT.symbols if item.name}
    finally:
        image.close()


def build(work, tombox, system, *, hevc_surface=False):
    source = ROOT / "tools/reference/media"
    work.mkdir(parents=True, exist_ok=True)
    proxy = work / "mfplat_tombox.dll"
    desktop = work / "mfplat_sys.dll"
    shutil.copyfile(tombox, proxy)
    shutil.copyfile(system, desktop)
    exports = named_exports(proxy)
    reset = "MFResetDXGIDeviceManagerX"
    if reset not in exports:
        raise ValueError("The TomBox proxy must export MFResetDXGIDeviceManagerX")
    # Preserve TomBox's ordinals. Wine MF components also delay-load additional
    # named exports (including MFAllocateWorkQueue) from the app-local MFPlat.
    rows = [f"    {name} @{ordinal}" if name == reset else f"    {name}=mfplat_tombox.{name} @{ordinal}"
            for name, ordinal in sorted(exports.items(), key=lambda item: item[1])]
    extra = sorted(named_exports(desktop).keys() - exports.keys())
    rows.extend(f"    {name}=mfplat_sys.{name} @{i}" for i, name in enumerate(extra, max(exports.values()) + 1))
    if hevc_surface:
        # The bundled decoder's four calls pass the standard surface IID, resource,
        # subresource, bottom-up flag and output used by MFCreateDXGISurfaceBuffer.
        rows.append("    MFCreateDXGISurfaceBufferX=mfplat_sys.MFCreateDXGISurfaceBuffer")
    definition = work / "mfplat.def"
    definition.write_text("LIBRARY mfplat\nEXPORTS\n" + "\n".join(rows) + "\n")
    tools = Toolchain()
    obj = tools.compile(source / "reset.cpp", work, "/W4", "/WX")
    tools.link(work / "mfplat.dll", obj, "d3d11.lib", "kernel32.lib", exports=definition)
    for name, extra_libs in (("sample", ("d3d11.lib",)), ("read_video", ("mfreadwrite.lib",))):
        obj = tools.compile(source / f"{name}.cpp", work, "/W4", "/WX", f"/I{ROOT / 'runtime/d3d12'}")
        tools.link(work / f"{name}.exe", obj, "mfplat.lib", "mfuuid.lib", "ole32.lib", "d3d12.lib", "kernel32.lib",
                   *extra_libs)


def invoke(host, exe, args, report):
    report.unlink(missing_ok=True)
    run = host.run(exe, args, capture=True)
    observation = {"exit": run.returncode}
    if report.exists():
        observation.update(line.split("=", 1) for line in report.read_text().splitlines())
    if run.returncode:
        sys.stderr.buffer.write(run.stdout[-2000:])
    return observation


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tombox-mfplat", required=True, type=Path)
    parser.add_argument("--proton", required=True, type=Path)
    parser.add_argument("--prefix", required=True, type=Path)
    parser.add_argument("--work", required=True, type=Path, help="dedicated output directory for the bridge and guests")
    parser.add_argument("--video", action="append", type=Path, default=[])
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args()
    work = args.work.resolve()
    build(work, args.tombox_mfplat, args.proton / "files/lib/wine/x86_64-windows/mfplat.dll")
    host = Host("proton", args.proton, args.prefix)
    # Cover the full launcher process group when a media provider stalls during teardown.
    host.command = ["timeout", "--kill-after=3s", "25s", *host.command]
    sample = invoke(host, work / "sample.exe", [], work / "sample.txt")
    result = {"host": host.version, "dll_sha256": {p.name: digest(p) for p in sorted(work.glob("mfplat*.dll"))},
              "sample": sample, "videos": []}
    success = not sample["exit"] and sample.get("verified_pixel_words") == "4"
    for video in args.video:
        oracle = subprocess.run(["ffmpeg", "-v", "error", "-i", str(video), "-frames:v", "1", "-pix_fmt", "nv12",
                                 "-f", "rawvideo", "pipe:1"], stdout=subprocess.PIPE, check=True).stdout
        with tempfile.TemporaryDirectory(dir=work) as directory:
            raw = Path(directory) / "first.nv12"
            report = Path(directory) / "read.txt"
            win_path = "Z:" + str(video.resolve()).replace("/", "\\")
            read = invoke(host, work / "read_video.exe",
                          [win_path, str(report.relative_to(work)), str(raw.relative_to(work))], report)
            actual = raw.read_bytes() if raw.exists() else b""
        matched = bool(oracle) and oracle == actual and read.get("timestamp") == "0"
        result["videos"].append({"name": video.name, **read, "oracle_sha256": hashlib.sha256(oracle).hexdigest(),
                                  "frame_sha256": hashlib.sha256(actual).hexdigest() if actual else None,
                                  "matched": matched})
        success &= not read["exit"] and matched
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")
    return 0 if success else 1


if __name__ == "__main__":
    sys.exit(main())
