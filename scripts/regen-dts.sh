#!/usr/bin/env bash
# Regenerate plugin/.claude/types/claude-code.d.ts, the full function-hooks
# declarations (surface plus the built-in tool-input tables), from the
# installed `claude`.
#
# Makes one model call (`claude -p --max-turns 1`) and needs a logged-in
# `claude`: invoking the bundled /plugin-authoring skill is what makes Claude
# Code write the file, into a temp CLAUDE_CODE_TMPDIR that this script then reads and
# removes. scripts/extract-dts.py is the credential-free route for the surface
# alone, without the tool tables.
#
# Exit 0 with a one-line summary, or exit 1 naming the path it searched.
set -euo pipefail

root=$(cd "$(dirname "$0")/.." && pwd)
dest="$root/plugin/.claude/types/claude-code.d.ts"

T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT

v=$(claude --version | awk '{print $1}')

CLAUDE_CODE_TMPDIR=$T CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 \
  claude -p --max-turns 1 "/plugin-authoring reply with ok and do nothing else" </dev/null >/dev/null || true

dir="$T/claude-*/bundled-skills/$v/*/plugin-authoring/types"
# shellcheck disable=SC2012,SC2086
src=$(ls -t $dir/claude-code.d.ts 2>/dev/null | head -n 1 || true)
if [ -z "$src" ]; then
  echo "regen-dts: no claude-code.d.ts under $dir/ — is claude logged in?" >&2
  exit 1
fi

line1=$(head -n 1 "$src")
if [ "$line1" != "// Written by Claude Code $v." ]; then
  echo "regen-dts: line 1 names ${line1#// Written by Claude Code }, claude is $v" >&2
  exit 1
fi

cp "$src" "$dest"
echo "regen-dts: wrote ${dest#"$root"/} ($(wc -l <"$dest" | tr -d ' ') lines, $v)"
