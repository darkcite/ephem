#!/usr/bin/env bash
# Ephem offline Tor lab (docs/P2P-CHAT.md Appendix C.5): a private Tor network (chutney) with a
# Snowflake bridge, plus the real Go Snowflake broker, proxy and NAT probe on localhost.
# Nothing here touches the Internet once the tools are installed.
#
#   checks/tor-lab/lab.sh up       build tools if needed, start everything, write $LAB/lab.env
#   checks/tor-lab/lab.sh down     stop everything
#   checks/tor-lab/lab.sh verify   a system `tor` client bootstraps through Snowflake and
#                                  reaches the lab onion (proof that the lab works)
#
# Needs: tor + tor-gencert (apt), go, python3, git.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
BIN="$HERE/bin"
CHUTNEY="$HERE/.chutney"
# Unix socket paths must stay under 108 bytes, so the data directory is short.
LAB="${EPHEM_LAB:-/tmp/ephlab}"
SF_VERSION="v2.9.2"
SF_MOD="gitlab.torproject.org/tpo/anti-censorship/pluggable-transports/snowflake/v2"
BROKER_PORT=18080
PROBE_PORT=18443
STUN_PORT=3478
ECHO_PORT=4747
NET="$HERE/ephem-net"
export PATH="$BIN:$PATH" CHUTNEY_DATA_DIR="$LAB"

log() { printf '\033[1m[lab]\033[0m %s\n' "$*"; }

tools() {
  mkdir -p "$BIN"
  if [ ! -x "$BIN/sf-server" ]; then
    log "building snowflake $SF_VERSION (broker, proxy, server, client, probetest)"
    local mod; mod="$(mktemp -d)"
    ( cd "$mod" && printf 'module lab\ngo 1.24\n' > go.mod &&
      GOFLAGS=-mod=mod go get "$SF_MOD@$SF_VERSION" >/dev/null 2>&1 &&
      for c in broker proxy server client probetest; do GOFLAGS=-mod=mod go build -o "$BIN/sf-$c" "$SF_MOD/$c"; done )
    rm -rf "$mod"
  fi
  if [ ! -d "$CHUTNEY" ]; then
    log "fetching chutney"
    git clone -q --depth 1 https://github.com/torproject/chutney "$CHUTNEY"
    # Containers without IPv6: an unqualified ORPort also tries [::] and fails.
    sed -i 's/^OrPort \$orport$/OrPort $orport IPv4Only/' "$CHUTNEY/torrc_templates/relay-non-dir.tmpl"
  fi
  cp "$HERE/bridge-snowflake.tmpl" "$CHUTNEY/torrc_templates/"
  # Onion-service timing like the real network: one shared-random round (24 votes) equals one
  # time period (30 min, the minimum). With chutney's 20 s votes the rounds last 8 minutes and
  # arti finds no SRV for the time period (it then falls back to "disaster" parameters and asks
  # other HSDirs than the service used; C tor tolerates that mismatch).
  sed -i -e 's/^V3AuthVotingInterval .*/V3AuthVotingInterval 75/' "$CHUTNEY/torrc_templates/authority.i"
  grep -q '^ConsensusParams hsdir_interval=30' "$CHUTNEY/torrc_templates/authority.i" ||
    echo 'ConsensusParams hsdir_interval=30' >> "$CHUTNEY/torrc_templates/authority.i"
}

bg() { # name, command...
  local name="$1"; shift
  nohup "$@" >"$LAB/$name.log" 2>&1 &
  echo $! > "$LAB/$name.pid"
}

node_dir() { ls -d "$LAB"/nodes/*"$1" | head -1; }

up() {
  tools
  down >/dev/null 2>&1 || true
  rm -rf "$LAB"; mkdir -p "$LAB"
  bg echo python3 "$HERE/echo_server.py" "$ECHO_PORT"
  bg stun python3 "$HERE/stun_server.py" "$STUN_PORT"
  log "configuring and starting the Tor network (chutney)"
  ( cd "$CHUTNEY" && ./chutney configure "$NET" >"$LAB/chutney.log" 2>&1 && ./chutney start "$NET" >>"$LAB/chutney.log" 2>&1 )
  # A fresh test network needs a few voting rounds; chutney's own wait gives up early.
  local ok=0
  for _ in 1 2 3 4; do
    if ( cd "$CHUTNEY" && CHUTNEY_START_TIME=300 ./chutney wait_for_bootstrap "$NET" >>"$LAB/chutney.log" 2>&1 ); then ok=1; break; fi
  done
  (( ok )) || { log "bootstrap failed, see $LAB/chutney.log"; exit 1; }

  local br hs fp num ptport onion
  br="$(node_dir br)"; hs="$(node_dir h)"
  fp="$(awk '{print $2}' "$br/fingerprint")"
  num="$(basename "$br" | sed 's/^0*\([0-9]*\).*/\1/')"; num="${num:-0}"
  ptport=$((9900 + num))
  onion="$(cat "$hs/hidden_service/hostname")"

  printf '{"displayName":"lab","webSocketAddress":"ws://127.0.0.1:%s/","fingerprint":"%s"}\n' "$ptport" "$fp" > "$LAB/bridges.json"
  bg probe "$BIN/sf-probetest" -disable-tls -addr "127.0.0.1:$PROBE_PORT" -stun "stun:127.0.0.1:$STUN_PORT" -unsafe-logging
  bg broker "$BIN/sf-broker" -disable-tls -disable-geoip -addr "127.0.0.1:$BROKER_PORT" -bridge-list-path "$LAB/bridges.json" -allowed-relay-pattern "^127.0.0.1$" -unsafe-logging
  sleep 1
  bg proxy "$BIN/sf-proxy" -broker "http://127.0.0.1:$BROKER_PORT/" -relay "ws://127.0.0.1:$ptport/" -allow-non-tls-relay \
    -allow-proxying-to-private-addresses -keep-local-addresses -allowed-relay-hostname-pattern "^127.0.0.1$" \
    -nat-probe-server "http://127.0.0.1:$PROBE_PORT/probe" -stun "stun:127.0.0.1:$STUN_PORT" -capacity 20 -verbose -unsafe-logging

  # The lab client's configuration (authorities, testing options) minus its own paths and
  # ports: the base of every extra lab client (tor for `verify`; arti gets the authorities).
  grep -vE '^(RunAsDaemon|Sandbox|DataDirectory|SocksPort|ControlPort|ControlSocket|PidFile|Log |Nickname|CookieAuthentication|#|$)' "$(node_dir c)/torrc" > "$LAB/client-base.torrc"
  # The lab network for arti 0.46 clients (chutney writes an older authorities format).
  python3 - "$LAB/nodes/arti.toml" > "$LAB/arti-net.toml" <<'PY'
import re
import sys
text = open(sys.argv[1]).read()
net = text[text.index("[path_rules]"):]
idents = re.findall(r'v3ident = "([0-9A-F]{40})"', net)
net = re.sub(r"authorities = \[.*?\](\n|$)", "", net, flags=re.S)
print(net.rstrip())
print()
print("[tor_network.authorities]")
print("v3idents = [" + ", ".join('"%s"' % i for i in idents) + "]")
PY
  cat > "$LAB/lab.env" <<EOF
LAB=$LAB
BRIDGE_FP=$fp
BRIDGE_PTPORT=$ptport
BROKER_URL=http://127.0.0.1:$BROKER_PORT/
STUN_URL=stun:127.0.0.1:$STUN_PORT
ONION=$onion
ONION_PORT=5858
ECHO_PORT=$ECHO_PORT
EOF
  log "up: $(tr '\n' ' ' < "$LAB/lab.env")"
}

down() {
  [ -d "$CHUTNEY" ] && ( cd "$CHUTNEY" && ./chutney stop "$NET" >/dev/null 2>&1 || true )
  for p in "$LAB"/*.pid; do [ -f "$p" ] && kill "$(cat "$p")" 2>/dev/null || true; rm -f "$p"; done
  local t; t="$(pgrep -x tor || true)"; [ -n "$t" ] && kill $t 2>/dev/null || true
  log "down"
}

verify() {
  # shellcheck disable=SC1091
  . "$LAB/lab.env"
  local d="$LAB/verify-client"; rm -rf "$d"; mkdir -p "$d"
  cat "$LAB/client-base.torrc" > "$d/torrc"
  cat >> "$d/torrc" <<EOF
DataDirectory $d
SocksPort 127.0.0.1:19050
UseBridges 1
Sandbox 0
Bridge snowflake 192.0.2.3:80 $BRIDGE_FP fingerprint=$BRIDGE_FP
ClientTransportPlugin snowflake exec $BIN/sf-client -url $BROKER_URL -ice $STUN_URL -keep-local-addresses -log $d/sf-client.log -unsafe-logging
EOF
  tor -f "$d/torrc" >"$d/tor.log" 2>&1 &
  local pid=$! t0=$SECONDS
  until grep -q "Bootstrapped 100%" "$d/tor.log"; do
    if (( SECONDS - t0 > 240 )); then log "verify: no bootstrap in 240 s"; tail -5 "$d/tor.log"; kill $pid; exit 1; fi
    sleep 2
  done
  log "verify: bootstrapped through Snowflake in $((SECONDS - t0)) s"
  local got
  got="$(python3 - "$ONION" <<'PY'
import socket
import struct
import sys
s = socket.create_connection(("127.0.0.1", 19050), timeout=120)
s.sendall(b"\x05\x01\x00")
s.recv(2)
host = sys.argv[1].encode()
s.sendall(b"\x05\x01\x00\x03" + bytes([len(host)]) + host + struct.pack("!H", 5858))
rep = s.recv(10)
if rep[1] != 0:
    print("socks error", rep[1])
    sys.exit(0)
s.sendall(b"ephem-lab")
print(s.recv(64).decode())
PY
)"
  kill $pid
  [ "$got" = "ephem-lab" ] && log "verify: onion echo OK" || { log "verify: onion echo failed: $got"; exit 1; }
}

case "${1:-}" in
  up) up ;;
  down) down ;;
  verify) verify ;;
  *) echo "usage: $0 up|down|verify"; exit 2 ;;
esac
