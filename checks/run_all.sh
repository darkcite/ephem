#!/usr/bin/env bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 Anton (darkcite)
# Runs every checkpoint that can be automated on a laptop (macOS or Linux; on Windows use WSL2).
# Plan, meaning of each ID and pass criteria: docs/P2P-CHAT.md §23 (gates) and §24 (checkpoints).
#
# Usage:   ./checks/run_all.sh [label]
#   label       free text stored with the results, e.g. "home-wifi" or "warp-on"
# Env:
#   BROWSERS=chrome   engines to test (default: your installed Chrome only; Safari is covered by SAFARI=1
#                     and the iPhone page). Options:
#                chrome   = your installed Google Chrome (no download)
#                chromium, firefox, webkit = Playwright builds (downloaded once; webkit = Safari's engine)
#   FORCE_BROWSER_INSTALL=1   re-download Playwright engines even if cached
#   ONLY=a,b       run only these sections: app, tor, stun, gateways, browser, safari, iphone, e8real, sizes, argon2, arti
#                  (app = native tests + end-to-end tests of /app/ (MVP-1, MVP-2) in Chrome;
#                   tor = Tor mode end to end on the REAL Tor network (T-10, needs ./build.sh), plus
#                   the offline lab runs if checks/tor-lab/lab.sh is up (Linux))
#   E8REAL=1       E8 in your real Chrome/Safari/Firefox (macOS): e.g. E8REAL=1 ONLY=e8real ./checks/run_all.sh e8
#   IPHONE=1       serve the checks page to your iPhone through a free Cloudflare quick tunnel
#                  (needs `brew install cloudflared`; works while the repo is private). S6_SECONDS=120
#   SAFARI=1       also run the checks in your real Safari (macOS; opens a Safari tab, results collected automatically)
#   NET=0          skip live-network checks (Snowflake, STUN, IPFS)
#   SOAK=30        also leave a Tor chat idle for 30 minutes and probe it every 5 (tor section)
#   E8=1           also run the 7-minute hidden-tab test (opens a visible Chrome window)
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
want() { [ -z "${ONLY:-}" ] || case ",$ONLY," in *",$1,"*) return 0 ;; *) return 1 ;; esac; }

# ---------------------------------------------------------------- prerequisites
say "Prerequisites"
MISSING=0
need node    "Node.js >= 20: https://nodejs.org"
need npm     "comes with Node.js"
need python3 "Python >= 3.9"
need cargo   "Rust via https://rustup.rs"
need rustup  "Rust via https://rustup.rs"
need gzip    "system package"
[ "$MISSING" = 1 ] && { echo "Install the missing tools and re-run."; exit 2; }
rustup target add wasm32-unknown-unknown >/dev/null

# C code for wasm32 (only `ring`, used by Tor's TLS) needs an LLVM clang with the WebAssembly
# backend. Apple's Xcode clang has none, so on macOS use Homebrew LLVM (brew install llvm).
WASM_CC=""
if [ "$(uname -s)" = Darwin ]; then
  for d in "$(brew --prefix llvm 2>/dev/null)" /opt/homebrew/opt/llvm /usr/local/opt/llvm; do
    [ -n "$d" ] && [ -x "$d/bin/clang" ] && { WASM_CC="$d/bin/clang"; WASM_AR="$d/bin/llvm-ar"; break; }
  done
elif command -v clang >/dev/null 2>&1; then
  WASM_CC="$(command -v clang)"; WASM_AR="$(command -v llvm-ar || command -v ar)"
fi
if [ -n "$WASM_CC" ]; then
  export CC_wasm32_unknown_unknown="$WASM_CC" AR_wasm32_unknown_unknown="$WASM_AR"
else
  echo "note: no LLVM clang with a wasm32 backend found; E1 (Tor build) will be skipped."
  echo "      macOS: brew install llvm      Debian/Ubuntu: sudo apt install clang llvm"
fi

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
BROWSERS="${BROWSERS:-chrome}"
# Playwright's engine installer hangs on Node > 24. If Playwright engines are requested and a
# Node 22 (Homebrew node@22) exists, use it for this run.
case ",$BROWSERS," in *,firefox,*|*,webkit,*|*,chromium,*)
  if [ "$(node -p 'process.versions.node.split(".")[0]')" -gt 24 ]; then
    for d in "$(brew --prefix node@22 2>/dev/null)" /opt/homebrew/opt/node@22 /usr/local/opt/node@22; do
      if [ -n "$d" ] && [ -x "$d/bin/node" ]; then export PATH="$d/bin:$PATH"; echo "using Node $(node --version) from $d for Playwright engines"; break; fi
    done
  fi ;;
esac
# npm: only when node_modules is missing or older than package.json / package-lock.json
if [ ! -d "$ROOT/node_modules/playwright" ] || [ "$ROOT/package.json" -nt "$ROOT/node_modules/.package-lock.json" ] \
   || [ "$ROOT/package-lock.json" -nt "$ROOT/node_modules/.package-lock.json" ]; then
  say "Installing JS dependencies (npm)"
  ( cd "$ROOT" && npm install --no-audit --no-fund 2>&1 ) | tee "$OUT/npm.log" | tail -3
else
  echo "npm dependencies: up to date (skipped)"
fi

# Playwright engines: only those whose binary is not already in the Playwright cache
# (~/Library/Caches/ms-playwright on macOS, ~/.cache/ms-playwright on Linux).
# FORCE_BROWSER_INSTALL=1 re-installs anyway. 'chrome' is your installed Google Chrome: never downloaded.
DL=""
for b in $(echo "$BROWSERS" | tr ',' ' '); do
  case "$b" in
    chromium|firefox|webkit)
      if [ "${FORCE_BROWSER_INSTALL:-0}" = 1 ] || ! ( cd "$ROOT" && node -e "
        const pw = require('playwright'), fs = require('fs');
        process.exit(fs.existsSync(pw['$b'].executablePath()) ? 0 : 1);" 2>/dev/null ); then
        DL="$DL $b"
      else
        echo "Playwright $b: already installed (skipped)"
      fi ;;
  esac
done
if [ -n "$DL" ]; then
  NODE_MAJOR="$(node -p 'process.versions.node.split(".")[0]')"
  if [ "$NODE_MAJOR" -gt 24 ]; then
    echo "warning: Node $NODE_MAJOR is newer than this Playwright supports; its installer can hang after downloading."
    echo "         If it does, use Node 22 LTS (brew install node@22) or drop those engines from BROWSERS."
  fi
  say "Downloading Playwright engines:$DL (one-time; progress below; gives up after 5 minutes)"
  # macOS has no `timeout`; perl's alarm is available everywhere.
  # shellcheck disable=SC2086
  ( cd "$ROOT" && perl -e 'alarm shift; exec @ARGV' 300 npx --no-install playwright install $DL 2>&1 ) | tee "$OUT/playwright-install.log"
  [ "${PIPESTATUS[0]}" = 0 ] || echo "warning: Playwright install did not finish; engines that are not installed will be skipped."
fi

# ---------------------------------------------------------------- S8 / TS3 raw STUN
if [ "${NET:-1}" != 0 ] && want stun; then
  say "S8 / TS3: raw STUN over UDP (what a peer sees) + VPN/WARP verdict"
  { echo "## S8 / TS3: raw STUN over UDP"; echo; python3 "$ROOT/stun_udp.py" "$LABEL"; echo; } | tee -a "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- C-P1 diagnostic (outside the browser)
if [ "${NET:-1}" != 0 ] && want gateways; then
  say "C-P1 diagnostic: gateway status, redirects and CORS headers (curl)"
  CID=bafybeigdyrzt5sfp7udm7hu76uh7y26nf3efuylqabf3oclgtqy55fbzdi
  {
    echo "## C-P1 diagnostic (curl, Origin: http://127.0.0.1)"; echo
    echo "| Gateway | HTTP | Redirect to | Access-Control-Allow-Origin | Content-Type |"; echo "|---|---|---|---|---|"
    for gw in https://trustless-gateway.link https://ipfs.io https://dweb.link; do  # the latter two: expected 301 without CORS
      H="$(curl -sS -m 20 -o /dev/null -D - -H 'Origin: http://127.0.0.1' -H 'Accept: application/vnd.ipld.car' "$gw/ipfs/$CID?format=car&dag-scope=entity" 2>&1 | tr -d '\r')"
      code="$(echo "$H" | awk '/^HTTP/{c=$2} END{print c}')"
      loc="$(echo "$H" | awk 'tolower($1)=="location:"{print $2}' | tail -1)"
      acao="$(echo "$H" | awk 'tolower($1)=="access-control-allow-origin:"{print $2}' | tail -1)"
      ctype="$(echo "$H" | awk 'tolower($1)=="content-type:"{$1=""; print}' | tail -1)"
      [ -z "$code" ] && code="error: $(echo "$H" | head -1 | cut -c1-80)"
      echo "| $gw | $code | ${loc:-–} | ${acao:-none} | ${ctype:-–} |"
    done
    echo
  } | tee -a "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- browser checks
if want app; then
say "App: native tests + end-to-end chat (MVP-1, MVP-2) and rooms (MVP-3) in Chrome"
REPO="$(cd "$ROOT/.." && pwd)"
if command -v cargo >/dev/null 2>&1; then
  NT=$( (cd "$REPO" && env -u CARGO_TARGET_DIR cargo test --workspace --quiet 2>&1) | tee "$OUT/app-native.log" | grep -E "^test result" | awk '{p+=$4; f+=$6} END {printf "%d passed, %d failed", p, f}')
else
  NT="SKIPPED (no cargo)"
fi
# Every source file carries its SPDX license header (tools/spdx.py, LICENSE).
SPDX=$( (cd "$REPO" && python3 tools/spdx.py --check) 2>&1 | tail -1)
echo "$SPDX"
E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/e2e_app.mjs" 2>&1 | tee "$OUT/app-e2e.log"
E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/e2e_mvp2.mjs" 2>&1 | tee "$OUT/app-e2e-mvp2.log"
E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/e2e_room.mjs" 2>&1 | tee "$OUT/app-e2e-room.log"
E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/e2e_cards.mjs" 2>&1 | tee "$OUT/app-e2e-cards.log"
E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/e2e_multi.mjs" 2>&1 | tee "$OUT/app-e2e-multi.log"
node "$ROOT/e2e_keyboard.mjs" 2>&1 | tee "$OUT/app-e2e-keyboard.log"
node "$ROOT/e2e_sw_update.mjs" 2>&1 | tee "$OUT/app-e2e-sw-update.log"
node "$ROOT/channel_interop.mjs" 2>&1 | tee "$OUT/channel-interop.log"
{ echo "## App"; echo; echo "Native tests: $NT"; echo; echo "License headers: $SPDX"; echo; echo "### MVP-1"; echo '```'; cat "$OUT/app-e2e.log"; echo '```'; echo; echo "### MVP-2"; echo '```'; cat "$OUT/app-e2e-mvp2.log"; echo '```'; echo; echo "### MVP-3 rooms"; echo '```'; cat "$OUT/app-e2e-room.log"; echo '```'; echo; echo "### Contact cards"; echo '```'; cat "$OUT/app-e2e-cards.log"; echo '```'; echo; echo "### Several chats in one tab"; echo '```'; cat "$OUT/app-e2e-multi.log"; echo '```'; echo; echo "### Phone keyboard layout"; echo '```'; cat "$OUT/app-e2e-keyboard.log"; echo '```'; echo; echo "### Updates only with consent (service worker)"; echo '```'; cat "$OUT/app-e2e-sw-update.log"; echo '```'; echo; echo "### Channel formats vs the JS IPFS libraries"; echo '```'; cat "$OUT/channel-interop.log"; echo '```'; echo; } >> "$OUT/REPORT.md"
fi

if want tor; then
say "Tor mode (§28): 1:1 and rooms over Tor in Chrome"
{ echo "## Tor mode"; echo; } >> "$OUT/REPORT.md"
if [ -f /tmp/ephlab/lab.env ]; then
  for t in e2e_tor_app e2e_tor_room e2e_tor_cards e2e_tor_channel e2e_tor_vault e2e_tor_incognito e2e_tor_bridges e2e_tor_multi; do
    E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/tor-lab/$t.mjs" 2>&1 | tee "$OUT/lab-$t.log"
    { echo "### Lab: $t"; echo '```'; cat "$OUT/lab-$t.log"; echo '```'; echo; } >> "$OUT/REPORT.md"
  done
  if [ -n "${SOAK:-}" ]; then
    SOAK_MIN="$SOAK" E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/tor-lab/soak_tor.mjs" 2>&1 | grep -vE '^  (alice|bob) \|' | tee "$OUT/lab-soak.log"
    { echo "### Lab: soak, $SOAK min"; echo '```'; cat "$OUT/lab-soak.log"; echo '```'; echo; } >> "$OUT/REPORT.md"
  fi
fi
# A container without UDP: the real Tor network through `checks/tor-lab/lab.sh relay`.
if [ -f /tmp/ephrelay/relay.env ]; then
  for t in e2e_tor_app e2e_tor_room e2e_tor_cards e2e_tor_channel e2e_tor_vault e2e_tor_incognito e2e_tor_multi; do
    RELAY=1 NODE_USE_ENV_PROXY=1 E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/tor-lab/$t.mjs" 2>&1 | tee "$OUT/relay-$t.log"
    { echo "### Real Tor network via the local relay: $t"; echo '```'; cat "$OUT/relay-$t.log"; echo '```'; echo; } >> "$OUT/REPORT.md"
  done
fi
if [ "${NET:-1}" != 0 ]; then
  for t in e2e_tor_app e2e_tor_room e2e_tor_cards e2e_tor_channel e2e_tor_multi; do
    LIVE=1 E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/tor-lab/$t.mjs" 2>&1 | tee "$OUT/live-$t.log"
    { echo "### Live Tor network: $t"; echo '```'; cat "$OUT/live-$t.log"; echo '```'; echo; } >> "$OUT/REPORT.md"
  done
  if [ -n "${SOAK:-}" ]; then
    LIVE=1 SOAK_MIN="$SOAK" E2E_BROWSER="${E2E_BROWSER:-chrome}" node "$ROOT/tor-lab/soak_tor.mjs" 2>&1 | grep -vE '^  (alice|bob) \|' | tee "$OUT/live-soak.log"
    { echo "### Live Tor network: soak, $SOAK min"; echo '```'; cat "$OUT/live-soak.log"; echo '```'; echo; } >> "$OUT/REPORT.md"
  fi
fi
fi

if want browser; then
say "Browser checks: S1 S2 S4 S8 TS4 E2 C-P1 C-P4 ${E8:+E8}"
( cd "$ROOT" && NET="${NET:-1}" E8="${E8:-0}" BROWSERS="$BROWSERS" node browser_checks.mjs "$OUT" ) 2>&1 | tee "$OUT/browser.log"
{ echo "## Browser checks"; echo; cat "$OUT/browser.md" 2>/dev/null || echo "browser checks did not produce a report (see browser.log)"; echo; } >> "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- real Safari (macOS)
if [ "${SAFARI:-0}" = 1 ] && want safari; then
  say "Real Safari: S1 (one tab + Chrome↔Safari cross-engine) S2 S4 S8 E2 C-P1 C-P4 (a Safari tab opens; leave it until DONE)"
  ( cd "$ROOT" && NET="${NET:-1}" node safari_checks.mjs "$OUT" ) 2>&1 | tee "$OUT/safari.log"
  { echo "## Real Safari"; echo; cat "$OUT/safari.md" 2>/dev/null || echo "Safari checks did not produce a report (see safari.log)"; echo; } >> "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- iPhone over a Cloudflare quick tunnel
if [ "${IPHONE:-0}" = 1 ] && want iphone; then
  say "iPhone: G2 S1 S2 S4 S6 S8 C-P1 C-P4 via a Cloudflare quick tunnel (scan the QR code with the iPhone)"
  if command -v cloudflared >/dev/null 2>&1; then
    ( cd "$ROOT" && S6_SECONDS="${S6_SECONDS:-120}" node iphone_checks.mjs "$OUT" ) 2>&1 | tee "$OUT/iphone.log"
    { echo "## iPhone"; echo; cat "$OUT/iphone.md" 2>/dev/null || echo "no iPhone results (see iphone.log)"; echo; } >> "$OUT/REPORT.md"
  else
    echo "cloudflared is not installed: brew install cloudflared" | tee -a "$OUT/REPORT.md"
  fi
fi

# ---------------------------------------------------------------- E8 in real desktop browsers
if [ "${E8REAL:-0}" = 1 ] && want e8real; then
  say "E8 in your real browsers: Chrome, Safari, Firefox open in E8 mode (about 6 minutes)"
  ( cd "$ROOT" && node e8_checks.mjs "$OUT" ) 2>&1 | tee "$OUT/e8.log"
  { echo "## E8 (real browsers)"; echo; cat "$OUT/e8.md" 2>/dev/null || echo "no E8 results (see e8.log)"; echo; } >> "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- S5: wasm sizes
if want sizes; then
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
fi

# ---------------------------------------------------------------- S5b: Argon2id timing
if want argon2; then
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
fi

# ---------------------------------------------------------------- E1 / G1: arti for wasm32
if ! want arti; then
  :
elif [ "${SKIP_ARTI:-0}" != 1 ] && [ -z "$WASM_CC" ]; then
  { echo "## E1 / G1: arti for wasm32"; echo; echo "SKIPPED: needs an LLVM clang with a wasm32 backend (macOS: \`brew install llvm\`)."; echo; } | tee -a "$OUT/REPORT.md"
elif [ "${SKIP_ARTI:-0}" != 1 ]; then
  say "E1 / G1: arti-client (Tor) for wasm32 with the Tor-mode feature set (slow the first time)"
  if ( cd "$ROOT/rust/arti-wasm32" && RUSTFLAGS="$WASM_FLAGS" cargo build -q --release --target wasm32-unknown-unknown >"$OUT/arti.log" 2>&1 ); then
    R="PASS"
  else
    R="FAIL (first error: $(grep -m1 -E '^error' "$OUT/arti.log" | cut -c1-160))"
  fi
  { echo "## E1 / G1: arti for wasm32"; echo; echo "| Check | Result |"; echo "|---|---|"; echo "| arti-client 0.46 + onion client/service, ephemeral keystore, bridges, PT, rustls+ring (no C except ring) | $R |"; echo; } | tee -a "$OUT/REPORT.md"
fi

# ---------------------------------------------------------------- manual checks
cat >> "$OUT/REPORT.md" <<'EOF'
## Manual checks (not automatable on a laptop)

| ID | What to do | Pass criterion |
|---|---|---|
| TS3 | Run this script twice: `./checks/run_all.sh warp-off`, then with Cloudflare WARP (or your VPN) on: `./checks/run_all.sh warp-on`. Compare the S8 tables | With the VPN on, every IPv4 **and** IPv6 address shown belongs to the VPN, not to your ISP |
| S3 | Two devices in a chat (once MVP-1 exists); switch one device from Wi-Fi to mobile data | Recorded per browser: does the chat recover without a new code (T0)? |
| G2, S6, S4 (iPhone) | Open https://darkcite.github.io/ephem/checks/web/ in Safari on the iPhone: tap 1 (checks), 2 (S6: leave for ~60 s, come back), 3 (S4 camera), then Share/Copy the results | E2 rows PASS (gate G2 on iOS); S6 PASS after ≥ 60 s in the background |
| S7 | Desktop: open an answer link in a new tab while the inviting tab is open | The answer is handed to the inviting tab and the new tab closes |
| S9 | iPhone: a room with 15 peers for 10 minutes | No reload or memory kill; battery use recorded |
| E3–E7 | Snowflake Turbotunnel, arti bootstrap, onion hosting in WASM | Needs the TOR-1 implementation; not runnable yet |
| E8 | `E8=1 ./checks/run_all.sh` (Chrome, visible window, 7 minutes) | Automated when E8=1; repeat by hand in Firefox and Safari |
EOF

say "Done"
echo "Report: $OUT/REPORT.md"
grep -E '\| (FAIL|INCONCLUSIVE)' "$OUT/REPORT.md" >/dev/null && echo "Some checks FAILED; see the report." || true
