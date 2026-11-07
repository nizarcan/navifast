# Gapless reference fixtures

These files contain an original synthetic 440 Hz sine wave, stereo PCM16 at
44,100 Hz, amplitude 4,000. One second is split after frame 12,347, away from a
zero crossing and codec/output block boundaries. They contain no recorded music.
The fixtures and generator are supplied under the repository's license.

`generate.py` produces two lossless FLAC files and two LAME 128 kb/s MP3 files.
The MP3 headers include LAME encoder delay and padding. Regenerate with
`uv run --with lameenc --with soundfile tests/fixtures/gapless/generate.py`.
Those packages are only generation tools. Tests use the checked-in small files
and require no encoder, Python package, network connection or audio device.

The player tests compare joined output with the continuous reference, including
EQ/limiter state, exact per-occurrence sample counts and metadata boundaries.
FLAC matches the original PCM exactly. MP3 removes its declared delay/padding,
retains the exact frame count, and is compared with its decoded reference because
lossy compression changes samples. Both also pass through rodio's 48 kHz stereo
output converter. WAV references are generated in Rust.

Validation platform: macOS arm64. The tests capture the software output path,
not a physical device loopback. Hardware drivers, Bluetooth, high-rate source
handoffs, and other codec/container combinations are not certified by these
fixtures. No bit-perfect output claim is made.
