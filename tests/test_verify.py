import unittest
from unittest import mock

from project_tuxbox.verify import lanes_imports, screen_capture_problem


class ScreenCaptureTests(unittest.TestCase):
    def test_only_an_x_display_on_linux_is_probed(self):
        with mock.patch("project_tuxbox.verify.ctypes.CDLL") as load:
            with mock.patch("project_tuxbox.verify.sys.platform", "win32"):
                self.assertIsNone(screen_capture_problem(display=":0"))
            with mock.patch("project_tuxbox.verify.sys.platform", "linux"):
                self.assertIsNone(screen_capture_problem(display=""))
            load.assert_not_called()

    def test_a_missing_xlib_leaves_the_guest_check_to_report(self):
        with mock.patch("project_tuxbox.verify.sys.platform", "linux"):
            self.assertIsNone(screen_capture_problem(display=":0", library="/nonexistent/libX11.so"))


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
