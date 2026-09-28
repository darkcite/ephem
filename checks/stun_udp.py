"""S8 / TS3: raw STUN binding requests over UDP (IPv4 and IPv6).

Prints the public address each STUN server reports, which is what a peer
would see in direct mode. Run once without and once with a VPN / WARP to
compare. Output: one markdown table on stdout.
"""
import os
import socket
import struct
import sys

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


def main() -> int:
    label = sys.argv[1] if len(sys.argv) > 1 else "default"
    print(f"| Server | IPv4 (as seen by peer) | IPv6 (as seen by peer) | Run |")
    print("|---|---|---|---|")
    ok = 0
    for host, port in SERVERS:
        v4 = probe(host, port, socket.AF_INET)
        v6 = probe(host, port, socket.AF_INET6)
        ok += (":" in v4 and "no " not in v4)
        print(f"| {host}:{port} | {v4} | {v6} | {label} |")
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main())
