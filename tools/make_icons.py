#!/usr/bin/env python3
"""Generates the Ephem app icons (app/icons/*.png, app/icons/icon.svg) without dependencies.

The mark: an accent (#5b9cf5) chat bubble on the dark panel colour (#171a21), the palette of
the darkcite dashboards,, with three text lines that fade out
(messages that disappear). Run: python3 tools/make_icons.py
"""
import os
import struct
import zlib

BG = (23, 26, 33)   # panel #171a21 (background)
BUBBLE = (91, 156, 245)   # accent #5b9cf5 (bubble)
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "app", "icons")
SS = 4  # supersampling per axis


def rounded_rect(x, y, x0, y0, x1, y1, r):
    """Inside test for a rounded rectangle (unit square coordinates)."""
    if x < x0 or x > x1 or y < y0 or y > y1:
        return False
    cx = min(max(x, x0 + r), x1 - r)
    cy = min(max(y, y0 + r), y1 - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def tail(x, y):
    """The bubble's tail: a triangle below the bubble's lower left."""
    ax, ay, bx, by, cx, cy = 0.30, 0.66, 0.46, 0.66, 0.27, 0.80
    d = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
    l1 = ((by - cy) * (x - cx) + (cx - bx) * (y - cy)) / d
    l2 = ((cy - ay) * (x - cx) + (ax - cx) * (y - cy)) / d
    return l1 >= 0 and l2 >= 0 and 1 - l1 - l2 >= 0


LINES = ((0.30, 0.70, 0.36, 1.0), (0.30, 0.62, 0.47, 0.6), (0.30, 0.50, 0.58, 0.28))


def sample(x, y, full_bleed):
    """RGBA of one sample point of the icon (unit square)."""
    if full_bleed:
        bg = True
        m, s = 0.10, 0.80  # keep the mark inside the maskable safe zone
    else:
        bg = rounded_rect(x, y, 0.0, 0.0, 1.0, 1.0, 0.22)
        m, s = 0.0, 1.0
    if not bg:
        return (0, 0, 0, 0)
    u, v = (x - m) / s, (y - m) / s
    if rounded_rect(u, v, 0.18, 0.22, 0.82, 0.68, 0.12) or tail(u, v):
        for x0, x1, yc, alpha in LINES:
            if rounded_rect(u, v, x0, yc - 0.035, x1, yc + 0.035, 0.035):
                mix = tuple(round(BUBBLE[i] * (1 - alpha) + BG[i] * alpha) for i in range(3))
                return mix + (255,)
        return BUBBLE + (255,)
    return BG + (255,)


def render(size, full_bleed):
    rows = []
    n = SS * SS
    for py in range(size):
        row = bytearray([0])  # PNG filter: none
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(SS):
                for sx in range(SS):
                    r, g, b, a = sample((px + (sx + 0.5) / SS) / size, (py + (sy + 0.5) / SS) / size, full_bleed)
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            a = acc[3] / n
            if acc[3]:
                row += bytes((round(acc[0] / acc[3]), round(acc[1] / acc[3]), round(acc[2] / acc[3]), round(a)))
            else:
                row += b"\0\0\0\0"
        rows.append(bytes(row))
    return b"".join(rows)


def png(path, size, full_bleed):
    def chunk(kind, data):
        return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)

    raw = render(size, full_bleed)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(data)


SVG = """<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">
<rect width="100" height="100" rx="22" fill="#171a21"/>
<path d="M30 22h40a12 12 0 0 1 12 12v22a12 12 0 0 1-12 12H46L27 80l3-12a12 12 0 0 1-12-12V34a12 12 0 0 1 12-12z" fill="#5b9cf5"/>
<rect x="30" y="32.5" width="40" height="7" rx="3.5" fill="#171a21"/>
<rect x="30" y="43.5" width="32" height="7" rx="3.5" fill="#171a21" fill-opacity=".6"/>
<rect x="30" y="54.5" width="20" height="7" rx="3.5" fill="#171a21" fill-opacity=".28"/>
</svg>
"""


def main():
    os.makedirs(OUT, exist_ok=True)
    png(os.path.join(OUT, "icon-192.png"), 192, False)
    png(os.path.join(OUT, "icon-512.png"), 512, False)
    png(os.path.join(OUT, "icon-maskable-512.png"), 512, True)
    png(os.path.join(OUT, "apple-touch-icon.png"), 180, True)
    with open(os.path.join(OUT, "icon.svg"), "w") as f:
        f.write(SVG)


if __name__ == "__main__":
    main()
