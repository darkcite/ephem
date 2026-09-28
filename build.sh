#!/usr/bin/env bash
# Builds the Ephem web app into app/pkg/ (committed, served by GitHub Pages).
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

cargo build --release --locked --target wasm32-unknown-unknown -p ephem-wasm
rm -rf app/pkg
wasm-bindgen --target web --no-typescript --out-dir app/pkg --out-name ephem \
  target/wasm32-unknown-unknown/release/ephem_wasm.wasm
if command -v wasm-opt >/dev/null; then
  wasm-opt -Os --enable-bulk-memory --enable-nontrapping-float-to-int -o app/pkg/ephem_bg.wasm app/pkg/ephem_bg.wasm
fi
python3 tools/stamp.py
ls -l app/pkg
