#!/usr/bin/env python3
"""Anonymise a Claude Code transcript for use as a cctop fixture.

Keeps every structural property the parsers depend on (line types and order,
message ids and their repetition, usage, timestamps, durations, tool ids and
names, cost-state numbers) and replaces every human-authored or
project-identifying string with same-length filler so token estimates stay
representative.

usage: anonymise-transcript.py <in.jsonl> <out.jsonl> [--max-str N]
"""
import hashlib
import json
import sys

FILLER = ("lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod "
          "tempor incididunt ut labore et dolore magna aliqua ")
KEEP_KEYS = {"type", "subtype", "role", "model", "id", "name", "tool_use_id", "requestId",
             "timestamp", "version", "effort", "stop_reason", "stop_sequence", "level",
             "userType", "entrypoint", "isSidechain", "isMeta", "operation", "durationMs",
             "hookCount", "preventedContinuation", "hasOutput", "interrupted", "isImage",
             "noOutputExpected", "is_error", "permissionMode", "mode", "speed", "service_tier",
             "inference_geo", "status", "success", "totalCostUSD", "totalAPIDuration",
             "totalAPIDurationWithoutRetries", "totalToolDuration", "totalLinesAdded",
             "totalLinesRemoved", "totalDuration", "startTime", "lastSequenceNum"}
ID_KEYS = {"sessionId", "session_id", "uuid", "parentUuid", "promptId", "leafUuid", "messageId",
           "sourceToolAssistantUUID", "toolUseID", "bridgeSessionId", "ownerAccountUuid",
           "ownerOrganizationUuid", "agentId", "parentSessionId", "parentLastUuid"}
MAX_STR = 4000


def fill(n):
    return (FILLER * (n // len(FILLER) + 1))[:n]


def hid(s):
    h = hashlib.sha1(s.encode()).hexdigest()
    return f"{h[:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def anon(v, key=None):
    if isinstance(v, dict):
        return {k: anon(x, k) for k, x in v.items()}
    if isinstance(v, list):
        return [anon(x, key) for x in v]
    if isinstance(v, str):
        if key in KEEP_KEYS:
            return v
        if key in ID_KEYS:
            return hid(v)
        if key in ("cwd", "gitBranch"):
            return "/home/user/project" if key == "cwd" else "main"
        if key in ("file_path", "filePath", "path", "notebook_path", "file"):
            return "/home/user/project/src/" + hashlib.sha1(v.encode()).hexdigest()[:6] + ".rs"
        if key == "command":
            return "make check"
        if key in ("model", "usage"):
            return v
        return fill(min(len(v), MAX_STR))
    return v


def main():
    src, dst = sys.argv[1], sys.argv[2]
    with open(src) as f, open(dst, "w") as out:
        for line in f:
            try:
                o = json.loads(line)
            except json.JSONDecodeError:
                continue
            out.write(json.dumps(anon(o), ensure_ascii=False, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
