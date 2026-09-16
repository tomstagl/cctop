#!/usr/bin/env bash
# Guards the checked-in function-hooks contract against drift: line 1 of
# plugin/.claude/types/claude-code.d.ts names the Claude Code version it was
# generated from (`// Written by Claude Code 2.1.269.`); this compares that
# against the installed `claude --version` and fails loudly on a mismatch,
# rather than letting the pane silently typecheck against a stale API.
set -euo pipefail
cd "$(dirname "$0")/.."

dts=plugin/.claude/types/claude-code.d.ts
dts_version=$(sed -n '1p' "$dts" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if [ -z "$dts_version" ]; then
  echo "check-plugin-types: could not read a version from line 1 of $dts" >&2
  exit 1
fi

claude_version=$(claude --version | grep -oE '^[0-9]+\.[0-9]+\.[0-9]+')
if [ -z "$claude_version" ]; then
  echo "check-plugin-types: could not read a version from 'claude --version'" >&2
  exit 1
fi

if [ "$dts_version" != "$claude_version" ]; then
  echo "check-plugin-types: $dts is written for Claude Code $dts_version, but this machine has $claude_version" >&2
  echo "run: CLAUDE_CODE_ENABLE_FUNCTION_HOOKS=1 claude -p '/plugin-types ./.claude/types'" >&2
  exit 1
fi

echo "check-plugin-types: $dts matches claude $claude_version"

# The binary's constants (src/harness_facts.rs) carry the Claude Code
# version they were read from; a newer claude means they are unverified.
facts=src/harness_facts.rs
facts_version=$(grep -oE 'READ_FROM: &str = "[0-9]+\.[0-9]+\.[0-9]+"' "$facts" | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')
if [ -n "$facts_version" ] && [ "$(printf '%s\n%s\n' "$facts_version" "$claude_version" | sort -V | tail -1)" != "$facts_version" ]; then
  echo "check-plugin-types: warning: $facts was read from Claude Code $facts_version, this machine has $claude_version — re-verify the constants (autocompact buffer, /usage weights, /context thresholds, the first-seen map, the teams module: docs/teams.md)" >&2
else
  echo "check-plugin-types: $facts read from claude $facts_version (installed $claude_version)"
fi
