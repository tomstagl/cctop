#!/usr/bin/env python3
"""Extract the function-hooks d.ts core from a Claude Code binary, no login needed.

Usage: python3 -I scripts/extract-dts.py <claude-binary> <out.d.ts>

The Bun-compiled `claude` executable embeds the plugin API declarations as a
zstd frame. This scans the binary for zstd frame magic (from the end, where
the frame sits), decompresses each candidate with the `zstd` CLI and writes
the first output that starts with the declarations header, preceded by
`// Written by Claude Code <version>.`. The result is the surface only; the
tool-input tables come from the full file, which scripts/regen-dts.sh makes
with one model call and a logged-in `claude`.

Exit 0 and the file written, or exit 1 with a message on stderr.
"""
import mmap
import os
import re
import shutil
import subprocess
import sys

MAGIC = b"\x28\xb5\x2f\xfd"
HEADER = b"// Claude Code function hooks: the plugin API's TypeScript declarations."
CHUNK = 3 * 1024 * 1024
MIN_LINES = 1000


def fail(msg):
    sys.stderr.write("extract-dts: %s\n" % msg)
    sys.exit(1)


def offsets(mm):
    found = []
    pos = mm.find(MAGIC)
    while pos != -1:
        found.append(pos)
        pos = mm.find(MAGIC, pos + 1)
    return found


def decode(chunk):
    try:
        done = subprocess.run(
            ["zstd", "-dc", "--no-check"],
            input=chunk,
            capture_output=True,
            timeout=10,
        )
    except subprocess.TimeoutExpired:
        return b""
    # A frame cut at CHUNK still decodes its prefix, so the exit code is ignored.
    return done.stdout


def find_declarations(path):
    with open(path, "rb") as f:
        try:
            mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
        except ValueError:  # empty file
            return None
        try:
            for off in reversed(offsets(mm)):
                out = decode(mm[off:off + CHUNK])
                if out.startswith(HEADER):
                    return out
        finally:
            mm.close()
    return None


def main():
    if len(sys.argv) != 3:
        fail("usage: extract-dts.py <claude-binary> <out.d.ts>")
    if shutil.which("zstd") is None:
        fail("zstd not found on PATH (brew install zstd / apt-get install zstd)")
    binary = os.path.realpath(sys.argv[1])
    dest = sys.argv[2]
    if not os.path.isfile(binary):
        fail("%s is not a file" % binary)

    raw = find_declarations(binary)
    if raw is None:
        fail("no zstd frame in %s holds the function-hooks declarations" % binary)
    text = raw.decode("utf-8").rstrip("\n")
    lines = text.count("\n") + 1
    if lines < MIN_LINES:
        fail("the match is only %d lines" % lines)

    try:
        ver = subprocess.run([binary, "--version"], capture_output=True, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as e:
        fail("could not run %s --version: %s" % (binary, e))
    m = re.search(r"\d+\.\d+\.\d+", ver.stdout.decode("utf-8", "replace"))
    if not m:
        fail("no version in the output of %s --version" % binary)

    with open(dest, "w", encoding="utf-8") as f:
        f.write("// Written by Claude Code %s.\n%s\n" % (m.group(0), text))


if __name__ == "__main__":
    main()
