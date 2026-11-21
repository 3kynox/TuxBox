"""Behavioral gates for the project-owned guest fixtures."""
from datetime import datetime, timezone
import json
from pathlib import Path
import platform
import statistics
import re
import shutil
import sys
import subprocess
import tempfile

from .inspect import digest, image, launch_target

ROOT = Path(__file__).resolve().parents[1]
BUILD = ROOT / "build"
BASE_SUITES = ("cpu", "xmem", "graphics", "graphics-abi", "guest-frame", "present", "lanes",
               "cpu-benchmark", "xg-provider", "gameconfig", "gamechat", "mfreadwrite", "xaudio")
OPTIONAL_PROVIDERS = {"dxc-utils": "dxc-utils/dxcompiler.dll", "xg": "xg/xg_xs.dll"}
SUITES = (*BASE_SUITES, *OPTIONAL_PROVIDERS)


def cpu_model():
    if sys.platform == "linux" and Path("/proc/cpuinfo").exists():
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name") and ":" in line:
                return line.partition(":")[2].strip()
    return platform.processor() or platform.machine()


def verify(host, output, adapter, selected=None, samples=1):
    if adapter and not re.fullmatch(r"[0-9a-fA-F]{4}:[0-9a-fA-F]{4}", adapter):
        raise ValueError("Adapter must be a PCI vendor:device pair in hexadecimal")
    if not selected:
        selected = (*BASE_SUITES, *(suite for suite, provider in OPTIONAL_PROVIDERS.items()
                                   if (BUILD / provider).is_file()))
    cases = []
    builds = {}
    BUILD.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="guest-check-", dir=BUILD) as tmp:
        work = Path(tmp)

        def stage(relative, folder):
            src = BUILD / relative
            if not src.is_file():
                raise ValueError(f"Build the missing fixture first: {src}")
            dest = folder / src.name
            if not dest.exists():
                shutil.copy2(src, dest)
                builds[relative] = digest(dest)
            return dest

        def check(name, binary, arguments, match, expected_code=0, providers=None, config=None, guest_factory=False, dependencies=()):
            row = {"gate": name, "passed": False}
            cases.append(row)
            folder = work / str(len(cases))
            folder.mkdir()
            receipt = folder / "result.txt"
            try:
                exe = stage(binary, folder)
                if providers:
                    modules = image(exe)["imports"]
                    if guest_factory:
                        from_desktop = any(entry["module"].lower() == "d3d12.dll"
                                           for entry in modules)
                        if from_desktop:
                            raise ValueError("Frame fixture imports desktop D3D12 directly")
                    for dll, symbols in providers.items():
                        provider = stage(dll, folder)
                        found = {symbol for mod in modules if mod["module"].lower() == provider.name.lower()
                                 for symbol in mod["symbols"]}
                        missing = set(symbols) - found
                        if missing:
                            raise ValueError(f"Missing static guest imports from {provider.name}: {sorted(missing)}")
                for dependency in dependencies:
                    stage(dependency, folder)
                if config:
                    stage(config, folder)
                    exe = launch_target(folder, "Lanes")
                else:
                    exe = launch_target(exe, None)
                proc = host.run(exe, [*arguments, receipt.name], timeout=120, capture=True)
                text = receipt.read_text(encoding="utf-8").rstrip("\r\n") if receipt.is_file() else None
                row.update(exit_code=proc.returncode, receipt=text)
                row["passed"] = (proc.returncode == expected_code and
                                 (text is None if match is None else text is not None and re.fullmatch(match, text) is not None))
                if not row["passed"]:
                    print(proc.stdout[-2000:].decode(errors="replace"))
            except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                row["error"] = str(error)
            print(f"{'PASS' if row['passed'] else 'FAIL'}  {name}", flush=True)
            return row

        if "cpu" in selected:
            for route, state in [("RRRRDD", "coin_mask=1 ticks=6 bumps=0"), ("LDDRRRR", "coin_mask=2 ticks=7 bumps=1")]:
                check(f"route-{route}", "route-game.exe", [route], f"route-game x=5 y=3 {state} exit=1")
            check("invalid-route", "route-game.exe", ["X"], None, 2)
        gpu_args = ["--adapter", adapter] if adapter else []
        pci = adapter.lower() if adapter else r"[0-9a-f]{4}:[0-9a-f]{4}"
        memory_symbols = ("XMemAllocDefault", "XMemFreeDefault")
        graphics_symbols = ("D3D12SerializeRootSignature",)
        if "dxc-utils" in selected:
            check("dxc-create-blob", "dxc-utils/dxc-utils.exe", ["--report"],
                  r"dxc-utils PASS bytes=257 source-freed=1 writable=257 retained=1",
                  providers={"dxc-utils/dxcompiler_xs.dll": ("DxcCreateInstance",)},
                  dependencies=("dxc-utils/dxcompiler.dll",))
        xg_addresses = r"xg-texture PASS layouts=13 traced-descriptors=4 element-addresses=273263"
        if "xg" in selected:
            check("xg-native-layout", "xg/xg-texture.exe", ["--report"],
                  r"xg-texture PASS descriptors=4 layout-queries=8 plane-records=4 releases=4",
                  providers={"xg/xg_xs.dll": ("XGCreateTextureComputer",)},
                  dependencies=("xmem-smoke/xmem.dll",))
            check("xg-native-addresses", "xg/xg-texture.exe", ["--addresses", "--report"], xg_addresses,
                  providers={"xg/xg_xs.dll": ("XGCreateTextureComputer",)},
                  dependencies=("xmem-smoke/xmem.dll",))
        if "xg-provider" in selected:
            check("xg-addresses", "xg-provider/xg-texture.exe", ["--addresses", "--report"], xg_addresses,
                  providers={"xg-provider/xg_xs.dll": ("XGCreateTextureComputer",)})
        if "gameconfig" in selected:
            check("gameconfig-query", "gameconfig/gameconfig-query.exe", ["--report"],
                  r"gameconfig-query PASS paths=7 hives=2 missing-key=1",
                  providers={"gameconfig/gameconfighelper.dll":
                             ("GetRegistryPathForPackage", "OpenGameConfigKeyForPackage")})
        if "gamechat" in selected:
            check("gamechat-roster", "gamechat/gamechat-session.exe", ["--report"],
                  r"gamechat-session PASS sessions=2 remote-users=3 bitrate-settings=3 stale-handles=1",
                  providers={"gamechat/GameChat2.dll": ("ChatManagerInitialize", "ChatManagerCleanup",
                             "ChatManagerAddRemoteUser", "ChatManagerGetChatUsers", "ChatManagerRemoveUser",
                             "ChatUserGetIsLocal", "ChatManagerSetAudioEncodingBitrate",
                             "ChatManagerGetAudioEncodingBitrate")})
        if "mfreadwrite" in selected:
            check("mfreadwrite-pcm", "mfreadwrite/mfreadwrite-smoke.exe", ["--report"],
                  r"mfreadwrite PASS pcm=48000/2/16 frames=2501 bytes=10004 seek=1200 eos=2 invalid=2",
                  providers={"mfreadwrite/mfreadwrite.dll": ("MFCreateSourceReaderFromByteStream",)})
        if "xaudio" in selected:
            check("xaudio-mix", "xaudio/xaudio.exe", ["--report"],
                  r"xaudio PASS factories=1,9 effects=3 spatial=2/2 mix=2/2",
                  providers={"xaudio/XAudio2_9.dll": ("#1", "#2", "#3", "#4", "#5", "#6", "#9")})
        if "xmem" in selected:
            check("xmem-default", "xmem-smoke/xmem-smoke.exe", ["--report"],
                  re.escape("xmem-default: PASS (ordinary and 64 KiB allocations, host working set v2, virtual reserve/commit)"), providers={"xmem-smoke/xmem.dll": memory_symbols})
        if "graphics" in selected:
            check("desktop-clear", "graphics/desktop-clear.exe", [*gpu_args, "--report"],
                  rf"desktop-clear PASS adapter=.+ pci={pci} pixels=481/481 row_bytes=148 row_pitch=[1-9][0-9]*")
        if "graphics-abi" in selected:
            check("root-signature", "d3d12-smoke/d3d12-root-smoke.exe", [*gpu_args, "--report"],
                  rf"d3d12-root-smoke PASS import=D3D12SerializeRootSignature pci={pci} "
                  r"constant=b5/space2 uav=u3/space4 dispatches=2 words=64/64 guards=16",
                  providers={"d3d12-smoke/d3d12_xs.dll": graphics_symbols})
        frame_match = (rf"xbox-frame PASS pci={pci} submissions=2 snapshots=2 pixels=962/962 "
                       r"inset=208 guards=3 fence=2 compute_fence=7")
        frame_provider = {"d3d12-smoke/d3d12_xs.dll": ("D3D12XboxCreateDevice",)}
        if "guest-frame" in selected:
            check("guest-command-list", "d3d12-smoke/xbox-frame.exe", [*gpu_args, "--report"],
                  frame_match, providers=frame_provider, guest_factory=True)
            check("guest-command-errors", "d3d12-smoke/xbox-frame-negative.exe", [*gpu_args, "--report"],
                  rf"xbox-frame-negative PASS pci={pci} foreign-parents=2 copy-errors=2 recovered=1 rejected-batches=3 compute-present-rejected=1",
                  providers=frame_provider, guest_factory=True)
        if "present" in selected:
            check("guest-present", "d3d12-smoke/xbox-frame.exe", [*gpu_args, "--present", "--report"],
                  frame_match + r" displayed=2/2", providers=frame_provider, guest_factory=True)
        if "lanes" in selected:
            check("lanes-package-replay", "lanes/lanes.exe", ["--replay", *gpu_args, "--report"],
                  rf"lanes-replay PASS pci={pci} course=16/16 win=16/16 safe-miss=1 collision=1",
                  providers={"lanes/xmem.dll": memory_symbols, "lanes/d3d12_xs.dll": graphics_symbols},
                  config="lanes/MicrosoftGame.config")
        if "cpu-benchmark" in selected:
            readings = []
            for _ in range(samples):
                sample = check("cpu-replay", "route-game.exe", ["--bench"],
                               r"route-bench routes=65536 moves=8388608 checksum=24316bf89788468d "
                               r"elapsed_ticks=[1-9][0-9]* frequency=[1-9][0-9]*")
                readings.append(sample)
            if samples > 1:
                del cases[-samples:]
                series = {"gate": "cpu-replay", "passed": all(row["passed"] for row in readings), "samples": readings}
                if series["passed"]:
                    seconds = []
                    for run in readings:
                        ticks, hz = (int(s.split("=")[1]) for s in run["receipt"].split()[-2:])
                        seconds.append(ticks / hz)
                    series["median_ms"] = round(statistics.median(seconds) * 1000, 4)
                cases.append(series)

    report = {"schema": 1, "type": "homebrew", "date_utc": datetime.now(timezone.utc).date().isoformat(),
              "host": platform.system(), "os_release": platform.release(), "cpu": cpu_model(),
              "machine": platform.machine(), "backend": host.backend,
              "provider": host.version, "binaries_sha256": builds, "checks": cases}
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=output.parent, delete=False) as tmp:
        json.dump(report, tmp, indent=2)
        tmp.write("\n")
    Path(tmp.name).replace(output)
    return int(any(not row["passed"] for row in cases))


GATE_LABELS = {
    "route-RRRRDD": "Upper route and guest output", "route-LDDRRRR": "Lower route and collision",
    "invalid-route": "Invalid input exit status", "xmem-default": "Xbox memory allocation, working set and CPU pages",
    "xg-native-layout": "Native Scarlett XG texture layouts with Rust XMem",
    "xg-native-addresses": "Supplied XG texture layouts and element addresses",
    "xg-addresses": "TuxBox XG texture layouts and element addresses",
    "gameconfig-query": "Package registry paths and configuration keys",
    "gamechat-roster": "Game-chat session, remote roster and bitrate settings",
    "mfreadwrite-pcm": "PCM source reading, seeking and end of stream",
    "xaudio-mix": "Xbox audio ordinals, spatial routing and stereo mix",
    "dxc-create-blob": "Xbox DXC utilities and binary blob lifetime",
    "desktop-clear": "Desktop D3D12 pixel readback", "root-signature": "Xbox root-signature import and compute readback",
    "guest-command-list": "Xbox command-list clear and ordered copy readback",
    "guest-command-errors": "Guest command rejection, fence isolation and list recovery",
    "guest-present": "Xbox PresentX and two screen-verified frames",
    "lanes-package-replay": "Lanes package launch and gameplay replay", "cpu-replay": "8,388,608-move CPU replay",
}


def homebrew_matrix(evidence):
    rows = []
    for filename in sorted(evidence.glob("*.json")):
        data = json.loads(filename.read_text(encoding="utf-8"))
        if data.get("type") == "homebrew":
            rows.append((filename.name, data))

    gates = list(dict.fromkeys(r["gate"] for _, data in rows for r in data["checks"]))
    return rows, gates


def status():
    rows, gates = homebrew_matrix(ROOT / "evidence")
    lines = ["# Measured homebrew results", "",
             "Each column represents one run on its named host, provider and hardware. "
             "Each receipt records the guest output, exit status and binary hashes.", ""]
    names = [f"[{Path(name).stem}](../evidence/{name})" for name, _ in rows]
    pending_windows = not any(data["host"] == "Windows" for _, data in rows)
    if pending_windows:
        names.append("Physical Windows x64")
    lines.append("| Gate | " + " | ".join(names) + " |")
    lines.append("|---" * (1 + len(rows) + int(pending_windows)) + "|")
    for gate in gates:
        cells = []
        for _, d in rows:
            result = next((r for r in d["checks"] if r["gate"] == gate), None)
            cells.append("Pass" if result and result["passed"] else "Fail" if result else "—")
        if pending_windows:
            cells.append("Pending")
        lines.append("| " + " | ".join([GATE_LABELS.get(gate, gate), *cells]) + " |")
    lines += ["", "Rebuild this table from the repository root with "
              "`python tuxbox.py status > docs/RESULTS.md`. A dash means that the gate was omitted from that run. "
              "[Xbox DLL checks](LIBRARIES.md#run-the-provider-checks) · [Milestones and retail evidence](PROGRESS.md)"]
    return "\n".join(lines)
