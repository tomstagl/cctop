#!/usr/bin/env python3
"""Does Claude Code's `cost-state` include subagent calls? Read-only; counts only.

For every session under `~/.claude/projects` that has a `cost-state` line and
at least one subagent transcript with API calls, compare the last
`cost-state.modelUsage[<model>]` token counts against (a) the main
transcript's usage, deduplicated by `message.id`, and (b) main + subagents,
and say which of the two the ledger matches within 1 %. A ledger *above*
both is the usual case: it also holds calls no transcript shows (haiku side
calls, retries). No text is read or printed — only numbers and ids' first
eight characters.

Findings on 2026-09-15 (79 sessions with a cost-state, 8 with subagent
usage, Claude Code 2.1.247 – 2.1.272): the ledger is never below
main + subagents, and matches it within 0.2 % on cache reads where the
subagents dominate (a 336-agent workflow run). Recorded in
`src/harness_facts.rs` (`cost_state::INCLUDES_SUBAGENTS`) and
`tasks/prd-cctop-agent-costs.md` §3.2.

usage: ledger-vs-agents.py [--root ~/.claude/projects]
"""
import collections
import glob
import json
import os
import sys

# ledger key in cost-state.modelUsage → API key in message.usage
FIELDS = [
    ("inputTokens", "input_tokens"),
    ("outputTokens", "output_tokens"),
    ("cacheReadInputTokens", "cache_read_input_tokens"),
    ("cacheCreationInputTokens", "cache_creation_input_tokens"),
]


def usage_by_model(path):
    """Per-model token sums, one usage per `message.id` (the last line of a
    message carries the complete count — subagent transcripts stream
    `output_tokens` across a message's lines)."""
    per_id, model_of = {}, {}
    with open(path, errors="replace") as f:
        for line in f:
            try:
                v = json.loads(line)
            except ValueError:
                continue
            if v.get("type") != "assistant":
                continue
            m = v.get("message") or {}
            mid, u = m.get("id"), m.get("usage")
            if not mid or not isinstance(u, dict):
                continue
            per_id[mid] = u
            model_of[mid] = m.get("model") or ""
    out = collections.defaultdict(collections.Counter)
    for mid, u in per_id.items():
        for ledger_key, api_key in FIELDS:
            out[model_of[mid]][ledger_key] += int(u.get(api_key) or 0)
    return out


def last_cost_state(path):
    last = None
    with open(path, errors="replace") as f:
        for line in f:
            if '"cost-state"' not in line:
                continue
            try:
                v = json.loads(line)
            except ValueError:
                continue
            if v.get("type") == "cost-state":
                last = v
    return last


def main(root):
    rows, sessions = [], 0
    for main_path in sorted(glob.glob(os.path.join(root, "*", "*.jsonl"))):
        session_dir = main_path[: -len(".jsonl")]
        agent_paths = glob.glob(
            os.path.join(session_dir, "subagents", "**", "agent-*.jsonl"), recursive=True
        )
        if not agent_paths:
            continue
        ledger = last_cost_state(main_path)
        if not ledger:
            continue
        agents = collections.defaultdict(collections.Counter)
        for p in agent_paths:
            for model, counts in usage_by_model(p).items():
                agents[model].update(counts)
        if not any(sum(c.values()) for c in agents.values()):
            continue
        sessions += 1
        main_usage = usage_by_model(main_path)
        for model, lu in (ledger.get("modelUsage") or {}).items():
            am = agents.get(model, collections.Counter())
            if sum(am.values()) == 0:
                continue  # no subagent usage on this model: uninformative
            mm = main_usage.get(model, collections.Counter())
            for key, _ in FIELDS:
                ledger_n = int(lu.get(key) or 0)
                if ledger_n == 0:
                    continue
                rows.append((os.path.basename(session_dir)[:8], model, key, ledger_n, mm[key], mm[key] + am[key]))

    print(f"sessions with a cost-state and subagent usage: {sessions}; rows: {len(rows)}")
    print(f"{'session':8} {'model':28} {'field':24} {'ledger':>12} {'main':>12} {'main+agents':>12} {'Δmain':>7} {'Δm+a':>7}  verdict")
    verdicts = collections.Counter()
    for s, model, key, ledger_n, main_n, both_n in rows:
        d_main = abs(ledger_n - main_n) / ledger_n
        d_both = abs(ledger_n - both_n) / ledger_n
        if d_both < 0.01:
            verdict = "main+agents"
        elif d_main < 0.01:
            verdict = "main"
        elif ledger_n > both_n:
            verdict = "above both (untranscribed calls)"
        else:
            verdict = "below both"
        verdicts[verdict] += 1
        print(f"{s:8} {model[:28]:28} {key:24} {ledger_n:12d} {main_n:12d} {both_n:12d} {d_main:7.1%} {d_both:7.1%}  {verdict}")
    print("verdicts:", dict(verdicts))
    return 0


if __name__ == "__main__":
    root = os.path.expanduser("~/.claude/projects")
    args = sys.argv[1:]
    if args[:1] == ["--root"] and len(args) == 2:
        root = os.path.expanduser(args[1])
    elif args:
        print(__doc__.strip().splitlines()[-1], file=sys.stderr)
        sys.exit(2)
    sys.exit(main(root))
