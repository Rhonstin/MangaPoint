#!/usr/bin/env python3
"""Versioning for MangaPoint: what the next version is, and writing it down.

The version lives in one place, `[workspace.package] version` in Cargo.toml.
What decides a new version is the *subject line* of every commit that reached
`main` since the last release tag, written as a Conventional Commit
(https://www.conventionalcommits.org). Pull requests are squash-merged, so the
subject is the pull request's title.

    feat: ...                      -> minor  (1.0.0 -> 1.1.0)
    fix: ... / perf: ...           -> patch  (1.0.0 -> 1.0.1)
    feat!: ... / BREAKING CHANGE   -> major  (1.0.0 -> 2.0.0)
    docs, chore, ci, test, refactor, style, build -> no release by themselves

Commands (run from the repository root):

    release.py current            print the version in Cargo.toml
    release.py next [--since TAG] print the next version, or "none"
    release.py bump KIND          print the current version raised by patch, minor or major
    release.py apply VERSION      set the version in Cargo.toml and add a
                                  CHANGELOG.md section built from the commits
    release.py notes VERSION      print that version's CHANGELOG.md section
"""

import argparse
import datetime
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CARGO = ROOT / "Cargo.toml"
CHANGELOG = ROOT / "CHANGELOG.md"

# type -> (bump, heading in the changelog)
TYPES = {
    "feat": ("minor", "Features"),
    "fix": ("patch", "Fixes"),
    "perf": ("patch", "Performance"),
}
# Types that are listed in the changelog but do not cause a release alone.
QUIET = {"docs": "Documentation", "refactor": "Changes under the hood", "build": "Build", "ci": "Build"}
ORDER = ["major", "minor", "patch"]

SUBJECT = re.compile(r"^(?P<type>[a-z]+)(\((?P<scope>[^)]+)\))?(?P<bang>!)?: (?P<text>.+?)(?: \(#\d+\))?$")


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def current_version():
    text = read(CARGO)
    in_section = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            in_section = stripped == "[workspace.package]"
        elif in_section:
            m = re.match(r'version\s*=\s*"(\d+\.\d+\.\d+)"', stripped)
            if m:
                return m.group(1)
    sys.exit("no version in [workspace.package] of Cargo.toml")


def latest_tag():
    out = git("tag", "--list", "v[0-9]*", "--sort=-v:refname").split()
    return out[0] if out else None


def commits_since(tag):
    """(subject, body) of each commit since the tag, newest first."""
    rng = f"{tag}..HEAD" if tag else "HEAD"
    raw = git("log", rng, "--no-merges", "--format=%s%x1f%b%x1e")
    result = []
    for record in raw.split("\x1e"):
        record = record.strip("\n")
        if not record.strip():
            continue
        subject, _, body = record.partition("\x1f")
        result.append((subject.strip(), body.strip()))
    return result


def classify(subject, body=""):
    """(type, text, bump or None, breaking) for a commit, or None if it is not a conventional one."""
    m = SUBJECT.match(subject)
    if not m:
        return None
    kind = m.group("type")
    breaking = bool(m.group("bang")) or "BREAKING CHANGE" in body
    bump = TYPES.get(kind, (None,))[0]
    if breaking:
        bump = "major"
    return kind, m.group("text"), bump, breaking


def bump_version(version, bump):
    major, minor, patch = (int(p) for p in version.split("."))
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def decide(commits, version):
    """The next version for these commits, or None when nothing calls for a release."""
    best = None
    for subject, body in commits:
        c = classify(subject, body)
        if c and c[2] and (best is None or ORDER.index(c[2]) < ORDER.index(best)):
            best = c[2]
    return bump_version(version, best) if best else None


def changelog_section(version, commits, today):
    groups = {}
    breaking = []
    for subject, body in commits:
        c = classify(subject, body)
        if not c:
            continue
        kind, text, _, is_breaking = c
        heading = TYPES[kind][1] if kind in TYPES else QUIET.get(kind)
        if is_breaking:
            breaking.append(text)
        if heading:
            groups.setdefault(heading, []).append(text)
    lines = [f"## [{version}] - {today}", ""]
    if breaking:
        lines += ["### Breaking changes", ""] + [f"- {t}" for t in breaking] + [""]
    for heading in ["Features", "Fixes", "Performance", "Documentation", "Changes under the hood", "Build"]:
        if heading in groups:
            lines += [f"### {heading}", ""] + [f"- {t}" for t in groups[heading]] + [""]
    if len(lines) == 2:
        lines += ["- Maintenance release.", ""]
    return "\n".join(lines)


def read(path):
    """The file's text with plain newlines, whatever the platform wrote it with."""
    return path.read_bytes().decode("utf-8").replace("\r\n", "\n")


def write(path, text):
    """Write with plain newlines (git settles the platform's own)."""
    path.write_bytes(text.encode("utf-8"))


def set_cargo_version(version):
    text = read(CARGO)
    lines = text.split("\n")
    in_section = False
    for i, line in enumerate(lines):
        stripped = line.strip()
        if stripped.startswith("["):
            in_section = stripped == "[workspace.package]"
        elif in_section and re.match(r'version\s*=\s*"', stripped):
            lines[i] = re.sub(r'"\d+\.\d+\.\d+"', f'"{version}"', line, count=1)
            write(CARGO, "\n".join(lines))
            return
    sys.exit("no version in [workspace.package] of Cargo.toml")


def add_changelog(section):
    header = (
        "# Changelog\n\nAll notable changes to MangaPoint are written down here. "
        "The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and "
        "the project follows [Semantic Versioning](https://semver.org/).\n\n"
    )
    old = read(CHANGELOG) if CHANGELOG.exists() else header
    if "## [" in old:
        head, _, rest = old.partition("## [")
        new = head + section + "\n## [" + rest
    else:
        new = old.rstrip("\n") + "\n\n" + section
    write(CHANGELOG, new)


def notes_for(version):
    text = read(CHANGELOG)
    m = re.search(rf"^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## \[|\Z)", text, re.S | re.M)
    if not m:
        sys.exit(f"no section for {version} in CHANGELOG.md")
    return m.group(1).strip() + "\n"


def main(argv=None):
    # The changelog holds characters (arrows, ellipses) that a Windows console
    # cannot print in its own encoding.
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8")
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("current")
    nxt = sub.add_parser("next")
    nxt.add_argument("--since")
    sub.add_parser("bump").add_argument("kind", choices=ORDER[::-1])
    for name in ("apply", "notes"):
        sub.add_parser(name).add_argument("version")
    args = parser.parse_args(argv)

    if args.command == "current":
        print(current_version())
    elif args.command == "next":
        tag = args.since or latest_tag()
        base = tag.lstrip("v") if tag else current_version()
        print(decide(commits_since(tag), base) or "none")
    elif args.command == "bump":
        print(bump_version(current_version(), args.kind))
    elif args.command == "apply":
        commits = commits_since(latest_tag())
        set_cargo_version(args.version)
        add_changelog(changelog_section(args.version, commits, datetime.date.today().isoformat()))
    elif args.command == "notes":
        sys.stdout.write(notes_for(args.version))


if __name__ == "__main__":
    main()
