#!/usr/bin/env python3
"""Exercise Frosty's two geometry/pixel links and their cache rejection paths."""
import argparse
import json
from pathlib import Path
import shutil
import sys
from tempfile import TemporaryDirectory

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from build import Toolchain
from project_tuxbox.host import Host
from project_tuxbox.inspect import digest
from project_tuxbox.shader_prep import FROSTY_PLAN, cooked_shader

PROGRAMS = {"vertex": ("global", 2423), "geometry": ("global", 2420),
            "pixel0": ("global", 2421), "pixel1": ("material", 687)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("pak", "cache", "provider", "prefix", "proton", "output"):
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--adapter", required=True)
    parser.add_argument("--frame-seven", action="store_true", help="also check the five-target material vertex/pixel pipeline")
    args = parser.parse_args()
    profile = json.loads(FROSTY_PLAN.read_text())["source_cache"]
    rows = {(row["archive"], row["entry"]): row for row in profile["additional_shaders"]}
    programs = {name: rows[key] for name, key in PROGRAMS.items()}
    if args.frame_seven:
        programs.update(vertex7=rows["material", 2517], pixel7=rows["material", 2522])
    parent = ROOT / "build/frosty-geometry-link"
    parent.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(dir=parent) as temporary:
        trial = Path(temporary)
        staging = trial / "by_source"
        staging.mkdir()
        for name, row in programs.items():
            (trial / f"{name}.dxbc").write_bytes(cooked_shader(args.pak, row))
            key = row["dxbc_sha256"] + ".cso"
            shutil.copyfile(args.cache / "by_source" / key, staging / key)
        shutil.copyfile(staging / (programs["vertex"]["dxbc_sha256"] + ".cso"), trial / "wrong-stage.cso")
        links = []
        for n in range(2):
            pair = programs["geometry"]["dxbc_sha256"] + "_" + programs[f"pixel{n}"]["dxbc_sha256"]
            source = args.cache / "by_gs_ps" / (pair + ".cso")
            links.append(dict(pair=pair, compiled_sha256=digest(source)))
            shutil.copyfile(source, trial / f"linked{n}.cso")
        if args.frame_seven:
            vs, ps = (programs[name]["dxbc_sha256"] for name in ("vertex7", "pixel7"))
            pair = f"{vs}_{ps}"
            source = args.cache / "by_vs_ps" / (pair + ".cso")
            links.append(dict(pair=pair, compiled_sha256=digest(source)))
            shutil.copyfile(source, trial / "linked7.cso")
            shutil.copyfile(staging / (ps + ".cso"), trial / "wrong-vertex-stage.cso")
            (trial / "vertex7.key").write_text(pair + ".cso")
            (trial / "other-consumer.key").write_text(f"{vs}_{programs['pixel0']['dxbc_sha256']}.cso")
        t = Toolchain()
        source = ROOT / "tools/reference/chase-link/geometry.cpp"
        obj = t.compile(source, trial, "/W4", "/WX", f"/I{ROOT / 'runtime/d3d12'}")
        exe = trial / "geometry.exe"
        t.link(exe, obj, str(ROOT / "build/d3d12-smoke/d3d12_xs.lib"), "dxgi.lib", "kernel32.lib")
        shutil.copyfile(args.provider, trial / "d3d12_xs.dll")
        host = Host(None, args.proton, args.prefix)
        result = host.run(exe, [args.adapter], timeout=60, capture=True)
        receipt = trial / "result.txt"
        checks = dict(line.split("=", 1) for line in receipt.read_text().splitlines()) if receipt.exists() else {}
        expected = dict(pci=args.adapter.lower(), linked="2", absent="1", wrong_consumer="1", wrong_stage="1")
        if args.frame_seven:
            expected.update(vertex_linked="1", vertex_absent="1", vertex_wrong_consumer="1", vertex_wrong_stage="1")
        if result.returncode or checks != expected:
            sys.stderr.buffer.write(result.stdout)
            raise ValueError(f"Geometry pipeline control failed ({result.returncode}, {checks})")
        output = dict(proton=host.version, provider_sha256=digest(args.provider), guest_sha256=digest(exe),
                      source_shaders={name: row["dxbc_sha256"] for name, row in programs.items()},
                      links=links, checks=checks)
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(output, indent=2) + "\n")


if __name__ == "__main__":
    main()
