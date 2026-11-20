"""Tests for registry-template/scripts (validate.py, build_search.py) against throwaway registries.

Usage: python -I ci/test_registry_scripts.py
Standard library only; needs `git` for the pull-request (--base) checks. Assertions use exit codes
and the offending path or version, never message prose.
"""

import hashlib
import io
import json
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
SCRIPTS = REPO / "registry-template" / "scripts"
VALIDATE = SCRIPTS / "validate.py"
BUILD_SEARCH = SCRIPTS / "build_search.py"
CONFIG = '{"dl": "packages/{prefix}/{name}/{name}-{version}.tar.gz", "api": null}\n'
LIB = "public void hello(Qubit a) {\n    h(a);\n}\n"


def prefix(name: str) -> str:
    if len(name) <= 2:
        return str(len(name))
    if len(name) == 3:
        return f"3/{name[0]}"
    return f"{name[:2]}/{name[2:4]}"


def manifest(name: str, vers: str) -> str:
    return (
        f'[package]\nname = "{name}"\nversion = "{vers}"\ndescription = "d"\n'
        'license = "MIT"\nqxide = ">=0.6"\n'
    )


def make_tar(files: list[tuple[str, str]]) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", format=tarfile.GNU_FORMAT) as t:
        for name, text in files:
            data = text.encode()
            info = tarfile.TarInfo(name)
            info.size = len(data)
            t.addfile(info, io.BytesIO(data))
    return buf.getvalue()


def std_files(name: str, vers: str) -> list[tuple[str, str]]:
    root = f"{name}-{vers}"
    return [(f"{root}/qxide.toml", manifest(name, vers)), (f"{root}/src/lib.qx", LIB)]


class Registry:
    """A registry checkout in a temporary directory."""

    def __init__(self, test: unittest.TestCase):
        self.root = Path(tempfile.mkdtemp(prefix="qxide-registry-test-"))
        test.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        (self.root / "index").mkdir()
        (self.root / "config.json").write_text(CONFIG, encoding="utf-8", newline="\n")

    def add(self, name: str, vers: str, files=None, archive=True, **over) -> dict:
        """Appends one index line (and its archive unless `archive` is false)."""
        line = {"name": name, "vers": vers, "deps": [], "yanked": False, "v": 1, "qxide": ">=0.6", "description": "d"}
        if archive:
            data = make_tar(files if files is not None else std_files(name, vers))
            line["cksum"] = hashlib.sha256(data).hexdigest()
            path = self.root / "packages" / prefix(name) / name / f"{name}-{vers}.tar.gz"
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        else:
            line["cksum"] = "0" * 64
        line.update(over)
        self.append_raw(name, json.dumps(line, sort_keys=True))
        return line

    def append_raw(self, name: str, text: str) -> None:
        index = self.root / "index" / prefix(name) / name
        index.parent.mkdir(parents=True, exist_ok=True)
        with index.open("a", encoding="utf-8", newline="\n") as f:
            f.write(text + "\n")

    def validate(self, *args: str) -> subprocess.CompletedProcess:
        return subprocess.run(
            [sys.executable, "-I", str(VALIDATE), "--root", str(self.root), *args],
            capture_output=True,
            text=True,
            encoding="utf-8",
        )

    def git(self, *args: str) -> str:
        r = subprocess.run(
            ["git", "-C", str(self.root), "-c", "user.name=t", "-c", "user.email=t@t", "-c", "core.autocrlf=false", *args],
            capture_output=True,
            text=True,
        )
        if r.returncode:
            raise AssertionError(f"git {args}: {r.stderr}")
        return r.stdout


class ValidateArchives(unittest.TestCase):
    def assert_rejected(self, files, needle: str):
        reg = Registry(self)
        reg.add("evil", "1.0.0", files)
        r = reg.validate()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn(needle, r.stdout)

    def test_clean_package_passes(self):
        reg = Registry(self)
        reg.add("evil", "1.0.0")
        r = reg.validate()
        self.assertEqual(r.returncode, 0, r.stdout)

    def test_entries_that_collide_on_windows_or_macos(self):
        for second in [
            "src/lib.qx.",
            "src/lib.qx ",
            "src/LIB.qx",
            "src/lib.qx",
            "SRC/lib.qx",
            "src/lib.qx/x.qx",
            "src/nul.qx",
            "src/CON",
            "src/aux.tar.gz",
            "src/Com1.qx",
            "src/lpt9",
            "src/com¹.qx",
            "src/nul .qx",
            "src/a<b.qx",
            "src/a|b.qx",
            "src/a?b.qx",
            "src/a*b.qx",
            'src/a"b.qx',
            "src/a\x01b.qx",
            "src./x.qx",
        ]:
            with self.subTest(second=second):
                files = std_files("evil", "1.0.0") + [(f"evil-1.0.0/{second}", "x(a);\n")]
                self.assert_rejected(files, "evil 1.0.0")

    def test_lib_qx_must_be_exact(self):
        root = "evil-1.0.0"
        files = [(f"{root}/qxide.toml", manifest("evil", "1.0.0")), (f"{root}/src/lib.qx ", LIB)]
        self.assert_rejected(files, "evil 1.0.0")

    def test_portable_names_pass(self):
        reg = Registry(self)
        root = "evil-1.0.0"
        reg.add("evil", "1.0.0", std_files("evil", "1.0.0") + [(f"{root}/src/console.qx", LIB), (f"{root}/src/com10.qx", LIB)])
        r = reg.validate()
        self.assertEqual(r.returncode, 0, r.stdout)

    def test_file_limit_stops_early(self):
        root = "evil-1.0.0"
        files = std_files("evil", "1.0.0") + [(f"{root}/src/m{i}.qx", "") for i in range(2001)]
        self.assert_rejected(files, "evil 1.0.0")


if __name__ == "__main__":
    unittest.main(verbosity=2)
