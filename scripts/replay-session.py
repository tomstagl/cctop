#!/usr/bin/env python3
"""Replay a fixture transcript into a file cctop can tail, on the real clock.

For recording the dashboard without a live Claude Code session: the lines
are appended one by one at the fixture's own pace (divided by --speed), every
timestamp rewritten so the line being written is "now". Run cctop against
the output in another pane with the fixture treated as live:

    scripts/replay-session.py fixtures/session-b.jsonl /tmp/replay.jsonl --from 600 --speed 40
    CCTOP_FIXTURE_LIVE=1 cctop run --session /tmp/replay.jsonl

Lines before --from are written at once as history, on real time by default
(--history-speed), so the elapsed clock, cost and velocity read like the
session they came from; the rest play at --speed. --max-gap caps a pause between two lines (a long think, an idle hour),
--min-gap keeps bursts readable. Ctrl-C stops; the output file is left.
"""
import argparse, json, re, sys, time, pathlib
from datetime import datetime, timezone

TS = re.compile(r'"timestamp":"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z)"')

def parse(ts):
    return datetime.strptime(ts.replace("Z", "+0000"), "%Y-%m-%dT%H:%M:%S.%f%z").timestamp() \
        if "." in ts else datetime.strptime(ts.replace("Z", "+0000"), "%Y-%m-%dT%H:%M:%S%z").timestamp()

def fmt(t):
    return datetime.fromtimestamp(t, timezone.utc).strftime("%Y-%m-%dT%H:%M:%S.") + f"{int((t % 1) * 1000):03d}Z"

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("fixture", type=pathlib.Path)
    ap.add_argument("out", type=pathlib.Path)
    ap.add_argument("--from", dest="start", type=int, default=1, help="first line (1-based) to play; earlier lines are history")
    ap.add_argument("--to", type=int, default=None, help="last line to play (default: the end)")
    ap.add_argument("--speed", type=float, default=30.0, help="fixture seconds per real second while playing")
    ap.add_argument("--history-speed", type=float, default=1.0,
                    help="the same for the history before --from (1: real time, so the elapsed clock reads like the session it was)")
    ap.add_argument("--max-gap", type=float, default=4.0, help="longest real pause between two lines, seconds")
    ap.add_argument("--min-gap", type=float, default=0.05, help="shortest real pause between two lines, seconds")
    ap.add_argument("--lead", type=float, default=0.0, help="seconds to wait before the first played line")
    a = ap.parse_args()

    lines = a.fixture.read_text().splitlines(keepends=True)
    stamps = [None] * len(lines)
    for i, l in enumerate(lines):
        m = TS.search(l)
        stamps[i] = parse(m.group(1)) if m else None
    known = [s for s in stamps if s is not None]
    if not known:
        sys.exit("no timestamps in the fixture")
    # A line without a timestamp inherits the previous one, for pacing.
    last = known[0]
    for i, s in enumerate(stamps):
        if s is None:
            stamps[i] = last
        else:
            last = s

    start = max(1, a.start) - 1
    end = min(len(lines), a.to) if a.to else len(lines)
    t0 = stamps[start]                 # fixture time of the first played line
    wall0 = time.time() + a.lead       # its wall-clock time

    def shift(line, fixture_t):
        speed = a.speed if fixture_t >= t0 else a.history_speed
        new = wall0 + (fixture_t - t0) / speed
        return TS.sub(lambda m: f'"timestamp":"{fmt(new + (parse(m.group(1)) - fixture_t))}"', line)

    with a.out.open("w") as f:
        for i in range(start):
            f.write(shift(lines[i], stamps[i]))
        f.flush()
        print(f"history: {start} lines · playing {start + 1}..{end} at {a.speed:g}x", file=sys.stderr)
        if a.lead:
            time.sleep(a.lead)
        prev = stamps[start]
        for i in range(start, end):
            gap = (stamps[i] - prev) / a.speed
            time.sleep(min(a.max_gap, max(a.min_gap, gap)))
            prev = stamps[i]
            f.write(shift(lines[i], stamps[i]))
            f.flush()
            try:
                kind = json.loads(lines[i]).get("type", "?")
            except ValueError:
                kind = "?"
            print(f"\r{i + 1}/{end} {kind:<22}", end="", file=sys.stderr)
        print(file=sys.stderr)

if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print(file=sys.stderr)
