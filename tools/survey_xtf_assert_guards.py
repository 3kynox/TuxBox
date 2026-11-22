#!/usr/bin/env python3
"""Survey XTF assertion recursion and concurrent dispatch with the original DLL."""
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
    host.env["WINEDEBUG"] = "-all,+debugstr"
    with TemporaryDirectory(prefix="tuxbox-xtf-guards-") as temp:
        root = Path(temp)
        tools = Toolchain()
        obj = tools.compile(ROOT / "runtime/xtfbaseservices/handler_guard_probe.cpp", root, "/W4", "/WX")
        exe = root / "control.exe"
        tools.link(exe, obj, "kernel32.lib")
        guest_sha = digest(exe)
        shutil.copyfile(args.reference, root / "xtfbaseservices.dll")
        run = host.run(exe, ["result.txt"], timeout=90, capture=True)
        result = root / "result.txt"
        if run.returncode or not result.is_file():
            raise RuntimeError(f"Control failed ({run.returncode}): {result.read_text() if result.is_file() else run.stdout.decode(errors='replace')[-3000:]}")
        values = dict(line.split("=", 1) for line in result.read_text().splitlines())
    expected = {"normal": "00000000,1,0", "start": "1", "debug1": "Multiple calls to assert.", "recursive": "1,0,1",
                "debug2": "Multiple calls to assert.", "concurrent": "1,0,2", "id": "4"}
    if values != expected:
        raise ValueError(f"Original assertion guard behavior differs: {values}")
    receipt = {"scope": "Original DLL only. The guest temporarily intercepts OutputDebugStringA through the DLL import table. A vectored exception handler catches the breakpoints and advances RIP as needed by Proton. With level 5 a normal assertion returned success and generated no intercepted OutputDebugStringA message in this environment; other outputs, native Windows breakpoint handling and full built-in assertion logging are not established; no TuxBox handler implementation.",
               "reference_sha256": digest(args.reference), "guest_sha256": guest_sha,
               "host": host.version, "observations": values}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
