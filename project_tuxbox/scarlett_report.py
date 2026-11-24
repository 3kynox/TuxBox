"""Bounded, aggregate-only preparation diagnostics.

The existing triage tool gives detailed emitter reasons, but needs a separate
emitter pass over the archive. These fixed buckets are safe to print during
normal preparation; no shader bytes, digests, raw errors or paths escape.
"""
import re
import subprocess

# Deliberately fixed labels, never interpolated compiler stderr.
REFUSALS = (
    ("wave_divergent", r"wave reduction at 0x[0-9a-f]+ may execute in divergent control flow"),
    ("wave_prologue", r"wave reduction at 0x[0-9a-f]+: unrecognized DPP reduction prologue"),
    ("wave_saveexec", r"wave reduction at 0x[0-9a-f]+: prologue does not start with s_nand_saveexec_b64 sX, 0"),
    ("exec_election", r"unverified EXEC election at 0x[0-9a-f]+: lane is not compared for equality"),
    ("compute_swizzle", r"unsupported compute DS swizzle encoding"),
    ("readlane_uninitialized", r"v_readlane at 0x[0-9a-f]+: v\d+ lane \d+ read before any v_writelane"),
    ("scalar_dynamic_offset", r"s_load at 0x[0-9a-f]+: dynamic soffset unsupported"),
    ("wave_other", r"wave reduction\b"),
    ("exec_other", r"unverified EXEC\b"),
    ("scalar_buffer", r"s_buffer_load\b"),
    ("sampler", r"(?:unsupported|unverified|unmapped)\b[^\n]*\bsampler\b"),
    ("vector_op", r"unsupported vector op\b"),
    ("scalar_op", r"unsupported scalar op\b"),
    ("mesh", r"unverified mesh shader bindings\b"),
    ("mubuf", r"MUBUF\b"),
)

def refusal_bucket(error):
    if error.cached:
        # The persistent marker holds only a key, not its original reason.
        return "cached_reason_unavailable"
    message = str(error)
    # ToolFailure prefixes the emitter stderr. Do not expose it in the report.
    for label, pattern in REFUSALS:
        if re.search(pattern, message, re.IGNORECASE):
            return label
    return "other_refusal"

def failure_bucket(error):
    if isinstance(error, subprocess.TimeoutExpired):
        # TimeoutExpired.cmd may contain private paths; use only fixed labels.
        cmd = error.cmd
        if isinstance(cmd, (tuple, list)) and cmd:
            from pathlib import Path
            tool = Path(cmd[0]).name
            if tool in ("dxc", "dxv", "xshader"):
                return tool + "_timeout"
        return "other_timeout"
    if isinstance(error, OSError):
        return "io_error"
    return "other_failure"

def aggregate(totals, refusal_counts, failure_counts, request_counts):
    """The receipt covers the exact enumerated preparation work, not gameplay."""
    categories = {key: totals[key] for key in ("compiled", "cached", "unsupported", "failed")}
    checked = sum(categories.values())
    if (sum(refusal_counts.values()) != categories["unsupported"]
            or sum(failure_counts.values()) != categories["failed"]
            or sum(request_counts.values()) != checked):
        raise ValueError("incomplete preparation receipt")
    return {
        "schema": 1, "checked": checked, "totals": categories,
        "request_types": dict(sorted(request_counts.items())),
        "refusal_buckets": dict(sorted(refusal_counts.items())),
        "failure_buckets": dict(sorted(failure_counts.items())),
    }
