"""Optional desktop feedback while first-use shader preparation is running."""
from contextlib import contextmanager
from html import escape
import os
import shutil
import subprocess


@contextmanager
def desktop_progress(title, env):
    """Open a pulsing dialog only when the first compilation actually starts."""
    if env.get("TUXBOX_SHADER_PREP_UI") != "1" or not (env.get("DISPLAY") or env.get("WAYLAND_DISPLAY")):
        yield None
        return

    dialog = None
    attempted = False

    def update(message):
        nonlocal dialog, attempted
        label = escape(message)
        if not attempted:
            attempted = True
            zenity = shutil.which("zenity", path=env.get("PATH"))
            if zenity:
                try:
                    dialog = subprocess.Popen(
                        [zenity, "--progress", "--pulsate", "--no-cancel", "--width=460",
                         f"--title=Preparing {title} shaders", f"--text={label}"],
                        env=env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                    os.set_blocking(dialog.stdin.fileno(), False)
                except OSError:
                    return
        elif dialog and dialog.poll() is None:
            try:
                os.write(dialog.stdin.fileno(), f"# {label}\n".encode())
            except OSError:
                # A closed or busy dialog must never hold up the game.
                pass

    try:
        yield update
    finally:
        if dialog:
            dialog.stdin.close()
            if dialog.poll() is None:
                dialog.terminate()
                try:
                    dialog.wait(timeout=0.5)
                except subprocess.TimeoutExpired:
                    dialog.kill()
                    dialog.wait()
