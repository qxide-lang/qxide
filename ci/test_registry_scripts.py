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


def load_script(path: Path):
    import importlib.util

    sys.dont_write_bytecode = True  # keep registry-template/scripts free of __pycache__
    spec = importlib.util.spec_from_file_location(f"registry_{path.stem}", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_validate():
    return load_script(VALIDATE)


class ValidateEntries(unittest.TestCase):
    def assert_rejected(self, reg: Registry, needle: str):
        r = reg.validate()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn(needle, r.stdout)

    def test_semver_cases_match_the_client(self):
        # tests/data/semver_cases.json is checked against the client's parser by a Rust test.
        cases = json.loads((REPO / "tests" / "data" / "semver_cases.json").read_text(encoding="utf-8"))
        v = load_validate()
        for text, ok in cases["versions"]:
            self.assertEqual(v.registry_version(text), ok, repr(text))
        for text, ok in cases["reqs"]:
            self.assertEqual(v.valid_req(text), ok, repr(text))

    def test_fields_the_client_rejects(self):
        for vers, over in [
            ("1.0.99999999999999999999", {}),
            ("1.0.1-01", {}),
            ("1.0.0+evil", {}),
            ("1.0.0\n", {}),
            ("1.0.1", {"deps": [{"name": "other", "req": "not a req"}]}),
            ("1.0.1", {"deps": [{"name": "other\n", "req": "1"}]}),
            ("1.0.1", {"qxide": "bogus!"}),
            ("1.0.1", {"qxide": 99}),
            ("1.0.1", {"description": 5}),
            ("1.0.1", {"dl": 5}),
            ("1.0.1", {"v": True}),
            ("1.0.1", {"cksum": "0" * 64 + "\n"}),
        ]:
            with self.subTest(vers=vers, over=over):
                reg = Registry(self)
                reg.add("other", "1.0.0")
                reg.add("evil", "1.0.0")
                reg.add("evil", vers, archive=False, **over)
                self.assert_rejected(reg, "index/ev/il/evil:2")

    def test_download_locations(self):
        for dl in [
            "file:///C:/Windows/win.ini",
            "/etc/passwd",
            "packages/../../x.tar.gz",
            "http://example.invalid/x.tar.gz",
            "https://example.invalid/{name}[1-9].tar.gz",
            "https://example.invalid/a b.tar.gz",
            "packages\\x.tar.gz",
        ]:
            with self.subTest(dl=dl):
                reg = Registry(self)
                reg.add("evil", "1.0.0", archive=False, dl=dl)
                self.assert_rejected(reg, "evil 1.0.0")
        reg = Registry(self)
        reg.add("evil", "1.0.0", archive=False, dl="https://example.invalid/releases/{name}-{version}.tar.gz")
        r = reg.validate()
        self.assertEqual(r.returncode, 0, r.stdout)


class ValidatePullRequests(unittest.TestCase):
    def setUp(self):
        self.reg = Registry(self)
        self.reg.git("init", "-q", "-b", "main")
        self.reg.add("evil", "1.0.0")
        self.reg.git("add", "-A")
        self.reg.git("commit", "-qm", "base")
        self.reg.git("checkout", "-qb", "pr")

    def commit_and_validate(self) -> subprocess.CompletedProcess:
        self.reg.git("add", "-A")
        self.reg.git("commit", "-qm", "pr")
        return self.reg.validate("--base", "main")

    def test_a_new_version_passes(self):
        self.reg.add("evil", "1.0.1")
        r = self.commit_and_validate()
        self.assertEqual(r.returncode, 0, r.stdout)

    def test_build_metadata_twin_of_a_published_version(self):
        root = "evil-1.0.0+evil"
        self.reg.add("evil", "1.0.0+evil", [(f"{root}/qxide.toml", manifest("evil", "1.0.0+evil")), (f"{root}/src/lib.qx", LIB)])
        r = self.commit_and_validate()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("index/ev/il/evil:2", r.stdout)

    def test_config_json_is_locked(self):
        (self.reg.root / "config.json").write_text(
            '{"dl": "https://attacker.invalid/{name}-{version}.tar.gz", "api": null}\n', encoding="utf-8", newline="\n"
        )
        r = self.commit_and_validate()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("config.json", r.stdout)


class BuildSearch(unittest.TestCase):
    def newest(self, versions: list[str], yanked: tuple[str, ...] = ()) -> str:
        reg = Registry(self)
        for v in versions:
            reg.add("evil", v, archive=False, yanked=v in yanked)
        r = subprocess.run(
            [sys.executable, "-I", str(BUILD_SEARCH), "--root", str(reg.root)], capture_output=True, text=True
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        search = json.loads((reg.root / "search.json").read_text(encoding="utf-8"))
        return search["packages"][0]["version"]

    def test_semver_precedence(self):
        # Ordered by semver 2.0.0 precedence (its own example list, plus numeric identifiers).
        ordered = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-alpha.beta",
            "1.0.0-beta",
            "1.0.0-beta.2",
            "1.0.0-beta.11",
            "1.0.0-rc.1",
            "1.0.0-rc.2",
            "1.0.0-rc.10",
            "1.0.0",
            "1.0.1-0",
            "1.0.1-1",
            "1.0.1-1.0",
        ]
        key = load_script(BUILD_SEARCH).key
        for i in range(len(ordered)):
            for j in range(len(ordered)):
                with self.subTest(a=ordered[i], b=ordered[j]):
                    self.assertEqual(key(ordered[i]) < key(ordered[j]), i < j)
        self.assertEqual(self.newest(["1.0.0-rc.10", "1.0.0-rc.2"]), "1.0.0-rc.10")
        self.assertEqual(self.newest(["1.0.0-rc.2", "1.0.0-rc.10"]), "1.0.0-rc.10")

    def test_yanked_versions_are_skipped(self):
        self.assertEqual(self.newest(["1.0.0", "1.1.0-rc.10", "1.1.0-rc.2"], yanked=("1.1.0-rc.10",)), "1.1.0-rc.2")


if __name__ == "__main__":
    unittest.main(verbosity=2)
