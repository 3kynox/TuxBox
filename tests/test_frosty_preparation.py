import json
from pathlib import Path
import re
import sys
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch

from project_tuxbox.inspect import digest
from project_tuxbox.shader_prep import matching_tuxbox_provider

if sys.platform != "linux":
    raise unittest.SkipTest("Frosty's staging and launch tools require Linux")

from tools.prepare_frosty import BIN, ENGINE_INI, TITLE, verify_staged_files


class FrostyPreparationTests(unittest.TestCase):
    def setUp(self):
        scratch = TemporaryDirectory()
        self.addCleanup(scratch.cleanup)
        self.work = Path(scratch.name)
        stage = self.work / "Mount"
        self.binaries = stage / "Frosty/Binaries/XSX"
        self.binaries.mkdir(parents=True)
        (self.work / "stage").write_text(str(stage) + "\n")
        self.records = {}
        originals = ("Frosty-XSX-Shipping.exe", "GameChat2.dll", "MSVCP140.dll",
                     "VCRUNTIME140.dll", "VCRUNTIME140_1.dll")
        for name in (*originals, "d3d12_xs.dll"):
            path = self.binaries / name
            path.write_bytes(name.encode())
            self.records[str(path.relative_to(self.work))] = {
                "origin": "Original retail package" if name in originals else "TuxBox d3d12",
                "sha256": digest(path),
            }
        self.provider = self.binaries / "d3d12_xs.dll"
        self.manifest = self.work / "providers.json"
        self.manifest.write_text(json.dumps(self.records))

    def test_rejects_replacement_of_original_and_provider(self):
        verify_staged_files(self.work)
        for name in ("GameChat2.dll", "d3d12_xs.dll"):
            with self.subTest(name=name):
                path = self.binaries / name
                original = path.read_bytes()
                path.write_bytes(b"different DLL with the same name")
                with self.assertRaisesRegex(ValueError, re.escape(name)):
                    verify_staged_files(self.work)
                path.write_bytes(original)

    def test_rejects_preparation_record_missing_an_original(self):
        del self.records[str((self.binaries / "GameChat2.dll").relative_to(self.work))]
        self.manifest.write_text(json.dumps(self.records))
        with self.assertRaisesRegex(ValueError, "original retail binaries"):
            verify_staged_files(self.work)

    def test_preserves_existing_graphics_settings(self):
        staged = self.work / "Mount" / ENGINE_INI
        staged.parent.mkdir(parents=True, exist_ok=True)
        legacy = (Path(__file__).parent / "fixtures/frosty-legacy-user-engine.ini").read_bytes()
        for contents in (legacy, b"existing user settings"):
            staged.write_bytes(contents)
            verify_staged_files(self.work)
            self.assertEqual(staged.read_bytes(), contents)

    def test_staged_provider_survives_removal_of_shared_build(self):
        shared = self.work / "build/d3d12-smoke/d3d12_xs.dll"
        shared.parent.mkdir(parents=True)
        shared.write_bytes(self.provider.read_bytes())
        sha = digest(self.provider)
        with patch("project_tuxbox.shader_prep.TUXBOX_PROVIDER", shared):
            shared.unlink()
            self.assertTrue(matching_tuxbox_provider(self.provider, sha))

    def test_manifest_mismatch_takes_precedence_over_matching_shared_build(self):
        shared = self.work / "shared.dll"
        shared.write_bytes(self.provider.read_bytes())
        sha = digest(self.provider)
        relative = str(self.provider.relative_to(self.work))
        with patch("project_tuxbox.shader_prep.TUXBOX_PROVIDER", shared):
            for record in ({"origin": "TuxBox d3d12", "sha256": "0" * 64},
                           {"origin": "comparison provider", "sha256": sha}, None):
                with self.subTest(record=record):
                    self.manifest.write_text(json.dumps({relative: record} if record else {}))
                    self.assertFalse(matching_tuxbox_provider(self.provider, sha))
