"""Tests for `registry-template/scripts` (`validate.py`, `build_search.py`) run against throwaway
registries in temporary directories.

Usage: python -I ci/test_registry_scripts.py

Standard library only; requires `git` for the pull-request (`--base`) checks. Assertions check exit
codes and the offending path or version, never message prose.
"""

import gzip
import hashlib
import io
import json
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TEMPLATE = REPO / "registry-template"
SCRIPTS = TEMPLATE / "scripts"
VALIDATE = SCRIPTS / "validate.py"
BUILD_SEARCH = SCRIPTS / "build_search.py"
# A registry written by `qxide publish` (0.7.0): GNU long-name entry, requirements such as `1` in
# qxide.toml that the index line stores as `^1`.
PUBLISHED = REPO / "ci" / "data" / "published-registry"
CONFIG = '{"dl": "packages/{prefix}/{name}/{name}-{version}.tar.gz", "api": null}\n'
LIB = "public void hello(Qubit a) {\n    h(a);\n}\n"
# The client's limit on the decompressed tar stream (src/pkg/archive.rs `MAX_TAR`).
MAX_TAR = (50 << 20) + (16 << 20)


def prefix(name: str) -> str:
    if len(name) <= 2:
        return str(len(name))
    if len(name) == 3:
        return f"3/{name[0]}"
    return f"{name[:2]}/{name[2:4]}"


def manifest(name: str, vers: str, extra: str = "") -> str:
    return (
        f'[package]\nname = "{name}"\nversion = "{vers}"\ndescription = "d"\n'
        f'license = "MIT"\nqxide = ">=0.6"\n{extra}'
    )


# Raw tar building, for archives Python's tarfile will not write.
GNU_MAGIC = b"ustar  \x00"
USTAR_MAGIC = b"ustar\x0000"
END = b"\0" * 1024


def tar_header(name: bytes, size: int, kind: bytes = b"0", *, magic=GNU_MAGIC, prefix_field=b"", size_field=None) -> bytes:
    h = bytearray(512)
    h[0 : len(name)] = name
    h[100:108] = b"0000644\x00"
    h[108:116] = b"0000000\x00"
    h[116:124] = b"0000000\x00"
    h[124:136] = size_field if size_field is not None else b"%011o\x00" % size
    h[136:148] = b"00000000000\x00"
    h[156:157] = kind
    h[257:265] = magic
    h[345 : 345 + len(prefix_field)] = prefix_field
    h[148:156] = b" " * 8
    h[148:156] = b"%06o\x00 " % sum(h)
    return bytes(h)


def tar_member(name: bytes, data: bytes, kind: bytes = b"0", **kw) -> bytes:
    return tar_header(name, len(data), kind, **kw) + data + b"\0" * (-len(data) % 512)


def pax_record(key: str, value: str) -> bytes:
    body = f" {key}={value}\n"
    n = len(body) + 1
    while len(str(n)) + len(body) != n:
        n += 1
    return f"{n}{body}".encode()


def std_members(name: str, vers: str) -> bytes:
    root = f"{name}-{vers}".encode()
    return tar_member(root + b"/qxide.toml", manifest(name, vers).encode()) + tar_member(root + b"/src/lib.qx", LIB.encode())


def gz(data: bytes) -> bytes:
    return gzip.compress(data, compresslevel=6, mtime=0)


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
    """Registry checkout in a temporary directory, removed after the test."""

    def __init__(self, test: unittest.TestCase):
        self.root = Path(tempfile.mkdtemp(prefix="qxide-registry-test-"))
        test.addCleanup(shutil.rmtree, self.root, ignore_errors=True)
        (self.root / "index").mkdir()
        (self.root / "config.json").write_text(CONFIG, encoding="utf-8", newline="\n")

    def add(self, name: str, vers: str, files=None, archive=True, raw=None, **over) -> dict:
        """Appends one index line for the version and writes its archive unless `archive` is false;
        `raw` gives the archive bytes instead of `files`; `over` overrides fields on the line.
        """
        line = {"name": name, "vers": vers, "deps": [], "yanked": False, "v": 1, "qxide": ">=0.6", "description": "d"}
        if archive:
            data = raw if raw is not None else make_tar(files if files is not None else std_files(name, vers))
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

    def test_archives_written_by_qxide_publish_pass(self):
        reg = Registry(self)
        shutil.rmtree(reg.root)
        shutil.copytree(PUBLISHED, reg.root, ignore=shutil.ignore_patterns(".gitattributes"))
        r = reg.validate()
        self.assertEqual(r.returncode, 0, r.stdout)
        self.assertIn("4 packages, 4 versions, 0 errors", r.stdout)

    def assert_raw(self, raw: bytes, ok: bool):
        reg = Registry(self)
        reg.add("evil", "1.0.0", raw=raw)
        r = reg.validate()
        if ok:
            self.assertEqual(r.returncode, 0, r.stdout)
        else:
            self.assertEqual(r.returncode, 1, r.stdout)
            self.assertIn("evil 1.0.0", r.stdout)

    def test_archives_must_use_the_canonical_form(self):
        # M-09: anything CI accepts must read the same in the client, which takes one gzip member,
        # raw tar headers, and only regular files, directories and short GNU long names.
        std = std_members("evil", "1.0.0")
        root = b"evil-1.0.0"
        extra = tar_member(root + b"/src/second.qx", b"x(a);\n")
        long4097 = root + b"/src/" + b"a" * (4097 - len(root) - 8) + b".qx"
        pax_size = pax_record("size", "100")
        cases = {
            # The report's input: the client stops after the first gzip member.
            "two gzip members": gz(std) + gz(extra + END),
            "bytes after the gzip member": gz(std + END) + b"trailing",
            "PAX header with a size record": gz(std + tar_member(b"././@PaxHeader", pax_size, b"x") + tar_member(root + b"/src/p.qx", b"x" * 10) + END),
            "PAX global header": gz(tar_member(b"pax_global_header", pax_record("comment", "c"), b"g") + std + END),
            "GNU long link": gz(std + tar_member(b"././@LongLink", b"target\0", b"K") + extra + END),
            "contiguous file": gz(std + tar_member(root + b"/src/c.qx", b"x(a);\n", b"7") + END),
            "long name over 4096 bytes": gz(std + tar_member(b"././@LongLink", long4097, b"L") + tar_member(long4097[:99], b"x") + END),
            "two long names in a row": gz(std + tar_member(b"././@LongLink", root + b"/src/a.qx\0", b"L") + tar_member(b"././@LongLink", root + b"/src/b.qx\0", b"L") + tar_member(b"x", b"x") + END),
            "long name with no entry": gz(std + tar_member(b"././@LongLink", root + b"/src/a.qx\0", b"L") + END),
            "data after the end marker": gz(std + END + extra),
            "decompressed stream over the client limit": gz(std + tar_member(root + b"/src/", b"\0" * (MAX_TAR + 1), b"5") + END),
            "leading ./": gz(std + tar_member(b"./" + root + b"/src/x.qx", b"x(a);\n") + END),
            "empty segment": gz(std + tar_member(root + b"/src//x.qx", b"x(a);\n") + END),
            "base-256 size": gz(std + tar_header(root + b"/src/x.qx", 6, size_field=b"\x80" + bytes(3) + (6).to_bytes(8, "big")) + b"x(a);\n" + b"\0" * 506 + END),
            "truncated data": gz(std + tar_header(root + b"/src/x.qx", 100) + b"x" * 10),
        }
        for label, raw in cases.items():
            with self.subTest(label):
                self.assert_raw(raw, False)

    def test_canonical_forms_the_client_reads_pass(self):
        std = std_members("evil", "1.0.0")
        root = b"evil-1.0.0"
        long4096 = root + b"/src/" + b"a" * (4096 - len(root) - 8) + b".qx"
        self.assertEqual(len(long4096), 4096)
        for label, raw in {
            "plain": gz(std + END),
            "no end marker": gz(std),
            "long name of 4096 bytes": gz(std + tar_member(b"././@LongLink", long4096, b"L") + tar_member(long4096[:99], b"x") + END),
            "ustar prefix field": gz(std + tar_member(b"x.qx", b"x(a);\n", magic=USTAR_MAGIC, prefix_field=root + b"/src") + END),
            "directory entries": gz(tar_member(root + b"/", b"", b"5") + tar_member(root + b"/src/", b"", b"5") + std + END),
        }.items():
            with self.subTest(label):
                self.assert_raw(raw, True)

    def test_manifest_dependencies_match_the_index(self):
        # M-08: the client checks lock freshness against qxide.toml, resolution against the index.
        dep = [{"name": "other", "req": "^1"}]
        for label, deps, toml, ok in [
            ("the report's input", [], '\n[dependencies]\nother = "1"\n', False),
            ("missing in qxide.toml", dep, "", False),
            ("other requirement", dep, '\n[dependencies]\nother = "2"\n', False),
            ("other name", dep, '\n[dependencies]\nanother = "1"\n', False),
            ("path only", dep, '\n[dependencies]\nother = { path = "../other" }\n', False),
            ("non-string version", dep, "\n[dependencies]\nother = { version = 1 }\n", False),
            ("bad requirement", dep, '\n[dependencies]\nother = "one"\n', False),
            ("not a table", dep, '\ndependencies = "other"\n', False),
            ("listed twice in the index", dep + dep, '\n[dependencies]\nother = "1"\n', False),
            ("same requirement", dep, '\n[dependencies]\nother = "1"\n', True),
            ("table form with a path", dep, '\n[dependencies]\nother = { version = "^1", path = "../other" }\n', True),
            ("spacing", [{"name": "other", "req": ">=1.2, <2"}], '\n[dependencies]\nother = ">= 1.2,<2"\n', True),
        ]:
            with self.subTest(label):
                reg = Registry(self)
                reg.add("other", "1.2.0")
                files = [("evil-1.0.0/qxide.toml", manifest("evil", "1.0.0", toml)), ("evil-1.0.0/src/lib.qx", LIB)]
                reg.add("evil", "1.0.0", files, deps=deps)
                r = reg.validate()
                if ok:
                    self.assertEqual(r.returncode, 0, r.stdout)
                else:
                    self.assertEqual(r.returncode, 1, r.stdout)
                    self.assertIn("evil 1.0.0", r.stdout)


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

    def test_json_the_client_rejects(self):
        # src/pkg/tests.rs `index_json_is_strict` checks that the client rejects the same lines.
        deep = "[" * 130 + "]" * 130
        # Spots the other checks accept: a description, and extra keys inside a dependency.
        for old, new in [
            ('"description": "d"', '"description": "\\ud800"'),
            ('"req": "1"}', '"req": "1", "x": NaN}'),
            ('"req": "1"}', f'"req": "1", "x": {deep}}}'),
        ]:
            with self.subTest(new=new[:40]):
                reg = Registry(self)
                reg.add("other", "1.0.0")
                reg.add("evil", "1.0.0")
                reg.add("evil", "1.0.1", deps=[{"name": "other", "req": "1"}])
                index = reg.root / "index" / "ev" / "il" / "evil"
                text = index.read_bytes().decode("utf-8")
                lines = text.split("\n")
                self.assertIn(old, lines[1])
                lines[1] = lines[1].replace(old, new)
                index.write_bytes("\n".join(lines).encode("utf-8"))
                self.assert_rejected(reg, "index/ev/il/evil:2")

    def test_requirement_equality_matches_the_client(self):
        # tests/data/semver_cases.json `req_pairs` is checked against `semver::VersionReq` equality.
        cases = json.loads((REPO / "tests" / "data" / "semver_cases.json").read_text(encoding="utf-8"))
        v = load_validate()
        for a, b, same in cases["req_pairs"]:
            with self.subTest(a=a, b=b):
                self.assertEqual(v.same_req(a, b), same)
                self.assertEqual(v.same_req(b, a), same)

    def test_description_control_characters(self):
        # B-P4 / M-04: `qxide search` prints descriptions from search.json, built from index lines.
        for desc in [
            "plain \x1b]0;TITLE-SET-BY-REGISTRY\x07\x1b[2K",  # the report's input: ESC and BEL
            "a\x7fb",
            "a\x9b2Jb",
            "two\nlines",
            "carriage\rreturn",
            "tab\there",
            "nul\x00",
        ]:
            with self.subTest(desc=repr(desc)):
                reg = Registry(self)
                reg.add("evil", "1.0.0")
                reg.add("evil", "1.0.1", description=desc)
                self.assert_rejected(reg, "index/ev/il/evil:2")
        reg = Registry(self)
        reg.add("evil", "1.0.0", description="Bell-pair helpers — für Qubits, 量子 ‍")
        r = reg.validate()
        self.assertEqual(r.returncode, 0, r.stdout)

    def test_external_download_locations_are_checked(self):
        # B-P3: an https `dl` still needs the archive in this repository, at the default path, so
        # CI checks the exact bytes clients accept (they verify `cksum`).
        url = "https://example.invalid/releases/{name}-{version}.tar.gz"
        reg = Registry(self)
        reg.add("evil", "1.0.0", archive=False, dl=url)
        self.assert_rejected(reg, "evil 1.0.0")
        reg = Registry(self)
        reg.add("evil", "1.0.0", dl=url)
        r = reg.validate()
        self.assertEqual(r.returncode, 0, r.stdout)
        reg = Registry(self)
        reg.add("evil", "1.0.0", [("evil-1.0.0/qxide.toml", manifest("evil", "1.0.0"))], dl=url)
        self.assert_rejected(reg, "evil 1.0.0")
        # The same for an https template in config.json.
        for archive, ok in [(False, False), (True, True)]:
            with self.subTest(template_archive=archive):
                reg = Registry(self)
                (reg.root / "config.json").write_text(json.dumps({"dl": url, "api": None}), encoding="utf-8")
                reg.add("evil", "1.0.0", archive=archive)
                r = reg.validate()
                self.assertEqual(r.returncode, 0 if ok else 1, r.stdout)

    def test_only_newline_ends_a_line(self):
        for sep in ["\x0c", "\r", "\x1c", " "]:
            with self.subTest(sep=repr(sep)):
                reg = Registry(self)
                reg.add("evil", "1.0.0")
                reg.add("evil", "1.0.1")
                index = reg.root / "index" / "ev" / "il" / "evil"
                text = index.read_bytes().decode("utf-8").rstrip("\n").replace("\n", sep) + "\n"
                index.write_bytes(text.encode("utf-8"))
                self.assert_rejected(reg, "index/ev/il/evil:1")

    def test_self_dependency(self):
        reg = Registry(self)
        reg.add("evil", "1.0.0", deps=[{"name": "evil", "req": "^1"}])
        self.assert_rejected(reg, "evil 1.0.0")

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
        # An https `dl` passes only with the archive in this repository (B-P3).
        reg.add("evil", "1.0.0", dl="https://example.invalid/releases/{name}-{version}.tar.gz")
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

    def test_deleting_an_index_file(self):
        # B-P2: the report's input deletes index/ev/il/evil in a pull request.
        self.reg.git("rm", "-q", "index/ev/il/evil")
        r = self.commit_and_validate()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("index/ev/il/evil", r.stdout)

    def test_readding_a_deleted_version_with_another_archive(self):
        # B-P2 follow-up: once a deletion is merged, the version comes back with another cksum
        # and an external `dl`; the archive published first is still in packages/.
        self.reg.git("checkout", "-q", "main")
        self.reg.git("rm", "-q", "index/ev/il/evil")
        self.reg.git("commit", "-qm", "delete")
        self.reg.git("checkout", "-qb", "pr2")
        self.reg.add("evil", "1.0.0", archive=False, cksum="1" * 64, dl="https://example.invalid/evil-1.0.0.tar.gz")
        r = self.commit_and_validate()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("evil 1.0.0", r.stdout)

    def test_unknown_base_fails(self):
        self.reg.add("evil", "1.0.1")
        self.reg.git("add", "-A")
        self.reg.git("commit", "-qm", "pr")
        r = self.reg.validate("--base", "no-such-ref")
        self.assertNotEqual(r.returncode, 0, r.stdout)

    def test_config_json_is_locked(self):
        (self.reg.root / "config.json").write_text(
            '{"dl": "https://attacker.invalid/{name}-{version}.tar.gz", "api": null}\n', encoding="utf-8", newline="\n"
        )
        r = self.commit_and_validate()
        self.assertEqual(r.returncode, 1, r.stdout)
        self.assertIn("config.json", r.stdout)


class ValidateWorkflow(unittest.TestCase):
    """B-P1: a `pull_request` run uses the workflow file from the pull request itself, so a pull
    request could edit or skip its own check. The standard library has no YAML parser, so these
    tests read the workflow as text; the workflow itself only runs on GitHub.
    """

    def setUp(self):
        text = (TEMPLATE / ".github" / "workflows" / "validate.yml").read_text(encoding="utf-8")
        self.lines = [l.split(" #", 1)[0].rstrip() for l in text.splitlines() if not l.lstrip().startswith("#")]
        self.code = "\n".join(self.lines)

    def block(self, key: str) -> list[str]:
        """Keys one level below the top-level `key:`."""
        i = self.lines.index(f"{key}:")
        keys = []
        for line in self.lines[i + 1 :]:
            if line and not line.startswith(" "):
                break
            if re.fullmatch(r"  [A-Za-z_-]+:.*", line):
                keys.append(line.strip().split(":", 1)[0])
        return keys

    def test_pull_requests_run_the_target_branch_workflow(self):
        triggers = self.block("on")
        self.assertIn("pull_request_target", triggers)
        self.assertNotIn("pull_request", triggers)

    def test_checker_comes_from_the_target_branch(self):
        runs = re.findall(r"python[^\n]*validate\.py[^\n]*", self.code)
        self.assertEqual(len(runs), 2, runs)
        for run in runs:
            self.assertRegex(run, r"^python -I base/scripts/validate\.py ")
        self.assertRegex(self.code, r"--root pr --base ")
        # No fallback to a script taken from the pull request.
        self.assertNotRegex(self.code, r"(?<!base/)scripts/validate\.py")
        self.assertNotRegex(self.code, r"\bcp\b|git show")

    def test_pull_request_is_data_only(self):
        self.assertEqual(self.block("permissions"), ["contents"])
        self.assertRegex(self.code, r"\npermissions:\n  contents: read\n")
        self.assertNotIn("write", self.code)
        checkouts = self.code.count("uses: actions/checkout@")
        self.assertEqual(checkouts, 2)
        self.assertEqual(self.code.count("persist-credentials: false"), checkouts)
        self.assertIn("ref: ${{ github.event.pull_request.head.sha }}", self.code)
        self.assertEqual(self.code.count("path: pr"), 1)
        # Expressions reach shell commands through `env`, never spliced into `run:`.
        for line in self.lines:
            if "run:" in line:
                self.assertNotIn("${{", line)


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
