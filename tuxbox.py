#!/usr/bin/env python3
"""Project TuxBox development CLI."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
from xml.etree import ElementTree

try:
    import pefile
except ImportError:
    sys.exit("Install the inspection dependency with python -m pip install -r requirements.txt")

from project_tuxbox.host import Host
from project_tuxbox.inspect import image, launch_target, package
from project_tuxbox.progress_map import library_table, write_map
from project_tuxbox.shader_prep import prepare_for_launch
from project_tuxbox.verify import SUITES, verify, status


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    subs = parser.add_subparsers(required=True, dest="command")
    inspect = subs.add_parser("inspect", help="inventory a local package or PE, including ordinary and delay imports")
    inspect.add_argument("path", type=Path)
    inspect.add_argument("--json", action="store_true", help="include full symbol tables and file hashes")
    run = subs.add_parser("run", help="launch a prepared x64 executable or manifest entry")
    run.add_argument("--app-id", help="select a manifest entry by its Id")
    preparation = run.add_mutually_exclusive_group()
    preparation.add_argument("--prepare-only", action="store_true", help="prepare supported shader caches and exit")
    preparation.add_argument("--no-shader-prep", action="store_true", help="skip shader preparation for this launch")
    run.add_argument("target", type=Path)
    run.add_argument("arguments", nargs=argparse.REMAINDER)
    check = subs.add_parser("verify", help="run the built homebrew fixtures and write an evidence record")
    check.add_argument("--benchmark-samples", type=int, choices=range(1, 31), default=1, metavar="1..30")
    check.add_argument("--output", required=True, type=Path)
    check.add_argument("--only", choices=SUITES, action="append", help="restrict checks to the given suite (repeatable)")
    check.add_argument("--adapter", help="select a hardware graphics adapter by PCI vendor:device")
    for cmd in (run, check):
        cmd.add_argument("--backend", choices=("native", "proton", "wine"))
        cmd.add_argument("--proton", help="installed Valve or GE-Proton directory")
        cmd.add_argument("--prefix", type=Path, help="Linux prefix directory for this provider")
    show = subs.add_parser("status", help="show recorded milestone evidence by host")
    status_format = show.add_mutually_exclusive_group()
    status_format.add_argument("--svg", type=Path, metavar="PATH", help="write the visual progress map")
    status_format.add_argument("--libraries", action="store_true", help="list verified exports per Xbox DLL")
    args = parser.parse_args()
    if args.command == "status":
        evidence = Path(__file__).resolve().parent / "evidence"
        if args.svg:
            write_map(evidence, args.svg)
        elif args.libraries:
            print(library_table(evidence))
        else:
            print(status())
        return 0
    if args.command == "inspect":
        path = args.path.expanduser().resolve()
        if path.is_dir():
            data = package(path)
            if not args.json:
                print(f"Package {data['identity'].get('Name', '(unnamed)')} {data['identity'].get('Version', '')}")
                for entry in data["executables"]:
                    present = "present" if entry["present"] else "missing"
                    print(f"{entry['id']}  {entry['target']}  {entry['path']}  {present}")
                print(f"{len(data['libraries'])} DLLs inventoried; {sum('error' in lib for lib in data['libraries'])} unreadable")
                print("Files by extension:", json.dumps(data["files_by_extension"]))
                return 0
        else:
            data = image(path)
            if not args.json:
                print(f"{data['filename']}  {data['kind']}  machine {data['machine']}  entry {data['entry_rva']}")
                for kind in ("imports", "delay_imports"):
                    for desc in data[kind]:
                        print(f"{kind}  {desc['module']}  {len(desc['symbols'])} symbols")
                return 0
        print(json.dumps(data, indent=2))
        return 0
    if args.command == "run":
        exe = launch_target(args.target.expanduser(), args.app_id)
        host = Host(args.backend, args.proton, args.prefix)
        if args.prepare_only or (not args.no_shader_prep and os.environ.get("TUXBOX_SHADER_PREP") != "0"
                                 and "-nullrhi" not in (arg.lower() for arg in args.arguments)):
            try:
                supported = prepare_for_launch(exe, host)
            except (OSError, ValueError, subprocess.SubprocessError) as exc:
                if args.prepare_only:
                    raise
                print(f"Shader preparation incomplete: {exc}. Starting the game.", file=sys.stderr)
            else:
                if args.prepare_only:
                    if not supported:
                        print("No matching shader preparation profile for this title and provider.", file=sys.stderr)
                    return 0 if supported else 1
        return host.run(exe, args.arguments).returncode
    host = Host(args.backend, args.proton, args.prefix)
    return verify(host, args.output, args.adapter, args.only, args.benchmark_samples)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, pefile.PEFormatError, ElementTree.ParseError, subprocess.SubprocessError) as exc:
        sys.exit(f"tuxbox: {exc}")
