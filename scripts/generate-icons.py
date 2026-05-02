#!/usr/bin/env python3
from pathlib import Path
import math
import struct
import zlib

ROOT = Path(__file__).resolve().parents[1]
ICON_DIR = ROOT / "assets" / "icons"
PNG_DIR = ICON_DIR / "png"
SIZES = (16, 32, 48, 64, 128, 256, 512, 1024)

BG = (17, 19, 27, 255)
BORDER = (42, 47, 59, 255)
ACCENT = (94, 216, 255, 255)
TEXT = (244, 247, 251, 255)

GLYPHS = {
    "m": (
        "10001",
        "11011",
        "10101",
        "10101",
        "10001",
        "10001",
        "10001",
    ),
    "d": (
        "00001",
        "00001",
        "01101",
        "10011",
        "10001",
        "10001",
        "01111",
    ),
}


def png_chunk(kind, data):
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    )


def write_png(path, width, height, pixels):
    rows = []
    for y in range(height):
        row = bytearray()
        row.append(0)
        for x in range(width):
            row.extend(pixels[y * width + x])
        rows.append(bytes(row))

    data = b"".join(
        [
            b"\x89PNG\r\n\x1a\n",
            png_chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0)),
            png_chunk(b"IDAT", zlib.compress(b"".join(rows), 9)),
            png_chunk(b"IEND", b""),
        ]
    )
    path.write_bytes(data)


def distance_to_segment(px, py, ax, ay, bx, by):
    vx = bx - ax
    vy = by - ay
    wx = px - ax
    wy = py - ay
    denom = vx * vx + vy * vy
    if denom == 0:
        return math.hypot(px - ax, py - ay)
    t = max(0.0, min(1.0, (wx * vx + wy * vy) / denom))
    cx = ax + t * vx
    cy = ay + t * vy
    return math.hypot(px - cx, py - cy)


def smooth_edge(distance, radius, feather):
    return max(0.0, min(1.0, (radius + feather - distance) / (2.0 * feather)))


def rounded_rect_alpha(x, y, size, inset, radius, feather):
    left = inset
    top = inset
    right = size - inset
    bottom = size - inset
    cx = max(left + radius, min(x, right - radius))
    cy = max(top + radius, min(y, bottom - radius))
    distance = math.hypot(x - cx, y - cy)
    if left + radius <= x <= right - radius and top <= y <= bottom:
        distance = 0.0
    if top + radius <= y <= bottom - radius and left <= x <= right:
        distance = 0.0
    return smooth_edge(distance, radius, feather)


def line_alpha(x, y, size, points):
    stroke = size * 0.074
    feather = max(0.75, size * 0.005)
    alpha = 0.0
    for (ax, ay), (bx, by) in points:
        dist = distance_to_segment(x, y, ax * size, ay * size, bx * size, by * size)
        alpha = max(alpha, smooth_edge(dist, stroke / 2.0, feather))
    return alpha


def glyph_alpha(x, y, size):
    cell = size * 0.04
    gap = cell * 0.18
    x0 = size * 0.545
    y0 = size * 0.365
    cursor = x0

    for letter in "md":
        glyph = GLYPHS[letter]
        width = len(glyph[0])
        height = len(glyph)
        if cursor <= x < cursor + width * cell and y0 <= y < y0 + height * cell:
            col = int((x - cursor) / cell)
            row = int((y - y0) / cell)
            inner_x = (x - cursor) - col * cell
            inner_y = (y - y0) - row * cell
            if (
                glyph[row][col] == "1"
                and gap <= inner_x <= cell - gap
                and gap <= inner_y <= cell - gap
            ):
                return 1.0
        cursor += (width + 1) * cell
    return 0.0


def composite(dst, src, alpha):
    src_alpha = alpha * src[3] / 255.0
    dst_alpha = dst[3] / 255.0
    out_alpha = src_alpha + dst_alpha * (1.0 - src_alpha)
    if out_alpha <= 0.0:
        return (0, 0, 0, 0)
    out = []
    for index in range(3):
        value = (
            src[index] * src_alpha
            + dst[index] * dst_alpha * (1.0 - src_alpha)
        ) / out_alpha
        out.append(round(value))
    out.append(round(out_alpha * 255))
    return tuple(out)


def sample_icon(x, y, size):
    pixel = (0, 0, 0, 0)
    base_alpha = rounded_rect_alpha(x, y, size, size * 0.045, size * 0.185, 1.5)
    pixel = composite(pixel, BG, base_alpha)

    border_outer = rounded_rect_alpha(x, y, size, size * 0.045, size * 0.185, 1.5)
    border_inner = rounded_rect_alpha(x, y, size, size * 0.078, size * 0.15, 1.5)
    pixel = composite(pixel, BORDER, max(0.0, border_outer - border_inner))

    chevrons = [
        ((0.19, 0.305), (0.36, 0.5)),
        ((0.19, 0.695), (0.36, 0.5)),
        ((0.345, 0.305), (0.515, 0.5)),
        ((0.345, 0.695), (0.515, 0.5)),
    ]
    pixel = composite(pixel, ACCENT, line_alpha(x, y, size, chevrons))
    pixel = composite(pixel, TEXT, glyph_alpha(x, y, size))
    return pixel


def render_icon(size):
    scale = 4 if size >= 64 else 3
    pixels = []
    for y in range(size):
        for x in range(size):
            accum = [0, 0, 0, 0]
            samples = scale * scale
            for sy in range(scale):
                for sx in range(scale):
                    px = x + (sx + 0.5) / scale
                    py = y + (sy + 0.5) / scale
                    color = sample_icon(px, py, size)
                    for index, channel in enumerate(color):
                        accum[index] += channel
            pixels.append(tuple(round(channel / samples) for channel in accum))
    return pixels


def write_ico(path, png_paths):
    images = [png_path.read_bytes() for png_path in png_paths]
    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + len(images) * 16
    entries = []
    for png_path, data in zip(png_paths, images):
        size = int(png_path.stem.split("x", 1)[0])
        width = 0 if size >= 256 else size
        entries.append(
            struct.pack("<BBBBHHII", width, width, 0, 0, 1, 32, len(data), offset)
        )
        offset += len(data)
    path.write_bytes(header + b"".join(entries) + b"".join(images))


def write_icns(path, png_paths):
    mapping = {
        128: b"ic07",
        256: b"ic08",
        512: b"ic09",
        1024: b"ic10",
    }
    chunks = []
    for png_path in png_paths:
        size = int(png_path.stem.split("x", 1)[0])
        kind = mapping.get(size)
        if not kind:
            continue
        data = png_path.read_bytes()
        chunks.append(kind + struct.pack(">I", len(data) + 8) + data)
    payload = b"".join(chunks)
    path.write_bytes(b"icns" + struct.pack(">I", len(payload) + 8) + payload)


def main():
    PNG_DIR.mkdir(parents=True, exist_ok=True)
    generated = {}
    for size in SIZES:
        path = PNG_DIR / f"{size}x{size}.png"
        write_png(path, size, size, render_icon(size))
        generated[size] = path

    (ICON_DIR / "velocimd.png").write_bytes(generated[512].read_bytes())
    write_ico(ICON_DIR / "velocimd.ico", [generated[size] for size in (16, 32, 48, 64, 128, 256)])
    write_icns(ICON_DIR / "velocimd.icns", [generated[size] for size in (128, 256, 512, 1024)])


if __name__ == "__main__":
    main()
