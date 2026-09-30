#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 Anton (darkcite)
"""Stamps integrity hashes and the build id into the web app (docs/P2P-CHAT.md §17.1, §17.2).

- app/index.html, between `<!-- stamp:begin -->` and `<!-- stamp:end -->`:
  the CSP (with the hash of the inline import map), build id, SRI of app.css and app.js,
  an import map with the integrity of every JS module, and the SHA-384 of the wasm module
  (app.js fetches it with `integrity`).
- app/tor.html (§28.2), generated from app/index.html: the same page with `data-mode="tor"`
  and the Tor build (pkg/ephem_tor*). Both pages pin the Tor build: the direct page loads it
  only for its channel tabs (Appendix F.3.3).
- app/channel.html, generated: old channel links (`channel.html#c=…`) move to the app.
- app/sw.js: VERSION (= build id), FILES (the precache list: the direct app) and TOR_FILES
  (cached on first use, so direct users who never open a channel never download the Tor build).

The build id is the first 12 hex digits of SHA-256 over every precached file, so it changes
whenever any of them changes. Run by ./build.sh; run it again after editing app/*.js or *.css.
"""
import base64
import hashlib
import json
import os
import re

APP = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "app")
MODULES = ["app.js", "slots.js", "bridges.js", "channels.js", "ui.js", "perf.js", "pkg/ephem.js", "pkg/ephem_tor.js"]
TOR_MODULES = ["app.js", "slots.js", "bridges.js", "channels.js", "ui.js", "perf.js", "pkg/ephem_tor.js"]
HASHED = ["app.css", "app.js", "slots.js", "bridges.js", "channels.js", "ui.js", "perf.js", "pkg/ephem.js", "pkg/ephem_bg.wasm", "manifest.webmanifest"]
TOR_FILES = ["tor.html", "pkg/ephem_tor.js", "pkg/ephem_tor_bg.wasm", "channel.html", "redirect.js"]
PRECACHE = ["./", "index.html"] + HASHED + [
    "icons/icon.svg",
    "icons/icon-192.png",
    "icons/icon-512.png",
    "icons/icon-maskable-512.png",
    "icons/apple-touch-icon.png",
]
CSP = ("default-src 'none'; script-src 'self' 'wasm-unsafe-eval' '{importmap}'; style-src 'self'; "
       "img-src 'self' data: blob:; connect-src 'self'{connect}; worker-src 'self'; manifest-src 'self'; "
       "media-src 'self' blob:; base-uri 'none'; form-action 'none'")
# Pages reach only a Snowflake broker with fetch (§28.6; the direct page for its channel tabs);
# everything else goes through Tor or WebRTC. The broker may be the user's own (bridge lines,
# Appendix F.2), so any https: origin (decision D5: WebRTC, which carries the proxies and chats,
# is outside connect-src anyway; scripts stay hash-pinned). Channel readers without Tor reach
# the public trustless gateway (§D.6.3) under the same rule.
CONNECT = " https:"
# channel.html: a redirect, nothing else.
REDIRECT = """<!doctype html>
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->
<html lang="en">
<head>
<meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; base-uri 'none'; form-action 'none'">
<meta name="referrer" content="no-referrer">
<title>Ephem · Channels</title>
<script type="module" src="redirect.js" integrity="{sri}"></script>
</head>
<body><p>Channels are part of Ephem now: <a href="tor.html#tab=own">open Ephem</a>.</p></body>
</html>
"""


def read(rel):
    with open(os.path.join(APP, rel), "rb") as f:
        return f.read()


def sri(data, alg="sha384"):
    return f"{alg}-" + base64.b64encode(hashlib.new(alg, data).digest()).decode()


def stamp_block(hashes, modules, wasm, connect):
    importmap = json.dumps({"imports": {}, "integrity": {"./" + m: hashes[m] for m in modules}}, separators=(",", ":"))
    csp = CSP.format(importmap=sri(importmap.encode(), "sha256"), connect=connect)
    return "\n".join([
        "<!-- stamp:begin (tools/stamp.py) -->",
        f'<meta http-equiv="Content-Security-Policy" content="{csp}">',
        "{build}",
        f'<meta name="ephem-wasm" content="{hashes[wasm]}">',
        f'<meta name="ephem-tor-wasm" content="{hashes["pkg/ephem_tor_bg.wasm"]}">',
        f'<link rel="stylesheet" href="app.css" integrity="{hashes["app.css"]}">',
        f'<script type="importmap">{importmap}</script>',
        f'<script type="module" src="app.js" integrity="{hashes["app.js"]}"></script>',
        "<!-- stamp:end -->",
    ])


def restamp(html, block):
    html, n = re.subn(r"<!-- stamp:begin.*?<!-- stamp:end -->", lambda _: block, html, flags=re.S)
    if n != 1:
        raise SystemExit("app/index.html: stamp markers not found")
    return html


def write(rel, text):
    with open(os.path.join(APP, rel), "w", encoding="utf-8") as f:
        f.write(text)


def main():
    tor_pkg = [f for f in TOR_FILES if f.startswith("pkg/")]
    hashes = {rel: sri(read(rel)) for rel in HASHED + tor_pkg + ["redirect.js"]}
    with open(os.path.join(APP, "index.html"), encoding="utf-8") as f:
        index = f.read()
    tor = restamp(index, stamp_block(hashes, TOR_MODULES, "pkg/ephem_tor_bg.wasm", CONNECT))
    tor, n = re.subn(r'<html lang="en">', '<html lang="en" data-mode="tor">', tor, count=1)
    tor = tor.replace("<title>Ephem</title>", "<title>Ephem · Tor</title>", 1)
    if n != 1:
        raise SystemExit("app/index.html: <html lang=\"en\"> not found")
    # Every file feeds the build id (icons and the Tor build too), so any change makes a new SW
    # version. tor.html is derived from the others (and would contain the id itself).
    cached = [f for f in PRECACHE if f not in ("./", "index.html")] + tor_pkg + ["redirect.js"]
    # index.html counts too (a change to it alone, e.g. a <meta>, must reach installed apps): its
    # stamped text with the build id still a placeholder, so stamping twice gives the same id.
    direct = restamp(index, stamp_block(hashes, MODULES, "pkg/ephem_bg.wasm", CONNECT))
    build = hashlib.sha256(("".join(sri(read(r)) for r in cached) + sri(direct.encode())).encode()).hexdigest()[:12]
    meta = f'<meta name="ephem-build" content="{build}">'
    write("index.html", direct.replace("{build}", meta, 1))
    write("tor.html", tor.replace("{build}", meta, 1))
    write("channel.html", REDIRECT.replace("{sri}", hashes["redirect.js"]))

    path = os.path.join(APP, "sw.js")
    with open(path, encoding="utf-8") as f:
        sw = f.read()
    sw = re.sub(r"^const VERSION = .*;$", f"const VERSION = '{build}';", sw, count=1, flags=re.M)
    sw = re.sub(r"^const FILES = .*;$", "const FILES = " + json.dumps(PRECACHE) + ";", sw, count=1, flags=re.M)
    sw = re.sub(r"^const TOR_FILES = .*;$", "const TOR_FILES = " + json.dumps(TOR_FILES) + ";", sw, count=1, flags=re.M)
    with open(path, "w", encoding="utf-8") as f:
        f.write(sw)
    print(f"build {build}")


if __name__ == "__main__":
    main()
