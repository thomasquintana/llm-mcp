#!/usr/bin/env python3
"""Generate tiny attributable real-provider fixtures; --audio uses Docker/eSpeak NG."""

import argparse
import base64
import io
from pathlib import Path
import struct
import subprocess
import wave
import zlib

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "tests" / "fixtures"
SENTENCE = "The red square is next to the blue circle."
DEBIAN = "debian:bookworm-slim@sha256:7ff6ba31527b9e336162fa061746c366c4b2032f4d2cff2078bea43f78efe3f8"
FONT = {
    "R": ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "D": ["11110", "10001", "10001", "10001", "10001", "10001", "11110"],
    "B": ["11110", "10001", "10001", "11110", "10001", "10001", "11110"],
    "L": ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
    "U": ["10001", "10001", "10001", "10001", "10001", "10001", "01110"],
    "4": ["10010", "10010", "10010", "11111", "00010", "00010", "00010"],
    "2": ["01110", "10001", "00001", "00010", "00100", "01000", "11111"],
    " ": ["00000"] * 7,
}


def image():
    width, height = 480, 260
    pixels = bytearray([255] * width * height * 3)

    def pixel(x, y, rgb):
        start = (y * width + x) * 3
        pixels[start : start + 3] = bytes(rgb)

    for y in range(30, 170):
        for x in range(45, 185):
            pixel(x, y, (230, 25, 25))
    for y in range(30, 170):
        for x in range(285, 425):
            if (x - 355) ** 2 + (y - 100) ** 2 <= 70**2:
                pixel(x, y, (25, 55, 230))
    for text, left in [("RED 42", 40), ("BLUE", 290)]:
        for index, char in enumerate(text):
            for row, bits in enumerate(FONT[char]):
                for column, bit in enumerate(bits):
                    if bit == "1":
                        for dy in range(5):
                            for dx in range(5):
                                pixel(
                                    left + index * 30 + column * 5 + dx,
                                    195 + row * 5 + dy,
                                    (0, 0, 0),
                                )

    def chunk(kind, data):
        return (
            struct.pack(">I", len(data))
            + kind
            + data
            + struct.pack(">I", zlib.crc32(kind + data))
        )

    rows = b"".join(
        b"\0" + pixels[y * width * 3 : (y + 1) * width * 3] for y in range(height)
    )
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(rows, 9))
        + chunk(b"IEND", b"")
    )


def speech():
    audio = subprocess.check_output(
        [
            "docker",
            "run",
            "--rm",
            DEBIAN,
            "sh",
            "-ec",
            "apt-get update -qq >&2; apt-get install --no-install-recommends -y espeak-ng >&2; "
            'espeak-ng --stdout -v en-us -s 145 "$1"',
            "fixture",
            SENTENCE,
        ]
    )
    # eSpeak's streaming WAV header uses placeholder sizes. Rewrite a regular WAV.
    with wave.open(io.BytesIO(audio), "rb") as reader:
        params = reader.getparams()
        frames = reader.readframes(reader.getnframes())
    output = io.BytesIO()
    with wave.open(output, "wb") as writer:
        writer.setparams(params)
        writer.writeframes(frames)
    return output.getvalue()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--audio",
        action="store_true",
        help="also regenerate the spoken WAV using Docker",
    )
    args = parser.parse_args()
    FIXTURES.joinpath("shapes.png.base64").write_text(
        base64.b64encode(image()).decode() + "\n"
    )
    if args.audio:
        FIXTURES.joinpath("shapes.wav.base64").write_text(
            base64.b64encode(speech()).decode() + "\n"
        )


if __name__ == "__main__":
    main()
