"""Draws assets/nullshell.ico. Pure standard library, run with: python tools/make_icon.py

The icon is a cut-corner frame with a `>_` prompt, drawn on a 64x64 design
grid and supersampled down to each icon size.
"""

import struct
import zlib
from pathlib import Path

ACCENT = (0xFC, 0xEE, 0x0A)
ALT = (0x00, 0xF0, 0xFF)
BG = (0x08, 0x07, 0x0D)
SIZES = [256, 64, 48, 40, 32, 24, 20, 16]
CUT = 16.0


def sample(x, y, thick):
    """Color (r, g, b, a) at a point of the 64x64 design grid."""
    tl = x + y - CUT
    br = (64 - x) + (64 - y) - CUT
    if tl < 0 or br < 0:
        return (0, 0, 0, 0)
    edge = min(x, y, 64 - x, 64 - y)
    border = edge < 4 * thick or tl < 5.6 * thick or br < 5.6 * thick
    dy = abs(y - 30.5)
    chevron = dy <= 11.5 and abs(x - (29 - dy)) <= 3.2 * thick
    underscore = 34 <= x < 51 and 40 <= y < 40 + 5 * thick
    if border or underscore:
        return (*ACCENT, 255)
    if chevron:
        return (*ALT, 255)
    return (*BG, 255)


def render(size):
    """Rows of (r, g, b, a) pixels, top to bottom."""
    ss = 4 if size >= 64 else 8
    # Thicken strokes on tiny icons so they survive the downscale.
    thick = 1.0 if size >= 40 else 1.5
    rows = []
    for py in range(size):
        row = []
        for px in range(size):
            acc = [0, 0, 0, 0]
            for sy in range(ss):
                for sx in range(ss):
                    x = (px + (sx + 0.5) / ss) * 64 / size
                    y = (py + (sy + 0.5) / ss) * 64 / size
                    r, g, b, a = sample(x, y, thick)
                    acc[0] += r * a
                    acc[1] += g * a
                    acc[2] += b * a
                    acc[3] += a
            n = ss * ss
            if acc[3] == 0:
                row.append((0, 0, 0, 0))
            else:
                row.append((round(acc[0] / acc[3]), round(acc[1] / acc[3]), round(acc[2] / acc[3]), round(acc[3] / n)))
        rows.append(row)
    return rows


def png(rows):
    size = len(rows)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    raw = b"".join(b"\x00" + bytes(c for px in row for c in px) for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def dib(rows):
    """Classic 32-bit icon bitmap: header, BGRA rows bottom-up, empty AND mask."""
    size = len(rows)
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = b"".join(bytes(c for (r, g, b, a) in row for c in (b, g, r, a)) for row in reversed(rows))
    mask_row = ((size + 31) // 32) * 4
    return header + pixels + bytes(mask_row * size)


def main():
    images = []
    for size in SIZES:
        rows = render(size)
        images.append((size, png(rows) if size >= 256 else dib(rows)))

    out = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    for size, data in images:
        dim = 0 if size >= 256 else size
        out += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    for _, data in images:
        out += data

    path = Path(__file__).resolve().parent.parent / "assets" / "nullshell.ico"
    path.write_bytes(out)
    # Preview for eyeballing the result; not used by the build.
    (path.parent / "icon_preview.png").write_bytes(png(render(256)))
    print(f"wrote {path} ({len(out)} bytes)")


if __name__ == "__main__":
    main()
