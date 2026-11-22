#!/usr/bin/env python3
"""Identify Frosty's packaged temporal super-resolution maps and test their compute programs with DXC."""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import shlex
import struct
import subprocess
import sys
from tempfile import TemporaryDirectory
import zlib

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from project_tuxbox.chase_warmup import extract_programs
from project_tuxbox.inspect import digest
from project_tuxbox.shader_inventory import ARCHIVES, TITLE, decoder, parse_tables
from project_tuxbox.shader_prep import compiler_tools

CACHE = "d6640174767d44dfa1049fd147b495bab214bd3c5a27d931d628b3e2e95cf37c"
TSR_DIR = "/Engine/Private/TemporalSuperResolution/"


def name_keys(executable, work):
    data = executable.read_bytes()
    # Both the registered shader type name and its virtual source filename survive in the shipping PE.
    suffix = rb"(?:[A-Za-z0-9]\x00)+"
    def names(prefix, ending):
        regex = re.compile(re.escape(prefix.encode("utf-16le")) + suffix + re.escape(ending.encode("utf-16le")) + b"\0\0")
        return [m.group()[:-2].decode("utf-16le") for m in regex.finditer(data)]

    identifiers = sorted(set(names(TSR_DIR + "TSR", ".usf") + names("FTSR", "CS")))
    if not identifiers:
        raise ValueError("No TSR shader identifiers in this build")
    # FHashedName's non-numbered names are CityHash64WithSeed(uppercase ANSI bytes, 0).
    try:
        from cityhash import CityHash64WithSeed
    except ImportError:
        # Distribution libabsl-dev is sufficient even without the optional Python cityhash module.
        flags = shlex.split(subprocess.check_output(["pkg-config", "--cflags", "--libs", "absl_city"], text=True))
        helper = work / "name-keys"
        code = """#include <absl/hash/internal/city.h>
#include <iostream>
#include <string>
int main() {
    std::string name;
    while (std::getline(std::cin, name))
        std::cout << absl::hash_internal::CityHash64WithSeed(name.data(), name.size(), 0) << '\\n';
}
"""
        subprocess.run(["c++", "-x", "c++", "-", "-o", helper, *flags], input=code, text=True, check=True)
        out = subprocess.check_output([helper], input="".join(name.upper() + "\n" for name in identifiers), text=True)
        hashes = list(map(int, out.splitlines()))
    else:
        hashes = [CityHash64WithSeed(name.upper().encode("ascii"), 0) for name in identifiers]
    return dict(zip(identifiers, hashes, strict=True))


def cache_member(stream):
    # The named Engine/GlobalShaderCache-SF_XSX_SM6.bin is the next PAK member after the global archive.
    offset, size, _ = ARCHIVES["global"]
    base = offset + size
    stream.seek(base)
    header = stream.read(52)
    _, packed, length, method = struct.unpack_from("<QQQI", header)
    blocks, = struct.unpack_from("<I", header, 48)
    if (packed, length, method, blocks) != (331464, 2025537, 1, 31):
        raise ValueError("Unexpected global shader map cache header")
    spans = list(struct.iter_unpack("<QQ", stream.read(16 * blocks)))
    data = bytearray()
    for start, end in spans:
        if not 0 < end - start <= packed:
            raise ValueError("Unexpected global shader map block range")
        stream.seek(base + start)
        data.extend(zlib.decompress(stream.read(end - start)))
    if len(data) != length or hashlib.sha256(data).hexdigest() != CACHE:
        raise ValueError("Unexpected global shader map cache fingerprint")
    return data


def compile_entry(ref, shader, tools, work):
    index = int(ref.split("/")[1])
    key, stage = shader
    if stage != 5:
        raise ValueError(f"Unexpected stage {stage} in {ref}")
    xshader, dxc, dxv = tools
    hlsl, profile, dxil = (work / f"{index}.{suffix}" for suffix in ("hlsl", "profile", "cso"))
    result = dict(entry=index, source_sha256=key)
    command = [xshader, work / (key + ".dxbc"), "--hlsl-out", hlsl, "--profile-out", profile]
    translated = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=60)
    if translated.returncode or not hlsl.is_file():
        result.update(hlsl_emitted=False, dxil_validated=False, diagnostic=translated.stdout.strip().splitlines()[-1])
        return result
    result.update(hlsl_emitted=True, hlsl_sha256=digest(hlsl))
    command = [dxc, "-T", profile.read_text().strip(), "-E", "main", "-Fo", dxil, hlsl]
    for args in (command, [dxv, dxil]):
        run = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True, timeout=60)
        if run.returncode:
            result.update(dxil_validated=False, diagnostic=run.stdout.strip())
            return result
    return dict(result, dxil_validated=True, dxil_sha256=digest(dxil))


def survey(mount, work):
    executable = mount / "Frosty/Binaries/XSX/Frosty-XSX-Shipping.exe"
    if digest(executable) != TITLE:
        raise ValueError("Unexpected retail executable")
    keys = name_keys(executable, work)
    maps = []
    with (mount / "Frosty/Content/Paks/Frosty-XSX.pak").open("rb") as stream:
        offset, size, expected = ARCHIVES["global"]
        stream.seek(offset)
        archive = stream.read(size)
        if hashlib.sha256(archive).hexdigest() != expected:
            raise ValueError("Unexpected global shader archive")
        map_ids, _, _, groups, _ = parse_tables(memoryview(archive))
        cache = cache_member(stream)
        bounds = []
        for index, (key,) in enumerate(struct.iter_unpack("20s", map_ids)):
            first = cache.find(key)
            if first < 0 or first != cache.rfind(key):
                raise ValueError(f"Shader map {index} lacks a unique record boundary")
            bounds.append((first + 20, index, key.hex()))
        if len(bounds) != struct.unpack_from("<I", cache)[0]:
            raise ValueError("Shader map count differs")
        start = 4
        for end, index, key in sorted(bounds):
            record = cache[start:end]
            start = end
            matches = {name: value for name, value in keys.items() if struct.pack("<Q", value) in record}
            if not matches:
                continue
            source, = (name for name in matches if name.startswith(TSR_DIR))
            shader_type, = (name for name in matches if name.startswith("FTSR"))
            entries = [i for i, membership in enumerate(groups) if index in membership]
            maps.append(dict(map=index, map_sha1=key, shader_type=shader_type, shader_file=source,
                             type_key=f"{matches[shader_type]:016x}", file_key=f"{matches[source]:016x}", entries=entries))
        indices = sorted({i for m in maps for i in m["entries"]})
        sources = extract_programs(stream, "global", indices, work, decoder())
    tools = compiler_tools()
    def check(ref):
        return compile_entry(ref, sources[ref], tools, work)
    with ThreadPoolExecutor(max_workers=2) as pool:
        programs = {p["entry"]: p for p in pool.map(check, sources)}
    for m in maps:
        m["programs"] = [programs[index] for index in m.pop("entries")]
    return dict(title_sha256=TITLE, global_archive_sha256=expected, global_shader_map_cache_sha256=CACHE,
                compiler_sha256=digest(tools[0]), dxc_sha256=digest(tools[1]), dxv_sha256=digest(tools[2]),
                scope="Packaged permutations. A map match or standalone validation does not establish a gameplay request.",
                shader_maps=len(maps), programs=len(programs), hlsl_emitted=sum(p["hlsl_emitted"] for p in programs.values()),
                dxil_validated=sum(p["dxil_validated"] for p in programs.values()), maps=sorted(maps, key=lambda m: m["map"]))



def trace_requests(trace, report):
    ids = {p["source_sha256"]: p["entry"] for m in report["maps"] for p in m["programs"]}
    patterns = (
        ("request", re.compile(rb"shader request compute ([0-9a-f]{64})")),
        ("compiled", re.compile(rb"Compiled Xbox shader ([0-9a-f]{64})")),
        ("compile_failed", re.compile(rb"Xbox shader ([0-9a-f]{64}): xshader failed")),
    )
    observations = {}
    with trace.open("rb") as log:
        for line in log:
            for event, pattern in patterns:
                match = pattern.search(line)
                if match and (entry := ids.get(match[1].decode("ascii"))) is not None:
                    observations.setdefault(entry, set()).add(event)
    return dict(sha256=digest(trace), programs=[dict(entry=i, events=sorted(events)) for i, events in sorted(observations.items())])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mount", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--trace", type=Path, help="Optional retail tuxbox.log to match against packaged programs")
    args = parser.parse_args()
    with TemporaryDirectory(prefix="tsr-map-") as tmp:
        report = survey(args.mount, Path(tmp))
    if args.trace:
        report["retail_trace"] = trace_requests(args.trace, report)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
