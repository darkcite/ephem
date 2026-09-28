#!/usr/bin/env bash
# E1: try to build each arti crate for wasm32-unknown-unknown (default features off)
# and print the first compiler error. Run from an empty scratch directory.
set -u
cargo new -q --lib e1 && cd e1
for c in tor-llcrypto tor-bytes tor-cell tor-linkspec tor-proto tor-netdoc tor-netdir tor-rtcompat \
         tor-chanmgr tor-circmgr tor-dirmgr tor-guardmgr tor-keymgr tor-hsclient tor-hsservice arti-client; do
  sed -i '/^\[dependencies\]/,$d' Cargo.toml
  printf '[dependencies]\n%s = { version = "*", default-features = false }\ngetrandom = { version = "0.3", features = ["wasm_js"] }\ngetrandom02 = { package = "getrandom", version = "0.2", features = ["js"] }\n' "$c" >> Cargo.toml
  echo 'pub fn f() {}' > src/lib.rs
  RUSTFLAGS='--cfg getrandom_backend="wasm_js"' cargo build -q --release --target wasm32-unknown-unknown > "log_$c.txt" 2>&1
  echo "$c exit=$? $(grep -m1 -E '^error' -A3 "log_$c.txt" | tr '\n' ' ' | cut -c1-230)"
done
