#!/usr/bin/env python3
"""Survey XTF assertion callback dispatch with a locally supplied original DLL."""
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
    with TemporaryDirectory(prefix="tuxbox-xtf-handlers-") as temp:
        root = Path(temp)
        tools = Toolchain()
        obj = tools.compile(ROOT / "runtime/xtfbaseservices/handler_probe.cpp", root, "/W4", "/WX")
        exe = root / "control.exe"
        tools.link(exe, obj, "kernel32.lib")
        guest_sha = digest(exe)
        shutil.copyfile(args.reference, root / "xtfbaseservices.dll")
        run = host.run(exe, ["result.txt"], timeout=90, capture=True)
        result = root / "result.txt"
        if run.returncode or not result.is_file():
            raise RuntimeError(f"Control failed ({run.returncode}): {run.stdout.decode(errors='replace')[-2000:]}")
        values = dict(line.split("=", 1) for line in result.read_text().splitlines())
    expected = {
        "ids": "0,4,5,6,5,7",
        "dispatch": "C:condition,file.cpp,42,value=37;A:condition,file.cpp,42,value=37;B:condition,file.cpp,42,value=37;ret:1",
        "snapshot": "C:condition,file.cpp,42,value=38;A:condition,file.cpp,42,value=38;B:condition,file.cpp,42,value=38;ret:1",
        "after": "C:condition,file.cpp,42,value=39;ret:1", "remove": "1,0,0,0",
    }
    if values != expected:
        raise ValueError(f"Original assertion callback behavior differs: {values}")
    receipt = {"scope": "Original DLL survey of assert callbacks only; TuxBox does not implement handler registration or dispatch. Built-in assertion logging/debugging and recursive/concurrent dispatch remain unverified.",
               "reference_sha256": digest(args.reference), "guest_sha256": guest_sha,
               "host": host.version, "observations": values}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(values, indent=2))


if __name__ == "__main__":
    main()
