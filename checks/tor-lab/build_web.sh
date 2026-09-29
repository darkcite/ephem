#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 Anton (darkcite)
# Builds tor_bg.wasm for the lab page (checks/tor-lab/web/pkg, not committed).
set -euo pipefail
cd "$(dirname "$0")/../.."
cargo build --release --target wasm32-unknown-unknown -p ephem-tor
rm -rf checks/tor-lab/web/pkg
wasm-bindgen --target web --no-typescript --out-dir checks/tor-lab/web/pkg --out-name tor target/wasm32-unknown-unknown/release/ephem_tor.wasm
ls -l checks/tor-lab/web/pkg
