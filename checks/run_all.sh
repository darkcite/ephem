#!/usr/bin/env bash
# Runs every checkpoint that can be automated on a laptop (macOS or Linux; on Windows use WSL2).
# Plan, meaning of each ID and pass criteria: docs/P2P-CHAT.md §23 (gates) and §24 (checkpoints).
#
# Usage:   ./checks/run_all.sh [label]
#   label       free text stored with the results, e.g. "home-wifi" or "warp-on"
# Env:
#   BROWSERS=chromium,firefox,webkit   browsers to test (default: all three)
#   NET=0          skip live-network checks (Snowflake, STUN, IPFS)
#   E8=1           also run the 7-minute hidden-tab test (opens a visible Chromium window)
#   SKIP_ARTI=1    skip the arti wasm32 build (it takes 5–15 minutes the first time)
#
# Output: checks/out/<timestamp>-<label>/REPORT.md (plus raw JSON and logs)
set -uo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
LABEL="${1:-default}"
TS="$(date +%Y%m%d-%H%M%S)"
OUT="$ROOT/out/$TS-$LABEL"
mkdir -p "$OUT"
export CARGO_TARGET_DIR="$ROOT/rust/target"
WASM_FLAGS='--cfg getrandom_backend="wasm_js"'

say()  { printf '\n\033[1m== %s\033[0m\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || { echo "missing: $1 ($2)"; MISSING=1; }; }
bytes(){ wc -c < "$1" | tr -d ' '; }

# ---------------------------------------------------------------- prerequisites
say "Prerequisites"
MISSING=0
need node    "Node.js >= 20: https://nodejs.org"
need npm     "comes with Node.js"
need python3 "Python >= 3.9"
need cargo   "Rust via https://rustup.rs"
need rustup  "Rust via https://rustup.rs"
need clang   "needed to build ring for wasm32 (macOS: xcode-select --install; Debian/Ubuntu: apt install clang)"
need gzip    "system package"
[ "$MISSING" = 1 ] && { echo "Install the missing tools and re-run."; exit 2; }
rustup target add wasm32-unknown-unknown >/dev/null

{
  echo "# Checkpoint report"
  echo
  echo "| Field | Value |"
  echo "|---|---|"
  echo "| Run | $TS |"
  echo "| Label | $LABEL |"
  echo "| Host | $(uname -srm) |"
  echo "| Node | $(node --version) |"
  echo "| Rust | $(rustc --version) |"
  echo "| Network checks | $([ "${NET:-1}" = 0 ] && echo off || echo on) |"
  echo
} > "$OUT/REPORT.md"

# ---------------------------------------------------------------- browsers
say "Installing JS dependencies and Playwright browsers"
( cd "$ROOT" && npm install --no-audit --no-fund >"$OUT/npm.log" 2>&1 ) || { echo "npm install failed, see $OUT/npm.log"; exit 3; }
IFS=',' read -r -a BLIST <<< "${BROWSERS:-chromium,firefox,webkit}"
( cd "$ROOT" && npx playwright install "${BLIST[@]}" >"$OUT/playwright-install.log" 2>&1 ) || echo "warning: playwright browser install reported errors (see log)"

# ---------------------------------------------------------------- S8 / TS3 raw STUN
if [ "${NET:-1}" != 0 ]; then
  say "S8 / TS3: raw STUN over UDP (what a peer sees)"
  { echo "## S8 / TS3: raw STUN over UDP"; echo; python3 "$ROOT/stun_udp.py" "$LABEL"; echo; } | tee -a "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- browser checks
say "Browser checks: S1 S2 S4 S8 TS4 E2 C-P1 C-P4 ${E8:+E8}"
( cd "$ROOT" && NET="${NET:-1}" E8="${E8:-0}" BROWSERS="${BROWSERS:-chromium,firefox,webkit}" node browser_checks.mjs "$OUT" ) 2>&1 | tee "$OUT/browser.log"
{ echo "## Browser checks"; echo; cat "$OUT/browser.md" 2>/dev/null || echo "browser checks did not produce a report (see browser.log)"; echo; } >> "$OUT/REPORT.md"

# ---------------------------------------------------------------- S5: wasm sizes
say "S5: WASM sizes of protocol crates"
{
  echo "## S5: WASM sizes (opt-level=s, LTO, gzip -9)"; echo
  echo "| Features | wasm bytes | gzip bytes | Result |"; echo "|---|---|---|---|"
  for f in noise keyfile qr mls "noise,keyfile,qr" "noise,keyfile,qr,mls"; do
    if ( cd "$ROOT/rust/wasm-sizes" && RUSTFLAGS="$WASM_FLAGS" cargo build -q --release --target wasm32-unknown-unknown --features "$f" >"$OUT/wasm-sizes-${f//,/_}.log" 2>&1 ); then
      W="$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/wasm_sizes.wasm"
      echo "| $f | $(bytes "$W") | $(gzip -9c "$W" | wc -c | tr -d ' ') | PASS |"
    else
      echo "| $f | – | – | FAIL (see wasm-sizes-${f//,/_}.log) |"
    fi
  done
  echo
} | tee -a "$OUT/REPORT.md"

# ---------------------------------------------------------------- S5b: Argon2id timing
say "S5b: Argon2id timing in WASM (V8)"
{
  echo "## S5b: Argon2id in WASM (V8)"; echo
  echo "| Parameters | Time | Memory |"; echo "|---|---|---|"
  if ( cd "$ROOT/rust/argon2-timing" && cargo build -q --release --target wasm32-unknown-unknown >"$OUT/argon2.log" 2>&1 ); then
    node "$ROOT/rust/argon2-timing/run.mjs" "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/argon2_timing.wasm"
  else
    echo "| build failed | see argon2.log | |"
  fi
  echo
} | tee -a "$OUT/REPORT.md"

# ---------------------------------------------------------------- E1 / G1: arti for wasm32
if [ "${SKIP_ARTI:-0}" != 1 ]; then
  say "E1 / G1: arti-client (Tor) for wasm32 with the Tor-mode feature set (slow the first time)"
  if ( cd "$ROOT/rust/arti-wasm32" && RUSTFLAGS="$WASM_FLAGS" cargo build -q --release --target wasm32-unknown-unknown >"$OUT/arti.log" 2>&1 ); then
    R="PASS"
  else
    R="FAIL (first error: $(grep -m1 -E '^error' "$OUT/arti.log" | cut -c1-160))"
  fi
  { echo "## E1 / G1: arti for wasm32"; echo; echo "| Check | Result |"; echo "|---|---|"; echo "| arti-client 0.46 + onion client/service, bridges, PT, rustls+ring | $R |"; echo; } | tee -a "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- manual checks
cat >> "$OUT/REPORT.md" <<'EOF'
## Manual checks (not automatable on a laptop)

| ID | What to do | Pass criterion |
|---|---|---|
| TS3 | Run this script twice: `./checks/run_all.sh warp-off`, then with Cloudflare WARP (or your VPN) on: `./checks/run_all.sh warp-on`. Compare the S8 tables | With the VPN on, every IPv4 **and** IPv6 address shown belongs to the VPN, not to your ISP |
| S3 | Two devices in a chat (once MVP-1 exists); switch one device from Wi-Fi to mobile data | Recorded per browser: does the chat recover without a new code (T0)? |
| S6 | iPhone: create an invite, switch to a messenger for 30 / 60 / 120 s, come back | The pending connection survives at least 60 s (otherwise iOS users always answer, P2P-CHAT.md §17.5) |
| S7 | Desktop: open an answer link in a new tab while the inviting tab is open | The answer is handed to the inviting tab and the new tab closes |
| S9 | iPhone: a room with 15 peers for 10 minutes | No reload or memory kill; battery use recorded |
| E3–E7 | Snowflake Turbotunnel, arti bootstrap, onion hosting in WASM | Needs the TOR-1 implementation; not runnable yet |
| E8 | `E8=1 ./checks/run_all.sh` (Chromium, visible window, 7 minutes) | Automated when E8=1; repeat by hand in Firefox and Safari |
EOF

say "Done"
echo "Report: $OUT/REPORT.md"
grep -E '\| (FAIL|INCONCLUSIVE)' "$OUT/REPORT.md" >/dev/null && echo "Some checks FAILED; see the report." || true
