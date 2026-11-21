import unittest

from project_tuxbox.verify import lanes_imports


class LanesImportsTests(unittest.TestCase):
    def test_game_imports_and_shared_os_services(self):
        pe = {"imports": [{"module": "d3d12_xs.dll"}, {"module": "api-ms-win-core-synch-l1-2-0.dll"}],
              "delay_imports": [{"module": "GameInput.DLL"}]}
        self.assertEqual(lanes_imports(pe), ["api-ms-win-core-synch-l1-2-0.dll", "d3d12_xs.dll", "gameinput.dll"])

    def test_desktop_bypasses_fail_in_either_table(self):
        for table in ("imports", "delay_imports"):
            for desktop in ("d3d12.dll", "dxgi.dll", "GDI32.dll", "USER32.dll", "XInput1_4.dll"):
                with self.subTest(table=table, dll=desktop):
                    pe = {"imports": [], "delay_imports": []}
                    pe[table].append({"module": desktop})
                    with self.assertRaisesRegex(ValueError, "Xbox title profile"):
                        lanes_imports(pe)
