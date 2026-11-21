#!/usr/bin/env python3
"""Validate a packaged video MFT with actual decoded frames and both D3D12 NV12 planes.

The output directory keeps the packaged decoder, media bridge, registration utility and probe.
Use a dedicated prefix. HEVC registers a private decoder CLSID, while H.264 registers the
standard decoder CLSID that Frosty activates directly.
Use the default HEVC profile for the 4K intro or --codec h264 for the 1024x512 selection videos.
"""
import argparse
from fractions import Fraction
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain
from check_frosty_media import build
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest


def run_host(host, exe, work, *args):
    run = subprocess.run([*host.command, str(exe), *args], cwd=work, env=host.env,
                         stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=65)
    if run.returncode:
        raise ValueError(f"{exe.name} returned {run.returncode}: " + run.stdout.decode(errors="replace")[-1200:])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for flag in ("packaged-os", "video", "xmem", "tombox-mfplat", "proton", "prefix", "work", "output"):
        parser.add_argument("--" + flag, type=Path, required=True)
    parser.add_argument("--codec", choices=("hevc", "h264"), default="hevc")
    parser.add_argument("--frames", nargs="+", type=int, default=[0, 12, 13, 14], help="zero-based decoded frame indices")
    args = parser.parse_args()
    codec = args.codec
    h264 = codec == "h264"
    extra_args = ["h264"] if h264 else []
    frame_ids = sorted(set(args.frames))
    if frame_ids[0] < 0:
        parser.error("Frame indices must be nonnegative")
    [stream] = json.loads(subprocess.check_output(["ffprobe", "-v", "error", "-select_streams", "v:0",
        "-show_entries", "stream=codec_name,width,height,r_frame_rate", "-of", "json", str(args.video)]))["streams"]
    if stream["codec_name"] != codec:
        parser.error("The selected codec does not match the video stream")
    width, height = stream["width"], stream["height"]
    rate = Fraction(stream["r_frame_rate"])
    elementary = "source.h264" if h264 else "logo.h265"
    work = args.work.resolve()
    src = ROOT / "tools/reference/hevc"
    build(work, args.tombox_mfplat, args.proton / "files/lib/wine/x86_64-windows/mfplat.dll", hevc_surface=True)
    original = "mfh264dec.dll" if h264 else "mfhevcdec.dll"
    packaged = "frosty_packaged_h264.dll" if h264 else original
    shutil.copyfile(args.packaged_os / "Windows/System32" / original, work / packaged)
    shutil.copyfile(args.xmem, work / "xmem.dll")
    tools = Toolchain()
    definition = work / (codec + ".def")
    definition.write_text(f"LIBRARY frosty_{codec}\nEXPORTS\n    DllGetClassObject\n")
    def compile_guest(name):
        return tools.compile(src / (name + ".cpp"), work, "/W4", "/WX", f"/I{ROOT / 'runtime/d3d12'}")
    libs = ("mfplat.lib", "mfuuid.lib", "d3d12.lib", "ole32.lib", "kernel32.lib")
    tools.link(work / f"frosty_{codec}.dll", compile_guest("transform"), compile_guest("surface"), *libs, exports=definition)
    tools.link(work / f"register_{codec}.exe", compile_guest("register"), *libs, "advapi32.lib")
    tools.link(work / f"verify_{codec}.exe", compile_guest("decode"), *libs)
    host = Host("proton", args.proton, args.prefix)
    run_host(host, work / f"register_{codec}.exe", work, f"frosty_{codec}.dll", *extra_args)
    luma = width * height
    frame_bytes = luma * 3 // 2
    select = "+".join(f"eq(n\\,{i})" for i in frame_ids)
    raw = subprocess.check_output(["ffmpeg", "-loglevel", "error", "-i", str(args.video), "-map", "0:v:0",
                                   "-vf", "select=" + select, "-fps_mode", "passthrough", "-frames:v", str(len(frame_ids)),
                                   "-pix_fmt", "nv12", "-f", "rawvideo", "-"])
    if len(raw) != len(frame_ids) * frame_bytes:
        raise ValueError("Video ended before all requested frames were decoded")
    oracle = {n: memoryview(raw)[i*frame_bytes:(i+1)*frame_bytes] for i, n in enumerate(frame_ids)}
    result = dict(host=host.version, video_sha256=digest(args.video), video=args.video.name,
                  gpu_sample="IMFDXGIBuffer, ID3D12Resource NV12, IMFD3D12SynchronizationObjectCommands",
                  decode=f"Original packaged Microsoft {codec.upper()} decoder, software transform",
                  size=[width, height], frames={}, dll_sha256={
                      p.name: digest(p) for p in sorted(work.glob("*.dll"))})
    if h264:
        result["activation_clsid"] = "62ce7e72-4c71-4d20-b15d-452831a87d9d"
    else:
        result["register_flags"] = 1
    with TemporaryDirectory(dir=work) as temp:
        trial = Path(temp)
        subprocess.run(["ffmpeg", "-loglevel", "error", "-i", str(args.video), "-map", "0:v:0", "-frames:v", str(frame_ids[-1] + 36),
                        "-c:v", "copy", "-bsf:v", f"{codec}_mp4toannexb", "-f", codec, str(trial / elementary)],
                       check=True)
        packets = subprocess.check_output(["ffprobe", "-loglevel", "error", "-select_streams", "v:0",
                                          "-show_packets", "-show_entries", "packet=pos,size", "-of", "csv=p=0",
                                          str(trial / elementary)])
        (trial / "packets.csv").write_bytes(packets)
        for mode in ("cpu", "gpu"):
            run_host(host, work / f"verify_{codec}.exe", trial, mode, codec, str(width), str(height),
                     str(rate.numerator), str(rate.denominator), *(str(n) for n in frame_ids))
            for n in frame_ids:
                frame = trial / f"{mode}-frame-{n:02d}.nv12"
                reference = oracle[n]
                row = result["frames"].setdefault(str(n), {
                    "ffmpeg_sha256": hashlib.sha256(reference).hexdigest(),
                    "ffmpeg_y_sha256": hashlib.sha256(reference[:luma]).hexdigest(),
                    "ffmpeg_uv_sha256": hashlib.sha256(reference[luma:]).hexdigest(),
                    "ffmpeg_y_range": [min(reference[:luma]), max(reference[:luma])],
                    "ffmpeg_uv_range": [min(reference[luma:]), max(reference[luma:])]})
                sample = frame.read_bytes()
                row[mode + "_sha256"] = hashlib.sha256(sample).hexdigest()
                row[mode + "_y_sha256"] = hashlib.sha256(sample[:luma]).hexdigest()
                row[mode + "_uv_sha256"] = hashlib.sha256(sample[luma:]).hexdigest()
                if sample != reference:
                    raise ValueError(f"Frame {n} differs through {mode} path")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(result, indent=2) + "\n")


if __name__ == "__main__":
    main()
