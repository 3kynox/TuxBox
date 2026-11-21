"""Launch through the host PE loader, or through a selected userspace Wine provider."""
import os
from pathlib import Path
import platform
import re
import shutil
import subprocess
import sys


def proton_installation(selection):
    if selection:
        path = Path(selection).expanduser().resolve()
        return path.parent if path.is_file() and path.name == "proton" else path
    steam = Path(os.environ.get("STEAM_COMPAT_CLIENT_INSTALL_PATH") or Path.home() / ".steam/root")
    libraries = [steam, Path.home() / ".local/share/Steam"]
    vdf = steam / "steamapps/libraryfolders.vdf"
    if vdf.is_file():
        libraries += [Path(p) for p in re.findall(r'"path"\s*"([^"]+)"', vdf.read_text(errors="replace"))]
    for library in libraries:
        candidate = library / "steamapps/common/Proton - Experimental"
        if (candidate / "proton").is_file():
            return candidate.resolve()
    local = steam / "compatibilitytools.d/Proton - Experimental"
    if (local / "proton").is_file():
        return local.resolve()
    raise ValueError("Select an installed Proton distribution with --proton or PROTONPATH")


class Host:
    def __init__(self, backend, proton, prefix):
        if platform.machine().lower() not in {"amd64", "x86_64"}:
            raise ValueError("This native CPU baseline requires an x86-64 host")
        self.backend = backend or ("native" if os.name == "nt" else "proton")
        self.env = os.environ.copy()
        self.command = []
        self.version = platform.platform()
        if self.backend == "native":
            if os.name != "nt" or proton or prefix:
                raise ValueError("Native execution requires Windows and takes no Proton or prefix arguments")
            return
        if sys.platform != "linux":
            raise ValueError("The Wine and Proton backends require Linux")
        if self.backend == "proton":
            installation = proton_installation(proton or os.getenv("PROTONPATH"))
            if not (installation / "proton").is_file():
                raise ValueError(f"Missing Proton distribution launcher in {installation}")
            tool = "umu-run"
            self.env.update(PROTONPATH=str(installation), GAMEID="umu-default")
            version = installation / "version"
            self.version = version.read_text().strip() if version.is_file() else installation.name
            prefix_name = installation.name
        else:
            if proton:
                raise ValueError("--proton selects a distribution for the proton backend")
            tool = "wine"
            prefix_name = "wine"
        executable = shutil.which(tool)
        if not executable:
            raise ValueError(f"Install {tool} to use the {self.backend} backend")
        if self.backend == "wine":
            self.version = subprocess.check_output([executable, "--version"], text=True).strip()
        data_home = Path(os.getenv("XDG_DATA_HOME") or Path.home() / ".local/share")
        prefix = (prefix or data_home / "project-tuxbox/prefixes" / prefix_name).expanduser().resolve()
        prefix.mkdir(parents=True, exist_ok=True)
        self.env["WINEPREFIX"] = str(prefix)
        self.command = [executable]

    def run(self, executable, arguments, *, timeout=None, capture=False):
        options = {"stdout": subprocess.PIPE, "stderr": subprocess.STDOUT} if capture else {}
        return subprocess.run([*self.command, str(executable), *arguments], cwd=executable.parent,
                              env=self.env, timeout=timeout, **options)
