"""Compile unprofiled Xbox shaders requested by a running Proton game."""
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
from tempfile import TemporaryDirectory
from threading import Event, Thread

from .inspect import digest
from .shader_prep import SOURCE_CACHE, compiler_tools, has_dxil_container, run_tool


def compile_request(source, key, tools, cache, session):
    # Hash the exact snapshot sent to the compiler. Other GPU workers can
    # submit the same source while the first request is in flight.
    if source.stat().st_size > 64 << 20:
        raise ValueError("shader exceeds the 64 MiB runtime limit")
    with source.open("rb") as stream:
        data = stream.read((64 << 20) + 1)
    if len(data) > 64 << 20:
        raise ValueError("oversized shader request")
    if hashlib.sha256(data).hexdigest() != key:
        raise ValueError("shader contents differ from the requested digest")
    dest = cache / (key + ".cso")
    xshader, dxc, dxv = tools
    with TemporaryDirectory(dir=cache) as tmp:
        work = Path(tmp)
        checksum = dest.with_suffix(".sha256")
        if (dest.is_file() and dest.stat().st_size <= 64 << 20 and checksum.is_file()
                and checksum.stat().st_size == 64 and checksum.read_bytes() == digest(dest).encode("ascii")):
            compiled = False
        else:
            cooked, hlsl, output, profile = (work / name for name in ("input.dxbc", "shader.hlsl", "out.cso", "profile"))
            cooked.write_bytes(data)
            run_tool([xshader, cooked, "--hlsl-out", hlsl, "--profile-out", profile], timeout=30)
            target = profile.read_text()
            if not re.fullmatch(r"(cs|ps|vs|gs)_6_[016]", target):
                raise ValueError(f"unsupported compiler profile {target!r}")
            run_tool([dxc, "-T", target, "-E", "main", "-Fo", output, hlsl], timeout=50)
            run_tool([dxv, output], timeout=10)
            if not has_dxil_container(output.read_bytes()):
                raise ValueError("compiler returned an invalid DXIL container")
            stamp = work / "sha256"
            stamp.write_text(digest(output))
            os.replace(output, dest)
            os.replace(stamp, checksum)
            compiled = True
        # The persistent cache is shared by launches; every running title gets
        # its own reply directory and only sees a completely written file.
        reply = work / "reply"
        shutil.copyfile(dest, reply)
        os.replace(reply, session / dest.name)
    return compiled


def serve(session, stop, report):
    tools = cache = None
    while not stop.is_set():
        requests = list((session / "requests").glob("*.dxbc"))
        if not requests:
            stop.wait(0.05)
            continue
        for source in requests:
            if stop.is_set():
                return
            key = source.stem
            if not re.fullmatch("[a-f0-9]{64}", key):
                source.unlink(missing_ok=True)
                continue
            try:
                if tools is None:
                    selected = compiler_tools()
                    version = subprocess.check_output([selected[1], "--version"], timeout=10)
                    fingerprint = hashlib.sha256(version)
                    for tool in selected:
                        fingerprint.update(bytes.fromhex(digest(Path(tool))))
                    cache = session.parent / "runtime" / fingerprint.hexdigest()
                    cache.mkdir(parents=True, exist_ok=True)
                    tools = selected
                compiled = compile_request(source, key, tools, cache, session)
                if compiled:
                    report(f"Compiled Xbox shader {key}")
            except (OSError, ValueError, subprocess.SubprocessError) as exc:
                (session / (key + ".failed")).touch()
                report(f"Xbox shader {key}: {exc}")
            finally:
                source.unlink(missing_ok=True)


@contextmanager
def runtime_shader_compiler(backend, env, executable, report):
    if backend != "proton" or env.get("TUXBOX_RUNTIME_SHADERS") == "0" or not executable.with_name("d3d12_xs.dll").is_file():
        yield
        return
    cache = Path(env["WINEPREFIX"]) / SOURCE_CACHE.parent
    with shader_session(cache, env, report):
        yield


@contextmanager
def shader_session(cache, env, report):
    cache.mkdir(parents=True, exist_ok=True)
    with TemporaryDirectory(prefix="run-", dir=cache) as tmp:
        session = Path(tmp)
        (session / "requests").mkdir()
        env["TUXBOX_SHADER_SESSION"] = session.name
        stop = Event()
        worker = Thread(target=serve, args=(session, stop, report), name="shader-compiler", daemon=True)
        worker.start()
        try:
            yield
        finally:
            stop.set()
            worker.join()
            env.pop("TUXBOX_SHADER_SESSION", None)
