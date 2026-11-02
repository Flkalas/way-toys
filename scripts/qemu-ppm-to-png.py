"""Convert a QEMU screendump P6 image to PNG using Python's standard library."""

import struct
import sys
import zlib
from pathlib import Path


def chunk(kind, data):
    return (
        struct.pack(">I", len(data))
        + kind
        + data
        + struct.pack(">I", zlib.crc32(kind + data))
    )


source, destination = map(Path, sys.argv[1:])
with source.open("rb") as stream:
    if stream.readline().strip() != b"P6":
        raise ValueError("Expected QEMU's binary P6 image")
    width, height = map(int, stream.readline().split())
    if stream.readline().strip() != b"255":
        raise ValueError("Expected 8-bit color channels")
    pixels = stream.read()
if len(pixels) != width * height * 3:
    raise ValueError("Incomplete pixel data")
rows = b"".join(
    b"\0" + pixels[y * width * 3 : (y + 1) * width * 3] for y in range(height)
)
destination.write_bytes(
    b"\x89PNG\r\n\x1a\n"
    + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
    + chunk(b"IDAT", zlib.compress(rows))
    + chunk(b"IEND", b"")
)
print(f"{width}x{height}: {destination}")
