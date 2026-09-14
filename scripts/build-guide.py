#!/usr/bin/env python3
"""Build site/guide/*.html — one page per panel, hand-authored "why it
matters / what to do" text over the metrics from docs/metrics.md and the
Advisor rules from src/advisor/rules.rs. Run via `make site`.
"""
import re, html, pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
OUT = ROOT / "site/guide"
OUT.mkdir(parents=True, exist_ok=True)

FONTS_CSS = (ROOT / "site/fonts.css").read_text().strip()
SITE_CSS = (ROOT / "site/site.css").read_text().strip()

# ---- parse docs/metrics.md into {section: [(id, name, unit), ...]} --------
md = (ROOT / "docs/metrics.md").read_text()
sections = {}
cur = None
for line in md.splitlines():
    if line.startswith("## "):
        cur = line[3:].strip()
        sections[cur] = []
    elif line.startswith("| **") and cur:
        cells = [c.strip() for c in line.strip("|").split("|")]
        m = re.match(r"\*\*(.+?)\*\* <a id=\"(.+?)\"></a> `(.+?)`", cells[0])
        if m:
            name, mid, code = m.groups()
            sections[cur].append((mid, name, cells[1]))

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

def rule_link(code):
    return f'<a href="advisor.html#{code}"><code>{code}</code></a>'

def rules_html(codes):
    return " · ".join(f"{rule_link(c)} {esc(RULE_NAMES[c])}" for c in codes)

# id -> (why it matters, what to do, related rule codes)
GUIDE = {
    "context_size": ("Literally how much of the model's attention window is occupied right now — it drives both cost (a bigger context costs more even when cached) and answer quality, which degrades as a session crowds toward the limit.",
        "If it's growing fast, check the Tools panel's <em>tokens → context</em> column for which single tool call pushed the most in before reaching for anything else.", []),
    "context_window": ("The ceiling this session is filling toward. It changes with the model, so the same context size means a different amount of headroom on different models.",
        "Compare it against context size to see real headroom — there's nothing to configure here directly.", []),
    "context_prefix": ("The fixed cost paid on every single turn regardless of task: system prompt, CLAUDE.md, tool schemas, skills. A bloated prefix taxes the whole session, forever.",
        "Trim CLAUDE.md, move rarely-used rules into skills, and disable MCP servers this project doesn't use.", ["A17"]),
    "context_velocity": ("How fast the remaining headroom is being burned — a leading indicator that context size alone can't give you.",
        "A high velocity late in a session is the cue to hand exploration off to a subagent, or to wrap up before the next compaction.", ["A05"]),
    "turns_until_compaction": ("A forecast of when the session's history gets summarized — and how much detail that summary will drop.",
        "If this is low and you're mid-task, write a short hand-off note now rather than after the compaction happens.", ["A18"]),
    "compactions": ("Each compaction spends output tokens on a summary and loses detail the model then has to re-derive.",
        "Two or more compactions in one session usually means it would have been cheaper to end the session at a natural boundary and start fresh.", ["A06", "A18"]),
    "cache_read": ("The cheap tokens — read at roughly a tenth of the input price. The bigger this is as a share of the total, the less the session costs.",
        "Read this next to cache hit ratio, not on its own — it's the numerator, not the whole health check.", []),
    "cache_write": ("Written at 1.25×–2× the input price, and it happens whenever the prefix changed or the cache entry expired.",
        "A big or repeated cache-write bill points straight at cache misses or cache expiry — check the Advisor.", ["A01", "A02"]),
    "fresh_input": ("Uncached tokens billed in full, almost always a large paste or genuinely new content introduced this turn.",
        "A spike here is usually a paste that could have been a file reference instead.", ["A11"]),
    "output": ("What you pay for the model's replies, thinking tokens included.",
        "Compare against thinking to see how much of the reply is reasoning versus visible text.", []),
    "thinking": ("Billed as output but never shown as text — a silent cost that's easy to miss.",
        "A high thinking share on routine edits is a sign to drop the effort level for that kind of work.", ["A08"]),
    "cache_hit_ratio": ("The single best proxy for whether this session is using prompt caching well.",
        "Sustained below 60%? Something in the prefix is changing every turn — a hook or status line injecting the time is the usual suspect.", ["A01"]),
    "cache_ttl": ("Your real budget for how long you can pause mid-session before the next request pays a full cold rewrite.",
        "Pace batched questions to land inside this window rather than trickling them out past it.", ["A02"]),
    "cost": ("What this session would cost at API list price, even on a subscription plan with no per-token bill.",
        "Watch it alongside burn rate if you're trying to keep a session inside a budget.", []),
    "cost_by_model": ("Splits spend by model — the number that matters once subagents are running on something other than the main thread's model.",
        "If a cheap search/summarize task is running on your priciest model, point that subagent at a smaller one.", ["A14"]),
    "burn_rate": ("An early-warning $/hour figure extrapolated from the last 15 minutes, well before the final bill or a usage limit surprises you.",
        "A sudden jump is the moment to decide whether to keep going at this rate or change course.", ["A13"]),
    "input_rate": ("A proxy for how chatty the session is — tokens per minute sent to the API.",
        "Many tiny prompts cost more in aggregate than one prompt that bundles the same instructions.", ["A12"]),
    "limit_5h": ("Account-wide usage against the rolling 5-hour window — every session and subagent on the account draws from the same pool.",
        "If you're running several sessions in parallel, this is the number that actually caps you, not any single session's cost.", ["A13"]),
    "limit_7d": ("The longer account-wide window; a slower-moving version of the same constraint as the 5-hour figure.",
        "Useful for pacing heavy work across a week rather than a single session.", ["A13"]),
    "limit_reset": ("When the current usage window clears — tells you whether it's worth waiting it out instead of changing behavior now.",
        "Close to the cap with a long time left before reset? That's when to act, not wait.", []),
    "limit_exhaustion": ("Projects, from the recent trend, whether usage hits 100% before the window resets.",
        "If exhaustion is projected before the reset, move exploration and summarizing work to cheaper subagents or pause heavy work.", ["A13"]),
    "turn_duration": ("The wall-clock time actually experienced — what the user waited for this turn.",
        "Split it against API time, hook time and permission wait below to see where the time really went.", []),
    "api_calls": ("How many round-trips happened inside one turn — each one re-sends the entire context.",
        "A turn with many calls often means many small tool round-trips; fewer, larger tool calls usually cost less.", []),
    "api_time": ("Time actually spent waiting on the model to respond — the baseline the other overheads are measured against.",
        "Not directly actionable; it's the number everything else in this panel is subtracted from.", []),
    "retry_time": ("Time spent on requests that had to be retried — pure waste, since no new work happened during it.",
        "Persistent retries point at a network or rate-limit issue outside a single session's control, but this confirms it wasn't something you did.", []),
    "hook_runs": ("How many hooks fired during the turn — each one runs synchronously and adds to what the user waits for.",
        "A turn with unexpectedly many hook runs is worth checking against your matcher patterns.", ["A16"]),
    "hook_ms": ("The total time hooks added to this turn, on top of the model's own response time.",
        "A slow PostToolUse or Stop hook should run asynchronously, or have its matcher narrowed to the tools and paths it actually cares about.", ["A16"]),
    "permission_wait": ("Time the model sat completely idle waiting on a human permission decision.",
        "A tool pattern approved the same way repeatedly is a candidate for <code>permissions.allow</code>, which removes the prompt entirely.", ["A09"]),
    "queued_prompts": ("Prompts typed while the model was still working on the previous one.",
        "Informational — a growing queue late in a task can mean it's time to interrupt instead of keep queuing.", []),
    "tool_calls": ("Which tools actually dominate this session's activity, by call count.",
        "A tool called far more than the task seems to need can indicate retrying or inefficient exploration.", []),
    "tool_errors": ("Errors are billed and re-injected into context exactly like successes are.",
        "The same tool failing with the same input repeatedly is the signal to interrupt and hand the model the fix directly instead of letting it keep retrying.", ["A15"]),
    "tool_p50": ("The typical duration for this tool — the number to expect on a normal call.",
        "Read alongside p95 rather than alone; a high median usually means the tool itself is just slow.", []),
    "tool_p95": ("The tail latency — what occasionally makes a turn feel much slower than usual.",
        "A high p95 on a tool that also runs in the foreground (long Bash calls, builds, test suites) is a candidate to background instead.", ["A10"]),
    "tool_last_call": ("How long since this tool was last used — staleness, not activity.",
        "A tool or MCP server that hasn't been called in a while is still paying its schema tax on every prefix; pairs with Agents & MCP's MCP call count.", ["A07"]),
    "tokens_to_ctx": ("Exactly how much context each tool's results added — the direct, measurable cause of context growth.",
        "The biggest single contributor here is almost always the right fix target: narrow the command, delegate to a subagent, or stop re-reading it.", ["A03", "A04", "A05"]),
    "top_ctx": ("The single largest results by context cost, named individually rather than summed by tool.",
        "Start with the top entry, not the tool average — one oversized result usually explains most of the growth.", ["A03"]),
    "agent_state": ("At a glance: is a subagent still running, finished cleanly, or stuck.",
        "A \"failed\" state with nothing following for a while is worth checking on directly rather than assuming it will recover.", []),
    "agent_tokens": ("Subagents have their own context and their own cost, entirely separate from the main thread's numbers.",
        "An expensive subagent doing simple search or summary work is a candidate to move to a cheaper model.", ["A14"]),
    "mcp_rss": ("Memory footprint of MCP server processes running alongside the session.",
        "Mostly diagnostic — a steadily growing RSS over a long session is worth reporting to that server's maintainer.", []),
    "mcp_calls": ("How much an MCP server is actually used, in raw call count.",
        "Cross-reference with tool last-call: a server whose schemas sit in every prefix but shows no recent calls is pure overhead worth disabling.", ["A07"]),
    "file_touches": ("The blast radius of this session — every file it read, edited, or wrote, independent of what git ends up showing.",
        "Useful for a quick review of what a session actually touched before deciding what to commit.", []),
    "file_lines": ("Signed lines added and removed per file, from git — the real size of the change, not just the file count.",
        "A file with disproportionate churn relative to the stated task is worth a second look before commit.", []),
    "file_rereads": ("Re-reading a file that hasn't changed re-injects the whole thing into context for zero new information.",
        "Three or more re-reads is flagged — point the model at a narrower offset/limit range, or ask it to keep its own notes instead.", ["A04"]),
}

PANELS = [
    ("context", "1", "Context",
     "How full the context window is, how fast it's filling, and how many turns are left before the next compaction rewrites it.",
     "Context"),
    ("tokens-cost", "2", "Tokens & Cost",
     "Where every token went — cache read, cache write, fresh input, output and thinking — and what the session costs, live.",
     "Tokens & Cost"),
    ("limits", "3", "Limits",
     "Account-wide 5-hour and 7-day usage, when the window resets, and whether the current trend runs out before it does.",
     "Limits"),
    ("turn", "4", "Turn",
     "What the current turn is actually waiting on — the model, a hook, a permission prompt, or nothing at all.",
     "Turn"),
    ("tools", "5", "Tools",
     "Call counts, error rates, latency, and exactly how many tokens each tool pushed into context.",
     "Tools"),
    ("agents-mcp", "6", "Agents & MCP",
     "Subagents and MCP server processes running alongside the main thread, and what each is costing.",
     "Agents & MCP"),
    ("files", "7", "Files",
     "Every file this session touched, how much changed, and which reads were wasted re-reads.",
     "Files"),
]

HEAD = f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>@TITLE@ — cctop guide</title>
<meta name="description" content="@DESC@">
<link rel="icon" href="../assets/favicon.svg" type="image/svg+xml">
<link rel="alternate icon" href="../assets/favicon.ico">
<style>
{FONTS_CSS}
{SITE_CSS}
.why{{background:var(--panel);border:1px solid var(--line);border-radius:6px;padding:10px 14px;margin:6px 0 18px}}
.why b{{font-family:var(--mono);font-size:11.5px;letter-spacing:.06em;text-transform:uppercase;color:var(--accent);display:block;margin-bottom:4px}}
.why.do b{{color:var(--ok)}}
.metric{{margin-top:28px;padding-top:4px;border-top:1px solid var(--line-soft)}}
.metric:first-of-type{{border-top:0}}
.metric h3{{margin-bottom:2px}}
.metric .unit{{font-family:var(--mono);font-size:12px;color:var(--muted);margin:0 0 10px}}
.rules{{font-family:var(--mono);font-size:12.5px;color:var(--muted);margin:2px 0 0}}
.guide-grid{{display:grid;grid-template-columns:repeat(auto-fit,minmax(260px,1fr));gap:12px;margin-top:20px}}
.guide-grid a{{display:block;background:var(--panel);border:1px solid var(--line);border-radius:6px;padding:14px 16px;text-decoration:none;color:var(--ink)}}
.guide-grid a:hover,.guide-grid a:focus-visible{{border-color:var(--accent)}}
.guide-grid h3{{margin:0 0 6px;color:var(--accent);font-family:var(--mono);font-size:14px;font-weight:500}}
.guide-grid p{{margin:0;color:var(--muted);font-size:14px}}
.rule{{background:var(--term-bg);color:var(--term-fg);border:1px solid var(--term-border);border-radius:6px;padding:12px 14px;font-family:var(--mono);font-size:13px;margin-bottom:10px;scroll-margin-top:16px}}
.rule .h{{color:var(--term-warn)}}
.rule .a{{color:var(--term-acc)}}
.rule .t{{color:var(--term-dim)}}
</style>
</head>
<body>
<div class="wrap">
<header class="top">
  <a href="../" aria-label="cctop home">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="../assets/cctop-lockup-horizontal-dark.svg">
      <img src="../assets/cctop-lockup-horizontal-light.svg" alt="cctop" width="90" height="36">
    </picture>
  </a>
  <nav>
    <a href="./">Guide</a>
    <a href="../metrics.html">Metrics</a>
    <a href="../#panels">Panels</a>
    <a href="https://github.com/tomstagl/cctop">GitHub</a>
  </nav>
</header>
<main class="metrics">
"""

FOOT = """</main>
<footer><a href="./">← Guide</a><a href="../">cctop</a><a href="https://github.com/tomstagl/cctop">GitHub</a></footer>
</div>
</body>
</html>
"""

def page(slug, title, desc, body):
    out = HEAD.replace("@TITLE@", esc(title)).replace("@DESC@", esc(desc)) + body + FOOT
    (OUT / f"{slug}.html").write_text(out)

# ---- index -------------------------------------------------------------
cards = "\n".join(
    f'<a href="{slug}.html"><h3>{num} {esc(name)}</h3><p>{esc(blurb)}</p></a>'
    for slug, num, name, blurb, _ in PANELS
) + '\n<a href="events.html"><h3>8 Events</h3><p>The chronological stream of tool, hook, permission and compaction events behind every other panel.</p></a>' \
    + '\n<a href="advisor.html"><h3>9 Advisor</h3><p>All 18 rules the Advisor checks, what triggers each one, and what to do about it.</p></a>'
index_body = f"""<h1>Guide</h1>
<p>What each of cctop's nine panels shows, why each metric is worth looking at, and what to actually do when it moves. For how each number is computed and which data source it comes from, see <a href="../metrics.html">every metric, explained</a>.</p>
<div class="guide-grid">{cards}</div>
"""
page("index", "Guide", "What every cctop panel and metric means, why it matters, and what to do about it.", index_body)

# ---- one page per metrics-backed panel ----------------------------------
for slug, num, name, blurb, md_key in PANELS:
    rows = sections.get(md_key, [])
    parts = [f"<h1>{num} {esc(name)}</h1>", f"<p>{esc(blurb)}</p>"]
    for mid, mname, unit in rows:
        why, do, codes = GUIDE.get(mid, ("", "", []))
        parts.append(f'<div class="metric" id="{mid}">')
        parts.append(f"<h3>{esc(mname)}</h3>")
        parts.append(f'<p class="unit"><code>{esc(mid)}</code> · {esc(unit)}</p>')
        if why:
            parts.append(f'<div class="why"><b>Why it matters</b>{why}</div>')
        if do:
            extra = f' <span class="rules">Advisor: {rules_html(codes)}</span>' if codes else ""
            parts.append(f'<div class="why do"><b>What to do</b>{do}{extra}</div>')
        parts.append("</div>")
    page(slug, name, f"What {name} shows in cctop, why each metric matters, and what to do about it.", "\n".join(parts))

# ---- Events (no metrics.md table — it's a log, not a computed metric) ----
events_body = """<h1>8 Events</h1>
<p>The chronological stream every other panel is built from: tool calls starting and finishing, hooks firing, permission prompts opening and closing, compactions, and notes cctop records about the session. Nothing here is aggregated — it's the raw timeline.</p>
<div class="metric">
<div class="why"><b>Why it matters</b>The other panels show you totals and rates; Events shows you the order things happened in. That's what you need to reconstruct why one particular turn was slow or expensive, rather than just knowing that it was.</div>
<div class="why do"><b>What to do</b>Use it to find the exact moment things went sideways — a run of tool errors, a compaction, a long permission wait — then switch to the relevant numbered panel for the detail behind that moment.</div>
</div>
"""
page("events", "Events", "What the Events panel shows and how to use it.", events_body)

# ---- Advisor: full rule reference ---------------------------------------
rule_cards = []
for code in sorted(explains, key=lambda c: int(c[1:])):
    trig = triggers.get(code, "")
    action = explains[code]
    rule_cards.append(
        f'<div class="rule" id="{code}"><span class="t">{code}</span> — <span class="h">{esc(RULE_NAMES.get(code, ""))}</span><br>'
        f'<span class="t">Fires when:</span> {esc(trig)}<br>'
        f'<span class="a">{esc(action)}</span></div>'
    )
advisor_body = f"""<h1>9 Advisor</h1>
<p>Eighteen deterministic rules over the same data the other eight panels show — no model call, no network. Each rule fires on a concrete threshold, names the evidence it found, and estimates what following it saves for the rest of the session. The Advisor panel surfaces one recommendation at a time, ranked by that estimate; every rule below can also fire silently and show up cross-referenced from the panel page where its trigger lives.</p>
<p>Ask a running session about any of them directly — the bundled <code>cctop-insights</code> skill answers from <code>cctop query</code> — or run <code>cctop advise</code> for the current recommendations.</p>
{"".join(rule_cards)}
"""
page("advisor", "Advisor", "All 18 Advisor rules: what triggers each one and what to do about it.", advisor_body)

print(f"site/guide/*.html written ({len(PANELS) + 3} pages)")
