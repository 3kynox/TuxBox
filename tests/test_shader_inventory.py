import csv
import json
from pathlib import Path
import struct
from tempfile import TemporaryDirectory
import unittest
from unittest.mock import patch
from xml.etree import ElementTree as ET

from project_tuxbox.progress_map import render_map, shader_table
from project_tuxbox.shader_inventory import ENTRY, MAP, TITLE, inventory, parse_tables, use_frontend_survey
from project_tuxbox.inspect import digest


def counted(records):
    return struct.pack("<I", len(records)) + b"".join(records)


class ShaderInventoryTests(unittest.TestCase):
    def test_archive_counts_memberships_distinct_sources_and_request_evidence(self):
        with TemporaryDirectory() as tmp:
            root = Path(tmp)
            title = root / "Frosty/Binaries/XSX/Frosty-XSX-Shipping.exe"
            title.parent.mkdir(parents=True)
            title.write_bytes(b"test game")
            pak = root / "Frosty/Content/Paks/Frosty-XSX.pak"
            pak.parent.mkdir(parents=True)
            blob = b"DXBC" + bytes(16) + struct.pack("<4I", 1, 44, 1, 36) + b"DXIL" + bytes(4)
            length = len(blob)
            slots = [ENTRY.pack(0, length, length, 0), ENTRY.pack(length, length, length, 0),
                     ENTRY.pack(2*length, 4, 4, 8)]
            pak.write_bytes(struct.pack("<I", 2) + counted([bytes(20)]) + counted([bytes([i])*20 for i in range(3)]) +
                            counted([MAP.pack(0, 2, 0, 0)]) + counted(slots) + counted([]) +
                            counted([struct.pack("<I", i) for i in (0, 2)]) + blob*2 + b"SPTR")
            raw = root / "shader.dxbc"
            raw.write_bytes(blob)
            source = digest(raw)
            log = root / "game.log"
            absent = "7" * 64
            log.write_text(f"TuxBox d3d12_xs: shader request vertex {source}\n" * 2 +
                           f"TuxBox d3d12_xs: prepared compute shader missing or invalid, source SHA-256 {absent}\n")
            plan = {"executable_sha256": digest(title), "shaders": [{"archive": "global", "entry": 0,
                    "dxbc_sha256": source, "profile": "vs_6_0"}]}
            index = root / "index.csv"
            compiler, dxc, dxv = (root / tool for tool in ("xshader", "dxc", "dxv"))
            for tool in (compiler, dxc, dxv):
                tool.write_text(tool.name)

            def run(command):
                if command[0] == compiler:
                    Path(command[command.index("--profile-out") + 1]).write_text("vs_6_0")
                return True

            with patch("project_tuxbox.shader_inventory.TITLE", digest(title)), \
                 patch.dict("project_tuxbox.shader_inventory.ARCHIVES",
                            {"global": (0, pak.stat().st_size, digest(pak)), "material": (0, pak.stat().st_size, digest(pak))},
                            clear=True), patch("project_tuxbox.shader_inventory.decoder"), \
                 patch("project_tuxbox.shader_inventory.survey_command", side_effect=run) as calls:
                result = inventory(root, plan, [], {"menu": log}, compiler, index=index, dxc=dxc)
            self.assertEqual([call.args[0][0] for call in calls.call_args_list], [compiler, dxc, dxv])
            self.assertEqual(result["desktop_tools"]["dxv_sha256"], digest(dxv))
            stages = {row["stage"]: row for row in result["stages"]}
            self.assertEqual(stages["Vertex"]["global"], 2)
            self.assertEqual(stages["Vertex"]["material"], 2)
            self.assertEqual(stages["Vertex"]["unique_dxbc"], 1)
            self.assertEqual(stages["Vertex"]["requested_in_profile"], 1)
            self.assertEqual(stages["Compute"]["requested_in_profile"], 0)
            self.assertEqual(stages["Ray hit group"]["unique_dxbc"], 0)
            self.assertEqual(result["traces"]["menu"]["unique_sources"], 2)
            self.assertEqual(result["traces"]["menu"]["trace"], "per-source")
            self.assertEqual(result["requested"][0]["archive_entries"], [])
            self.assertIsNone(result["requested"][0]["standalone_dxil_validated"])
            self.assertTrue(result["requested"][1]["standalone_dxil_validated"])
            with index.open() as stream:
                rows = list(csv.DictReader(stream))
            self.assertEqual(len(rows), 6)
            self.assertEqual(rows[0]["shader_maps"], "0")
            self.assertEqual(rows[1]["shader_maps"], "")
            self.assertEqual(rows[1]["standalone_dxil_validated"], "True")
            evidence = root / "evidence/matrix-awakens"
            evidence.mkdir(parents=True)
            (evidence / "shader-inventory.json").write_text(json.dumps(result))
            svg = ET.fromstring(render_map(evidence.parent))
            self.assertIn("Frosty shader coverage", [e.text for e in svg.findall(".//{*}text")])
            self.assertIn(absent, shader_table(evidence.parent))

    def test_archive_rejects_invalid_tables_and_shader_map_indices(self):
        with self.assertRaisesRegex(ValueError, "exceeds the member"):
            parse_tables(b"\x02\0\0\0\xff\xff\xff\xff")
        tables = [counted([bytes(20)]), counted([]), counted([MAP.pack(0, 1, 0, 0)]), counted([]),
                  counted([]), counted([struct.pack("<I", 99)])]
        with self.assertRaisesRegex(ValueError, "unknown entry"):
            parse_tables(struct.pack("<I", 2) + b"".join(tables))

    def test_survey_reuse_requires_matching_archive_and_stage_identity(self):
        report = {"archives": {"global": {"member_sha256": "a"}}, "executable_sha256": TITLE,
                  "stages": [{"stage": "Vertex", "unique_dxbc": 2}], "compiler_sha256": None}
        previous = dict(report, compiler_sha256="compiler", stages=[{"stage": "Vertex", "unique_dxbc": 2, "hlsl_emitted": 1}])
        use_frontend_survey(report, previous)
        self.assertEqual(report["stages"][0]["hlsl_emitted"], 1)
        self.assertEqual(report["compiler_sha256"], "compiler")
        with self.assertRaisesRegex(ValueError, "another build"):
            use_frontend_survey(report, dict(previous, archives={}))
        with self.assertRaisesRegex(ValueError, "source count"):
            use_frontend_survey(report, dict(previous, stages=[]))


if __name__ == "__main__":
    unittest.main()
