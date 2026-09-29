#!/usr/bin/env python3
"""Compose fixture W — one real `Workflow` run, cut down and anonymised.

usage: compose-workflow-fixture.py <session.jsonl> <run> <out-stem>
           [--mid N] [--ok N] [--ghosts N] [--live-cut ISO] [--name WORD]
           [--rename word=replacement …]

Reads the session's main transcript, `<session>/subagents/workflows/<run>/`
(journal and agent transcripts) and `<session>/workflows/<run>.json` (the run
record), and writes, beside `<out-stem>.jsonl`:

- `<out-stem>.jsonl` — the main transcript's `Workflow` tool_use lines, their
  `async_launched` results (one per invocation, resumes included), the run's
  `<task-notification>` deliveries, and every assistant response inside the
  run's window (the main thread's spend while it ran). `workflowName` in a
  launch result becomes `<name>-<n>`.
- `<out-stem>/subagents/workflows/<run>/` — every first-call 429 agent (last
  line a 429 with no completed call before it), `--mid` mid-task 429 agents
  and `--ok` agents with a result, spread across the phases; each transcript
  through `anonymise-transcript.py`, each meta file cut to `agentType` /
  `model`. The journal keeps every `launched` / `started` / `result` line and
  the `failed` lines of the copied agents plus `--ghosts` failed agents kept
  on purpose without a transcript; the `failed` lines of the other uncopied
  agents are dropped (their cause would read `Unknown` and outvote the real
  ones). Labels become `<prefix>:<n>`; a `result` line keeps `type`,
  `agentId` and `key` only (its value is the agent's prose).
- `<out-stem>/workflows/<run>.json` — the run record's `runId`,
  `workflowName` (`<name>-<n>`), `status`, `startTime`, `durationMs`,
  `scriptPath` (`/home/user/project/.claude/workflows/scripts/<name>-<n>.js`)
  and `script`, whose string and template literals are same-shape filler
  (newlines kept) except `phase:` values, `phase(` arguments and the prefix
  of `label` heads, whose comments are same-length filler, and whose
  identifiers containing `cctop` / `research` (or named by `--rename`) are
  renamed — so line numbers and the pointer scan (`src/workflow_script.rs`)
  come out the same. Every other key is dropped.

`--live-cut <ISO time>` also writes the same run cut mid-flight to
`<out-stem>-live`: the main transcript up to the cut, the journal lines of
agents started by the cut (a `result` / `failed` line only when the agent's
last line is before it), each copied agent's lines up to the cut, no run
record, and `<out-stem>-live.now` holding the cut + 5 s in epoch ms.

Nothing under the source session is written.
"""
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ANON = os.path.join(HERE, "anonymise-transcript.py")
MAX_STR = "2000"
FILLER = ("lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod "
          "tempor incididunt ut labore et dolore magna aliqua ")
RECORD_KEEP = ("runId", "workflowName", "status", "startTime", "durationMs", "scriptPath", "script")
META_KEEP = ("agentType", "model")
SENSITIVE = re.compile(r"cctop|research", re.I)
KEEP_MAX = 64


def ts_ms(s):
    return int(datetime.datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp() * 1000)


def read_jsonl(path):
    out = []
    with open(path) as f:
        for line in f:
            try:
                out.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return out


def write_jsonl(path, rows):
    os.makedirs(os.path.dirname(path) or ".", exist_ok=True)
    with open(path, "w") as f:
        for o in rows:
            f.write(json.dumps(o, ensure_ascii=False, separators=(",", ":")) + "\n")


RENAME = {}


def rename_word(word):
    """An identifier or object key naming the project: `--rename`'s
    replacement, else `cctop` → `tool`, `research` → `survey`."""
    if word in RENAME:
        return RENAME[word]
    if SENSITIVE.search(word):
        return re.sub("research", "survey", re.sub("cctop", "tool", word, flags=re.I), flags=re.I)
    return word


def scrub(v):
    """What the anonymiser keeps by design but must not survive here: object
    keys (a `StructuredOutput` input's schema fields) are renamed like the
    script's identifiers; in strings the words become `x`s, which the
    anonymiser fills (a Bash command keeps `cctop` as a command word)."""
    if isinstance(v, dict):
        return {rename_word(k): scrub(x) for k, x in v.items()}
    if isinstance(v, list):
        return [scrub(x) for x in v]
    if isinstance(v, str) and SENSITIVE.search(v):
        return SENSITIVE.sub(lambda m: "x" * len(m.group(0)), v)
    return v


def fill_leaves(v):
    """Every string leaf as same-length filler; keys and shape kept."""
    if isinstance(v, dict):
        return {k: fill_leaves(x) for k, x in v.items()}
    if isinstance(v, list):
        return [fill_leaves(x) for x in v]
    if isinstance(v, str):
        return (FILLER * (len(v) // len(FILLER) + 1))[:len(v)]
    return v


STRUCTURED_OUTPUT = "StructuredOutput"


def structured_output(o):
    """An agent's returned value is prose whose schema fields may share a
    name with a key the anonymiser keeps (`id`, `kind`, `trigger`,
    `action`, `name`): the `StructuredOutput` call's `input`, its
    `wireToolInputs` entry and the `structured_output` attachment's `data`
    are filled here, before the anonymiser sees them."""
    o = json.loads(json.dumps(o))
    if o.get("type") == "assistant":
        wire = o.get("wireToolInputs")
        for c in (o.get("message") or {}).get("content") or []:
            if isinstance(c, dict) and c.get("type") == "tool_use" and c.get("name") == STRUCTURED_OUTPUT:
                c["input"] = fill_leaves(c.get("input"))
                if isinstance(wire, dict) and c.get("id") in wire:
                    wire[c["id"]] = fill_leaves(wire[c["id"]])
    a = o.get("attachment")
    if isinstance(a, dict) and a.get("type") == "structured_output" and "data" in a:
        a["data"] = fill_leaves(a["data"])
    return o


def anonymise(rows, dst, tmp):
    """Every transcript goes through the anonymiser, file to file."""
    src = os.path.join(tmp, "raw-" + os.path.basename(dst))
    write_jsonl(src, [scrub(structured_output(o)) for o in rows])
    os.makedirs(os.path.dirname(dst), exist_ok=True)
    subprocess.run([sys.executable, ANON, "--max-str", MAX_STR, src, dst], check=True)


def line_ms(o):
    t = o.get("timestamp")
    return ts_ms(t) if isinstance(t, str) else None


def up_to(rows, cut):
    """The lines before the first one stamped after `cut` (unstamped lines
    go with their neighbours)."""
    out = []
    for o in rows:
        t = line_ms(o)
        if t is not None and t > cut:
            break
        out.append(o)
    return out


def renamed(name, base):
    """`cctop-research-sweep-4` → `sweep-4`: the trailing number is kept."""
    m = re.search(r"-(\d+)$", name or "")
    return f"{base}-{m.group(1)}" if m else base


# ---------------------------------------------------------------------------
# The script: a lexer mirroring `src/workflow_script.rs`, emitting as it goes.

class Filler:
    def __init__(self):
        self.i = 0

    def text(self, s):
        """Same shape: newlines stay, every other char becomes filler."""
        out = []
        for c in s:
            if c == "\n":
                out.append(c)
            else:
                out.append(FILLER[self.i % len(FILLER)])
                self.i += 1
        return "".join(out)


class ScriptAnon:
    NAMES = {"agent", "parallel", "pipeline", "phase", "label"}

    def __init__(self, src, rename):
        """`rename`: whether identifiers are renamed (off for the re-lex)."""
        self.s = src
        self.pos = 0
        self.line = 1
        self.out = []
        self.toks = []  # (kind, value, line); value is a kept literal or a name
        self.rename = rename
        self.fill = Filler()

    def peek(self, k=0):
        i = self.pos + k
        return self.s[i] if i < len(self.s) else None

    def bump(self):
        c = self.peek()
        if c is None:
            return None
        self.pos += 1
        if c == "\n":
            self.line += 1
        return c

    def push(self, kind, value, line):
        self.toks.append((kind, value, line))

    def keep_context(self):
        """`phase :`, `label :` or `phase (` before the literal: its key."""
        if len(self.toks) < 2:
            return None
        (k2, v2, _), (k1, _, _) = self.toks[-2], self.toks[-1]
        if k2 == "name" and v2 in ("phase", "label") and k1 == "colon":
            return v2
        if k2 == "name" and v2 == "phase" and k1 == "open":
            return "phase("
        return None

    def kept(self, ctx, text):
        """What a kept literal's text becomes in the copy."""
        if ctx == "label":
            if ":" in text:
                head, _, rest = text.partition(":")
                return head + ":" + self.fill.text(rest)
            return text if re.fullmatch(r"[A-Za-z0-9_-]{1,32}", text) else self.fill.text(text)
        return text

    def operand_before(self):
        if not self.toks:
            return False
        k, v, _ = self.toks[-1]
        return k in ("name", "ident", "lit", "close") or (k == "other" and v in ("]", "}"))

    def ident(self, word):
        return rename_word(word) if self.rename else word

    def run(self):
        self.code(False)
        return "".join(self.out), [(k, v, l) for k, v, l in self.toks]

    def code(self, in_sub):
        braces = 0
        while self.peek() is not None:
            c = self.peek()
            line = self.line
            if c.isspace():
                self.out.append(self.bump())
            elif c.isalnum() or c in "_$":
                start = self.pos
                while self.peek() is not None and (self.peek().isalnum() or self.peek() in "_$"):
                    self.bump()
                word = self.s[start:self.pos]
                self.out.append(self.ident(word))
                if word in self.NAMES:
                    self.push("name", word, line)
                else:
                    self.push("ident", None, line)
            elif c in "'\"":
                ctx = self.keep_context()
                self.bump()
                self.quoted(c, ctx, line)
            elif c == "`":
                self.bump()
                self.template(line)
            elif c == "/" and self.peek(1) == "/":
                start = self.pos
                while self.peek() is not None and self.peek() != "\n":
                    self.bump()
                self.out.append("//" + self.fill.text(self.s[start + 2:self.pos]))
            elif c == "/" and self.peek(1) == "*":
                start = self.pos
                self.bump()
                self.bump()
                while self.peek() is not None and not (self.peek() == "*" and self.peek(1) == "/"):
                    self.bump()
                body = self.s[start + 2:self.pos]
                self.bump()
                self.bump()
                self.out.append("/*" + self.fill.text(body) + "*/")
            elif c == "/" and not self.operand_before():
                self.bump()
                self.regex()
                self.push("lit", None, line)
            elif c == "{":
                braces += 1
                self.out.append(self.bump())
                self.push("other", "{", line)
            elif c == "}" and in_sub and braces == 0:
                self.out.append(self.bump())
                return
            elif c == "}":
                braces = max(0, braces - 1)
                self.out.append(self.bump())
                self.push("other", "}", line)
            elif c == "(":
                self.out.append(self.bump())
                self.push("open", None, line)
            elif c == ")":
                self.out.append(self.bump())
                self.push("close", None, line)
            elif c == ":":
                self.out.append(self.bump())
                self.push("colon", None, line)
            else:
                self.out.append(self.bump())
                self.push("other", c, line)

    def quoted(self, quote, ctx, line):
        body, escaped, closed = [], False, ""
        while self.peek() is not None:
            c = self.bump()
            if c == quote:
                closed = c
                break
            if c == "\\":
                escaped = True
                n = self.bump()
                body.append(c + (n or ""))
                continue
            if c == "\n":
                closed = c
                break
            body.append(c)
        text = "".join(body)
        keep = ctx is not None and not escaped and len(text) <= KEEP_MAX
        # Escapes become two filler chars (an escaped newline keeps its line).
        shaped = "".join(b if len(b) == 1 else ("x" + ("\n" if b[1:] == "\n" else "x")) for b in body)
        self.out.append(quote + (self.kept(ctx, text) if keep else self.fill.text(shaped)) + closed)
        self.push("lit", text if keep else None, line)

    def template(self, line):
        ctx = self.keep_context()
        self.out.append("`")
        seg, head_ok, pushed = [], True, False

        def flush(is_head):
            text = "".join(seg)
            if is_head and ctx is not None and head_ok and len(text) <= KEEP_MAX:
                self.out.append(self.kept(ctx, text))
                return text
            self.out.append(self.fill.text(text))
            return None

        while self.peek() is not None:
            c = self.bump()
            if c == "`":
                break
            if c == "\\":
                n = self.bump()
                if not pushed:
                    head_ok = False
                seg.append("x" + ("\n" if n == "\n" else "x"))
                continue
            if c == "$" and self.peek() == "{":
                self.bump()
                kept = flush(not pushed)
                seg = []
                if not pushed:
                    self.push("lit", kept, line)
                    pushed = True
                self.out.append("${")
                self.code(True)
                continue
            seg.append(c)
        kept = flush(not pushed)
        if not pushed:
            self.push("lit", kept, line)
        self.out.append("`")

    def regex(self):
        start = self.pos
        klass = False
        while self.peek() is not None:
            c = self.peek()
            if c == "\n":
                break
            self.bump()
            if c == "\\":
                if self.peek() != "\n":
                    self.bump()
            elif c == "[":
                klass = True
            elif c == "]":
                klass = False
            elif c == "/" and not klass:
                break
        body = self.s[start:self.pos]
        # Keep the structure the lexer reads (`\`, `[`, `]`, the closing
        # `/`); fill the letters.
        out = "".join(ch if not ch.isalnum() else "x" for ch in body)
        self.out.append("/" + out)
        # Flags.
        while self.peek() is not None and self.peek().isalpha():
            self.out.append(self.bump())


def anonymise_script(script):
    out, toks = ScriptAnon(script, True).run()
    # The copy must lex to the same tokens: kinds, lines, kept literals
    # (a kept label keeps only its prefix, which is all the scan compares).
    _, again = ScriptAnon(out, False).run()

    def key(toks):
        return [(k, (v.split(":")[0] + ":" if k == "lit" and v and ":" in v else v), l)
                for k, v, l in toks]
    if key(toks) != key(again):
        sys.exit("compose-workflow-fixture: the anonymised script lexes differently")
    if out.count("\n") != script.count("\n"):
        sys.exit("compose-workflow-fixture: the anonymised script lost a line")
    return out


# ---------------------------------------------------------------------------

def main():
    argv = sys.argv[1:]
    opts = {"mid": 6, "ok": 6, "ghosts": 2, "live-cut": None, "name": "sweep"}
    args = []
    i = 0
    while i < len(argv):
        a = argv[i]
        if a == "--rename":
            k, _, v = argv[i + 1].partition("=")
            RENAME[k] = v
            i += 2
        elif a.startswith("--") and a[2:] in opts:
            opts[a[2:]] = argv[i + 1]
            i += 2
        else:
            args.append(a)
            i += 1
    if len(args) != 3:
        sys.exit(__doc__)
    session, run, stem = args
    n_mid, n_ok, n_ghosts = int(opts["mid"]), int(opts["ok"]), int(opts["ghosts"])
    base = opts["name"]
    sdir = session[: -len(".jsonl")]
    rdir = os.path.join(sdir, "subagents", "workflows", run)
    record_path = os.path.join(sdir, "workflows", run + ".json")

    # --- The journal and the agents, classified.
    journal = read_jsonl(os.path.join(rdir, "journal.jsonl"))
    started = [o for o in journal if o.get("type") == "started"]
    phase_of = {o["agentId"]: o.get("phase") or "" for o in started}
    phases = list(dict.fromkeys(phase_of.values()))
    failed = {o["agentId"] for o in journal if o.get("type") == "failed"}
    results = {o["agentId"] for o in journal if o.get("type") == "result"}
    lines = {}
    for aid in phase_of:
        p = os.path.join(rdir, f"agent-{aid}.jsonl")
        if os.path.exists(p):
            lines[aid] = read_jsonl(p)

    def api_error(o):
        return o.get("type") == "assistant" and (
            o.get("isApiErrorMessage") or o.get("apiErrorStatus") is not None)

    def kind(aid):
        rows = lines.get(aid)
        if not rows:
            return None
        last = rows[-1]
        if aid in failed and api_error(last):
            done = any(o.get("type") == "assistant" and not api_error(o) for o in rows)
            if last.get("apiErrorStatus") == 429:
                return "mid" if done else "first"
            return "other-error"
        if aid in results:
            return "ok"
        return None

    kinds = {aid: kind(aid) for aid in phase_of}
    first = [a for a in phase_of if kinds[a] == "first"]
    too_long = [a for a in first if len(lines[a]) > 10]
    if too_long:
        sys.exit(f"compose-workflow-fixture: {len(too_long)} first-call agents exceed 10 lines")

    def spread(pool, n):
        """Round-robin over the phases in order; the shortest transcripts
        of each phase first (ties by id)."""
        by_phase = {p: sorted((a for a in pool if phase_of[a] == p),
                              key=lambda a: (len(lines[a]), a)) for p in phases}
        out = []
        while len(out) < n and any(by_phase.values()):
            for p in phases:
                if by_phase[p] and len(out) < n:
                    out.append(by_phase[p].pop(0))
        return out

    mids = spread([a for a in phase_of if kinds[a] == "mid"], n_mid)
    oks = spread([a for a in phase_of if kinds[a] == "ok"], n_ok)
    copied = set(first) | set(mids) | set(oks)
    ghosts = [o["agentId"] for o in journal
              if o.get("type") == "failed" and o["agentId"] not in copied][:n_ghosts]
    keep_failed = copied | set(ghosts)

    # Real first / last line times of every agent (copied or not): what
    # the live cut judges journal lines by.
    span = {}
    for aid, rows in lines.items():
        ts = [t for t in map(line_ms, rows) if t is not None]
        if ts:
            span[aid] = (min(ts), max(ts))

    ordinal = {}

    def journal_line(o):
        t = o.get("type")
        if t == "started":
            label = o.get("label") or ""
            out = {"type": t, "key": o.get("key"), "agentId": o.get("agentId")}
            if ":" in label:
                pre = label.split(":", 1)[0]
                ordinal[pre] = ordinal.get(pre, 0) + 1
                out["label"] = f"{pre}:{ordinal[pre]}"
            elif re.fullmatch(r"[A-Za-z0-9_-]{1,32}", label):
                out["label"] = label
            out["phase"] = o.get("phase")
            return out
        if t in ("result", "failed"):
            return {"type": t, "agentId": o.get("agentId"), "key": o.get("key")}
        return {"type": t}

    new_journal = []
    for o in journal:
        t = o.get("type")
        if t == "failed" and o.get("agentId") not in keep_failed:
            continue
        if t not in ("launched", "started", "result", "failed"):
            continue
        new_journal.append((o, journal_line(o)))

    # --- The main transcript.
    main_rows = read_jsonl(session)
    launches = [o for o in main_rows
                if isinstance(o.get("toolUseResult"), dict)
                and o["toolUseResult"].get("runId") == run
                and o["toolUseResult"].get("status") == "async_launched"]
    task_ids = {o["toolUseResult"].get("taskId") for o in launches} - {None}
    use_ids = set()
    for o in launches:
        for c in (o.get("message") or {}).get("content") or []:
            if isinstance(c, dict) and c.get("type") == "tool_result":
                use_ids.add(c.get("tool_use_id"))

    def notification_text(o):
        if o.get("type") == "queue-operation":
            return o.get("content") if isinstance(o.get("content"), str) else None
        if o.get("type") == "user":
            c = (o.get("message") or {}).get("content")
            if isinstance(c, str):
                return c
            if isinstance(c, list) and c and isinstance(c[0], dict):
                return c[0].get("text")
        if o.get("type") == "attachment":
            p = (o.get("attachment") or {}).get("prompt")
            return p if isinstance(p, str) else None
        return None

    def is_notification(o):
        t = notification_text(o)
        if not t or not t.lstrip().startswith("<task-notification>"):
            return False
        return any(f"<task-id>{x}</task-id>" in t for x in task_ids) or any(
            f"<tool-use-id>{x}</tool-use-id>" in t for x in use_ids)

    def is_launch_use(o):
        return o.get("type") == "assistant" and any(
            isinstance(c, dict) and c.get("type") == "tool_use" and c.get("id") in use_ids
            for c in (o.get("message") or {}).get("content") or [])

    record = None
    if os.path.exists(record_path):
        with open(record_path) as f:
            record = json.load(f)
    ends = [line_ms(o) for o in main_rows if is_notification(o)]
    if record and record.get("startTime") and record.get("durationMs"):
        ends.append(int(record["startTime"]) + int(record["durationMs"]))
    window = (min(line_ms(o) for o in launches), max(e for e in ends if e is not None))
    kept_main = []
    for o in main_rows:
        t = line_ms(o)
        in_window = o.get("type") == "assistant" and t is not None and window[0] <= t <= window[1]
        if o in launches or is_notification(o) or is_launch_use(o) or in_window:
            if o in launches:
                o = json.loads(json.dumps(o))
                o["toolUseResult"]["workflowName"] = renamed(o["toolUseResult"].get("workflowName"), base)
            kept_main.append(o)

    # --- The run record.
    new_record = None
    if record:
        new_record = {k: record[k] for k in RECORD_KEEP if k in record}
        n = renamed(record.get("workflowName"), base)
        new_record["workflowName"] = n
        new_record["scriptPath"] = f"/home/user/project/.claude/workflows/scripts/{n}.js"
        if "script" in new_record:
            new_record["script"] = anonymise_script(new_record["script"])

    def write(out_stem, cut):
        out_dir = out_stem
        out_run = os.path.join(out_dir, "subagents", "workflows", run)
        if os.path.exists(out_dir):
            shutil.rmtree(out_dir)
        with tempfile.TemporaryDirectory() as tmp:
            rows = kept_main if cut is None else up_to(kept_main, cut)
            anonymise(rows, out_stem + ".jsonl", tmp)
            for aid in sorted(copied):
                rows = lines[aid] if cut is None else up_to(lines[aid], cut)
                if not rows:
                    continue
                anonymise(rows, os.path.join(out_run, f"agent-{aid}.jsonl"), tmp)
                meta = os.path.join(rdir, f"agent-{aid}.meta.json")
                if os.path.exists(meta):
                    with open(meta) as f:
                        m = json.load(f)
                    with open(os.path.join(out_run, f"agent-{aid}.meta.json"), "w") as f:
                        json.dump({k: m[k] for k in META_KEEP if k in m}, f)
        jl = []
        for src, o in new_journal:
            if cut is not None and src.get("type") != "launched":
                s = span.get(src.get("agentId"))
                if s is None:
                    continue
                if src["type"] == "started" and s[0] > cut:
                    continue
                if src["type"] in ("result", "failed") and s[1] >= cut:
                    continue
            jl.append(o)
        write_jsonl(os.path.join(out_run, "journal.jsonl"), jl)
        if cut is None and new_record is not None:
            p = os.path.join(out_dir, "workflows", run + ".json")
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, "w") as f:
                json.dump(new_record, f, indent=2, ensure_ascii=False)
                f.write("\n")
        return len(jl)

    n = write(stem, None)
    print(f"{stem}: {len(kept_main)} main lines, {len(copied)} agents "
          f"({len(first)} first-call 429, {len(mids)} mid-task 429, {len(oks)} ok), "
          f"{len(ghosts)} ghosts, {n} journal lines")
    if opts["live-cut"]:
        cut = ts_ms(opts["live-cut"])
        n = write(stem + "-live", cut)
        with open(stem + "-live.now", "w") as f:
            f.write(f"{cut + 5000}\n")
        print(f"{stem}-live: cut {opts['live-cut']} ({cut}), {n} journal lines")


if __name__ == "__main__":
    main()
