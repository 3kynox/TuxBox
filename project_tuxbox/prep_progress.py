"""Display first-launch preparation in the terminal and optional desktop window."""
from contextlib import contextmanager
from html import escape
import os
import shutil
import subprocess
import sys
from time import monotonic


def report_progress(progress, message, completed=0, total=0):
    print(message, file=sys.stderr, flush=True)
    if progress:
        progress(message, completed, total)


class PrepPhase:
    """Report completed work, even on a warm cache, without logging each file."""

    def __init__(self, progress, label, total):
        self.progress = progress
        self.label = label
        self.total = total
        self.last_report = 0
        self.update(0, 0)

    def update(self, ready, compiled):
        now = monotonic()
        if ready not in (0, self.total) and now - self.last_report < 1:
            return
        self.last_report = now
        message = f"{self.label} · {ready}/{self.total} ready · {compiled} compiled · {ready - compiled} reused"
        report_progress(self.progress, message, ready, self.total)


@contextmanager
def desktop_progress(title, env):
    """Show progress within the current phase. Console output works independently."""
    zenity = None
    if env.get("TUXBOX_SHADER_PREP_UI") == "1" and (env.get("DISPLAY") or env.get("WAYLAND_DISPLAY")):
        zenity = shutil.which("zenity", path=env.get("PATH"))
    if not zenity:
        yield None
        return

    try:
        dialog = subprocess.Popen(
            [zenity, "--progress", "--no-cancel", "--ok-label=Hide", "--percentage=0", "--width=560",
             f"--title=Preparing {title}", "--text=Checking the shader cache…"],
            env=env, stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    except OSError:
        yield None
        return

    def update(message, completed=0, total=0):
        if dialog.poll() is None:
            percent = completed * 100 // total if total else 0
            label = escape(message).replace("\n", " ")
            try:
                os.write(dialog.stdin.fileno(), f"# {label}\n{percent}\n".encode())
            except OSError:
                # A busy or closed window must never hold up preparation.
                pass

    try:
        os.set_blocking(dialog.stdin.fileno(), False)
        yield update
    finally:
        dialog.stdin.close()
        if dialog.poll() is None:
            dialog.terminate()
            try:
                dialog.wait(timeout=0.5)
            except subprocess.TimeoutExpired:
                dialog.kill()
                dialog.wait()
