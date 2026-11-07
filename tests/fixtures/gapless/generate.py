"""Regenerate synthetic fixtures: uv run --with lameenc --with soundfile generate.py.

LAME is only a fixture-generation tool, never an application or test dependency.
"""

import ctypes as c
import math
from pathlib import Path
import struct
import numpy as np
import soundfile
import lameenc

ROOT = Path(__file__).parent
RATE, SPLIT = 44100, 12347
pcm = b"".join(
    struct.pack("<hh", value, value)
    for i in range(RATE)
    for value in [int(math.sin(i * 440 * math.tau / RATE) * 4000)]
)
lib = c.CDLL(lameenc.__file__)
lib.lame_init.restype = c.c_void_p
for name in [
    "lame_set_in_samplerate",
    "lame_set_num_channels",
    "lame_set_brate",
    "lame_set_quality",
    "lame_set_bWriteVbrTag",
]:
    getattr(lib, name).argtypes = [c.c_void_p, c.c_int]
lib.lame_init_params.argtypes = [c.c_void_p]
lib.lame_encode_buffer_interleaved.argtypes = [
    c.c_void_p,
    c.POINTER(c.c_short),
    c.c_int,
    c.c_void_p,
    c.c_int,
]
lib.lame_encode_flush.argtypes = [c.c_void_p, c.c_void_p, c.c_int]
lib.lame_get_lametag_frame.argtypes = [c.c_void_p, c.c_void_p, c.c_size_t]
lib.lame_get_lametag_frame.restype = c.c_size_t
lib.lame_close.argtypes = [c.c_void_p]


def mp3(data):
    encoder = lib.lame_init()
    try:
        for name, value in [
            ("in_samplerate", RATE),
            ("num_channels", 2),
            ("brate", 128),
            ("quality", 2),
            ("bWriteVbrTag", 1),
        ]:
            assert getattr(lib, "lame_set_" + name)(encoder, value) == 0
        assert lib.lame_init_params(encoder) == 0
        samples = (c.c_short * (len(data) // 2)).from_buffer_copy(data)
        output = c.create_string_buffer(len(data) * 2 + 7200)
        size = lib.lame_encode_buffer_interleaved(
            encoder, samples, len(data) // 4, output, len(output)
        )
        assert size >= 0
        encoded = bytearray(output.raw[:size])
        size = lib.lame_encode_flush(encoder, output, len(output))
        assert size >= 0
        encoded.extend(output.raw[:size])
        size = lib.lame_get_lametag_frame(encoder, output, len(output))
        assert 0 < size < len(encoded)
        encoded[:size] = output.raw[:size]
        return encoded
    finally:
        lib.lame_close(encoder)


for name, data in [("first", pcm[: SPLIT * 4]), ("second", pcm[SPLIT * 4 :])]:
    (ROOT / (name + ".mp3")).write_bytes(mp3(data))
    soundfile.write(
        ROOT / (name + ".flac"),
        np.frombuffer(data, dtype="<i2").reshape(-1, 2),
        RATE,
        subtype="PCM_16",
    )
