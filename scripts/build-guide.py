#!/usr/bin/env python3
"""Build site/guide/*.html — one page per panel, hand-authored "what it
means / what to do" text over the readings from docs/metrics.md and the
Advisor rules from src/advisor/rules.rs. Run via `make site`.

The registry's identifiers and source codes never reach the page as text:
an identifier becomes the entry's anchor, a source code becomes words.
"""
import re, html, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "site/guide"
OUT.mkdir(parents=True, exist_ok=True)

FONTS_CSS = (ROOT / "site/fonts.css").read_text().strip().replace("url(assets/", "url(../assets/")
SITE_CSS = (ROOT / "site/site.css").read_text().strip()

# ---- parse docs/metrics.md into {section: [(id, name, unit, sources, estimate), ...]} ----
md = (ROOT / "docs/metrics.md").read_text()
sections = {}
cur = None
for line in md.splitlines():
    if line.startswith("## "):
        cur = line[3:].strip()
        sections[cur] = []
    elif line.startswith("| **") and cur:
        cells = [c.strip() for c in re.split(r"(?<!\\)\|", line.strip("|"))]
        if len(cells) == 7:  # an unescaped "|" inside the unit, e.g. tokens|seconds
            cells[1:3] = [cells[1] + " or " + cells[2]]
        m = re.match(r"\*\*(.+?)\*\* <a id=\"(.+?)\"></a> `(.+?)`", cells[0])
        if m:
            name, mid, _ = m.groups()
            sections[cur].append((mid, name, cells[1], cells[3], cells[5]))

SOURCES = {
    "D1": "the session list", "D2": "the transcript", "D2a": "the subagent's transcript",
    "D3": "the status line (after cctop install)", "D4": "hook events (after cctop install)",
    "D5": "the process tree", "D7": "git", "D8": "file tool calls", "D9": "the price table",
    "D10": "OpenTelemetry", "D11": "Claude Code's own cost record",
}
EXACT = {
    "never": "exact",
    "≈ always": "always an estimate",
    "≈ (always priced from the table)": "always an estimate, priced from the list table",
    "≈ when any part is estimated": "≈ when any part of it is estimated",
    "≈ per turn": "exact for the session, ≈ per turn",
    "≈ until hook timings replace them": "≈ until hook timings replace it",
    "≈ without OTel": "≈ unless OpenTelemetry is on",
    "est when computed from the transcript alone": "≈ when read from the transcript alone",
    "est without the status-line shim": "≈ until cctop install",
    "est until a compaction has been observed": "≈ until the first compaction has been seen",
}

# ---- parse Advisor rules from src/advisor/rules.rs -------------------------
rules_src = (ROOT / "src/advisor/rules.rs").read_text()
triggers = dict(re.findall(r'/// (A\d\d) — (.+(?:\n///.*)*?)\n(?=pub struct)', rules_src))
triggers = {k: re.sub(r"\n/// ?", " ", v).strip() for k, v in triggers.items()}
explains = dict(re.findall(r'"(A\d\d)" => "(.+?)",\n', rules_src))
RULE_NAMES = {
    "A01": "Cache misses", "A02": "Cache expiry", "A03": "Runaway tool results",
    "A04": "Re-reads", "A05": "Exploring in the main context", "A06": "Compaction churn",
    "A07": "Idle MCP servers", "A08": "Thinking share", "A09": "Permission waits",
    "A10": "Long foreground commands", "A11": "Pasted input", "A12": "Chatty turns",
    "A13": "Rate-limit pacing", "A14": "Subagent model choice", "A15": "Error loops",
    "A16": "Hook overhead", "A17": "Oversized prefix", "A18": "Missing hand-off",
}

def esc(s):
    return html.escape(s, quote=False)

def code_md(s):
    return re.sub(r"`([^`]+)`", r"<code>\1</code>", esc(s))

def rule_link(code):
    return f'<a href="advisor.html#{code}">{code} {esc(RULE_NAMES[code])}</a>'

# id -> (what it means, what to do, related rule codes)
GUIDE = {
    "session_status": ("Whether Claude is working (BUSY), waiting for your next prompt (IDLE), waiting for you to answer a permission prompt (WAITING), or gone (ENDED — the process exited, the transcript is still readable).",
        "WAITING is the one to act on: nothing happens until you answer. If you keep missing prompts, allow the tool pattern in your settings instead.", ["A09"]),
    "turn_number": ("How many prompts you have sent this session. A resumed session counts from where it resumed.",
        "Nothing to do — it's the reference every per-turn figure uses.", []),
    "turn_elapsed": ("How long the current turn has been running, or how long the last one took.",
        "If it keeps climbing with a ▶ tool beside it, look at the Turn panel for what the wait is made of.", ["A10"]),
    "effort": ("The reasoning effort Claude is currently using — low, medium or high. Higher means more thinking tokens per reply.",
        "High effort on routine edits is billed thinking you rarely need; drop it for that kind of work and keep it for design and debugging.", ["A08"]),
    "process_cpu": ("How much of a CPU core the Claude Code process itself is using.",
        "Mostly diagnostic. Sustained high CPU while the session is idle is worth reporting.", []),
    "process_rss": ("How much memory the Claude Code process holds.",
        "A figure that only grows over a long session is a hint to end it at a natural boundary.", []),
    "context_size": ("How much of the model's attention window is occupied right now. It drives both cost (a bigger context costs more even when cached) and answer quality, which degrades as a session crowds toward the limit.",
        "If it's growing fast, check the Tools panel's <em>tokens → ctx</em> column for which single tool call pushed the most in before reaching for anything else.", []),
    "context_window": ("The ceiling this session is filling toward. It changes with the model, so the same context size means a different amount of headroom on different models.",
        "Compare it against context size to see real headroom — there's nothing to configure here directly.", []),
    "context_prefix": ("The fixed cost paid on every single turn regardless of task: system prompt, CLAUDE.md, tool definitions, skills. A bloated prefix taxes the whole session, forever.",
        "Trim CLAUDE.md, move rarely-used rules into skills, and disable MCP servers this project doesn't use.", ["A17"]),
    "context_velocity": ("How fast the remaining headroom is being burned — a leading indicator that context size alone can't give you.",
        "A high velocity late in a session is the cue to hand exploration off to a subagent, or to wrap up before the next compaction.", ["A05"]),
    "turns_until_compaction": ("A forecast of when the session's history gets summarised — and how much detail that summary will drop.",
        "If this is low and you're mid-task, write a short hand-off note now rather than after the compaction happens.", ["A18"]),
    "compactions": ("Each compaction spends output tokens on a summary and loses detail the model then has to re-derive.",
        "Two or more compactions in one session usually means it would have been cheaper to end the session at a natural boundary and start fresh.", ["A06", "A18"]),
    "cache_read": ("The cheap tokens — read at roughly a tenth of the input price. The bigger this is as a share of the total, the less the session costs.",
        "Read this next to the cache-hit ratio, not on its own — it's the numerator, not the whole health check.", []),
    "cache_write": ("Written at 1.25×–2× the input price, and it happens whenever the prefix changed or the cache entry expired.",
        "A big or repeated cache-write bill points straight at cache misses or cache expiry — check the Advisor.", ["A01", "A02"]),
    "fresh_input": ("Uncached tokens billed in full, almost always a large paste or genuinely new content introduced this turn.",
        "A spike here is usually a paste that could have been a file reference instead.", ["A11"]),
    "output": ("What you pay for the model's replies, thinking tokens included.",
        "Compare against thinking to see how much of the reply is reasoning versus visible text.", []),
    "thinking": ("Billed as output but never shown as text — a silent cost that's easy to miss.",
        "A high thinking share on routine edits is a sign to drop the effort level for that kind of work.", ["A08"]),
    "cache_hit_ratio": ("The single best proxy for whether this session is using prompt caching well. Green from 80 %, amber from 50 %, red below.",
        "Sustained below 60 %? Something in the prefix is changing every turn — a hook or status line printing the time is the usual suspect.", ["A01"]),
    "cache_ttl": ("Your real budget for how long you can pause mid-session before the next request pays a full cold rewrite.",
        "Pace batched questions to land inside this window rather than trickling them out past it.", ["A02"]),
    "cost": ("What this session would cost at API list price, even on a subscription plan with no per-token bill.",
        "Watch it alongside burn rate if you're trying to keep a session inside a budget.", []),
    "cost_by_model": ("Splits spend by model — the number that matters once subagents are running on something other than the main thread's model.",
        "If a cheap search or summarise task is running on your priciest model, point that subagent at a smaller one.", ["A14"]),
    "burn_rate": ("An early-warning $/hour figure extrapolated from the last 15 minutes, well before the final bill or a usage limit surprises you.",
        "A sudden jump is the moment to decide whether to keep going at this rate or change course.", ["A13"]),
    "input_rate": ("A proxy for how chatty the session is — tokens per minute sent to the model.",
        "Many tiny prompts cost more in aggregate than one prompt that bundles the same instructions.", ["A12"]),
    "limit_5h": ("Account-wide usage against the rolling 5-hour window — every session and subagent on the account draws from the same pool. Amber from 60 %, red from 85 %.",
        "If you're running several sessions in parallel, this is the number that actually caps you, not any single session's cost.", ["A13"]),
    "limit_7d": ("The longer account-wide window; a slower-moving version of the same constraint as the 5-hour figure.",
        "Useful for pacing heavy work across a week rather than a single session.", ["A13"]),
    "limit_reset": ("When the current usage window clears — tells you whether it's worth waiting it out instead of changing behaviour now.",
        "Close to the cap with a long time left before reset? That's when to act, not wait.", []),
    "limit_exhaustion": ("Projects, from the recent trend, whether usage hits 100 % before the window resets.",
        "If exhaustion is projected before the reset, move exploration and summarising work to cheaper subagents or pause heavy work.", ["A13"]),
    "turn_duration": ("The wall-clock time actually experienced — what you waited for this turn.",
        "Split it against model time, hook time and permission wait below to see where the time really went.", []),
    "api_calls": ("How many round-trips to the model happened inside one turn — each one re-sends the entire context.",
        "A turn with many calls often means many small tool round-trips; fewer, larger tool calls usually cost less.", []),
    "api_time": ("Time actually spent waiting on the model to respond — the baseline the other overheads are measured against.",
        "Not directly actionable; it's the number everything else in this panel is subtracted from.", []),
    "retry_time": ("Time spent on requests that had to be retried — pure waste, since no new work happened during it.",
        "Persistent retries point at a network or rate-limit issue outside a single session's control, but this confirms it wasn't something you did.", []),
    "hook_runs": ("How many hooks fired during the turn — each one runs synchronously and adds to what you wait for.",
        "A turn with unexpectedly many hook runs is worth checking against your hook matchers.", ["A16"]),
    "hook_ms": ("The total time hooks added to this turn, on top of the model's own response time.",
        "A slow hook after tool calls or at the end of a turn should run asynchronously, or match only the tools and paths it actually cares about.", ["A16"]),
    "permission_wait": ("Time the model sat completely idle waiting on a permission decision from you.",
        "A tool pattern approved the same way repeatedly is a candidate for the allow-list in your settings, which removes the prompt entirely.", ["A09"]),
    "queued_prompts": ("Prompts you typed while the model was still working on the previous one.",
        "Informational — a growing queue late in a task can mean it's time to interrupt instead of keep queuing.", []),
    "tool_calls": ("Which tools actually dominate this session's activity, by call count.",
        "A tool called far more than the task seems to need can indicate retrying or inefficient exploration.", []),
    "tool_errors": ("Errors are billed and re-injected into context exactly like successes are.",
        "The same tool failing with the same input repeatedly is the signal to interrupt and hand the model the fix directly instead of letting it keep retrying.", ["A15"]),
    "tool_p50": ("The typical duration for this tool — the number to expect on a normal call.",
        "Read alongside p95 rather than alone; a high median usually means the tool itself is just slow.", []),
    "tool_p95": ("The tail latency — what occasionally makes a turn feel much slower than usual.",
        "A high p95 on a tool that also runs in the foreground (long Bash calls, builds, test suites) is a candidate to run in the background instead.", ["A10"]),
    "tool_last_call": ("How long since this tool was last used — staleness, not activity.",
        "A tool or MCP server that hasn't been called in a while is still paying for its definitions on every turn; pairs with the MCP call count in Agents &amp; MCP.", ["A07"]),
    "tokens_to_ctx": ("Exactly how much context each tool's results added — the direct, measurable cause of context growth.",
        "The biggest single contributor here is almost always the right fix target: narrow the command, delegate to a subagent, or stop re-reading it.", ["A03", "A04", "A05"]),
    "top_ctx": ("The single largest results by context cost, named individually rather than summed by tool.",
        "Start with the top entry, not the tool average — one oversized result usually explains most of the growth.", ["A03"]),
    "agent_state": ("At a glance: is a subagent still running, finished cleanly, or stuck.",
        "A failed state with nothing following for a while is worth checking on directly rather than assuming it will recover.", []),
    "agent_tokens": ("Subagents have their own context and their own cost, entirely separate from the main thread's numbers.",
        "An expensive subagent doing simple search or summary work is a candidate to move to a cheaper model.", ["A14"]),
    "mcp_rss": ("Memory footprint of MCP server processes running alongside the session.",
        "Mostly diagnostic — a steadily growing figure over a long session is worth reporting to that server's maintainer.", []),
    "mcp_calls": ("How much an MCP server is actually used, in raw call count.",
        "Cross-reference with last call: a server whose definitions sit in every turn but shows no recent calls is pure overhead worth disabling.", ["A07"]),
    "file_touches": ("The blast radius of this session — every file it read, edited, or wrote, independent of what git ends up showing.",
        "Useful for a quick review of what a session actually touched before deciding what to commit.", []),
    "file_lines": ("Lines added and removed per file, from git — the real size of the change, not just the file count.",
        "A file with disproportionate churn relative to the stated task is worth a second look before commit.", []),
    "file_rereads": ("Re-reading a file that hasn't changed re-injects the whole thing into context for zero new information.",
        "Three or more re-reads is flagged ⚠ — point the model at a narrower range, or ask it to keep its own notes instead.", ["A04"]),
    "advice_saving": ("What following a recommendation saves for every remaining turn of the session — tokens, or seconds for the timing rules. It is also the ranking: the Advisor shows the biggest saving first.",
        "Treat it as an order of magnitude, not an invoice. When two recommendations are close, take the one that's easier to act on.", []),
}

# How each reading shows on screen, taken from the dashboard in the README.
READS = {
    "session_status": "● BUSY", "turn_number": "turn 14", "turn_elapsed": "02:31",
    "effort": "auto · medium · Max", "process_cpu": "3%", "process_rss": "412 MB",
    "context_size": "134k / 200k", "context_window": "/ 200k", "context_prefix": "sys+tools 28k",
    "context_velocity": "+9.4k/turn", "turns_until_compaction": "autocompact in ~3 turns est",
    "compactions": "compactions 1 (turn 9, −71k)",
    "cache_read": "cache read  ▇▇▇▇▇▇▇  1.62 M", "cache_write": "cache write ▇▇▇  148k",
    "fresh_input": "fresh in  ▇  24k", "output": "output  ▇▇  118k", "thinking": "└ thinking  ▇  61k",
    "cache_hit_ratio": "cache hit 90 %", "cache_ttl": "cache TTL  1h", "cost": "≈ $4.37",
    "burn_rate": "($3.61/h)", "input_rate": "in 12.4k/min",
    "limit_5h": "5 h  ▇▇▇▇▁▁  62 %", "limit_7d": "7 d  ▇▇▁▁▁▁  23 %", "limit_reset": "↺ 1h 48m",
    "limit_exhaustion": "exhausted in 2h 05m, after reset ✓",
    "turn_duration": "elapsed 2:31", "api_calls": "api 3 calls", "api_time": "api / tools  ≈0:02 / 0:46",
    "hook_runs": "hooks 6 runs", "hook_ms": "hooks 6 runs · 84 ms", "permission_wait": "permission waits 1 · 12 s",
    "queued_prompts": "queued  0",
    "tool_calls": "Read  71", "tool_errors": "ERR  3", "tool_p50": "p50  1.4s", "tool_p95": "p95  9.8s",
    "tool_last_call": "LAST  4:02", "tokens_to_ctx": "TOKENS→CTX  41.2k", "top_ctx": "top ctx: Read src/render.rs 6.1k",
    "agent_state": "◐ Explore  find render call sites  0:41", "agent_tokens": "92k  opus",
    "mcp_rss": "mcp playwright  188 MB", "mcp_calls": "9 calls · p95 2.1s",
    "file_touches": "R×6  E×5", "file_lines": "+210 −31", "file_rereads": "re-read ⚠",
    "advice_saving": "~4k/turn",
}

# slug, number, name, blurb, metrics.md section, margin note (html)
PANELS = [
    ("header", "top", "Header",
     "The two lines above the panels: the session at a glance — its state, the turn, what it is waiting on, where it runs, and what it has cost so far.",
     "Header", "The header is drawn in every view, terminal and docked."),
    ("context", "1", "Context",
     "How full the context window is, how fast it's filling, and how many turns are left before the next compaction rewrites it.",
     "Context", "Amber from 60 % of the window, red from 80 %."),
    ("tokens-cost", "2", "Tokens & Cost",
     "Where every token went — cache read, cache write, fresh input, output and thinking — and what the session costs, live.",
     "Tokens & Cost", "Cache hit is green from 80 %, amber from 50 %, red below."),
    ("limits", "3", "Limits",
     "Account-wide 5-hour and 7-day usage, when the window resets, and whether the current trend runs out before it does.",
     "Limits", "Exact figures need <code>cctop install</code>; without it the panel says so."),
    ("turn", "4", "Turn",
     "What the current turn is actually waiting on — the model, a tool, a hook, a permission prompt, or nothing at all.",
     "Turn", "Hook and permission timings become exact after <code>cctop install</code>."),
    ("tools", "5", "Tools",
     "Call counts, error rates, latency, and exactly how many tokens each tool pushed into context.",
     "Tools", "<kbd>s</kbd> sorts the table, <kbd>f</kbd> filters it."),
    ("agents-mcp", "6", "Agents & MCP",
     "Subagents and MCP server processes running alongside the main thread, and what each is costing.",
     "Agents & MCP", "Subagents have their own context; their tokens are not in panel 2."),
    ("files", "7", "Files",
     "Every file this session touched, how much changed, and which reads were wasted re-reads.",
     "Files", "Line counts come from git; outside a repository the column stays empty."),
]
ORDER = [p[0] for p in PANELS] + ["events", "advisor"]
TITLES = {p[0]: (p[1], p[2]) for p in PANELS}
TITLES["events"] = ("8", "Events")
TITLES["advisor"] = ("9", "Advisor")

MARK = '<svg viewBox="0 0 256 256" aria-hidden="true"><g fill="#D97757"><path d="M128 111 C118 98 100 98 92 106 C87 111 91 118 98 116 C107 113 117 115 128 121 C139 115 149 113 158 116 C165 118 169 111 164 106 C156 98 138 98 128 111 Z"/><rect x="78" y="118" width="100" height="30" rx="2"/><rect x="84" y="155" width="88" height="22" rx="2"/><rect x="90" y="184" width="76" height="20" rx="2"/><path d="M96 211 H160 L156 230 L146 222 L136 240 L128 254 L120 240 L110 222 L100 230 Z"/></g><g><circle cx="88" cy="60" r="30" fill="#3A3F47" stroke="currentColor" stroke-width="9"/><circle cx="168" cy="60" r="30" fill="#3A3F47" stroke="currentColor" stroke-width="9"/><path d="M118 56 Q128 45 138 56" fill="none" stroke="currentColor" stroke-width="8" stroke-linecap="round"/><path d="M58 55 L42 51 M198 55 L214 51" fill="none" stroke="currentColor" stroke-width="8" stroke-linecap="round"/></g></svg>'

HEAD = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>@TITLE@ — cctop panel guide</title>
<meta name="description" content="@DESC@">
<link rel="icon" href="../assets/favicon.svg" type="image/svg+xml">
<link rel="alternate icon" href="../assets/favicon.ico">
<link rel="preload" href="../assets/fonts/ibm-plex-sans-latin.woff2" as="font" type="font/woff2" crossorigin>
<link rel="preload" href="../assets/fonts/ibm-plex-mono-400-latin.woff2" as="font" type="font/woff2" crossorigin>
<link rel="preload" href="../assets/fonts/ibm-plex-sans-condensed-700-latin.woff2" as="font" type="font/woff2" crossorigin>
<style>
{FONTS_CSS}
{SITE_CSS}
</style>
</head>
<body>
<div class="page">
<header class="masthead">
  <a class="brand" href="../" aria-label="cctop home">{MARK}<b>cctop</b><span>Operator's manual</span></a>
  <nav class="nav" aria-label="Sections">
    <a href="../#dashboard">The dashboard</a>
    <a href="./" aria-current="page">Panel guide</a>
    <a href="advisor.html">Advisor</a>
    <a href="../#install">Install</a>
    <a href="../metrics.html">Reference</a>
    <a href="https://github.com/tomstagl/cctop">GitHub</a>
  </nav>
</header>
"""

FOOT = """<footer class="colophon">
  <a class="brand" href="../"><b>cctop</b></a>
  <div><nav aria-label="Footer"><a href="../">Home</a><a href="./">Panel guide</a><a href="../metrics.html">How each number is measured</a><a href="https://github.com/tomstagl/cctop">GitHub</a></nav><span>© 2026 Thomas Stagl</span></div>
</footer>
</div>
</body>
</html>
"""

def pager(slug):
    i = ORDER.index(slug)
    prev = ORDER[i - 1] if i > 0 else None
    nxt = ORDER[i + 1] if i + 1 < len(ORDER) else None
    def label(slug):
        num, name = TITLES[slug]
        return f"{esc(num)} {esc(name)}" if num.isdigit() else esc(name)
    left = f'<a href="{prev}.html">← {label(prev)}</a>' if prev else '<a href="./">← Panel guide</a>'
    right = f'<a href="{nxt}.html">{label(nxt)} →</a>' if nxt else '<a href="../metrics.html">How each number is measured →</a>'
    return f'<nav class="pager" aria-label="Panels">{left}{right}</nav>\n'

def page(slug, title, desc, body, crumb=None):
    crumbs = f'<p class="crumbs"><a href="../">cctop</a> / <a href="./">Panel guide</a>{" / " + esc(crumb) if crumb else ""}</p>\n'
    out = HEAD.replace("@TITLE@", esc(title)).replace("@DESC@", esc(desc)) + crumbs + body + FOOT
    (OUT / f"{slug}.html").write_text(out)

def opener(num, name, blurb, note, h1=None, extra=""):
    no = f'<small>§</small>{esc(num)}' if num.isdigit() else esc(num)
    return f"""<section class="sec first">
  <div class="mg"><span class="no">{no}</span><span class="lbl">{esc(name)}</span>{f'<span class="note"><b>Note</b>{note}</span>' if note else ''}</div>
  <div class="body">
    <h1 style="font-size:clamp(30px,3.6vw,44px);line-height:1.02;margin-bottom:14px">{h1 or esc(name)}</h1>
    <p>{blurb}</p>{extra}
  </div>
</section>
"""

def entry(mid, name, unit, sources, estimate):
    why, do, codes = GUIDE.get(mid, ("", "", []))
    reads = READS.get(mid)
    src = " · ".join(SOURCES.get(s, s) for s in sources.split())
    exact = EXACT.get(estimate, estimate)
    parts = [f'<article class="entry" id="{mid}">', '<div class="mg">', f"<h3>{esc(name)}</h3>"]
    if reads:
        parts.append(f'<span class="reads">reads as<br><code>{esc(reads)}</code></span>')
    parts.append("</div><dl>")
    if why:
        parts.append(f"<dt>What it means</dt><dd>{why}</dd>")
    if do:
        parts.append(f'<dt class="act">What to do</dt><dd>{do}</dd>')
    if codes:
        parts.append(f'<dt>Advisor</dt><dd class="meta">{" · ".join(rule_link(c) for c in codes)}</dd>')
    parts.append(f'<dt>Measured</dt><dd class="meta">{esc(unit)} · from {esc(src)} · {esc(exact)} · <a href="../metrics.html#{mid}">how</a></dd>')
    parts.append("</dl></article>")
    return "\n".join(parts)

# ---- index -------------------------------------------------------------
rows = "\n".join(
    f'<li><a href="{slug}.html"><span class="n">{esc(num) if num.isdigit() else "—"}</span><span class="t"><b>{esc(name)}</b><span>{esc(blurb)}</span></span></a></li>'
    for slug, num, name, blurb, _, _ in PANELS
) + (
    '\n<li><a href="events.html"><span class="n">8</span><span class="t"><b>Events</b><span>The chronological stream of tool, hook, permission and compaction events behind every other panel.</span></span></a></li>'
    '\n<li><a href="advisor.html"><span class="n">9</span><span class="t"><b>Advisor</b><span>All 18 rules the Advisor checks, what triggers each one, and what to do about it.</span></span></a></li>'
)
index_body = opener("Guide", "Panel by panel", "What each of the nine panels shows, what every reading on it means, and what to actually do when it moves. Each entry says how the reading appears on screen, what it tells you, the change to make, and where the number comes from.",
    'The numbers are the panel digits on screen: <kbd>1</kbd>–<kbd>9</kbd> show or hide each one.', h1="Every reading, explained",
    extra=f'\n    <ul class="toc">{rows}</ul>')
page("index", "Panel guide", "What every cctop panel and reading means, why it matters, and what to do about it.", index_body)

# ---- one page per metrics-backed panel ----------------------------------
for slug, num, name, blurb, md_key, note in PANELS:
    entries = "\n".join(entry(*row) for row in sections.get(md_key, []))
    body = opener(num, name, esc(blurb), note) + entries + "\n" + pager(slug)
    page(slug, name, f"What the {name} panel shows in cctop, what each reading means, and what to do about it.", body, crumb=name)

# ---- Events (no metrics.md table — it's a log, not a computed reading) ----
events_body = opener("8", "Events", "The chronological stream every other panel is built from: tool calls starting and finishing, hooks firing, permission prompts opening and closing, compactions, and notes cctop records about the session. Nothing here is aggregated — it's the raw timeline.",
    "The newest line is at the bottom; <kbd>f</kbd> filters by kind.") + """<article class="entry" id="events">
<div class="mg"><h3>The timeline</h3><span class="reads">reads as<br><code>20:41:17 perm   Bash allowed (auto)</code></span></div>
<dl>
<dt>What it means</dt><dd>The other panels show you totals and rates; Events shows you the order things happened in. That's what you need to reconstruct why one particular turn was slow or expensive, rather than just knowing that it was. Each line is a time, a kind — <code>tool</code>, <code>hook</code>, <code>perm</code>, <code>agent</code>, <code>compact</code>, <code>api</code>, <code>note</code>, <code>away</code> — and what happened.</dd>
<dt class="act">What to do</dt><dd>Use it to find the exact moment things went sideways — a run of tool errors, a compaction, a long permission wait — then switch to the relevant numbered panel for the detail behind that moment.</dd>
</dl></article>
<article class="entry" id="notes">
<div class="mg"><h3>Notes</h3><span class="reads">reads as<br><code>20:41:39 note   rate-limit 5h crossed 60 %</code></span></div>
<dl>
<dt>What it means</dt><dd>Lines cctop writes itself when a threshold is crossed: context past 80 % of the window, a compaction projected within two turns, the 5-hour limit past 60, 80 or 95 %, the limit projected to run out before its reset, a cache-hit ratio under 50 % over five turns, a tool running longer than a minute, a permission prompt open longer than half a minute, an MCP server that exited, or time spent on API retries. Each also shows briefly as a toast, and with <code>--notify</code> the critical ones reach your desktop.</dd>
<dt class="act">What to do</dt><dd>Treat them as the moments worth a glance at the dashboard; the panel the note names has the detail.</dd>
</dl></article>
""" + pager("events")
page("events", "Events", "What the Events panel shows and how to use it.", events_body, crumb="Events")

# ---- Advisor: the saving, then the full rule reference --------------------
rule_items = []
for code in sorted(explains, key=lambda c: int(c[1:])):
    trig = triggers.get(code, "")
    rule_items.append(
        f'<li id="{code}"><span class="code">{code}</span><div><h3>{esc(RULE_NAMES.get(code, ""))}</h3>'
        f'<p class="when"><b>Fires when</b>{esc(trig)}</p>'
        f'<p>{code_md(explains[code])}</p></div></li>'
    )
advisor_body = opener("9", "Advisor", "Eighteen deterministic rules over the same data the other eight panels show — no model call, no network. Each rule fires on a concrete threshold from this session's own evidence, names what it found, and estimates what following it saves for the rest of the session. The panel shows one recommendation at a time, ranked by that estimate; <kbd>n</kbd> shows the next, <kbd>Enter</kbd> the explanation below.",
    'Ask a running session about any of them — the bundled <code>cctop-insights</code> skill answers from the same numbers — or run <code>cctop advise</code>.') \
    + "\n".join(entry(*row) for row in sections.get("Advisor", [])) \
    + f"""
<section class="sec">
  <div class="mg"><span class="lbl">The 18 rules</span><span class="note"><b>Cross-references</b>Every panel page links the rules that watch its readings.</span></div>
  <div class="body">
    <h2>What each rule watches, and what to do when it fires</h2>
    <ul class="rules">{"".join(rule_items)}</ul>
  </div>
</section>
""" + pager("advisor")
page("advisor", "Advisor", "All 18 Advisor rules: what triggers each one and what to do about it.", advisor_body, crumb="Advisor")

print(f"site/guide/*.html written ({len(PANELS) + 3} pages)")
