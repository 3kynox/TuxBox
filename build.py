"""Build the Project TuxBox x64 guests and their app-local DLLs."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import sys


ROOT = Path(__file__).resolve().parent
OUTPUT = ROOT / "build"
TARGETS = ("route", "graphics", "xmem", "dxc-utils", "xg", "d3d12", "lanes")
# rustc --print native-static-libs for the Windows MSVC target with +crt-static.
RUST_STD_LIBS = ("kernel32.lib", "ntdll.lib", "userenv.lib", "ws2_32.lib", "dbghelp.lib",
                 "/defaultlib:libcmt")


def xwin_root():
    cache = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache"))
    return Path(os.environ.get("XWIN_SDK_ROOT", cache / "cargo-xwin/xwin")).expanduser().resolve()


def require_tool(name):
    executable = shutil.which(str(name))
    if executable is None:
        raise ValueError(f"Could not find executable {name!s}")
    return str(Path(executable).absolute())


class Toolchain:
    def __init__(self):
        self.includes = []
        self.libraries = []
        if os.name == "nt":
            if os.environ.get("VSCMD_ARG_TGT_ARCH", "x64").lower() != "x64":
                raise ValueError("Open an x64 Native Tools Command Prompt for Visual Studio")
            self.compiler = ["cl"]
            self.linker = require_tool("link")
        else:
            sdk = xwin_root()
            if not (sdk / "sdk/include/um/windows.h").is_file():
                raise ValueError(f"Set XWIN_SDK_ROOT to the xwin SDK/CRT tree. Looked in {sdk}")
            self.compiler = ["clang-cl", "--target=x86_64-pc-windows-msvc"]
            self.linker = require_tool("lld-link")
            for part in ("crt/include", "sdk/include/ucrt", "sdk/include/shared",
                         "sdk/include/um", "sdk/include/winrt"):
                self.includes.extend(["-imsvc", str(sdk / part)])
            self.libraries = [f"/libpath:{sdk / part}" for part in
                              ("crt/lib/x86_64", "sdk/lib/ucrt/x86_64", "sdk/lib/um/x86_64")]

    def compile(self, source, directory, *extra):
        language = ["/std:c11"] if source.suffix == ".c" else ["/std:c++17", "/EHsc"]
        obj = source.stem + ".obj"
        directory.mkdir(parents=True, exist_ok=True)
        subprocess.run([require_tool(self.compiler[0]), *self.compiler[1:], "/nologo", "/c", "/O2", "/MT", "/Brepro",
                        *language, *self.includes, *extra, f"/Fo{obj}", str(source)],
                       cwd=directory, check=True)
        return obj

    def rust_library(self, package, *, target_dir=None):
        target = "x86_64-pc-windows-msvc"
        directory = (target_dir or OUTPUT / "cargo").resolve()
        subprocess.run([require_tool("cargo"), "build", "--locked", "--release", "--target",
                        target, "--target-dir", str(directory), "--package", package],
                       cwd=ROOT, check=True)
        return directory / target / "release" / (package.replace("-", "_") + ".lib")

    def link(self, output, *inputs, exports=None, no_entry=False):
        flags = ["/subsystem:console"]
        if exports is not None:
            flags = ["/dll", f"/def:{exports}", f"/implib:{output.stem}.lib"]
            if no_entry:
                flags.extend(["/noentry", "/nodefaultlib"])
        subprocess.run([self.linker, "/nologo", "/machine:x64", "/Brepro", "/incremental:no",
                        *flags, f"/out:{output.name}", *self.libraries, *inputs],
                       cwd=output.parent, check=True)
        print(output, flush=True)


def find_dxc(override):
    if override or os.environ.get("DXC"):
        requested = Path(override or os.environ["DXC"]).expanduser()
        return require_tool(requested.resolve() if requested.is_file() else requested)
    installed = shutil.which("dxc")
    if installed:
        return installed
    local = OUTPUT / "d3d12-smoke/toolchain/bin" / ("dxc.exe" if os.name == "nt" else "dxc")
    if local.is_file():
        return require_tool(local)
    raise ValueError("Install DirectX Shader Compiler (dxc), set DXC, or pass --dxc PATH")


def build_target(tools, target, dxc):
    if target == "route":
        obj = tools.compile(ROOT / "fixtures/route_game.c", OUTPUT, "/W4", "/WX")
        tools.link(OUTPUT / "route-game.exe", obj, "kernel32.lib")
    elif target == "graphics":
        out = OUTPUT / "graphics"
        obj = tools.compile(ROOT / "graphics/desktop_clear.cpp", out)
        tools.link(out / "desktop-clear.exe", obj, "dxgi.lib", "d3d12.lib", "kernel32.lib")
    elif target == "xmem":
        src, out = ROOT / "runtime/xmem", OUTPUT / "xmem-smoke"
        out.mkdir(parents=True, exist_ok=True)
        dll = tools.rust_library("tuxbox-xmem")
        tools.link(out / "xmem.dll", dll, "kernel32.lib", exports=src / "xmem.def", no_entry=True)
        guest = tools.compile(src / "guest.c", out, "/W4")
        tools.link(out / "xmem-smoke.exe", guest, "xmem.lib", "kernel32.lib")
    elif target == "xg":
        src, out = ROOT / "runtime/xg", OUTPUT / "xg"
        guest = tools.compile(src / "guest.cpp", out, "/W4", "/WX")
        subprocess.run([tools.linker, "/lib", "/nologo", "/machine:x64",
                        f"/def:{src / 'xg_xs.def'}", "/out:xg_xs.lib"], cwd=out, check=True)
        tools.link(out / "xg-texture.exe", guest, "xg_xs.lib", "kernel32.lib")
    elif target == "dxc-utils":
        src, out = ROOT / "runtime/dxc", OUTPUT / "dxc-utils"
        obj = tools.compile(src / "module.c", out, "/W4", "/WX")
        tools.link(out / "dxcompiler_xs.dll", obj, exports=src / "dxcompiler_xs.def", no_entry=True)
        guest = tools.compile(src / "guest.cpp", out, "/W4", "/WX")
        tools.link(out / "dxc-utils.exe", guest, "dxcompiler_xs.lib", "kernel32.lib")
    elif target == "d3d12":
        src, out = ROOT / "runtime/d3d12", OUTPUT / "d3d12-smoke"
        out.mkdir(parents=True, exist_ok=True)
        provider = tools.rust_library("tuxbox-d3d12")
        tools.link(out / "d3d12_xs.dll", provider, "dxgi.lib", "d3d12.lib", "user32.lib",
                   *RUST_STD_LIBS, exports=src / "d3d12_xs.def")
        frame = tools.compile(src / "frame_guest.cpp", out, "/W4", "/WX")
        display = tools.compile(src / "frame_present.cpp", out, "/W4", "/WX")
        tools.link(out / "xbox-frame.exe", frame, display, "d3d12_xs.lib", "dxgi.lib",
                   "user32.lib", "gdi32.lib", "kernel32.lib")
        negative = tools.compile(src / "frame_negative.cpp", out, "/W4", "/WX")
        tools.link(out / "xbox-frame-negative.exe", negative, "d3d12_xs.lib", "dxgi.lib", "kernel32.lib")
        subprocess.run([dxc, "-T", "cs_6_0", "-E", "main", "-O3", "-Fh", "affine.h",
                        "-Vn", "affine_dxil", str(src / "affine.hlsl")], cwd=out, check=True)
        guest = tools.compile(src / "guest.cpp", out, f"/I{out}")
        # Both libraries export the serializer. Import it from the Xbox-facing DLL.
        tools.link(out / "d3d12-root-smoke.exe", guest, "d3d12_xs.lib", "d3d12.lib",
                   "dxgi.lib", "kernel32.lib")
    elif target == "lanes":
        src = ROOT / "examples/lanes"
        objects, package = OUTPUT / "lanes-obj", OUTPUT / "lanes"
        objects.mkdir(parents=True, exist_ok=True)
        package.mkdir(parents=True, exist_ok=True)
        subprocess.run([dxc, "-T", "cs_6_0", "-E", "main", "-O3", "-Fh", "course_shader.h",
                        "-Vn", "course_dxil", str(src / "course.hlsl")], cwd=objects, check=True)
        course = tools.compile(src / "course.cpp", objects, f"/I{objects}")
        game = tools.compile(src / "game.cpp", objects)
        xmem, d3d = OUTPUT / "xmem-smoke", OUTPUT / "d3d12-smoke"
        tools.link(objects / "lanes.exe", course, game, xmem / "xmem.lib", d3d / "d3d12_xs.lib",
                   "d3d12.lib", "dxgi.lib", "user32.lib", "gdi32.lib", "xinput.lib", "kernel32.lib")
        for source in (objects / "lanes.exe", xmem / "xmem.dll", d3d / "d3d12_xs.dll", src / "MicrosoftGame.config"):
            shutil.copyfile(source, package / source.name)
        print(package / "MicrosoftGame.config", flush=True)



def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("target", nargs="?", default="all", choices=("all", *TARGETS))
    parser.add_argument("--dxc", metavar="PATH", help="DXC executable to compile the D3D12 guest shader")
    args = parser.parse_args()
    try:
        # Preserve the standalone MinGW route build on hosts without an xwin SDK.
        if (args.target == "route" and os.name != "nt"
                and (not (xwin_root() / "sdk/include/um/windows.h").is_file()
                     or not shutil.which("clang-cl") or not shutil.which("lld-link"))
                and (mingw := shutil.which("x86_64-w64-mingw32-gcc"))):
            OUTPUT.mkdir(parents=True, exist_ok=True)
            subprocess.run([mingw, "-std=c11", "-O2", "-Wall", "-Wextra", "-Werror", "-static",
                            "-municode", "-Wl,--no-insert-timestamp", str(ROOT / "fixtures/route_game.c"),
                            "-o", "route-game.exe"], cwd=OUTPUT, check=True)
            print(OUTPUT / "route-game.exe")
            return 0
        targets = TARGETS if args.target == "all" else (args.target,)
        if args.target == "xg":
            targets = ("xmem", "xg")
        if args.target == "lanes":
            targets = ("xmem", "d3d12", "lanes")
        dxc = find_dxc(args.dxc) if "d3d12" in targets else None
        tools = Toolchain()
        for target in targets:
            build_target(tools, target, dxc)
    except (OSError, ValueError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"Build failed: {error}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
