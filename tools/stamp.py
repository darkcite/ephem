#!/usr/bin/env python3
"""Stamps integrity hashes and the build id into the web app (docs/P2P-CHAT.md §17.1, §17.2).

- app/index.html, between `<!-- stamp:begin -->` and `<!-- stamp:end -->`:
  the CSP (with the hash of the inline import map), build id, SRI of app.css and app.js,
  an import map with the integrity of every JS module, and the SHA-384 of the wasm module
  (app.js fetches it with `integrity`).
- app/sw.js: VERSION (= build id) and FILES (the precache list).

The build id is the first 12 hex digits of SHA-256 over every precached file, so it changes
whenever any of them changes. Run by ./build.sh; run it again after editing app/*.js or *.css.
"""
import base64
import hashlib
import json
import os
import re

APP = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "app")
MODULES = ["app.js", "slots.js", "pkg/ephem.js"]
HASHED = ["app.css", "app.js", "slots.js", "pkg/ephem.js", "pkg/ephem_bg.wasm", "manifest.webmanifest"]
PRECACHE = ["./", "index.html"] + HASHED + [
    "icons/icon.svg",
    "icons/icon-192.png",
    "icons/icon-512.png",
    "icons/icon-maskable-512.png",
    "icons/apple-touch-icon.png",
]
CSP = ("default-src 'none'; script-src 'self' 'wasm-unsafe-eval' '{importmap}'; style-src 'self'; "
       "img-src 'self' data: blob:; connect-src 'self'; worker-src 'self'; manifest-src 'self'; "
       "media-src 'self' blob:; base-uri 'none'; form-action 'none'")


def read(rel):
    with open(os.path.join(APP, rel), "rb") as f:
        return f.read()


def sri(data, alg="sha384"):
    return f"{alg}-" + base64.b64encode(hashlib.new(alg, data).digest()).decode()


def main():
    hashes = {rel: sri(read(rel)) for rel in HASHED}
    # Every precached file feeds the build id, so any change (icons too) makes a new SW version.
    cached = [f for f in PRECACHE if f not in ("./", "index.html")]
    build = hashlib.sha256("".join(sri(read(r)) for r in cached).encode()).hexdigest()[:12]
    importmap = json.dumps({"imports": {}, "integrity": {"./" + m: hashes[m] for m in MODULES}}, separators=(",", ":"))
    block = "\n".join([
        "<!-- stamp:begin (tools/stamp.py) -->",
        f'<meta http-equiv="Content-Security-Policy" content="{CSP.format(importmap=sri(importmap.encode(), "sha256"))}">',
        f'<meta name="ephem-build" content="{build}">',
        f'<meta name="ephem-wasm" content="{hashes["pkg/ephem_bg.wasm"]}">',
        f'<link rel="stylesheet" href="app.css" integrity="{hashes["app.css"]}">',
        f'<script type="importmap">{importmap}</script>',
        f'<script type="module" src="app.js" integrity="{hashes["app.js"]}"></script>',
        "<!-- stamp:end -->",
    ])
    path = os.path.join(APP, "index.html")
    with open(path, encoding="utf-8") as f:
        html = f.read()
    html, n = re.subn(r"<!-- stamp:begin.*?<!-- stamp:end -->", lambda _: block, html, flags=re.S)
    if n != 1:
        raise SystemExit("app/index.html: stamp markers not found")
    with open(path, "w", encoding="utf-8") as f:
        f.write(html)

    path = os.path.join(APP, "sw.js")
    with open(path, encoding="utf-8") as f:
        sw = f.read()
    sw = re.sub(r"^const VERSION = .*;$", f"const VERSION = '{build}';", sw, count=1, flags=re.M)
    sw = re.sub(r"^const FILES = .*;$", "const FILES = " + json.dumps(PRECACHE) + ";", sw, count=1, flags=re.M)
    with open(path, "w", encoding="utf-8") as f:
        f.write(sw)
    print(f"build {build}")


if __name__ == "__main__":
    main()
