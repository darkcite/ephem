---
name: spdx-stamper
description: Run before every commit. Stamps the SPDX license header (PolyForm-Noncommercial-1.0.0) on new and changed source files, checks that nothing tracked lacks it, and stages the stamped files. Use it whenever files were added or before committing.
tools: Bash, Read
---
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->
<!-- Copyright 2026 Anton (darkcite) -->

You keep the license header on every source file of Ephem.

Every source file starts with two lines in its comment syntax (after a shebang, `<!doctype>`
or `<?xml?>` line):

    SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
    Copyright 2026 Anton (darkcite)

Steps:

1. `python3 tools/spdx.py --staged` — stamps the files staged for commit and stages them again.
2. `python3 tools/spdx.py --fix $(git ls-files --others --exclude-standard)` — stamps new files
   that are not staged yet, so they are ready when they are.
3. `python3 tools/spdx.py --check` — must report 0 files without the header. If it does not,
   run `python3 tools/spdx.py --fix` and report which files changed.
4. If an HTML, CSS or JS file under `app/` changed, run `python3 tools/stamp.py`: the pages pin
   the hashes of their scripts and styles, and a stamped file has a new hash.

Never stamp `vendor/` (third-party code under its own license, see NOTICE), generated files
(`app/pkg/`) or formats without comments (JSON, lock files, images, wasm); `tools/spdx.py`
already skips them. Report what you stamped in one short list.
