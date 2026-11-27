#!/usr/bin/env bash
# Tests scripts/extract-dts.py against the installed `claude`: line 1 names the
# installed version, line 2 is the header comment, the file is at least 1000
# lines with exactly one trailing newline, and a non-binary input exits 1.
# Skipped (exit 0) when `claude` or `zstd` is absent.
set -euo pipefail
cd "$(dirname "$0")/.."

if ! command -v claude >/dev/null || ! command -v zstd >/dev/null; then
  echo "test-extract-dts: skipped (no claude|zstd)"
  exit 0
fi

out=$(mktemp)
trap 'rm -f "$out"' EXIT
version=$(claude --version | grep -oE '^[0-9]+\.[0-9]+\.[0-9]+')

python3 -I scripts/extract-dts.py "$(command -v claude)" "$out"

fail() { echo "test-extract-dts: FAIL: $1" >&2; exit 1; }

[ "$(sed -n 1p "$out")" = "// Written by Claude Code $version." ] || fail "line 1 is not the version stamp"
[ "$(sed -n 2p "$out")" = "// Claude Code function hooks: the plugin API's TypeScript declarations." ] || fail "line 2 is not the header comment"
[ "$(wc -l < "$out")" -ge 1000 ] || fail "fewer than 1000 lines"
[ "$(tail -c 2 "$out" | od -An -c | tr -d ' ')" != '\n\n' ] || fail "more than one trailing newline"
[ "$(tail -c 1 "$out" | od -An -c | tr -d ' ')" = '\n' ] || fail "no trailing newline"

if python3 -I scripts/extract-dts.py /etc/hosts "$out.bad" 2>/dev/null; then
  fail "a non-binary input exited 0"
fi
rm -f "$out.bad"
echo "test-extract-dts: ok ($version)"
