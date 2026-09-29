#!/bin/sh
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 Anton (darkcite)
# Claude Code PreToolUse hook (Bash): before an agent's `git commit`, stamp the SPDX header on
# the staged files (tools/spdx.py --staged), so a commit never lands without it, even where the
# git hook (.githooks/pre-commit) is not enabled. Other commands pass through untouched.
input=$(cat)
case "$input" in
  *'"command"'*'git commit'*|*'"command"'*'git -C '*' commit'*)
    root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0
    python3 "$root/tools/spdx.py" --staged >&2 || exit 2
    ;;
esac
exit 0
