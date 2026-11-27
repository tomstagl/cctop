#!/usr/bin/env bash
# Regenerate the pane test fixtures: one JSON file per `cctop query` verb the
# pane polls, taken from an anonymised fixture session. `usage.json` in the
# same directory is hand-written (the engine's SessionUsage shape, which the
# binary never produces) and is left alone.
#
#   scripts/pane-fixtures.sh                      # fixture A → tests/pane/fixtures/<verb>.json
#   scripts/pane-fixtures.sh fixtures/session-b.jsonl b   # → <verb>-b.json
#   scripts/pane-fixtures.sh fixtures/session-d.jsonl d   # → <verb>-d.json (a team; the coach and advice carry nudges)
#   scripts/pane-fixtures.sh fixtures/session-w.jsonl w   # → <verb>-w.json (a completed workflow run)
#   CCTOP_FAKE_NOW=$(cat fixtures/session-w-live.now) \
#     scripts/pane-fixtures.sh fixtures/session-w-live.jsonl wlive   # → <verb>-wlive.json
#
# Fixture W's live cut is read at its own clock (`session-w-live.now`, the
# one the TUI's fixture-W snapshots use), so its dashboard carries the
# header's workflow strip; every verb inherits CCTOP_FAKE_NOW.
#
# The coach object is also emitted at fixture B's six moments
# (`coach-<moment>.json`): a prefix of the transcript (`--lines`) and, for the
# idle moments, a clock four minutes past the last fed line (`CCTOP_FAKE_NOW`).
set -euo pipefail
cd "$(dirname "$0")/.."
session=${1:-fixtures/session-a.jsonl}
suffix=${2:+-$2}
out=tests/pane/fixtures
mkdir -p "$out"
for verb in summary dashboard overview tools files agents advice coach events; do
  cargo run -q -- query "$verb" --session "$session" > "$out/$verb$suffix.json"
  echo "$out/$verb$suffix.json"
done

# Epoch ms of the timestamp on line $2 of file $1, plus $3 seconds.
clock_after() {
  python3 - "$1" "$2" "$3" <<'EOF'
import json, sys
from datetime import datetime
path, n, plus = sys.argv[1], int(sys.argv[2]), int(sys.argv[3])
with open(path) as f:
    for i, line in enumerate(f, 1):
        if i == n:
            ts = json.loads(line)["timestamp"]
            t = datetime.fromisoformat(ts.replace("Z", "+00:00"))
            print(int(t.timestamp() * 1000) + plus * 1000)
            break
EOF
}

# The overview's charts as Rust draws them (`fallback_cells`: the cells of
# cctop_charts::render_cells at the spec's default size, in the Raster's
# encoding), with the specs, for fixture B: tests/pane compares the wasm's
# Raster cells for the same specs and sizes against these (row identity with
# the TUI, which draws the same render_cells).
if [[ "$session" == fixtures/session-b.jsonl ]]; then
  cargo run -q -- query overview --session "$session" | python3 -c '
import json, sys
o = json.load(sys.stdin)
out = []
for c in o["charts"]:
    r = c["fallback_cells"]
    out.append({"name": c["id"], "spec": c["spec"], "cols": r["cols"], "rows": r["rows"], "cells": r["cells"]})
    if "bar" in c:
        r = c["bar_fallback_cells"]
        out.append({"name": c["id"] + ".bar", "spec": c["bar"], "cols": r["cols"], "rows": r["rows"], "cells": r["cells"]})
json.dump(out, sys.stdout, indent=1)
print()
' > "$out/overview-cells-b.json"
  echo "$out/overview-cells-b.json"
fi

# The overview object at length (`overview<suffix>.json` above is each
# fixture's at the binary's default): a synthetic 300-turn session at the turns that fit a
# 105-column pane body (two cells a turn: `--max-turns 18`), for the "last N
# turns" form. The synthetic transcript is generated here and thrown away: a
# prompt, one Bash call and one answer per turn, the shapes the Rust tests
# build (src/advisor/rules/mod.rs `fixtures`).
if [[ "$session" == fixtures/session-b.jsonl ]]; then
  tmp=$(mktemp -d)
  long="$tmp/session-long.jsonl"
  python3 - "$long" <<'EOF'
import json, sys
with open(sys.argv[1], "w") as f:
    for i in range(300):
        h, m = 10 + i // 60, i % 60
        ts = lambda s: f"2026-01-01T{h:02}:{m:02}:{s:02}.000Z"
        line = lambda o: f.write(json.dumps(o) + "\n")
        line({"type": "user", "timestamp": ts(0), "promptSource": "typed", "message": {"role": "user", "content": "go"}})
        line({"type": "assistant", "timestamp": ts(10), "message": {"id": f"t{i}", "model": "claude-opus-5", "content": [{"type": "tool_use", "id": f"t{i}", "name": "Bash", "input": {"command": "ls"}}], "usage": {"output_tokens": 1}}})
        line({"type": "user", "timestamp": ts(10), "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": f"t{i}", "content": "x" * 200}]}})
        line({"type": "assistant", "timestamp": ts(20), "message": {"id": f"r{i}", "model": "claude-opus-5", "content": [{"type": "text", "text": "ok"}], "stop_reason": "end_turn", "usage": {"input_tokens": 5, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 20000 + i * 500, "output_tokens": 10}}})
EOF
  cargo run -q -- query overview --session "$long" --max-turns 18 > "$out/overview-long.json"
  rm -f "$long"
  rmdir "$tmp"
  echo "$out/overview-long.json"
fi

# The six moments of fixture B: name, line count, seconds after that line.
if [[ "$session" == fixtures/session-b.jsonl ]]; then
  for moment in explore:214:0 edits:300:0 denials:761:0 waiting:788:240 cold:789:0 idle:738:240; do
    IFS=: read -r name lines plus <<< "$moment"
    if [[ "$plus" -gt 0 ]]; then
      CCTOP_FAKE_NOW=$(clock_after "$session" "$lines" "$plus") \
        cargo run -q -- query coach --session "$session" --lines "$lines" > "$out/coach-$name.json"
    else
      cargo run -q -- query coach --session "$session" --lines "$lines" > "$out/coach-$name.json"
    fi
    echo "$out/coach-$name.json"
  done
fi
