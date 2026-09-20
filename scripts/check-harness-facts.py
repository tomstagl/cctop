#!/usr/bin/env python3
"""Re-verifies src/harness_facts.rs against the installed Claude Code.

The binary's constants (autocompact arithmetic, /usage weights, /context
thresholds, the model catalog's effort_cost_index) live as literals inside
Claude Code's bundle. Each probe below finds one by a stable anchor — a
user-facing string, an env-var name, a field name — never by a minified
identifier (those change every release), and compares what it finds with
what harness_facts.rs says. The team facts are read from the newest team
directory on this machine, when there is one.

    scripts/check-harness-facts.py            # the installed `claude`
    scripts/check-harness-facts.py --bundle ~/.local/share/claude/versions/2.1.270

Prints one line per fact (`ok`, `DIFF`, `missing`) and the model catalog,
and exits 1 when anything differs or a probe finds nothing — the person
then re-reads that constant by hand and bumps READ_FROM. Nothing in here
writes.
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FACTS = ROOT / "src" / "harness_facts.rs"


# ----------------------------------------------------------------- the bundle
def claude_version() -> str | None:
    claude = shutil.which("claude")
    if claude is None:
        return None
    out = subprocess.run([claude, "--version"], capture_output=True, text=True, check=False)
    m = re.search(r"\d+\.\d+\.\d+", out.stdout)
    return m.group(0) if m else None


def bundle_version(text: str) -> str | None:
    """The version a bundle says it is: the `VERSION:"2.1.278"` literal every
    build carries. (A bare `"2.1.278"` is not enough — a launcher's changelog
    strings name other releases too.)"""
    m = re.search(r'VERSION:"(\d+\.\d+\.\d+)"', text)
    return m.group(1) if m else None


def find_bundle(explicit: str | None) -> Path | None:
    """The file that holds Claude Code's JavaScript, tried in the order the
    installs lay it out: the executable `claude` resolves to, when it is the
    runtime itself (the native install's versioned binary, a machine image's
    /opt binary, or the npm launcher's `bin/claude.exe`, which its postinstall
    replaces with a hard link to the platform package's binary since 2.1.278);
    the platform package `@anthropic-ai/claude-code-<platform>/bin/claude`
    beside a launcher whose postinstall did not run; the single-file `cli.js`
    of the npm packages before 2.1.278. A shim, wrapper or stub is a few KB
    and a runtime tens of MB at least, so anything under a megabyte is
    skipped. The caller then checks the pick against `claude --version`: a
    global npm prefix can hold a stale package next to a newer shim (a 2.1.42
    `cli.js` beside a 2.1.278 binary on one machine read every probe as
    missing), and its constants are another release's."""
    if explicit:
        return Path(explicit).expanduser()
    claude = shutil.which("claude")
    if claude is None:
        return None
    real = Path(claude).resolve()
    candidates: list[Path] = [real]
    roots: list[Path] = list(real.parents[:4])
    roots += [real.parent.parent / "lib" / "node_modules", real.parent / "node_modules"]
    npm = shutil.which("npm")
    if npm is not None:
        out = subprocess.run([npm, "root", "-g"], capture_output=True, text=True, check=False)
        if out.stdout.strip():
            roots.append(Path(out.stdout.strip()))
    for root in roots:
        candidates.extend(sorted(root.glob("@anthropic-ai/claude-code-*/bin/claude*")))
        candidates.append(root / "@anthropic-ai" / "claude-code" / "bin" / "claude.exe")
        candidates.append(root / "@anthropic-ai" / "claude-code" / "cli.js")
    for c in candidates:
        if c.is_file() and c.stat().st_size >= 1 << 20:
            return c
    return real if real.is_file() else None


def bundle_text(path: Path) -> str:
    """The bundle's JavaScript as text: a .js file as is, a native executable
    reduced to its printable runs of 200 bytes or more (`strings -n 200`)."""
    data = path.read_bytes()
    if path.suffix == ".js" or data[:2] == b"#!":
        return data.decode("utf-8", errors="replace")
    runs = re.findall(rb"[\x20-\x7e\t]{200,}", data)
    return "\n".join(r.decode("ascii", errors="replace") for r in runs)


# ------------------------------------------------------------- the Rust side
def rust_consts(text: str, module: str) -> dict[str, str]:
    """`pub const NAME: T = VALUE;` inside `pub mod <module> { … }`."""
    m = re.search(r"pub mod " + module + r" \{(.*?)\n\}", text, re.S)
    if not m:
        return {}
    body = m.group(1)
    return {
        name: value.replace("_", "")
        for name, value in re.findall(r"pub const ([A-Z_]+): [a-z0-9]+ = ([0-9._]+);", body)
    }


def rust_table(text: str, name: str) -> list[tuple[str, list[str]]]:
    """`pub const NAME: &[(&str, [f64; N])] = &[("id", [a, b, …]), …];`"""
    m = re.search(r"pub const " + name + r": &\[\(&str, \[f64; \d+\]\)\] = &\[(.*?)\n\];", text, re.S)
    if not m:
        return []
    return [
        (id_, [v.strip() for v in values.split(",")])
        for id_, values in re.findall(r'\("([^"]+)", \[([^\]]+)\]\)', m.group(1))
    ]


def rust_pairs(text: str, name: str) -> list[tuple[str, str]]:
    """`pub const NAME: &[(&str, f64)] = &[("id", v), …];`"""
    m = re.search(r"pub const " + name + r": &\[\(&str, f64\)\] = &\[(.*?)\];", text, re.S)
    if not m:
        return []
    return re.findall(r'\("([^"]+)", ([0-9.]+)\)', m.group(1))


def num(s: str) -> float:
    return float(s.replace("_", ""))


# ------------------------------------------------------------------ probes
class Report:
    def __init__(self) -> None:
        self.rows: list[tuple[str, str, str, str]] = []
        self.failed = False

    def add(self, fact: str, expected: object, found: object | None, note: str = "") -> None:
        if found is None:
            status = "missing"
            self.failed = True
        elif str(expected) == str(found) or (
            isinstance(expected, (int, float)) and isinstance(found, (int, float)) and abs(float(expected) - float(found)) < 1e-9
        ):
            status = "ok"
        else:
            status = "DIFF"
            self.failed = True
        self.rows.append((status, fact, f"{expected} → {found}" if status == "DIFF" else str(expected), note))

    def skip(self, fact: str, why: str) -> None:
        self.rows.append(("skip", fact, "", why))

    def print(self) -> None:
        width = max(len(r[1]) for r in self.rows) if self.rows else 10
        for status, fact, values, note in self.rows:
            tail = f"  ({note})" if note else ""
            print(f"  {status:7} {fact:{width}}  {values}{tail}")


def js_number(s: str) -> float:
    return float(s)


# A minified identifier: the minifier renames locals every release (`b=y-20000`
# in 2.1.273 was `C=S-20000` in 2.1.270), so no probe may spell one.
ID = r"[A-Za-z0-9_$]+"


def declared(text: str, name: str, before: int) -> str | None:
    """The literal a minified `var …,name=VALUE,…` assigns, searched backwards
    from `before` (the declarations sit just ahead of their uses)."""
    seg = text[max(0, before - 20000) : before]
    ms = list(re.finditer(r"(?<![A-Za-z0-9_$.])" + re.escape(name) + r"=([0-9.e]+)(?=[,;])", seg))
    return ms[-1].group(1) if ms else None


def probe_autocompact(text: str, facts: str, r: Report) -> None:
    want = rust_consts(facts, "autocompact")
    # `var X=13000;var Y=3000,Z=0.2;function W(e){return typeof e==="number"
    # &&Number.isFinite(e)&&e>=0&&e<1?e:null}` — the buffer, the block
    # margin and the precompute buffer fraction, declared together.
    m = re.search(
        rf"var {ID}=(\d+);var {ID}=(\d+),{ID}=(0\.\d+);"
        rf'function {ID}\(({ID})\)\{{return typeof \4==="number"&&Number\.isFinite\(\4\)&&\4>=0&&\4<1',
        text,
    )
    r.add("autocompact::BUFFER_TOKENS", num(want["BUFFER_TOKENS"]), num(m.group(1)) if m else None, "the threshold is effective − this")
    r.add("autocompact::BLOCK_TOKENS", num(want["BLOCK_TOKENS"]), num(m.group(2)) if m else None, "blocked at window − this")
    r.add(
        "autocompact::PRECOMPUTE_RATIO",
        num(want["PRECOMPUTE_RATIO"]),
        round(1 - num(m.group(3)), 6) if m else None,
        "1 − the default precomputeBufferFraction; a per-window table can override it",
    )
    # The level function: `b=y-20000` is the warn band under the threshold.
    m = re.search(rf"={ID}-(\d+),{ID}={ID}\.testBlockingOverride", text)
    r.add("autocompact::WARN_TOKENS", num(want["WARN_TOKENS"]), num(m.group(1)) if m else None, "warn at threshold − this")
    # `{let r=e-13000,s=n.testPctOverride` — the threshold itself.
    m = re.search(rf"\{{let {ID}={ID}-(\d+),{ID}={ID}\.testPctOverride", text)
    r.add("autocompact threshold = effective − BUFFER", num(want["BUFFER_TOKENS"]), num(m.group(1)) if m else None)
    # The effective window: `window − min(maxOutputTokens, CAP)`; the cap is
    # a `var` near the level function.
    m = re.search(
        rf"\{{let {ID}=Math\.min\({ID}\({ID}\),({ID})\),{ID}={ID}\(\)\?{ID}:void 0,\{{window:{ID}\}}={ID}\({ID},{ID}\);return {ID}-{ID}\}}",
        text,
    )
    cap = None
    if m:
        v = re.search(r"var " + re.escape(m.group(1)) + r"=(\d+)", text)
        cap = num(v.group(1)) if v else None
    r.add(
        "autocompact::OUTPUT_RESERVE_TOKENS",
        num(want["OUTPUT_RESERVE_TOKENS"]),
        cap,
        "effective window = window − min(model max output, this)",
    )


def probe_usage_weight(text: str, facts: str, r: Report) -> None:
    want = rust_consts(facts, "usage_weight")
    m = re.search(rf"\(({ID})\.cached\+\1\.uncached\*([0-9.]+)\+\1\.cacheCreate\*([0-9.]+)\+\1\.output\*([0-9.]+)\)\*\1\.modelTier", text)
    r.add("usage_weight::UNCACHED", num(want["UNCACHED"]), num(m.group(2)) if m else None)
    r.add("usage_weight::CACHE_CREATE", num(want["CACHE_CREATE"]), num(m.group(3)) if m else None)
    r.add("usage_weight::OUTPUT", num(want["OUTPUT"]), num(m.group(4)) if m else None)
    tiers = dict(rust_pairs(facts, "TIERS"))
    default = rust_consts(facts, "usage_weight").get("TIER_DEFAULT")
    m = re.search(
        rf'if\(({ID})\.includes\("fable"\)\)return (\d+);if\(\1\.includes\("opus"\)\)return (\d+);if\(\1\.includes\("haiku"\)\)return (\d+);return (\d+)\}}',
        text,
    )
    for i, family in enumerate(("fable", "opus", "haiku")):
        r.add(f"usage_weight::TIERS[{family}]", num(tiers.get(family, "nan")), num(m.group(i + 2)) if m else None)
    r.add("usage_weight::TIER_DEFAULT", num(default) if default else float("nan"), num(m.group(5)) if m else None, "sonnet and anything unnamed")


def probe_context_suggestions(text: str, facts: str, r: Report) -> None:
    want = rust_consts(facts, "context_suggestions")
    # The `/context` suggestions, anchored on their titles; each threshold is
    # a `var` declared ahead of the functions.
    m = re.search(
        rf"if\({ID}>=({ID})&&{ID}>=({ID})\)return;if\({ID}>=({ID})&&{ID}\.resultTokens>=\2\){ID}\.push\(\{{severity:\"info\",title:`File reads using",
        text,
    )
    tool_share = num(declared(text, m.group(1), m.start())) / 100 if m and declared(text, m.group(1), m.start()) else None
    tool_min = num(declared(text, m.group(2), m.start())) if m and declared(text, m.group(2), m.start()) else None
    read_share = num(declared(text, m.group(3), m.start())) / 100 if m and declared(text, m.group(3), m.start()) else None
    r.add("context_suggestions::TOOL_WINDOW_SHARE", num(want["TOOL_WINDOW_SHARE"]), tool_share, "a tool's results, share of the window")
    r.add("context_suggestions::TOOL_MIN_TOKENS", num(want["TOOL_MIN_TOKENS"]), tool_min)
    r.add("context_suggestions::READ_WINDOW_SHARE", num(want["READ_WINDOW_SHARE"]), read_share)
    m = re.search(rf"if\({ID}>=({ID})&&{ID}>=({ID})\)\{{let {ID}=\[\.\.\.{ID}\.memoryFiles\]", text)
    mem_share = num(declared(text, m.group(1), m.start())) / 100 if m and declared(text, m.group(1), m.start()) else None
    mem_min = num(declared(text, m.group(2), m.start())) if m and declared(text, m.group(2), m.start()) else None
    r.add("context_suggestions::MEMORY_WINDOW_SHARE", num(want["MEMORY_WINDOW_SHARE"]), mem_share)
    r.add("context_suggestions::MEMORY_MIN_TOKENS", num(want["MEMORY_MIN_TOKENS"]), mem_min)
    m = re.search(rf"if\(({ID})\.percentage>=({ID})\){ID}\.push\(\{{severity:\"warning\",title:`Context is \$\{{\1\.percentage\}}% full`", text)
    ctx_share = num(declared(text, m.group(2), m.start())) / 100 if m and declared(text, m.group(2), m.start()) else None
    r.add("context_suggestions::CONTEXT_SHARE", num(want["CONTEXT_SHARE"]), ctx_share)


def catalog(text: str) -> list[dict[str, object]]:
    """The model catalog: id, family, context window, default max output,
    and effort_cost_index where the entry has one."""
    starts = [m.start() for m in re.finditer(r'\{id:"claude-[^"]+",family:"', text)]
    rows: list[dict[str, object]] = []
    for i, s in enumerate(starts):
        entry = text[s : starts[i + 1] if i + 1 < len(starts) else s + 8000]
        head = re.match(r'\{id:"([^"]+)",family:"([^"]+)"', entry)
        win = re.search(r"context:\{window:([0-9e]+)", entry)
        out = re.search(r"max_output_tokens:\{default:(\d+),upper:(\d+)\}", entry)
        eff = re.search(r"effort_cost_index:\{low:([0-9.]+),medium:([0-9.]+),high:([0-9.]+),xhigh:([0-9.]+),max:([0-9.]+)\}", entry)
        rows.append(
            {
                "id": head.group(1),
                "family": head.group(2),
                "window": int(js_number(win.group(1))) if win else None,
                "max_output": int(out.group(1)) if out else None,
                "effort": [num(x) for x in eff.groups()] if eff else None,
            }
        )
    return rows


def probe_catalog(text: str, facts: str, r: Report) -> list[dict[str, object]]:
    rows = catalog(text)
    if not rows:
        r.add("model catalog", "entries", None)
        return rows
    want = {id_: [num(v) for v in values] for id_, values in rust_table(facts, "EFFORT_COST_INDEX")}
    have = {str(row["id"]): row["effort"] for row in rows if row["effort"] is not None}
    for id_ in sorted(set(want) | set(have)):
        r.add(f"EFFORT_COST_INDEX[{id_}]", want.get(id_), have.get(id_), "low, medium, high, xhigh, max")
    # Every model but Claude 3.x defaults to at least the output reserve, so
    # the reserve is what comes off every window.
    reserve = num(rust_consts(facts, "autocompact").get("OUTPUT_RESERVE_TOKENS", "0"))
    under = [str(row["id"]) for row in rows if row["max_output"] is not None and row["max_output"] < reserve and not str(row["id"]).startswith("claude-3-")]
    r.add("models with max output < OUTPUT_RESERVE (Claude 3.x aside)", "none", "none" if not under else ", ".join(under))
    return rows


def probe_teams(facts: str, r: Report) -> None:
    """The newest ~/.claude/teams/<team>/config.json against docs/teams.md's
    shape and harness_facts::teams."""
    configs = sorted(glob.glob(os.path.expanduser("~/.claude/teams/*/config.json")), key=os.path.getmtime)
    if not configs:
        r.skip("teams", "no ~/.claude/teams/*/config.json on this machine")
        return
    path = configs[-1]
    doc = json.load(open(path))
    team = Path(path).parent.name
    prefix = re.search(r'pub const NAME_PREFIX: &str = "([^"]+)"', facts)
    chars = re.search(r"pub const LEAD_ID_CHARS: usize = (\d+)", facts)
    liveness = re.search(r'pub const LIVENESS_KEY: &str = "([^"]+)"', facts)
    r.add("teams config keys", "createdAt, leadAgentId, leadSessionId, members, name", ", ".join(sorted(doc)), team)
    lead_id = str(doc.get("leadSessionId", ""))
    r.add(
        "teams::NAME_PREFIX + LEAD_ID_CHARS",
        team,
        (prefix.group(1) if prefix else "?") + lead_id[: int(chars.group(1)) if chars else 8],
        "the directory is named after the lead's session id",
    )
    members = doc.get("members", [])
    base = {"agentId", "agentType", "backendType", "cwd", "joinedAt", "name", "subscriptions", "tmuxPaneId"}
    extra = {"color", "isActive", "model", "planModeRequired", "prompt"}
    lead = [m for m in members if m.get("backendType") == "in-process"]
    tmux = [m for m in members if m.get("backendType") == "tmux"]
    r.add("teams lead member keys", ", ".join(sorted(base)), ", ".join(sorted(lead[0])) if lead else None)
    if tmux:
        r.add("teams tmux member keys", ", ".join(sorted(base | extra)), ", ".join(sorted(tmux[0])), "spawned teammates add color, isActive, model, planModeRequired, prompt")
        r.add("teams::LIVENESS_KEY on a tmux member", liveness.group(1) if liveness else "?", liveness.group(1) if liveness and liveness.group(1) in tmux[0] else None)
    else:
        r.skip("teams tmux member keys", f"{team} has no tmux member (a solo session)")
    r.add("teams::MEMBER_HAS_SESSION_ID", "false", str(any("sessionId" in m for m in members)).lower())


# -------------------------------------------------------------------- main
def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--bundle", help="the Claude Code bundle to read (default: the installed claude)")
    ap.add_argument("--facts", default=str(FACTS), help="src/harness_facts.rs")
    args = ap.parse_args()

    facts = Path(args.facts).read_text()
    read_from = re.search(r'pub const READ_FROM: &str = "([^"]+)"', facts)
    bundle = find_bundle(args.bundle)
    if bundle is None or not bundle.is_file():
        print("check-harness-facts: no Claude Code bundle found (install claude or pass --bundle)", file=sys.stderr)
        return 1
    installed = claude_version() if args.bundle is None else None
    text = bundle_text(bundle)
    says = bundle_version(text)
    print(
        f"check-harness-facts: READ_FROM {read_from.group(1) if read_from else '?'}; bundle {bundle}"
        + (f" (claude {installed})" if installed else "")
        + (f", which says it is {says}" if says else "")
        + f", {len(text) // 1024} KiB of text"
    )
    if installed and says and says != installed:
        print(
            f"check-harness-facts: {bundle} is Claude Code {says}, but `claude --version` says {installed} — a stale package beside the shim, not the running runtime. "
            f"Pass --bundle with the executable that runs (`readlink -f \"$(command -v claude)\"`, or the platform package's bin/claude).",
            file=sys.stderr,
        )
        return 1

    r = Report()
    probe_autocompact(text, facts, r)
    probe_usage_weight(text, facts, r)
    probe_context_suggestions(text, facts, r)
    rows = probe_catalog(text, facts, r)
    probe_teams(facts, r)
    r.print()

    if rows:
        print("\n  the model catalog (id, family, window, default max output, effort_cost_index):")
        for row in rows:
            eff = row["effort"]
            print(
                f"    {row['id']:26} {row['family']:7} window={str(row['window'] or '-'):8} max_out={str(row['max_output'] or '-'):6} "
                + (f"effort={eff}" if eff else "")
            )

    facts_not_here = "first_seen, cost_state and task_notification are corpus facts: re-read them against transcripts (scripts/anonymise-transcript.py, scripts/ledger-vs-agents.py)"
    if r.failed:
        print(f"\ncheck-harness-facts: something differs or was not found — re-read it by hand, fix src/harness_facts.rs, then bump READ_FROM. {facts_not_here}.")
        return 1
    print(f"\ncheck-harness-facts: every probed constant matches; bump READ_FROM to {installed or says or 'this version'} if it is behind. {facts_not_here}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
