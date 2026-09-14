#!/usr/bin/env python3
"""Compose a fixture transcript from real sessions, then anonymise it.

`fixtures/session-b.jsonl` must contain every moment the coach PRD lists
(US-001), and no single real session does. This script takes one real
session as the spine and splices real line segments from other sessions
for the missing moments — the segments are copied verbatim (structure,
usage, ids, timestamps' internal spacing), then re-chained: each segment's
first `parentUuid` points at the line before it, its `sessionId`/`cwd`
become the spine's, its timestamps are shifted to follow the spine, and a
tool call spliced into the spine's last turn takes that turn's `promptId`.
Nothing is hand-written; run the anonymiser on the result.

Segments are found by predicate, never by id, so the recipe carries no
identifiers:

  --denials    the first tool call + result per `toolDenialKind` value
  --task       a `TaskUpdate` to completed and its result
  --synthetic  an `isApiErrorMessage` / `<synthetic>` assistant line
  --interrupt  the response an interrupt cut, the interrupt line, the re-prompt
  --compaction the `compact_boundary` line, the `isCompactSummary` line and
               the re-injected attachments up to the next assistant line
  --ask        an `AskUserQuestion` call whose answer arrived > 1 h later
  --context    a `/context` command line and the `local_command` line that
               captured its table
  --gitop      a Bash commit whose result carries `gitOperation.commit`
               (and a push / PR when the same session has them)
  --continued  a `continued-in` line (what `/clear` leaves behind)

usage: compose-fixture.py <spine.jsonl> --denials F --task F --synthetic F
                          --interrupt F --compaction F --ask F --context F
                          --gitop F --continued F <out.jsonl>
"""
import datetime as dt
import json
import sys

GAP_S = 45  # seconds between the spine's last line and the first splice


def load(path):
    out = []
    with open(path) as f:
        for line in f:
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return out


def ts(o):
    t = o.get("timestamp")
    if not t:
        return None
    return dt.datetime.fromisoformat(t.replace("Z", "+00:00"))


def fmt(t):
    return t.strftime("%Y-%m-%dT%H:%M:%S.") + f"{t.microsecond // 1000:03d}Z"


def text_of(o):
    c = (o.get("message") or {}).get("content")
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        return "".join(b.get("text", "") for b in c if isinstance(b, dict) and b.get("type") == "text")
    return ""


def has_tool_result(o):
    c = (o.get("message") or {}).get("content")
    return isinstance(c, list) and any(isinstance(b, dict) and b.get("type") == "tool_result" for b in c)


def tool_uses(o):
    if o.get("type") != "assistant":
        return []
    return [b for b in (o.get("message") or {}).get("content") or [] if isinstance(b, dict) and b.get("type") == "tool_use"]


def by_uuid(lines):
    return {o.get("uuid"): i for i, o in enumerate(lines) if o.get("uuid")}


def seg_denials(lines):
    idx = by_uuid(lines)
    seen = set()
    out = []
    for o in lines:
        k = o.get("toolDenialKind")
        if not k or k in seen:
            continue
        parent = idx.get(o.get("parentUuid"))
        if parent is None:
            continue
        seen.add(k)
        out.append([lines[parent], o])
    return out


def seg_task(lines):
    idx = by_uuid(lines)
    for i, o in enumerate(lines):
        for b in tool_uses(o):
            if b.get("name") == "TaskUpdate" and (b.get("input") or {}).get("status") == "completed":
                for r in lines[i + 1 : i + 6]:
                    if r.get("type") == "user" and idx.get(r.get("parentUuid")) == i:
                        return [o, r]
    return []


def seg_synthetic(lines):
    for o in lines:
        if o.get("type") == "assistant" and (o.get("isApiErrorMessage") or (o.get("message") or {}).get("model") == "<synthetic>"):
            return [o]
    return []


def seg_interrupt(lines):
    for i, o in enumerate(lines):
        if o.get("type") == "user" and o.get("interruptedMessageId"):
            mid = o["interruptedMessageId"]
            cut = [x for x in lines[:i] if x.get("type") == "assistant" and (x.get("message") or {}).get("id") == mid]
            follow = []
            for r in lines[i + 1 : i + 12]:
                if r.get("type") == "user" and o.get("promptSource") != "system" and not has_tool_result(r) and not r.get("isMeta"):
                    follow = [r]
                    break
            return cut + [o] + follow
    return []


def seg_compaction(lines):
    for i, o in enumerate(lines):
        if o.get("type") == "system" and o.get("subtype") == "compact_boundary":
            out = [o]
            for r in lines[i + 1 :]:
                out.append(r)
                if r.get("type") == "assistant":
                    break
            return out
    return []


def seg_ask(lines):
    idx = by_uuid(lines)
    for i, o in enumerate(lines):
        for b in tool_uses(o):
            if b.get("name") != "AskUserQuestion":
                continue
            for r in lines[i + 1 : i + 40]:
                if r.get("type") == "user" and has_tool_result(r) and idx.get(r.get("parentUuid")) == i:
                    a, b_ = ts(o), ts(r)
                    if a and b_ and (b_ - a).total_seconds() > 3600:
                        return [o, r]
                    break
    return []


def seg_context(lines):
    """The `/context` command as Claude Code records it: a `local_command`
    system line naming the command, then one with its stdout."""
    for i, o in enumerate(lines):
        if o.get("type") == "system" and o.get("subtype") == "local_command" and (o.get("content") or "").startswith("<command-name>/context"):
            out = [o]
            for r in lines[i + 1 : i + 6]:
                if r.get("type") == "system" and r.get("subtype") == "local_command":
                    out.append(r)
                    if "Context Usage" in (r.get("content") or ""):
                        return out
            return []
    return []


def seg_gitop(lines):
    """One tool call + result per `gitOperation` key (commit, push, pr,
    branch), the first of each."""
    idx = by_uuid(lines)
    seen = set()
    out = []
    for o in lines:
        tur = o.get("toolUseResult")
        if not (isinstance(tur, dict) and isinstance(tur.get("gitOperation"), dict)):
            continue
        keys = tuple(sorted(tur["gitOperation"].keys()))
        if keys in seen:
            continue
        parent = idx.get(o.get("parentUuid"))
        if parent is None:
            continue
        seen.add(keys)
        out.append([lines[parent], o])
    return out


def seg_continued(lines):
    return [o for o in lines if o.get("type") == "continued-in"][:1]


def splice(spine, segments, session_id, cwd):
    """Append segments after the spine's last timestamped line, before its
    `cost-state`, shifting each segment so it starts GAP_S after the line
    before it and keeps its internal spacing."""
    tail = []
    while spine and spine[-1].get("type") == "cost-state":
        tail.insert(0, spine.pop())
    last_ts = max(t for t in (ts(o) for o in spine) if t)
    last_uuid = next(o["uuid"] for o in reversed(spine) if o.get("uuid"))
    # promptId of the spine's last human prompt: spliced tool calls join it.
    last_prompt_id = next(
        (o.get("promptId") for o in reversed(spine) if o.get("type") == "user" and o.get("promptId") and not has_tool_result(o)),
        None,
    )
    out = list(spine)
    for seg in segments:
        seg = [json.loads(json.dumps(o)) for o in seg]  # deep copy
        first_ts = next((ts(o) for o in seg if ts(o)), None)
        base = last_ts + dt.timedelta(seconds=GAP_S)
        starts_turn = any(o.get("type") == "user" and not has_tool_result(o) and not o.get("isMeta") for o in seg)
        for j, o in enumerate(seg):
            if ts(o) and first_ts:
                o["timestamp"] = fmt(base + (ts(o) - first_ts))
            for k in ("sessionId", "session_id"):
                if k in o:
                    o[k] = session_id
            if "cwd" in o:
                o["cwd"] = cwd
            if j == 0 and "parentUuid" in o:
                o["parentUuid"] = last_uuid
            if not starts_turn and "promptId" in o and last_prompt_id:
                o["promptId"] = last_prompt_id
        for o in seg:
            if o.get("uuid"):
                last_uuid = o["uuid"]
            if ts(o):
                last_ts = max(last_ts, ts(o))
            if o.get("type") == "user" and o.get("promptId") and not has_tool_result(o):
                last_prompt_id = o["promptId"]
        out.extend(seg)
    return out + tail


def main():
    args = sys.argv[1:]
    opts = {}
    positional = []
    i = 0
    while i < len(args):
        if args[i].startswith("--"):
            opts[args[i][2:]] = args[i + 1]
            i += 2
        else:
            positional.append(args[i])
            i += 1
    spine_path, out_path = positional
    spine = load(spine_path)
    session_id = next(o["sessionId"] for o in spine if o.get("sessionId"))
    cwd = next(o["cwd"] for o in spine if o.get("cwd"))
    segments = []
    finders = [
        ("denials", lambda ls: seg_denials(ls)),
        ("task", lambda ls: [seg_task(ls)]),
        ("synthetic", lambda ls: [seg_synthetic(ls)]),
        ("interrupt", lambda ls: [seg_interrupt(ls)]),
        ("compaction", lambda ls: [seg_compaction(ls)]),
        ("ask", lambda ls: [seg_ask(ls)]),
        ("context", lambda ls: [seg_context(ls)]),
        ("gitop", lambda ls: seg_gitop(ls)),
        ("continued", lambda ls: [seg_continued(ls)]),
    ]
    for name, find in finders:
        if name not in opts:
            continue
        found = [s for s in find(load(opts[name])) if s]
        if not found:
            sys.exit(f"compose-fixture: no {name} segment in {opts[name]}")
        print(f"{name}: {sum(len(s) for s in found)} lines from {len(found)} segment(s)", file=sys.stderr)
        segments.extend(found)
    out = splice(spine, segments, session_id, cwd)
    with open(out_path, "w") as f:
        for o in out:
            f.write(json.dumps(o, ensure_ascii=False, separators=(",", ":")) + "\n")
    print(f"{len(out)} lines → {out_path}", file=sys.stderr)


if __name__ == "__main__":
    main()
