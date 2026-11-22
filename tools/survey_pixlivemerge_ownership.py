#!/usr/bin/env python3
"""Exercise caller-owned MSVC containers passed to the original PIX parser."""
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
    observations = {}
    with TemporaryDirectory(prefix="tuxbox-pix-ownership-") as temp:
        root = Path(temp)
        tools = Toolchain()
        for crt in ("MT", "MD"):
            work = root / crt
            work.mkdir()
            obj = tools.compile(ROOT / "runtime/pixlivemerge/ownership_probe.cpp", work, "/W4", "/WX", "/" + crt)
            exe = work / "control.exe"
            tools.link(exe, obj, "kernel32.lib")
            shutil.copyfile(args.reference, work / "pixlivemerge.dll")
            run = host.run(exe, ["result.txt"], timeout=90, capture=True)
            result = work / "result.txt"
            if run.returncode or not result.is_file():
                raise RuntimeError(f"{crt} failed ({run.returncode}): {result.read_text() if result.is_file() else run.stdout.decode(errors='replace')[-2000:]}")
            values = dict(line.split("=", 1) for line in result.read_text().splitlines())
            expected = {
                "bad_guid": "80004005,0,3:0040>0000;12:007d>0000;",
                "missing_close": "80004005,0,", "no_path": "80004005,0,",
                "parse": "00000000,1", "mutation": "3:0040>0000,41:007d>0000,",
                "path_length": "780", "spare": "00000000,2,existing,one",
                "growth": "parsed,existing", "ownership_stress": "1",
            }
            if values != expected:
                raise ValueError(f"Unexpected {crt} output: {values}")
            observations[crt] = {"guest_sha256": digest(exe), "results": values}
    receipt = {"scope": "Original DLL only; repeated existing-vector growth, long strings, caller-side modification and destruction with /MT and /MD. Success on this host does not prove compatibility with custom allocators or arbitrary CRTs.",
               "reference_sha256": digest(args.reference), "host": host.version, "runs": observations}
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(observations, indent=2))


if __name__ == "__main__":
    main()
