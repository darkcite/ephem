# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
# Copyright 2026 Anton (darkcite)
"""TCP echo server for the offline Tor lab: the target of the lab's onion service and of the
Snowflake interop tests. Every connection echoes what it receives until the peer closes.

Usage: python3 echo_server.py [port]   (default 4747)
"""
import socket
import sys
import threading


def serve(conn):
    with conn:
        while True:
            data = conn.recv(65536)
            if not data:
                return
            conn.sendall(data)


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 4747
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    s.bind(("127.0.0.1", port))
    s.listen(64)
    while True:
        conn, _addr = s.accept()
        threading.Thread(target=serve, args=(conn,), daemon=True).start()


if __name__ == "__main__":
    main()
