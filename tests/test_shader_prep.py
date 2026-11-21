import hashlib
from pathlib import Path
import struct
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from project_tuxbox.shader_prep import cooked_shader, prepare_shaders


def record(key, program):
    return {"cache_key": key, "archive": "global", "entry": int(key), "profile": "cs_6_0",
            "observed_dxil_sha256": hashlib.sha256(program).hexdigest()}


@unittest.skipUnless(sys.platform == "linux", "This cache layout belongs to Proton on Linux")
class ShaderPreparation(unittest.TestCase):
    def test_missing_compiler_quarantines_broken_files_after_first_miss(self):
        with TemporaryDirectory() as tmp:
            cache = Path(tmp) / "by_hlsl"
            cache.mkdir()
            # Preserve an unfamiliar compiler's complete container. The two
            # other copies have a truncated TOC and a truncated DXIL chunk.
            bitcode = b"BC\xc0\xde"
            program = struct.pack("<II4sIII", 0x50060, 7, b"DXIL", 0x100, 16, len(bitcode)) + bitcode
            chunk = struct.pack("<4sI", b"DXIL", len(program)) + program
            good = b"DXBC" + bytes(16) + struct.pack("<IIII", 1, 36 + len(chunk), 1, 36) + chunk
            plan = {"title": "Fixture", "shaders": [record(str(i), b"other compiler") for i in range(4)]}
            (cache / "1.cso").write_bytes(good)
            bad = {"2": good[:35], "3": good[:-1]}
            for key, data in bad.items():
                (cache / (key + ".cso")).write_bytes(data)

            with patch("project_tuxbox.shader_prep.compiler_tools", side_effect=ValueError("missing compiler")):
                with self.assertRaisesRegex(ValueError, "missing compiler"):
                    prepare_shaders(None, plan, cache)
            self.assertEqual((cache / "1.cso").read_bytes(), good)
            for key, data in bad.items():
                self.assertFalse((cache / (key + ".cso")).exists())
                saved = cache.parent / "tuxbox-invalid" / f"{key}.{hashlib.sha256(data).hexdigest()}.cso"
                self.assertEqual(saved.read_bytes(), data)

    def test_existing_runtime_shader_and_corrupt_cache_recovery(self):
        with TemporaryDirectory() as tmp:
            cache = Path(tmp) / "by_hlsl"
            cache.mkdir()
            a, b = record("1", b"already compiled"), record("2", b"other compiler")
            (cache / "1.cso").write_bytes(b"already compiled")
            (cache / "2.cso").write_bytes(b"truncated")
            plan = {"title": "Fixture", "shaders": [a, b]}

            def compile_one(pak, shader, tools, work):
                self.assertEqual(shader, b)
                result = work / "shader.cso"
                result.write_bytes(b"new verified DXIL")
                return result

            with patch("project_tuxbox.shader_prep.compiler_tools"), \
                 patch("project_tuxbox.shader_prep.compile_shader", side_effect=compile_one) as compile_call:
                self.assertEqual(prepare_shaders(None, plan, cache), (1, 1))
                self.assertEqual(prepare_shaders(None, plan, cache), (0, 2))
                (cache / "2.cso").unlink()
                self.assertEqual(prepare_shaders(None, plan, cache), (1, 1))
                (cache / "2.cso").write_bytes(b"corrupt same size")
                self.assertEqual(prepare_shaders(None, plan, cache), (1, 1))
                # The old receipt belongs to a different provider.
                self.assertEqual(prepare_shaders(None, dict(plan, provider_sha256="new"), cache), (1, 1))
                self.assertEqual(compile_call.call_count, 4)

    def test_interrupted_preparation_keeps_completed_shader(self):
        with TemporaryDirectory() as tmp:
            cache = Path(tmp) / "by_hlsl"
            plan = {"title": "Fixture", "shaders": [record(str(i), b"other compiler") for i in (1, 2)]}
            calls = []
            interrupted = True

            def compile_one(pak, shader, tools, work):
                calls.append(shader["entry"])
                if shader["entry"] == 2 and interrupted:
                    raise ValueError("compiler failed")
                result = work / "shader.cso"
                result.write_bytes(b"new DXIL")
                return result

            with patch("project_tuxbox.shader_prep.compiler_tools"), \
                 patch("project_tuxbox.shader_prep.compile_shader", side_effect=compile_one):
                with self.assertRaisesRegex(ValueError, "compiler failed"):
                    prepare_shaders(None, plan, cache)
                interrupted = False
                self.assertEqual(prepare_shaders(None, plan, cache), (1, 1))
            self.assertEqual(calls, [1, 2, 2])

    def test_rejects_different_cooked_input(self):
        with TemporaryDirectory() as tmp:
            pak = Path(tmp) / "game.pak"
            pak.write_bytes(b"xxAABB")
            shader = dict(entry=5, archive="global", offset=2, packed_bytes=4, unpacked_bytes=4,
                          dxbc_offset=1, dxbc_bytes=2, dxbc_sha256=hashlib.sha256(b"AB").hexdigest())
            self.assertEqual(cooked_shader(pak, shader), b"AB")
            pak.write_bytes(b"xxCDEF")
            with self.assertRaisesRegex(ValueError, "Package shader differs"):
                cooked_shader(pak, shader)

    def test_simultaneous_preparations_share_published_output(self):
        from concurrent.futures import ThreadPoolExecutor
        from threading import Barrier

        with TemporaryDirectory() as tmp:
            cache = Path(tmp) / "by_hlsl"
            plan = {"title": "Fixture", "shaders": [record("1", b"other compiler")]}
            start = Barrier(2)

            def prepare_once():
                start.wait(timeout=5)
                return prepare_shaders(None, plan, cache)

            def compile_one(pak, shader, tools, work):
                result = work / "shader.cso"
                result.write_bytes(b"new DXIL")
                return result

            with patch("project_tuxbox.shader_prep.compiler_tools"), \
                 patch("project_tuxbox.shader_prep.compile_shader", side_effect=compile_one) as compile_call, \
                 ThreadPoolExecutor(max_workers=2) as pool:
                launches = [pool.submit(prepare_once) for _ in range(2)]
                self.assertCountEqual([job.result() for job in launches], [(0, 1), (1, 0)])
                compile_call.assert_called_once()


class ShaderLaunch(unittest.TestCase):
    def run_cli(self, flags, *, prepare=True, game_arguments=(), env=None):
        import os
        from types import SimpleNamespace
        import tuxbox

        with patch("sys.argv", ["tuxbox", "run", *flags, "game.exe", *game_arguments]), \
             patch.dict(os.environ, {"TUXBOX_SHADER_PREP": "1", **(env or {})}), \
             patch("tuxbox.launch_target", return_value=Path("game.exe")), \
             patch("tuxbox.Host") as host, patch("tuxbox.prepare_for_launch") as prepare_fn:
            host.return_value.run.return_value = SimpleNamespace(returncode=19)
            if isinstance(prepare, Exception):
                prepare_fn.side_effect = prepare
            else:
                prepare_fn.return_value = prepare
            status = tuxbox.main()
            return status, prepare_fn.call_count, host.return_value.run.call_count

    def test_prepare_only_never_starts_game(self):
        self.assertEqual(self.run_cli(["--prepare-only"]), (0, 1, 0))
        self.assertEqual(self.run_cli(["--prepare-only"], prepare=False), (1, 1, 0))
        with self.assertRaisesRegex(ValueError, "compiler failed"):
            self.run_cli(["--prepare-only"], prepare=ValueError("compiler failed"))

    def test_normal_launch_preserves_status_and_bypasses(self):
        self.assertEqual(self.run_cli([]), (19, 1, 1))
        self.assertEqual(self.run_cli([], prepare=ValueError("compiler failed")), (19, 1, 1))
        self.assertEqual(self.run_cli(["--no-shader-prep"]), (19, 0, 1))
        self.assertEqual(self.run_cli([], game_arguments=["-NullRHI"]), (19, 0, 1))
        self.assertEqual(self.run_cli([], env={"TUXBOX_SHADER_PREP": "0"}), (19, 0, 1))
