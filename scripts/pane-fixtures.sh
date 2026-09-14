#!/usr/bin/env bash
# Regenerate the pane test fixtures: one JSON file per `cctop query` verb the
# pane polls, taken from an anonymised fixture session. `usage.json` in the
# same directory is hand-written (the engine's SessionUsage shape, which the
# binary never produces) and is left alone.
#
#   scripts/pane-fixtures.sh                      # fixture A → tests/pane/fixtures/<verb>.json
#   scripts/pane-fixtures.sh fixtures/session-b.jsonl b   # → <verb>-b.json
set -euo pipefail
cd "$(dirname "$0")/.."
session=${1:-fixtures/session-a.jsonl}
suffix=${2:+-$2}
out=tests/pane/fixtures
mkdir -p "$out"
for verb in summary tools files agents advice events; do
  cargo run -q -- query "$verb" --session "$session" > "$out/$verb$suffix.json"
  echo "$out/$verb$suffix.json"
done
