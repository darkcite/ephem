#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 Anton (darkcite)
# Builds the Ephem web app into app/pkg/ (committed, served by GitHub Pages), and app/tor.html.
# Needs: rustup target wasm32-unknown-unknown, wasm-bindgen-cli matching crates/wasm (=0.2.129).
# Optional: wasm-opt (binaryen) shrinks the module further.
set -euo pipefail
cd "$(dirname "$0")"

want=$(grep -oE 'wasm-bindgen = "=[0-9.]+"' crates/wasm/Cargo.toml | grep -oE '[0-9.]+[0-9]')
have=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}' || true)
if [[ "$have" != "$want" ]]; then
  echo "wasm-bindgen-cli $want required (found: ${have:-none}): cargo install wasm-bindgen-cli --version $want" >&2
  exit 1
fi

# The Tor and channel builds compile ring's C code for wasm32 (the TLS of the Tor link). Apple's
# clang has no wasm32 target; Homebrew's LLVM has. Pick the first clang that can, with its llvm-ar.
wasm_cc() { echo 'int x;' | "$1" --target=wasm32-unknown-unknown -x c -c - -o /dev/null 2>/dev/null; }
if [[ -z "${CC_wasm32_unknown_unknown:-}" ]]; then
  for cc in "${CC:-clang}" "$(brew --prefix llvm 2>/dev/null)/bin/clang" /opt/homebrew/opt/llvm/bin/clang /usr/local/opt/llvm/bin/clang; do
    if [[ -x "$(command -v "$cc" 2>/dev/null)" ]] && wasm_cc "$cc"; then
      export CC_wasm32_unknown_unknown="$cc"
      ar="$(dirname "$(command -v "$cc")")/llvm-ar"
      [[ -x "$ar" ]] && export AR_wasm32_unknown_unknown="$ar"
      break
    fi
  done
  if [[ -z "${CC_wasm32_unknown_unknown:-}" ]]; then
    echo "a clang with the wasm32 target is required (Apple's clang has none): brew install llvm" >&2
    exit 1
  fi
fi

# Two builds of the same adapter (§28.2): direct (index.html) and Tor (tor.html, cargo feature
# `tor`: arti + Snowflake inside, and public channels). The direct page downloads the Tor build
# only if its user opens a channel tab (Appendix F.3.3).
rm -rf app/pkg
for variant in ephem:"" ephem_tor:"--features tor"; do
  name=${variant%%:*}
  # shellcheck disable=SC2086
  cargo build --release --locked --target wasm32-unknown-unknown -p ephem-wasm ${variant#*:}
  wasm-bindgen --target web --no-typescript --out-dir app/pkg --out-name "$name" \
    target/wasm32-unknown-unknown/release/ephem_wasm.wasm
  if command -v wasm-opt >/dev/null; then
    wasm-opt -Os --enable-bulk-memory --enable-nontrapping-float-to-int -o "app/pkg/${name}_bg.wasm" "app/pkg/${name}_bg.wasm"
  fi
done
# The boards' proof-of-work solver for Web Workers (crates/pow): a raw module with no imports and
# no JS glue; pages fetch it with its SHA-384 and hand the compiled module to their Workers.
cargo build --release --locked --target wasm32-unknown-unknown -p ephem-pow
cp target/wasm32-unknown-unknown/release/ephem_pow.wasm app/pkg/ephem_pow.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -O3 --enable-bulk-memory --enable-nontrapping-float-to-int -o app/pkg/ephem_pow.wasm app/pkg/ephem_pow.wasm
fi
python3 tools/stamp.py
ls -l app/pkg
