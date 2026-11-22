#!/usr/bin/env python3
"""Compare the PIX PDB record lookup with a locally supplied original DLL."""
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


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    for name in ("reference", "provider", "output"):
        ap.add_argument("--" + name, type=Path, required=True)
    ap.add_argument("--proton", type=Path)
    ap.add_argument("--prefix", type=Path)
    args = ap.parse_args()
    host = Host(None, args.proton, args.prefix)
    host.env["WINEDEBUG"] = "-all"
    expected = {"short": "0", "long": "1", "case": "0", "first": "0", "miss": "-1",
                "null": "-1", "nonascii_equal": "3", "nonascii_case": "-1", "empty": "-1",
                "blank": "0", "blank_path": "0", "records_intact": "1"}
    with TemporaryDirectory(prefix="tuxbox-pixlivemerge-") as temp:
        root = Path(temp)
        tools = Toolchain()
        obj = tools.compile(ROOT / "runtime/pixlivemerge/guest.cpp", root, "/W4", "/WX")
        exe = root / "control.exe"
        tools.link(exe, obj, "kernel32.lib")
        guest_hash = digest(exe)
        runs = {}
        for label, source in (("reference", args.reference), ("provider", args.provider)):
            dest = root / label
            dest.mkdir()
            shutil.copyfile(exe, dest / exe.name)
            shutil.copyfile(source, dest / "pixlivemerge.dll")
            run = host.run(dest / exe.name, ["result.txt"], timeout=90, capture=True)
            result = dest / "result.txt"
            if run.returncode or not result.is_file():
                raise RuntimeError(f"{label} failed ({run.returncode}): {run.stdout.decode(errors='replace')[-2000:]}")
            runs[label] = dict(line.split("=", 1) for line in result.read_text().splitlines())
            if runs[label] != expected:
                raise ValueError(f"Unexpected {label} result: {runs[label]}")
    receipt = {"scope": "FindPdbInfo ordinal 2 only; x64 MSVC vector and wstring ABI, valid terminated UTF-16 strings. No PDB parsing or capture backend.",
               "reference_sha256": digest(args.reference), "provider_sha256": digest(args.provider),
               "guest_sha256": guest_hash, "host": host.version, "runs": runs}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")
    print("PIX PDB lookup reference and provider controls passed")


if __name__ == "__main__":
    main()
