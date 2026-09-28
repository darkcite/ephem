"""Minimal STUN server for the offline Tor lab (RFC 5389 Binding requests only).

The lab's Snowflake proxy and NAT probe need a STUN server; the public ones are
unreachable from the lab, and on localhost every candidate is a host candidate
anyway. Answers each Binding request with XOR-MAPPED-ADDRESS.

Usage: python3 stun_server.py [port]   (default 3478)
"""
import socket
import struct
import sys

MAGIC = 0x2112A442
BINDING_REQUEST = 0x0001
BINDING_SUCCESS = 0x0101
XOR_MAPPED_ADDRESS = 0x0020


def response(txid, addr):
    ip, port = addr
    xport = port ^ (MAGIC >> 16)
    xip = struct.unpack("!I", socket.inet_aton(ip))[0] ^ MAGIC
    attr = struct.pack("!HHBBHI", XOR_MAPPED_ADDRESS, 8, 0, 1, xport, xip)
    return struct.pack("!HHI", BINDING_SUCCESS, len(attr), MAGIC) + txid + attr


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 3478
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind(("127.0.0.1", port))
    while True:
        data, addr = s.recvfrom(2048)
        if len(data) < 20:
            continue
        kind, _length, magic = struct.unpack("!HHI", data[:8])
        if kind == BINDING_REQUEST and magic == MAGIC:
            s.sendto(response(data[8:20], addr), addr)


if __name__ == "__main__":
    main()
