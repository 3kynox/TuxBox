import hashlib
import os
from pathlib import Path
import struct
import subprocess
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from project_tuxbox.runtime_shaders import compile_request, serve


def portable_program(stage, body):
    bitcode = b"BC\xc0\xde" + body
    header = struct.pack("<II4sIII", stage << 16 | 0x60, (24 + len(bitcode)) // 4,
                         b"DXIL", 0x100, 16, len(bitcode))
    chunk = struct.pack("<4sI", b"DXIL", len(header) + len(bitcode)) + header + bitcode
    return b"DXBC" + bytes(16) + struct.pack("<IIII", 1, 36 + len(chunk), 1, 36) + chunk


class RuntimeShaderLinks(unittest.TestCase):
    def setUp(self):
        temporary = TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        directory = Path(temporary.name)
        self.session = directory / "run"
        self.requests = self.session / "requests"
        self.requests.mkdir(parents=True)
        self.cache = directory / "cache"
        self.cache.mkdir()
        self.tools = ("xshader", "dxc", "dxv")

    def request(self, pixel_order, pixel=b"Xbox pixel shader"):
        inputs = {"producer.dxbc": b"Xbox geometry shader", "pixel.dxbc": pixel, "pixel.cso": pixel_order}
        key = "_".join(hashlib.sha256(data).hexdigest() for data in inputs.values())
        source = self.requests / (key + ".link")
        source.mkdir()
        for name, data in inputs.items():
            (source / name).write_bytes(data)
        return key, source

    def test_exact_consumer_order_selects_variant_and_warm_hit_rechecks_inputs(self):
        def run(args, *, timeout):
            if args[0] == "xshader":
                self.assertEqual(args[1].read_bytes(), b"Xbox geometry shader")
                self.assertEqual(args[args.index("--link-pixel") + 1].read_bytes(), b"Xbox pixel shader")
                order = args[args.index("--pixel-order") + 1].read_bytes()
                args[args.index("--hlsl-out") + 1].write_bytes(order[-4:])
                args[args.index("--profile-out") + 1].write_text("gs_6_0")
            elif args[0] == "dxc":
                self.assertEqual(args[args.index("-T") + 1], "gs_6_0")
                args[args.index("-Fo") + 1].write_bytes(portable_program(2, args[-1].read_bytes()))
            else:
                self.assertEqual(args[0], "dxv")

        for signature in [b"r0.z", b"r2.x"]:
            key, source = self.request(portable_program(0, signature))
            dest = self.session / (key + ".cso")
            with patch("project_tuxbox.runtime_shaders.run_tool", side_effect=run) as pipeline:
                self.assertTrue(compile_request(source, key, self.tools, self.cache, self.session))
                self.assertEqual(dest.read_bytes(), portable_program(2, signature))
                self.assertEqual(pipeline.call_count, 4)
                dest.unlink()
                self.assertFalse(compile_request(source, key, self.tools, self.cache, self.session))
                self.assertEqual(dest.read_bytes(), portable_program(2, signature))
                for name in ["producer.dxbc", "pixel.dxbc", "pixel.cso"]:
                    path = source / name
                    old = path.read_bytes()
                    path.write_bytes(old + b"changed")
                    with self.assertRaisesRegex(ValueError, "digest"):
                        compile_request(source, key, self.tools, self.cache, self.session)
                    path.write_bytes(old)
                self.assertEqual(pipeline.call_count, 4)

    def test_rejects_native_code_non_pixel_dxil_and_non_regular_input(self):
        for program in [b"Xbox-native shader", portable_program(1, b"r0.z")]:
            key, source = self.request(program)
            with self.assertRaisesRegex(ValueError, "portable pixel"):
                compile_request(source, key, self.tools, self.cache, self.session)
        key, source = self.request(portable_program(0, b"r0.z"))
        path = source / "pixel.cso"
        with path.open("r+b") as stream:
            stream.truncate((64 << 20) + 1)
        with self.assertRaisesRegex(ValueError, "64 MiB"):
            compile_request(source, key, self.tools, self.cache, self.session)
        path.unlink()
        os.mkfifo(path)
        with self.assertRaisesRegex(ValueError, "regular"):
            compile_request(source, key, self.tools, self.cache, self.session)
        path.unlink()
        path.symlink_to(source / "producer.dxbc")
        with self.assertRaises(OSError):
            compile_request(source, key, self.tools, self.cache, self.session)

    def test_worker_removes_failed_link_and_leaves_unpublished_directories_alone(self):
        from threading import Event
        key, completed = self.request(portable_program(0, b"r0.z"))
        pending = self.requests / (key + ".tmp")
        pending.mkdir()
        stop = Event()
        with patch("project_tuxbox.runtime_shaders.compiler_tools", side_effect=ValueError("compiler unavailable")):
            serve(self.session, stop, lambda line: stop.set())
        self.assertTrue((self.session / (key + ".failed")).is_file())
        self.assertFalse(completed.exists())
        self.assertTrue(pending.is_dir())

    def test_running_worker_keeps_compiler_after_release_replacement(self):
        from threading import Event
        release = self.cache / "xshader"
        replacement = self.cache / "new"
        dxc = self.cache / "dxc"
        for binary, text in [(release, "original"), (replacement, "rebuilt"), (dxc, "toolchain")]:
            binary.write_text(f"#!/bin/sh\necho {text}\n")
            binary.chmod(0o700)
        stop = Event()
        calls = []

        def request(data):
            key = hashlib.sha256(data).hexdigest()
            (self.requests / (key + ".dxbc")).write_bytes(data)

        def compile_one(source, key, tools, cache, session):
            calls.append((subprocess.check_output([tools[0]]).strip(), cache.name))
            if len(calls) == 1:
                replacement.replace(release)
                request(b"second")
            else:
                stop.set()
            return True

        with patch("project_tuxbox.runtime_shaders.compiler_tools", return_value=(release, dxc, dxc)), \
                patch("project_tuxbox.runtime_shaders.compile_request", side_effect=compile_one):
            request(b"first")
            serve(self.session, stop, lambda line: None)
            stop.clear()
            request(b"third")
            serve(self.session, stop, lambda line: None)
        self.assertEqual([entry[0] for entry in calls], [b"original", b"original", b"rebuilt"])
        self.assertEqual(calls[0][1], calls[1][1])
        self.assertNotEqual(calls[1][1], calls[2][1])
