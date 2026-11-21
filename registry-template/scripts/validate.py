#!/usr/bin/env python3
"""Validates the Qxide registry: index format, archive checksums and contents, and (with
--base) that a pull request only appends versions or toggles `yanked`.

Usage: python scripts/validate.py [--base <git ref>] [--summary <file>] [--root <dir>]
Standard library only (Python 3.11+ for tomllib).
"""

import argparse
import hashlib
import io
import json
import re
import subprocess
import sys
import tarfile
import tomllib
import unicodedata
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
# Patterns are used with fullmatch (`$` would also accept a trailing newline).
NAME = re.compile(r"[a-z](?:[a-z0-9]|-(?=[a-z0-9])){0,63}")
SHA256 = re.compile(r"[0-9a-f]{64}")
U64_MAX = (1 << 64) - 1
MAX_COMPARATORS = 32
# Download locations after template expansion: an https URL or a plain path in this repository.
DL_URL = re.compile(r"https://[A-Za-z0-9.-]+(?::[0-9]{1,5})?(?:/[A-Za-z0-9._~%+!$&'()*,;=:@/-]*)?")
DL_SEGMENT = re.compile(r"[A-Za-z0-9._+-]+")
REQUIRED = {"name", "vers", "deps", "cksum", "yanked", "v"}
KNOWN = REQUIRED | {"qxide", "description", "dl"}
DEFAULT_DL = "packages/{prefix}/{name}/{name}-{version}.tar.gz"
MAX_ARCHIVE = 10 << 20
MAX_UNPACKED = 50 << 20
MAX_FILES = 2000

# Windows device names, reserved with or without an extension (mirrors src/pkg/archive.rs).
RESERVED = {"CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"} | {
    f"{dev}{n}" for dev in ("COM", "LPT") for n in "123456789¹²³"
}
WINDOWS_INVALID = set('<>:"/\\|?*')
ASCII_UPPER = str.maketrans("abcdefghijklmnopqrstuvwxyz", "ABCDEFGHIJKLMNOPQRSTUVWXYZ")
INVALID_VALUE = "`{key}` has invalid value {value}"
CONFIG_LOCKED = "cannot be modified in a pull request"
SELF_DEPENDENCY = "cannot depend on itself"

errors: list[str] = []
summary: list[str] = []


def portable(part: str) -> bool:
    """Same rule as `portable_component` in the client: a name every OS stores unchanged."""
    if not part or part.endswith((".", " ")):
        return False
    if any(c in WINDOWS_INVALID or unicodedata.category(c) == "Cc" for c in part):
        return False
    return part.split(".")[0].rstrip(" ").translate(ASCII_UPPER) not in RESERVED


def fold(part: str) -> str:
    """Case folding used to find entries that collide on case-insensitive file systems."""
    return part.lower().upper()


def error(where: str, what: str) -> None:
    errors.append(f"{where}: {what}")


def prefix(name: str) -> str:
    if len(name) == 1:
        return "1"
    if len(name) == 2:
        return "2"
    if len(name) == 3:
        return f"3/{name[0]}"
    return f"{name[:2]}/{name[2:4]}"


def dl_template() -> str:
    config = ROOT / "config.json"
    if not config.is_file():
        return DEFAULT_DL
    dl = json.loads(config.read_text(encoding="utf-8")).get("dl", DEFAULT_DL)
    if not isinstance(dl, str):
        error("config.json", INVALID_VALUE.format(key="dl", value=repr(dl)))
        return DEFAULT_DL
    return dl


def archive_path(entry: dict, template: str) -> str:
    return (
        (entry.get("dl") or template)
        .replace("{name}", entry["name"])
        .replace("{version}", entry["vers"])
        .replace("{prefix}", prefix(entry["name"]))
        .replace("{sha256-checksum}", entry["cksum"])
    )


def valid_dl(target: str) -> bool:
    """An expanded download location: an https URL, or a relative path of plain segments."""
    if target.startswith("https://"):
        return DL_URL.fullmatch(target) is not None
    parts = target.split("/")
    return all(DL_SEGMENT.fullmatch(p) and p not in (".", "..") and portable(p) for p in parts)


# A port of the `semver` crate's parser (the one the client uses), so this checker accepts exactly
# the versions and requirements the client accepts: u64 numbers, no leading zeros, ASCII only.
class _Bad(Exception):
    pass


def _number(s: str, i: int) -> tuple[int, int]:
    j = i
    while j < len(s) and "0" <= s[j] <= "9":
        j += 1
    if j == i or (j - i > 1 and s[i] == "0"):
        raise _Bad
    value = int(s[i:j])
    if value > U64_MAX:
        raise _Bad
    return value, j


def _dot(s: str, i: int) -> int:
    if s[i : i + 1] != ".":
        raise _Bad
    return i + 1


def _identifier(s: str, i: int, pre: bool) -> tuple[str, int]:
    """Dot-separated [0-9A-Za-z-] segments; numeric pre-release segments have no leading zero."""
    start = i
    while True:
        j = i
        while j < len(s) and s[j].isascii() and (s[j].isalnum() or s[j] == "-"):
            j += 1
        segment = s[i:j]
        if not segment or (pre and len(segment) > 1 and segment.isdigit() and segment[0] == "0"):
            raise _Bad
        if s[j : j + 1] == ".":
            i = j + 1
            continue
        return s[start:j], j


def parse_version(s: str):
    """(major, minor, patch, pre, build) as the client parses it, or None."""
    try:
        major, i = _number(s, 0)
        minor, i = _number(s, _dot(s, i))
        patch, i = _number(s, _dot(s, i))
        pre = build = ""
        if s[i : i + 1] == "-":
            pre, i = _identifier(s, i + 1, True)
        if s[i : i + 1] == "+":
            build, i = _identifier(s, i + 1, False)
        if i != len(s):
            raise _Bad
        return major, minor, patch, pre, build
    except _Bad:
        return None


def registry_version(s) -> bool:
    """A valid `vers`: semver without build metadata, which would tie with the plain version."""
    v = parse_version(s) if isinstance(s, str) else None
    return v is not None and v[4] == ""


def _comparator(s: str) -> int:
    """Parses one comparator at the start of `s`; returns the index after it and its spaces."""
    i = 0
    if s.startswith((">=", "<=")):
        i = 2
    elif s[:1] in ("=", ">", "<", "~", "^"):
        i = 1
    while s[i : i + 1] == " ":
        i += 1
    _, i = _number(s, i)
    wild = False
    patch = None
    if s[i : i + 1] == ".":
        i += 1
        if s[i : i + 1] in ("*", "x", "X"):
            wild, i = True, i + 1
        else:
            _, i = _number(s, i)
    if s[i : i + 1] == ".":
        i += 1
        if s[i : i + 1] in ("*", "x", "X"):
            i += 1
        elif wild:
            raise _Bad
        else:
            patch, i = _number(s, i)
    if patch is not None and s[i : i + 1] == "-":
        _, i = _identifier(s, i + 1, True)
    if patch is not None and s[i : i + 1] == "+":
        _, i = _identifier(s, i + 1, False)
    while s[i : i + 1] == " ":
        i += 1
    return i


def valid_req(s) -> bool:
    """Whether the client accepts `s` as a version requirement."""
    if not isinstance(s, str):
        return False
    try:
        s = s.lstrip(" ")
        if s[:1] in ("*", "x", "X"):
            return s[1:].lstrip(" ") == ""
        count = 0
        while True:
            s = s[_comparator(s) :]
            count += 1
            if not s:
                return True
            if s[0] != "," or count == MAX_COMPARATORS:
                raise _Bad
            s = s[1:].lstrip(" ")
    except _Bad:
        return False


def precedence(vers: str):
    """Sort key giving semver precedence; versions with equal keys are the same release."""
    major, minor, patch, pre, _ = parse_version(vers)
    if not pre:
        return (major, minor, patch, 1, ())
    ids = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split("."))
    return (major, minor, patch, 0, ids)


def check_entry(where: str, entry: dict, file_name: str) -> bool:
    if not isinstance(entry, dict):
        error(where, "not a JSON object")
        return False
    missing = REQUIRED - entry.keys()
    if missing:
        error(where, f"missing keys {sorted(missing)}")
        return False
    unknown = entry.keys() - KNOWN
    if unknown:
        error(where, f"unknown keys {sorted(unknown)}")
    ok = True
    # `True == 1` in Python, so check the type before the value.
    if type(entry["v"]) is not int or entry["v"] != 1:
        error(where, "`v` must be 1")
        ok = False
    if entry["name"] != file_name or not isinstance(entry["name"], str) or not NAME.fullmatch(entry["name"]):
        error(where, f"name `{entry['name']}` does not match the index file `{file_name}`")
        ok = False
    if not isinstance(entry["vers"], str) or parse_version(entry["vers"]) is None:
        error(where, f"`vers` {entry['vers']!r} is not a semantic version")
        ok = False
    elif not registry_version(entry["vers"]):
        error(where, INVALID_VALUE.format(key="vers", value=repr(entry["vers"])))
        ok = False
    if not isinstance(entry["cksum"], str) or not SHA256.fullmatch(entry["cksum"]):
        error(where, "`cksum` must be 64 lowercase hex characters")
        ok = False
    if not isinstance(entry["yanked"], bool):
        error(where, "`yanked` must be true or false")
        ok = False
    if "qxide" in entry and not valid_req(entry["qxide"]):
        error(where, INVALID_VALUE.format(key="qxide", value=repr(entry["qxide"])))
        ok = False
    for key in ("description", "dl"):
        if key in entry and not isinstance(entry[key], str):
            error(where, INVALID_VALUE.format(key=key, value=repr(entry[key])))
            ok = False
    if not isinstance(entry["deps"], list):
        error(where, "`deps` must be a list")
        return False
    for d in entry["deps"]:
        if not (
            isinstance(d, dict)
            and isinstance(d.get("name"), str)
            and NAME.fullmatch(d["name"])
            and valid_req(d.get("req"))
        ):
            error(where, f"bad dependency {d!r}")
            ok = False
    return ok


def check_archive(where: str, entry: dict, data: bytes) -> list[str]:
    """Checks one archive; returns its file list for the review summary."""
    digest = hashlib.sha256(data).hexdigest()
    if digest != entry["cksum"]:
        error(where, f"sha256 of the archive is {digest}, the index says {entry['cksum']}")
        return []
    if len(data) > MAX_ARCHIVE:
        error(where, "archive is larger than 10 MiB")
        return []
    root = f"{entry['name']}-{entry['vers']}"
    files, total, manifest = [], 0, None
    shipped: set[str] = set()
    folded_files: set[tuple[str, ...]] = set()
    folded_dirs: set[tuple[str, ...]] = set()
    try:
        with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as tar:
            count = 0
            # Iterate lazily: a huge member list is refused without being read in full.
            for m in tar:
                count += 1
                if count > MAX_FILES:
                    error(where, f"more than {MAX_FILES} files")
                    break
                p = PurePosixPath(m.name)
                if (
                    p.is_absolute()
                    or ".." in p.parts
                    or p.parts[:1] != (root,)
                    or any(":" in s or "\\" in s for s in p.parts)
                    or not all(portable(s) for s in p.parts[1:])
                ):
                    error(where, f"unsafe path `{m.name}`")
                    continue
                if m.isdir():
                    continue
                if not m.isfile():
                    error(where, f"`{m.name}` is not a regular file")
                    continue
                rel = "/".join(p.parts[1:])
                if not rel:
                    error(where, f"unsafe path `{m.name}`")
                    continue
                folded = tuple(fold(s) for s in p.parts[1:])
                prefixes = [folded[:k] for k in range(1, len(folded))]
                if folded in folded_files or folded in folded_dirs or any(d in folded_files for d in prefixes):
                    error(where, "entry `{path}` collides case-insensitively with another entry".format(path=m.name))
                    continue
                folded_files.add(folded)
                folded_dirs.update(prefixes)
                shipped.add(rel)
                total += m.size
                files.append(f"{rel} ({m.size} B)")
                if rel == "qxide.toml":
                    manifest = tar.extractfile(m).read().decode("utf-8")
    except (tarfile.TarError, OSError, EOFError) as e:
        error(where, f"not a gzip tar archive: {e}")
        return []
    if total > MAX_UNPACKED:
        error(where, "unpacked size is larger than 50 MiB")
    if manifest is None:
        error(where, "archive has no qxide.toml")
        return files
    try:
        pkg = tomllib.loads(manifest).get("package", {})
    except tomllib.TOMLDecodeError as e:
        error(where, f"qxide.toml is not valid TOML: {e}")
        return files
    if pkg.get("name") != entry["name"] or pkg.get("version") != entry["vers"]:
        error(where, "qxide.toml name/version do not match the index entry")
    for key in ("description", "license", "qxide"):
        if not pkg.get(key):
            error(where, f"qxide.toml has no `{key}`")
    if "src/lib.qx" not in shipped:
        error(where, "archive has no src/lib.qx")
    return files


def git(*args: str) -> str:
    return subprocess.run(["git", "-C", str(ROOT), *args], check=True, capture_output=True, text=True).stdout


def base_lines(base: str, rel: str) -> list[str]:
    try:
        return git("show", f"{base}:{rel}").splitlines()
    except subprocess.CalledProcessError:
        return []


def squash(name: str) -> str:
    return name.replace("-", "")


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", help="git ref of the target branch, to check what a PR changes")
    parser.add_argument("--summary", help="markdown file for reviewers (GITHUB_STEP_SUMMARY)")
    parser.add_argument("--root", help="registry checkout (default: this script's repository)")
    args = parser.parse_args()
    global ROOT
    if args.root:
        ROOT = Path(args.root).resolve()

    template = dl_template()
    index_dir = ROOT / "index"
    names: dict[str, list[dict]] = {}
    for path in sorted(p for p in index_dir.rglob("*") if p.is_file() and p.name != ".gitkeep"):
        rel = path.relative_to(ROOT).as_posix()
        name = path.name
        if rel != f"index/{prefix(name)}/{name}" or not NAME.fullmatch(name):
            error(rel, f"index files live at index/{prefix(name)}/{name} and use valid package names")
            continue
        entries, seen = [], set()
        for i, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            if not line.strip():
                continue
            where = f"{rel}:{i}"
            try:
                entry = json.loads(line)
            except json.JSONDecodeError as e:
                error(where, f"invalid JSON: {e}")
                continue
            if not check_entry(where, entry, name):
                continue
            # Compared by precedence, so no two lines can ever stand for the same release.
            key = precedence(entry["vers"])
            if key in seen:
                error(where, f"version {entry['vers']} is listed twice")
            seen.add(key)
            entries.append(entry)
        names[name] = entries

    # Archives hosted in this repository must exist and match.
    new_files: dict[str, list[str]] = {}
    for name, entries in names.items():
        for entry in entries:
            target = archive_path(entry, template)
            where = f"{name} {entry['vers']}"
            if not valid_dl(target):
                error(where, INVALID_VALUE.format(key="dl", value=repr(target)))
                continue
            if "://" in target:
                continue
            archive = ROOT / target
            if not archive.is_file():
                error(where, f"archive {target} is missing")
                continue
            new_files[where] = check_archive(where, entry, archive.read_bytes())

    # Dependencies must name packages in this registry.
    for name, entries in names.items():
        for entry in entries:
            for d in entry["deps"]:
                if d["name"] == name:
                    error(f"{name} {entry['vers']}", SELF_DEPENDENCY)
                if d["name"] not in names:
                    error(f"{name} {entry['vers']}", f"dependency `{d['name']}` is not in the registry")

    changed = set(new_files)
    if args.base:
        changed = set()
        for name, entries in names.items():
            rel = f"index/{prefix(name)}/{name}"
            old = [l for l in base_lines(args.base, rel) if l.strip()]
            new = [json.dumps(e, sort_keys=True) for e in entries]
            old_entries = [json.loads(l) for l in old]
            # Published lines may only flip `yanked`; nothing is removed or rewritten.
            for i, o in enumerate(old_entries):
                if i >= len(entries):
                    error(rel, f"version {o.get('vers')} was removed; yank it instead")
                    continue
                n = dict(entries[i], yanked=o.get("yanked"))
                if json.dumps(n, sort_keys=True) != json.dumps(o, sort_keys=True):
                    error(rel, f"line {i + 1} changed; published versions are immutable except `yanked`")
            for e in entries[len(old_entries):]:
                changed.add(f"{name} {e['vers']}")
            if not old and new:
                # A new package name: reject look-alikes of existing names.
                for other in names:
                    if other != name and squash(other) == squash(name) and base_lines(args.base, f"index/{prefix(other)}/{other}"):
                        error(rel, f"`{name}` is too similar to the existing package `{other}`")
        diff = [l.split("\t") for l in git("diff", "--name-status", f"{args.base}...HEAD").splitlines()]
        touched = [p for _, *paths in diff for p in paths]
        # Publishing and yanking pull requests change registry data only, never scripts or workflows.
        if any(p.startswith(("index/", "packages/")) for p in touched):
            for p in touched:
                if not p.startswith(("index/", "packages/")):
                    error(p, "a pull request that changes index/ or packages/ may not change other files")
        for status, *paths in diff:
            for p in paths:
                if p == "config.json":
                    error(p, CONFIG_LOCKED)
                if p.startswith("packages/") and status[0] in "MDR":
                    error(p, "published archives cannot be modified or deleted")
                if p == "search.json":
                    error(p, "search.json is generated after merge; do not edit it in a pull request")

    if args.summary:
        lines = ["## Registry check", ""]
        lines += [f"- {e}" for e in errors] or ["All checks passed."]
        for where in sorted(changed):
            lines += ["", f"### {where}", ""] + [f"- `{f}`" for f in new_files.get(where, [])]
        Path(args.summary).open("a", encoding="utf-8").write("\n".join(lines) + "\n")

    for e in errors:
        print(f"error: {e}")
    print(f"{len(names)} packages, {sum(len(v) for v in names.values())} versions, {len(errors)} errors")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
