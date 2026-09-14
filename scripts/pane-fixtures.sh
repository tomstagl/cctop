#!/usr/bin/env bash
# Regenerate the pane test fixtures: one JSON file per `cctop query` verb the
# pane polls, taken from an anonymised fixture session. `usage.json` in the
# same directory is hand-written (the engine's SessionUsage shape, which the
# binary never produces) and is left alone.
#
#   scripts/pane-fixtures.sh                      # fixture A → tests/pane/fixtures/<verb>.json
#   scripts/pane-fixtures.sh fixtures/session-b.jsonl b   # → <verb>-b.json
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
for verb in summary dashboard tools files agents advice coach events; do
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
