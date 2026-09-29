#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 Anton (darkcite)
"""Stamps the SPDX license header on the project's source files (LICENSE, NOTICE).

Every source file starts with two lines, in the comment syntax of its language:

    SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
    Copyright 2026 Anton (darkcite)

after a shebang, an `<!doctype …>` or an `<?xml …?>` line if the file has one. Third-party
code keeps its own license and is never stamped (vendor/: arti, MIT OR Apache-2.0), nor are
generated files (app/pkg/: wasm-bindgen output) and formats without comments (JSON, lock
files, images, wasm).

    tools/spdx.py --check          list tracked files without the header (exit 1 if any)
    tools/spdx.py --fix [FILE…]    stamp FILEs (default: every tracked file)
    tools/spdx.py --staged         stamp the files staged for commit and stage them again

`--staged` is what runs before every commit: the git hook (.githooks/pre-commit, enabled with
`git config core.hooksPath .githooks`) and the Claude Code hook (.claude/settings.json).
"""
import os
import subprocess
import sys

LICENSE_ID = "PolyForm-Noncommercial-1.0.0"
OWNER = "Anton (darkcite)"
YEAR = "2026"
MARK = "SPDX-License-Identifier:"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

# Paths never stamped: third-party code, generated files, test fixtures of other projects.
SKIP_PREFIXES = ("vendor/", "app/pkg/", "checks/tor-lab/web/pkg/", "target/", "checks/node_modules/")
SKIP_SUFFIXES = (".json", ".lock", ".png", ".jpg", ".ico", ".wasm", ".pyc", ".webmanifest", ".car", ".bin", ".y4m")
SKIP_NAMES = ("LICENSE", "NOTICE", ".nojekyll")

# Comment syntax by file suffix (or by name for files without one): (start, end).
LINE = {"//": ("// ", ""), "#": ("# ", ""), "css": ("/* ", " */"), "html": ("<!-- ", " -->")}
BY_SUFFIX = {
    ".rs": "//", ".js": "//", ".mjs": "//",
    ".py": "#", ".sh": "#", ".toml": "#", ".tmpl": "#", ".gitignore": "#", ".yml": "#", ".yaml": "#",
    ".css": "css",
    ".html": "html", ".svg": "html", ".md": "html",
}
BY_NAME = {"ephem-net": "#", ".gitignore": "#", "pre-commit": "#"}


def style(path):
    """The comment style of `path`, or None if it is never stamped."""
    rel = path.replace(os.sep, "/")
    name = os.path.basename(rel)
    if rel.startswith(SKIP_PREFIXES) or rel.endswith(SKIP_SUFFIXES) or name in SKIP_NAMES:
        return None
    if name in BY_NAME:
        return BY_NAME[name]
    return BY_SUFFIX.get(os.path.splitext(name)[1])


def header(kind):
    start, end = LINE[kind]
    return f"{start}{MARK} {LICENSE_ID}{end}\n{start}Copyright {YEAR} {OWNER}{end}\n"


def stamped(text):
    return MARK in "".join(text.splitlines(keepends=True)[:6])


def stamp(path):
    """Adds the header to `path` if it is missing. True if the file changed."""
    kind = style(path)
    full = os.path.join(ROOT, path)
    if kind is None or not os.path.isfile(full):
        return False
    with open(full, encoding="utf-8") as f:
        text = f.read()
    if stamped(text):
        return False
    lines = text.splitlines(keepends=True)
    first = lines[0] if lines else ""
    keep = 1 if first.startswith("#!") or first.lower().startswith("<!doctype") or first.startswith("<?xml") else 0
    if keep and not first.endswith("\n"):
        lines[0] = first + "\n"
    out = "".join(lines[:keep]) + header(kind) + "".join(lines[keep:])
    with open(full, "w", encoding="utf-8") as f:
        f.write(out)
    return True


def git(*args):
    return subprocess.run(["git", *args], cwd=ROOT, check=True, capture_output=True, text=True).stdout


def tracked():
    return [p for p in git("ls-files").splitlines() if p]


def staged():
    # Added, copied, modified or renamed; deletions have nothing to stamp.
    return [p for p in git("diff", "--cached", "--name-only", "--diff-filter=ACMR").splitlines() if p]


def main(argv):
    if not argv or argv[0] not in ("--check", "--fix", "--staged"):
        print(__doc__)
        return 2
    mode = argv[0]
    if mode == "--check":
        missing = []
        for p in tracked():
            kind = style(p)
            if kind is None or not os.path.isfile(os.path.join(ROOT, p)):
                continue
            with open(os.path.join(ROOT, p), encoding="utf-8") as f:
                if not stamped(f.read()):
                    missing.append(p)
        for p in missing:
            print(f"missing SPDX header: {p}")
        print(f"spdx: {len(missing)} file(s) without the header")
        return 1 if missing else 0
    if mode == "--fix":
        files = argv[1:] or tracked()
        changed = [p for p in files if stamp(p)]
        for p in changed:
            print(f"stamped {p}")
        return 0
    # --staged: stamp, then stage the stamped version (a partly staged file is stamped whole).
    changed = [p for p in staged() if stamp(p)]
    if changed:
        git("add", "--", *changed)
        for p in changed:
            print(f"spdx: stamped {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
