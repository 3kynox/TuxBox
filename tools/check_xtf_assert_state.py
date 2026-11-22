#!/usr/bin/env python3
"""Compare XTF assertion state behavior with a supplied original DLL."""
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
    expected = {"initial": "1", "underflow": "1", "disable": "0", "nested": "1",
                "restore1": "0", "thread": "0,1,0", "after_thread": "0",
                "restore2": "1", "deep": "1,1", "level0": "<null>,0",
                "level1": "Assertion,124", "level2": "Error,12", "level3": "Warning,14",
                "level4": "Trace,11", "level5": "Spam,3", "level6": "<null>,0",
                "level7": "<null>,0", "large": "<null>,0"}
    with TemporaryDirectory(prefix="tuxbox-xtf-assert-") as temp:
        root = Path(temp)
        tools = Toolchain()
        obj = tools.compile(ROOT / "runtime/xtfbaseservices/guest.cpp", root, "/W4", "/WX")
        exe = root / "control.exe"
        tools.link(exe, obj, "kernel32.lib")
        guest_hash = digest(exe)
        runs = {}
        for label, source in (("reference", args.reference), ("provider", args.provider)):
            dest = root / label
            dest.mkdir()
            shutil.copyfile(exe, dest / exe.name)
            shutil.copyfile(source, dest / "xtfbaseservices.dll")
            run = host.run(dest / exe.name, ["result.txt"], timeout=90, capture=True)
            result = dest / "result.txt"
            if run.returncode or not result.is_file():
                raise RuntimeError(f"{label} failed ({run.returncode}): {run.stdout.decode(errors='replace')[-2000:]}")
            runs[label] = dict(line.split("=", 1) for line in result.read_text().splitlines())
            if runs[label] != expected:
                raise ValueError(f"Unexpected {label} output: {runs[label]}")
    receipt = {"scope": "Assertion-enable state and severity label/color APIs only. Assertion callbacks, reporting, other XTF APIs and underflow diagnostic logging are not implemented.",
               "reference_sha256": digest(args.reference), "provider_sha256": digest(args.provider),
               "guest_sha256": guest_hash, "host": host.version, "runs": runs}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")
    print("XTF assertion state original and provider controls passed")


if __name__ == "__main__":
    main()
