#!/usr/bin/env bash
# Regenerate the pane test fixtures: one JSON file per `cctop query` verb the
# pane polls, taken from the anonymised fixture session. `usage.json` in the
# same directory is hand-written (the engine's SessionUsage shape, which the
# binary never produces) and is left alone.
set -euo pipefail
cd "$(dirname "$0")/.."
out=tests/pane/fixtures
mkdir -p "$out"
for verb in summary tools files agents advice events; do
  cargo run -q -- query "$verb" --session fixtures/session-a.jsonl > "$out/$verb.json"
  echo "$out/$verb.json"
done
