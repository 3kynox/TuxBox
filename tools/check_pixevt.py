#!/usr/bin/env python3
"""Compare PIX no-capturer behavior and opt-in counter logging with a supplied DLL.

The reference is used only in a local control. Xbox PIX capture remains
unsupported; this check exercises no-capturer results and diagnostic counters.
"""
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
    for name in ("reference", "provider", "xmem", "output"):
        ap.add_argument("--" + name, type=Path, required=True)
    ap.add_argument("--proton", type=Path)
    ap.add_argument("--prefix", type=Path)
    a = ap.parse_args()
    host = Host(None, a.proton, a.prefix)
    host.env["WINEDEBUG"] = "-all,warn+debugstr"
    host.env["TUXBOX_LOG"] = "1"
    with TemporaryDirectory(prefix="tuxbox-pixevt-") as temp:
        root = Path(temp)
        tools = Toolchain()
        obj = tools.compile(ROOT / "runtime/pixevt/guest.cpp", root, "/W4", "/WX")
        exe = root / "pixevt-control.exe"
        tools.link(exe, obj, "kernel32.lib")
        guest_hash = digest(exe)
        runs = {}
        for label, source, trace in (("reference", a.reference, False),
                                     ("provider", a.provider, False),
                                     ("provider_trace", a.provider, True)):
            dest = root / label
            dest.mkdir()
            shutil.copyfile(exe, dest / exe.name)
            shutil.copyfile(source, dest / "pixevt.dll")
            shutil.copyfile(a.xmem, dest / "xmem.dll")
            host.env["TUXBOX_TRACE_PIX"] = "1" if trace else "0"
            run = host.run(dest / exe.name, ["result.txt"], timeout=90, capture=True)
            path = dest / "result.txt"
            if run.returncode or not path.is_file():
                raise RuntimeError(f"{label} failed ({run.returncode}): {run.stdout.decode(errors='replace')[-2000:]}")
            log = run.stdout.decode(errors="replace")
            found = all(line in log for line in ("TuxBox PIX counter: frame-cost-ms = 2.25", "TuxBox PIX counter: queue-depth = 17"))
            if found != trace:
                raise ValueError(f"Counter trace did not match opt-in state for {label}")
            runs[label] = {"results": dict(line.split("=", 1) for line in path.read_text().splitlines()),
                           "counter_trace_found": found}
        if any(runs[label]["results"] != runs["reference"]["results"] for label in ("provider", "provider_trace")):
            raise ValueError("No-capturer API results differ from the reference")
    result = {"scope": "No PIX capture backend: capture state, unavailable begin/end operations, ordinal lookups and optional TuxBox counter log. No actual PIX capture or backend registration is implemented.",
              "reference_sha256": digest(a.reference), "provider_sha256": digest(a.provider),
              "guest_sha256": guest_hash, "xmem_sha256": digest(a.xmem), "host": host.version, "runs": runs}
    a.output.parent.mkdir(parents=True, exist_ok=True)
    a.output.write_text(json.dumps(result, indent=2) + "\n")
    print("PIX no-capturer and counter controls passed")


if __name__ == "__main__":
    main()
