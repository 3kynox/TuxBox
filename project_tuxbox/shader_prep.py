"""Prepare profiled retail shaders in the cache read by the staged graphics provider."""
import ctypes
from ctypes.util import find_library
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory

from .inspect import digest
from .prep_progress import desktop_progress


ROOT = Path(__file__).resolve().parents[1]
FROSTY_PLAN = ROOT / "tools/reference/frosty-first-boot.json"
PROFILES = {"frosty-xsx-shipping.exe": FROSTY_PLAN}
CACHE = Path("drive_c/users/steamuser/AppData/Local/Temp/tombox_shader_cache/by_hlsl")


def cooked_shader(pak, record):
    """Read one profiled, possibly LZ4-packed program from the local retail package."""
    with pak.open("rb") as stream:
        stream.seek(record["offset"])
        source = stream.read(record["packed_bytes"])
    if len(source) != record["packed_bytes"]:
        raise ValueError(f"Truncated shader entry {record['entry']} in {pak}")
    size = record["unpacked_bytes"]
    if len(source) != size:
        library = find_library("lz4")
        if not library:
            raise ValueError("liblz4 is required to prepare the packaged shaders")
        decode = ctypes.CDLL(library).LZ4_decompress_safe
        decode.argtypes = [ctypes.c_char_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_int]
        target = ctypes.create_string_buffer(size)
        if decode(source, target, len(source), size) != size:
            raise ValueError(f"Cannot decompress shader entry {record['entry']}")
        source = target.raw
    start = record["dxbc_offset"]
    cooked = source[start:start + record["dxbc_bytes"]]
    if hashlib.sha256(cooked).hexdigest() != record["dxbc_sha256"]:
        raise ValueError(f"Package shader differs at {record['archive']} entry {record['entry']}")
    return cooked


def compiler_tools():
    from build import find_dxc

    override = os.environ.get("TUXBOX_XSHADER")
    local = ROOT / "build/frosty-shader-prep/xshader"
    xshader = shutil.which(override or "xshader") or (None if override else local)
    if not xshader or not os.access(xshader, os.X_OK):
        raise ValueError("Set TUXBOX_XSHADER to xshader built from the staged TomBox sources")
    dxc = Path(find_dxc(None))
    dxv = dxc.with_name("dxv")
    if not dxv.is_file():
        dxv = shutil.which("dxv")
    if not dxv:
        raise ValueError("Install the DXIL validator (dxv) from the DirectX Shader Compiler toolchain")
    return xshader, dxc, dxv


def run_tool(command):
    result = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
                            text=True, errors="replace")
    if result.returncode:
        raise ValueError(f"{Path(command[0]).name} failed ({result.returncode}): {result.stderr[-600:].strip()}")


def compile_shader(pak, record, tools, work):
    xshader, dxc, dxv = tools
    cooked, hlsl, dxil = (work / name for name in ("shader.dxbc", "shader.hlsl", "shader.cso"))
    cooked.write_bytes(cooked_shader(pak, record))
    hlsl.unlink(missing_ok=True)
    run_tool([xshader, cooked, "--hlsl-out", hlsl])
    if not hlsl.is_file() or digest(hlsl) != record["hlsl_sha256"]:
        raise ValueError(f"xshader translation differs from the staged provider for entry {record['entry']}")
    run_tool([dxc, "-T", record["profile"], "-E", "main", "-Fo", dxil, hlsl])
    run_tool([dxv, dxil])
    return dxil


def has_dxil_container(data):
    """Check the container and bitcode bounds without depending on compiler tools."""
    if len(data) < 32 or data[:4] != b"DXBC":
        return False
    size, count = struct.unpack_from("<II", data, 24)
    toc_end = 32 + count * 4
    if not count or not toc_end <= size <= len(data):
        return False
    found = False
    for offset, in struct.iter_unpack("<I", data[32:toc_end]):
        if offset < toc_end or offset + 8 > size:
            return False
        tag, length = struct.unpack_from("<4sI", data, offset)
        start = offset + 8
        if start + length > size:
            return False
        if tag == b"DXIL":
            if length < 24 or data[start + 8:start + 12] != b"DXIL":
                return False
            bit_offset, bit_size = struct.unpack_from("<II", data, start + 16)
            bit_start = 8 + bit_offset
            if bit_size < 4 or bit_start + bit_size > length:
                return False
            found = data[start + bit_start:start + bit_start + 4] == b"BC\xc0\xde"
    return found


def checked_entries(cache, records, previous):
    """Remove structurally broken programs from the provider's lookup path even if compilation fails."""
    outputs = {}
    for key, record in records.items():
        entry = cache / (key + ".cso")
        try:
            data = entry.read_bytes()
        except FileNotFoundError:
            continue
        actual = hashlib.sha256(data).hexdigest()
        if actual in (previous.get(key), record["observed_dxil_sha256"]):
            outputs[key] = actual
        elif not has_dxil_container(data):
            # TomBox trusts any nonempty cache file. Keep a recoverable copy and
            # let its normal cache miss path rebuild this shader on demand.
            quarantine = cache.parent / "tuxbox-invalid"
            quarantine.mkdir(exist_ok=True)
            os.replace(entry, quarantine / f"{key}.{actual}.cso")
    return outputs


def prepare_shaders(pak, plan, cache, progress=None):
    """Recompile missing or damaged entries. Return counts of compiled and reused files."""
    import fcntl  # The profiled cache layout belongs to the Linux Proton backend.

    cache.mkdir(parents=True, exist_ok=True)
    state = cache.parent / "tuxbox-first-boot.json"
    # Keep this inside the provider cache so a cache reset also resets the receipt.
    with (cache.parent / "tuxbox-first-boot.lock").open("a+b") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        version = hashlib.sha256(json.dumps(plan, sort_keys=True).encode()).hexdigest()
        try:
            receipt = json.loads(state.read_text())
        except (FileNotFoundError, UnicodeDecodeError, json.JSONDecodeError):
            receipt = {}
        previous = receipt.get("outputs", {}) if isinstance(receipt, dict) and receipt.get("plan") == version else {}
        if not isinstance(previous, dict):
            previous = {}
        compiled = 0
        tools = None
        with TemporaryDirectory(dir=cache.parent, prefix=".prepare-") as directory:
            work = Path(directory)
            unique = {record["cache_key"]: record for record in plan["shaders"]}
            # Inspect every file before looking for tools; one missing compiler
            # must not leave later truncated entries in the runtime cache.
            outputs = checked_entries(cache, unique, previous)
            for key, record in unique.items():
                entry = cache / (key + ".cso")
                if key in outputs:
                    continue
                if tools is None:
                    tools = compiler_tools()
                    print(f"Preparing {plan['title']} shaders for first use ({len(unique)} profiled programs)...",
                          file=sys.stderr, flush=True)
                message = f"Compiling {record['archive']} shader {record['entry']} ({record['profile']})"
                if progress:
                    progress(message)
                print(f"  {message}", file=sys.stderr, flush=True)
                dxil = compile_shader(pak, record, tools, work)
                outputs[key] = digest(dxil)
                os.replace(dxil, entry)
                compiled += 1
                # Preserve completed work if a later compile fails or is interrupted.
                pending = work / "receipt.json"
                pending.write_text(json.dumps({"plan": version, "outputs": {**previous, **outputs}}) + "\n")
                os.replace(pending, state)
            if receipt != {"plan": version, "outputs": outputs}:
                pending = work / "receipt.json"
                pending.write_text(json.dumps({"plan": version, "outputs": outputs}) + "\n")
                os.replace(pending, state)
        return compiled, len(outputs) - compiled


def prepare_for_launch(executable, host):
    """Return False when the selected target has no matching preparation profile."""
    profile = PROFILES.get(executable.name.lower())
    if host.backend != "proton" or profile is None:
        return False
    plan = json.loads(profile.read_text())
    provider = executable.with_name(plan["provider"])
    if not provider.is_file() or digest(provider) != plan["provider_sha256"]:
        return False
    if digest(executable) != plan["executable_sha256"]:
        return False
    pak = executable.parent / plan["shader_package"]
    cache = Path(host.env["WINEPREFIX"]) / CACHE
    with desktop_progress(plan["title"], host.env) as progress:
        compiled, reused = prepare_shaders(pak, plan, cache, progress=progress)
        print(f"{plan['title']} shaders ready: {compiled} compiled, {reused} cached.", file=sys.stderr, flush=True)
        if plan.get("pipeline_profile") == "frosty-chase":
            from .pipeline_prep import prepare_pipelines
            prepare_pipelines(executable, host, plan, cache, progress=progress)
    return True
