"""Python wrapper for the `qxide` command-line tool bundled in this wheel, running `.qx` programs (a
language for classical and quantum computers).

Every function launches the `qxide` executable as a subprocess and returns its exit code and
output, with no native extension involved. This is the first step of the Python API (roadmap 4c)
and may change before 1.0.
"""

import os
import shutil
import subprocess
import sys
import sysconfig
import tempfile
from dataclasses import dataclass
from typing import Optional, Sequence

__all__ = ["Result", "binary", "check", "check_file", "run", "run_file", "test", "version"]


@dataclass(frozen=True)
class Result:
    """Exit code, standard output, and standard error from a single `qxide` command."""

    returncode: int
    stdout: str
    stderr: str

    @property
    def ok(self) -> bool:
        """Whether the command exited with status code 0."""
        return self.returncode == 0


def binary() -> str:
    """Return the path to the `qxide` executable from this Python environment's scripts directory,
    falling back to `PATH`. Raises `FileNotFoundError` if none is found.
    """
    name = "qxide.exe" if sys.platform == "win32" else "qxide"
    # The wheel installs the CLI as a script of the same environment as this package.
    for scheme_dir in (sysconfig.get_path("scripts"), os.path.dirname(sys.executable)):
        if scheme_dir:
            candidate = os.path.join(scheme_dir, name)
            if os.path.isfile(candidate):
                return candidate
    found = shutil.which("qxide")
    if found is None:
        raise FileNotFoundError(name)
    return found


def _call(args: Sequence[str], stdin: Optional[str] = None, cwd: Optional[str] = None) -> Result:
    p = subprocess.run(
        [binary(), *args],
        input=stdin,
        capture_output=True,
        text=True,
        encoding="utf-8",
        cwd=cwd,
    )
    return Result(p.returncode, p.stdout, p.stderr)


def _flags(shots: int, seed: Optional[int], engine: Optional[str], allow: Sequence[str]) -> list:
    out = ["--shots", str(shots)]
    if seed is not None:
        out += ["--seed", str(seed)]
    if engine is not None:
        out += ["--engine", engine]
    for a in allow:
        out += ["--allow", a]
    return out


def run_file(
    path: str,
    *,
    shots: int = 1,
    seed: Optional[int] = None,
    args: Sequence[str] = (),
    stdin: Optional[str] = None,
    engine: Optional[str] = None,
    allow: Sequence[str] = (),
    cwd: Optional[str] = None,
) -> Result:
    """Run a `.qx` file as `qxide run` with the specified shots, seed (random when `None`), program
    arguments after `--`, and optional standard input text.

    Supports selecting the execution engine (`ast`, `ir`, or `jit`), passing `--allow` grants (such
    as `fs`), and setting the working directory.
    """
    return _call(["run", path, *_flags(shots, seed, engine, allow), "--", *args], stdin, cwd)


def run(source: str, **options) -> Result:
    """Run `.qx` source text by writing it to a temporary `main.qx` file; accepts the same keyword
    options as `run_file`.
    """
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "main.qx")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(source)
        return run_file(path, **options)


def check_file(path: str, *, allow: Sequence[str] = ()) -> Result:
    """Check a `.qx` file as `qxide check`, returning diagnostics in `stderr`."""
    flags = []
    for a in allow:
        flags += ["--allow", a]
    return _call(["check", path, *flags])


def check(source: str, *, allow: Sequence[str] = ()) -> Result:
    """Check `.qx` source text as `qxide check` via a temporary `main.qx` file."""
    with tempfile.TemporaryDirectory() as d:
        path = os.path.join(d, "main.qx")
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(source)
        return check_file(path, allow=allow)


def test(target: Optional[str] = None, *, filter: Optional[str] = None, cwd: Optional[str] = None) -> Result:
    """Run `qxide test` on a file or directory (defaulting to the project's `tests/` directory when
    `None`), optionally running only tests whose name contains `filter`.
    """
    args = ["test"]
    if target is not None:
        args.append(target)
    if filter is not None:
        args += ["--filter", filter]
    return _call(args, cwd=cwd)


def version() -> str:
    """Return the version string of the installed `qxide` CLI (for example, `0.8.0`)."""
    out = _call(["--version"]).stdout.strip()
    return out.split(" ", 1)[-1]
