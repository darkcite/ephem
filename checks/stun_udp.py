"""S8 / TS3: raw STUN binding requests over UDP (IPv4 and IPv6), plus a VPN/WARP verdict.

Prints the public address each STUN server reports, which is what a peer
would see in direct mode. Run once without and once with a VPN / WARP to
compare. Output: one markdown table on stdout.
"""
import os
import socket
import struct
import sys
import urllib.request

MAGIC = 0x2112A442
SERVERS = [
    ("stun.l.google.com", 19302),
    ("stun.cloudflare.com", 3478),
    ("global.stun.twilio.com", 3478),
]


def xor_mapped(data: bytes, tid: bytes) -> str:
    pos = 20
    while pos + 4 <= len(data):
        atype, alen = struct.unpack("!HH", data[pos:pos + 4])
        val = data[pos + 4:pos + 4 + alen]
        if atype == 0x0020 and len(val) >= 8:
            fam = val[1]
            port = struct.unpack("!H", val[2:4])[0] ^ (MAGIC >> 16)
            if fam == 1:
                raw = struct.unpack("!I", val[4:8])[0] ^ MAGIC
                return f"{socket.inet_ntop(socket.AF_INET, struct.pack('!I', raw))}:{port}"
            if fam == 2 and len(val) >= 20:
                key = struct.pack("!I", MAGIC) + tid
                raw = bytes(a ^ b for a, b in zip(val[4:20], key))
                return f"[{socket.inet_ntop(socket.AF_INET6, raw)}]:{port}"
        pos += 4 + alen + ((4 - alen % 4) % 4)
    return "no XOR-MAPPED-ADDRESS"


def probe(host: str, port: int, family: int) -> str:
    try:
        infos = socket.getaddrinfo(host, port, family, socket.SOCK_DGRAM)
    except socket.gaierror:
        return "no DNS record"
    addr = infos[0][4]
    if family == socket.AF_INET6 and str(addr[0]).startswith("::ffff:"):
        return "no IPv6 route (resolver returned only an IPv4-mapped address)"
    try:
        sock = socket.socket(family, socket.SOCK_DGRAM)
    except OSError as exc:
        return f"no local support ({exc.__class__.__name__})"
    sock.settimeout(3.0)
    tid = os.urandom(12)
    try:
        sock.sendto(struct.pack("!HHI", 0x0001, 0, MAGIC) + tid, addr)
        data = sock.recvfrom(2048)[0]
        seen = xor_mapped(data, tid)
        if family == socket.AF_INET6 and not seen.startswith("["):
            return f"no IPv6 route (reply came over IPv4: {seen})"
        return seen
    except OSError as exc:
        return f"no reply ({exc.__class__.__name__})"
    finally:
        sock.close()


# Cloudflare WARP consumer egress ranges (IPv4 104.28.0.0/16, IPv6 2a09:bac0::/29).
WARP_V4 = ("104.28.",)
WARP_V6_PREFIXES = tuple(f"2a09:bac{n}:" for n in "01234567")


def in_warp_range(ip: str) -> bool:
    return ip.startswith(WARP_V4) or ip.lower().startswith(WARP_V6_PREFIXES)


def nat_mapping() -> str:
    """S10: send from ONE local socket to two STUN servers and compare the mapped ports.

    Same public port for both = endpoint-independent mapping ("cone" NAT): direct P2P usually works.
    Different ports = address-dependent mapping ("symmetric" NAT): direct P2P with another NATed
    peer usually fails without a relay (which this design forbids).
    """
    targets = []
    for host, port in SERVERS[:2]:
        try:
            targets.append(socket.getaddrinfo(host, port, socket.AF_INET, socket.SOCK_DGRAM)[0][4])
        except socket.gaierror:
            return "INCONCLUSIVE: DNS failed"
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.settimeout(3.0)
    mapped = []
    try:
        for addr in targets:
            tid = os.urandom(12)
            sock.sendto(struct.pack("!HHI", 0x0001, 0, MAGIC) + tid, addr)
            mapped.append(xor_mapped(sock.recvfrom(2048)[0], tid))
    except OSError as exc:
        return f"INCONCLUSIVE: no reply ({exc.__class__.__name__})"
    finally:
        sock.close()
    local = "one local socket"
    if mapped[0] == mapped[1]:
        return f"endpoint-independent (cone): {local} -> {mapped[0]} for both servers; direct P2P usually works"
    return (f"address-dependent (SYMMETRIC): {local} -> {mapped[0]} and {mapped[1]}; direct P2P with another "
            "NATed peer usually FAILS without a relay")


def https_view() -> dict:
    """Public IP as seen over HTTPS (TCP), and Cloudflare's WARP flag."""
    try:
        with urllib.request.urlopen("https://www.cloudflare.com/cdn-cgi/trace", timeout=8) as resp:
            body = resp.read().decode("ascii", "replace")
    except OSError as exc:
        return {"error": exc.__class__.__name__}
    kv = dict(line.split("=", 1) for line in body.splitlines() if "=" in line)
    return {"ip": kv.get("ip", "?"), "warp": kv.get("warp", "?")}


def main() -> int:
    label = sys.argv[1] if len(sys.argv) > 1 else "default"
    print(f"| Server | IPv4 (as seen by peer) | IPv6 (as seen by peer) | Run |")
    print("|---|---|---|---|")
    ok = 0
    seen_v4 = []
    for host, port in SERVERS:
        v4 = probe(host, port, socket.AF_INET)
        seen_v4.append(v4)
        v6 = probe(host, port, socket.AF_INET6)
        ok += (":" in v4 and "no " not in v4)
        print(f"| {host}:{port} | {v4} | {v6} | {label} |")
    web = https_view()
    stun_ips = sorted({r.split("]:")[0].lstrip("[") if r.startswith("[") else r.rsplit(":", 1)[0]
                       for r in seen_v4 if ":" in r and not r.startswith("no ")})
    print()
    print(f"HTTPS (TCP) public IP: {web.get('ip', web.get('error'))}; Cloudflare warp={web.get('warp', '?')}")
    print(f"STUN (UDP, what a WebRTC peer sees) IPv4: {', '.join(stun_ips) or 'none'}")
    if not stun_ips or "ip" not in web:
        verdict = "INCONCLUSIVE: no STUN reply over UDP or no HTTPS answer (UDP blocked, or offline)"
    elif web.get("warp") in ("on", "plus"):
        # WARP uses several egress addresses (different ones for TCP and UDP), all in its own ranges.
        via_warp = all(in_warp_range(x) for x in stun_ips)
        verdict = ("PASS: WARP is on and UDP/WebRTC leaves through a WARP address (peers do not see your ISP address)"
                   if via_warp else
                   "FAIL: WARP is on for HTTPS but UDP/WebRTC leaves from a non-WARP address (peers see your real IP)")
    elif stun_ips and web.get("ip") and web.get("ip") not in stun_ips:
        verdict = ("CHECK: HTTPS and UDP leave from different IPs. Fine if both belong to your VPN (VPNs may use "
                   "several exit addresses); a leak if the UDP address is your ISP's")
    else:
        verdict = "INFO: no WARP detected; TCP and UDP leave from the same IP. If a VPN is supposed to be on, it is not active"
    print(f"TS3 verdict: {verdict}")
    print(f"S10 NAT mapping (IPv4): {nat_mapping()}")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
