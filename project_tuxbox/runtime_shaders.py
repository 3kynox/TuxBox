"""Compile Xbox shaders and link raster stages requested by a running Proton game."""
from contextlib import contextmanager
import hashlib
import os
from pathlib import Path
import re
import shutil
import stat
import struct
import subprocess
from tempfile import TemporaryDirectory
from threading import Event, Thread

from .inspect import digest
from .shader_prep import SOURCE_CACHE, compiler_tools, has_dxil_container, run_tool


MAX_SHADER_BYTES = 64 << 20
SOURCE_KEY = r"[a-f0-9]{64}"
LINK_KEY = "_".join([SOURCE_KEY] * 3)


def verified_input(path, expected):
    with os.fdopen(os.open(path, os.O_RDONLY | os.O_NONBLOCK | os.O_NOFOLLOW), "rb") as stream:
        info = os.fstat(stream.fileno())
        if not stat.S_ISREG(info.st_mode):
            raise ValueError(f"{path.name} is not a regular shader file")
        if info.st_size > MAX_SHADER_BYTES:
            raise ValueError("shader exceeds the 64 MiB runtime limit")
        data = stream.read(MAX_SHADER_BYTES + 1)
    if len(data) > MAX_SHADER_BYTES or hashlib.sha256(data).hexdigest() != expected:
        raise ValueError(f"{path.name} contents differ from the requested digest")
    return data


def portable_pixel(data):
    if not has_dxil_container(data):
        return False
    count, = struct.unpack_from("<I", data, 28)
    stages = [struct.unpack_from("<H", data, offset + 10)[0]
              for offset, in struct.iter_unpack("<I", memoryview(data)[32:32 + count * 4])
              if data[offset:offset + 4] == b"DXIL"]
    return stages == [0]  # D3D12_SHVER_PIXEL_SHADER


def compile_request(source, key, tools, cache, session):
    linked = source.suffix == ".link"
    xshader, dxc, dxv = tools
    with TemporaryDirectory(dir=cache) as tmp:
        work = Path(tmp)
        flags = []
        if linked:
            if not stat.S_ISDIR(source.lstat().st_mode):
                raise ValueError("linked shader request must be a directory")
            producer_hash, pixel_hash, dxil_hash = key.split("_")
            cooked = work / "producer.dxbc"
            cooked.write_bytes(verified_input(source / "producer.dxbc", producer_hash))
            pixel = work / "pixel.dxbc"
            pixel.write_bytes(verified_input(source / "pixel.dxbc", pixel_hash))
            pixel_code = verified_input(source / "pixel.cso", dxil_hash)
            if not portable_pixel(pixel_code):
                raise ValueError("pixel order requires a portable pixel DXIL container")
            order = work / "pixel.cso"
            order.write_bytes(pixel_code)
            flags = ["--link-pixel", pixel, "--pixel-order", order]
        else:
            cooked = work / "input.dxbc"
            cooked.write_bytes(verified_input(source, key))

        dest = cache / (key + ".cso")
        checksum = dest.with_suffix(".sha256")
        reply = work / "reply"
        if (dest.is_file() and dest.stat().st_size <= MAX_SHADER_BYTES
                and checksum.is_file() and checksum.stat().st_size == 64):
            # Check the exact bytes that will be returned even if a concurrent
            # launch replaces the persistent entry while this worker is reading.
            shutil.copyfile(dest, reply)
            if checksum.read_bytes() == digest(reply).encode("ascii"):
                os.replace(reply, session / dest.name)
                return False

        hlsl, output, profile = (work / name for name in ("shader.hlsl", "out.cso", "profile"))
        if linked:
            run_tool([dxv, order], timeout=10)
        run_tool([xshader, cooked, *flags, "--hlsl-out", hlsl, "--profile-out", profile], timeout=30)
        target = profile.read_text()
        # Mesh and amplification stages start at shader model 6.5. Linked
        # requests currently repack the outputs of vertex and geometry shaders.
        profiles = r"(vs|gs)_6_[016]" if linked else r"((cs|ps|vs|gs)_6_[016]|(ms|as)_6_[56])"
        if not re.fullmatch(profiles, target):
            raise ValueError(f"unsupported compiler profile {target!r}")
        run_tool([dxc, "-T", target, "-E", "main", "-Fo", output, hlsl], timeout=50)
        run_tool([dxv, output], timeout=10)
        if not has_dxil_container(output.read_bytes()):
            raise ValueError("compiler returned an invalid DXIL container")
        stamp = work / "sha256"
        stamp.write_text(digest(output))
        shutil.copyfile(output, reply)
        os.replace(output, dest)
        os.replace(stamp, checksum)
        os.replace(reply, session / dest.name)
    return True


def remove_request(path):
    if not path.is_symlink() and path.is_dir():
        shutil.rmtree(path)
    else:
        path.unlink(missing_ok=True)


def serve(session, stop, report):
    tools = cache = None
    while not stop.is_set():
        directory = session / "requests"
        requests = [*directory.glob("*.dxbc"), *directory.glob("*.link")]
        if not requests:
            stop.wait(0.05)
            continue
        for source in requests:
            if stop.is_set():
                return
            key = source.stem
            if not re.fullmatch(LINK_KEY if source.suffix == ".link" else SOURCE_KEY, key):
                remove_request(source)
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
                remove_request(source)


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
