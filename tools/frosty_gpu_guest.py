"""Build and execute an app-local D3D12 control in a disposable, offline Proton prefix."""
from pathlib import Path
import shutil
import subprocess

from build import ROOT, Toolchain


def run_gpu_guest(source, name, trial, work, proton, prefix_source, provider, compiler_dir, *, vkd3d_dir=None):
    files = proton / "files"
    tools = Toolchain()
    obj = tools.compile(source, trial, "/W4", "/WX", f"/I{ROOT / 'runtime/d3d12'}")
    exe = trial / name
    tools.link(exe, obj, "d3d12.lib", "dxgi.lib", "kernel32.lib")
    prefix = trial / "prefix"
    subprocess.run(["cp", "-a", "--reflink=auto", prefix_source.resolve(), prefix], check=True)
    shutil.copyfile(provider, trial / "d3d12_xs.dll")
    for lib in ("dxcompiler.dll", "dxil.dll"):
        shutil.copyfile(compiler_dir / lib, trial / lib)
    wine_libs = files / "lib/wine"
    vkd3d_dir = vkd3d_dir or wine_libs / "vkd3d-proton/x86_64-windows"
    for lib in ("d3d12.dll", "d3d12core.dll"):
        shutil.copyfile(vkd3d_dir / lib, trial / lib)
    shutil.copyfile(wine_libs / "dxvk/x86_64-windows/dxgi.dll", trial / "dxgi.dll")
    for name in ("tmp", "runtime", "cache"):
        (trial / name).mkdir(mode=0o700)
    # Terminate the disposable Wine server before the caller removes its prefix.
    command = ["bwrap", "--die-with-parent", "--unshare-pid", "--unshare-net", "--ro-bind", "/", "/", "--dev-bind", "/dev",
               "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "--bind", str(trial), str(trial),
               "--unsetenv", "WAYLAND_DISPLAY", "--chdir", str(trial)]
    environment = dict(WINEPREFIX=prefix, WINESERVER=files / "bin/wineserver", WINEDEBUG="-all", VKD3D_DEBUG="warn",
                       WINEDLLOVERRIDES="d3d12,d3d12core,dxgi=n,b", XDG_CACHE_HOME=trial / "cache",
                       XDG_RUNTIME_DIR=trial / "runtime", TMPDIR=trial / "tmp", VKD3D_SHADER_CACHE_PATH=trial)
    for key, value in environment.items():
        command.extend(["--setenv", key, str(value)])
    command.extend(["--", "xvfb-run", "-a", "timeout", "-k", "2s", "45s", str(files / "bin/wine"), str(exe)])
    with (work / "gpu.log").open("w") as log:
        result = subprocess.run(command, stdout=log, stderr=subprocess.STDOUT, timeout=55)
    raw = trial / "result.txt"
    observed = dict(line.split("=", 1) for line in raw.read_text().splitlines()) if raw.exists() else {}
    return exe, result.returncode, observed
