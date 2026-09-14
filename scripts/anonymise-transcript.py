#!/usr/bin/env python3
"""Anonymise a Claude Code transcript for use as a cctop fixture.

Keeps every structural property the parsers depend on (line types and order,
message ids and their repetition, usage, timestamps, durations, tool ids and
names, cost-state numbers, the enum-valued keys such as promptSource /
toolDenialKind / cache_miss_reason.type / gitOperation) and replaces every
human-authored or project-identifying string with same-length filler so token
estimates stay representative.

Shapes the parsers classify are preserved without their content:

- Bash commands keep their command words, flags, operators and file
  extensions (`cargo test`, `git commit -m "…"`, `cat src/x.rs | head`);
  every other token becomes filler of the same length.
- File paths become `/home/user/project/src/<hash>.<ext>` (the extension is
  kept: the doc-vs-source distinction matters to the rules).
- Bash stdout keeps test-runner summary lines (`test result: ok. 12 passed`),
  and nothing else.
- User text keeps the markers that identify a line's kind
  (`[Request interrupted by user]`, `<command-name>/clear</command-name>`,
  `<task-notification>`, …) and the verbatim `/context` table.

usage: anonymise-transcript.py <in.jsonl> <out.jsonl> [--max-str N]
"""
import hashlib
import json
import os
import re
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
             "totalLinesRemoved", "totalDuration", "startTime", "lastSequenceNum",
             # enum-valued keys the coach parsers read (values are Claude Code's own words)
             "promptSource", "toolDenialKind", "kind", "trigger", "apiErrorStatus", "error",
             "rateLimitType", "overageStatus", "lowPriorityOffer", "commandMode",
             "returnCodeInterpretation", "hookEvent", "hookName", "reminderType",
             "perTurnEffort", "attributionSkill", "attributionPlugin", "attributionAgent",
             "attributionMcpServer", "attributionMcpTool", "agent_type", "subagent_type",
             "resolvedModel", "action", "sha", "sessionKind", "notification_type",
             "commandName", "skill", "statusChange", "from", "to", "agentSetting",
             "contentType", "met", "planExists", "bashFirst", "steerOnly", "bypass",
             "isInitial", "changed", "itemCount", "skillCount", "exitCode", "truncatedByTokenCap",
             "numLines", "startLine", "totalLines", "userModified", "staleRecovered",
             "replaceAll", "created", "moreFiles", "unavailable", "shared", "isAsync",
             "canReadOutputFile", "plan_mode_required", "is_splitpane", "isCompactSummary",
             "isVisibleInTranscriptOnly", "turnCompanion", "queueSkipAttachments",
             "isSnapshotUpdate", "prNumber", "messageCount", "pendingBackgroundAgentCount",
             "pendingWorkflowCount", "preTokens", "postTokens", "cumulativeDroppedTokens",
             "isUsingOverage", "unifiedRateLimitFallbackAvailable", "lowPriorityRetryAfterSeconds",
             "lowPriorityMaxWaitSeconds", "resetsAt", "warm", "ttl", "iterations", "tokens"}
ID_KEYS = {"sessionId", "session_id", "uuid", "parentUuid", "logicalParentUuid", "promptId",
           "leafUuid", "messageId", "snapshotMessageId", "sourceToolAssistantUUID", "toolUseID",
           "sourceToolUseID", "interruptedMessageId", "bridgeSessionId", "ownerAccountUuid",
           "ownerOrganizationUuid", "agentId", "agent_id", "parentSessionId", "parentLastUuid",
           "continuedInSessionId", "source_uuid", "headUuid", "anchorUuid", "tailUuid", "taskId",
           "prompt_id"}
PATH_KEYS = {"file_path", "filePath", "path", "notebook_path", "file", "filename", "trackingPath",
             "changedFiles", "displayPath", "planFilePath", "realParentDir", "outputFile",
             "persistedOutputPath", "transcriptDir", "scriptPath", "agent_transcript_path"}
# Command words, subcommands and operators a Bash command keeps.
CMD_WORDS = {"cargo", "test", "build", "check", "clippy", "fmt", "run", "bench", "doc", "add",
             "git", "commit", "push", "pull", "tag", "merge", "rebase", "status", "diff", "log",
             "show", "branch", "remote", "stash", "list", "blame", "describe", "rev-parse",
             "ls-files", "checkout", "restore", "apply", "worktree", "switch", "reset", "clean",
             "reflog", "gh", "pr", "create", "view", "release", "api", "issue", "workflow", "auth",
             "repo", "npm", "npx", "pnpm", "yarn", "bun", "ci", "start", "install", "uninstall",
             "make", "pytest", "python3", "python", "node", "go", "vet", "tsc", "vitest", "jest",
             "eslint", "ruff", "mypy", "cat", "ls", "rg", "grep", "egrep", "find", "head", "tail",
             "sed", "awk", "wc", "sort", "uniq", "tr", "cut", "xargs", "tree", "echo", "printf",
             "tee", "mv", "cp", "mkdir", "rm", "touch", "chmod", "ln", "patch", "sleep", "wait",
             "until", "while", "do", "done", "for", "in", "if", "then", "else", "fi", "true",
             "false", "cd", "export", "env", "curl", "wget", "ssh", "scp", "rsync", "docker",
             "kubectl", "aws", "terraform", "pkill", "kill", "open", "which", "command", "type",
             "time", "date", "brew", "cctop", "claude", "query", "split", "install", "hook",
             "metrics", "advise", "report", "export", "plugin", "validate", "-p", "--version",
             "diff", "jq", "yq", "perl", "shellcheck", "lighthouse", "localhost", "127.0.0.1",
             "EOF", "PY", "SH", "END", "HEREDOC", "&&", "||", "|", ";", ">", ">>", "<", "<<",
             "2>&1", "2>/dev/null", "/dev/null", "-", "--", "-n", "-i", "-r", "-l", "-a", "-la",
             "-C", "-m", "-am", "-q", "-v", "-e", "-c", "-f", "-rf", "-p", "-s", "-t", "-1", "-0",
             "--release", "--all", "--all-targets", "--", "-D", "warnings", "--check", "--json",
             "--once", "--session", "--size", "--keys", "--no-pager", "--oneline", "--stat",
             "--numstat", "--cached", "--hard", "--soft", "--force", "-u", "-b", "-d", "-x", "-h",
             "--help", "-A", "-B", "-E", "-F", "-o", "--include", "--exclude", "--type", "--glob",
             "--count", "--files", "--max-count", "-w", "-z", "-S", "-G", "--name-only", "--short",
             "-sb", "-sS", "-o", "-fsSL", "--noEmit", "--strict", "-k", "-x", "--tb=short"}
# Test-runner summary lines only (no test names, no paths): what the phase
# classifier confirms a test run with.
TEST_SUMMARY = re.compile(
    r"^(test result: (ok|FAILED)\. \d+ passed; \d+ failed; \d+ ignored; \d+ measured; \d+ filtered out(; finished in [\d.]+s)?|"
    r"\d+ passed(, \d+ failed)?( in [\d.]+s)?|\d+ failed, \d+ passed( in [\d.]+s)?|"
    r"# (tests|pass|fail|suites|cancelled|skipped|todo) \d+|"
    r"running \d+ tests?|Tests:\s+\d+ (passed|failed).*|Test Suites: .*|"
    r"[✔✓] Validation passed|[✖✗] Validation failed|"
    r"\s*Finished `?\w+`? profile.*|FAILED|PASSED|\s*ok)$")
DIAG_PREFIX = re.compile(r"^(error(\[E\d+\])?:|warning:|thread '[^']*' panicked)")
MARKERS = ("[Request interrupted by user for tool use]", "[Request interrupted by user]")
TAGS = ("<task-notification>", "<teammate-message", "<bash-input>", "<bash-stdout>", "<bash-stderr>",
        "<ide_opened_file>", "<system-reminder>", "<local-command-caveat>", "<command-message>",
        "<command-args>", "<ide_selection>")
MAX_STR = 4000


def fill(n):
    return (FILLER * (n // len(FILLER) + 1))[:n]


def hid(s):
    h = hashlib.sha1(s.encode()).hexdigest()
    return f"{h[:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def anon_path(p):
    base = os.path.basename(p)
    ext = ""
    if "." in base and not base.startswith("."):
        ext = "." + base.rsplit(".", 1)[1][:8]
    elif base.startswith("."):
        ext = base[:12]
    return "/home/user/project/src/" + hashlib.sha1(p.encode()).hexdigest()[:6] + ext


def anon_token(tok):
    """One whitespace-delimited Bash token: keep command words, flags and
    operators; keep the extension of anything that looks like a path;
    fill the rest at the same length."""
    if not tok or tok in CMD_WORDS or (tok.startswith("-") and len(tok) <= 12):
        return tok
    core = tok.strip("'\"`()[]{}$")
    if core in CMD_WORDS:
        return tok
    m = re.search(r"\.([A-Za-z0-9]{1,5})$", core)
    if m and len(core) < 80:
        ext = m.group(1)
        stem = fill(max(1, len(core) - len(ext) - 1)).replace(" ", "_")
        return stem + "." + ext
    if tok.startswith("$"):
        return "$" + fill(len(tok) - 1).replace(" ", "_")
    return fill(len(tok)).replace(" ", "_")


def anon_command(cmd):
    out_lines = []
    for line in cmd.split("\n"):
        out_lines.append(" ".join(anon_token(t) for t in line.split(" ")))
    return "\n".join(out_lines)


def anon_stdout(s):
    """Line by line: a test-runner summary line stays, a compiler diagnostic
    keeps its `error:`/`warning:` prefix, everything else becomes filler of
    the same length. The line count is preserved; the total is capped."""
    out = []  # (kept, is_filler)
    for line in s.split("\n"):
        if TEST_SUMMARY.match(line):
            out.append((line, False))
        else:
            m = DIAG_PREFIX.match(line)
            if m and not line.startswith("thread"):
                out.append((m.group(0) + fill(len(line) - len(m.group(0))), False))
            elif m:
                out.append(("thread panicked" + fill(max(0, len(line) - 15)), False))
            else:
                out.append((fill(len(line)), True))
    # Cap the total by shrinking filler lines, never the kept ones (the
    # summary is usually the tail of a long output).
    fixed = sum(len(k) + 1 for k, f in out if not f)
    filler = sum(len(k) + 1 for k, f in out if f)
    if fixed + filler > MAX_STR and filler > 0:
        share = max(0.0, (MAX_STR - fixed) / filler)
        out = [(k if not f else k[: int(len(k) * share)], f) for k, f in out]
    return "\n".join(k for k, _ in out)


def anon_text(s):
    """Human/harness text: keep line-kind markers, fill the rest."""
    for m in MARKERS:
        if s.startswith(m):
            return m + fill(len(s) - len(m))
    if s.startswith("<command-name>"):
        end = s.find("</command-name>")
        if end > 0:
            head = s[: end + len("</command-name>")]
            return head + re.sub(r"[^\n<>/-]", "x", s[len(head):])
    if s.startswith("<local-command-stdout>"):
        if "Context Usage" in s:
            return s  # Claude Code's own /context table, nothing personal in it
        return "<local-command-stdout>" + fill(len(s) - len("<local-command-stdout>"))
    for t in TAGS:
        if s.startswith(t):
            return t + fill(len(s) - len(t))
    return fill(min(len(s), MAX_STR))


def anon(v, key=None, parent=None):
    if isinstance(v, dict):
        return {k: anon(x, k, v) for k, x in v.items()}
    if isinstance(v, list):
        return [anon(x, key, parent) for x in v]
    if isinstance(v, str):
        if key == "error":
            # `error: "rate_limit"` on an API-error line is an enum; a longer
            # error is free text.
            return v if len(v) <= 40 and " " not in v else fill(min(len(v), MAX_STR))
        if key in KEEP_KEYS:
            return v
        if key in ID_KEYS:
            return hid(v)
        if key == "cwd":
            return "/home/user/project"
        if key in ("gitBranch", "branch"):
            return "main"
        if key in PATH_KEYS:
            return anon_path(v)
        if key == "command":
            return anon_command(v)[:MAX_STR]
        if key == "staleReadFileStateHint":
            # "[This command modified 2 files you've previously read: a, b]"
            head, _, paths = v.partition("previously read:")
            if not paths:
                return fill(len(v))
            keep = ", ".join(anon_path(p.strip().rstrip("]")) for p in paths.split(",") if p.strip())
            return head + "previously read: " + keep + "]"
        if key == "stdout":
            return anon_stdout(v)
        if key in ("text", "content", "prompt", "display"):
            return anon_text(v)
        return fill(min(len(v), MAX_STR))
    return v


def main():
    global MAX_STR
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if "--max-str" in sys.argv:
        MAX_STR = int(sys.argv[sys.argv.index("--max-str") + 1])
        args = [a for a in args if a != str(MAX_STR)]
    src, dst = args[0], args[1]
    with open(src) as f, open(dst, "w") as out:
        for line in f:
            try:
                o = json.loads(line)
            except json.JSONDecodeError:
                continue
            out.write(json.dumps(anon(o), ensure_ascii=False, separators=(",", ":")) + "\n")


if __name__ == "__main__":
    main()
