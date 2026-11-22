#!/usr/bin/env python3
"""Survey PIX ParsePdbInfo with a locally supplied original DLL."""
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
    ap.add_argument("--reference", type=Path, required=True)
    ap.add_argument("--output", type=Path, required=True)
    ap.add_argument("--proton", type=Path)
    ap.add_argument("--prefix", type=Path)
    args = ap.parse_args()
    host = Host(None, args.proton, args.prefix)
    host.env["WINEDEBUG"] = "-all"
    with TemporaryDirectory(prefix="tuxbox-pix-pdb-parse-") as temp:
        root = Path(temp)
        tools = Toolchain()
        obj = tools.compile(ROOT / "runtime/pixlivemerge/parse_probe.cpp", root, "/W4", "/WX")
        exe = root / "control.exe"
        tools.link(exe, obj, "kernel32.lib")
        guest_sha = digest(exe)
        shutil.copyfile(args.reference, root / "pixlivemerge.dll")
        run = host.run(exe, ["result.txt"], timeout=90, capture=True)
        result = root / "result.txt"
        if run.returncode or not result.is_file():
            raise RuntimeError(f"Control failed ({run.returncode}): {run.stdout.decode(errors='replace')[-2000:]}")
        values = dict(line.split("=", 1) for line in result.read_text().splitlines())
    expected = {
        "empty": "00000000,0", "malformed": "80004005,0",
        "valid": "00000000,1,a:12345678:42:test.pdb",
        "missing_space": "00000000,1,a:12345678:2:test",
        "multi": "00000000,2,one:00000000:1:first.pdb,wo:12345678:2:second.pdb",
    }
    if values != expected:
        raise ValueError(f"Original DLL survey differs from expected values: {values}")
    receipt = {"scope": "Original DLL survey only; no provider implementation. ParsePdbInfo mutates input and appends to MSVC C++ containers; allocator compatibility and malformed-input behavior need further validation.",
               "reference_sha256": digest(args.reference), "guest_sha256": guest_sha,
               "host": host.version, "observations": values}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(values, indent=2))


if __name__ == "__main__":
    main()
