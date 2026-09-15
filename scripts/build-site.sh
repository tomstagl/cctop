#!/usr/bin/env bash
# Build site/index.html and site/metrics.html from the README's marked blocks,
# docs/metrics.md and the themes. No dependencies beyond python3.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 - <<'PY'
import re, html, pathlib, glob

readme = pathlib.Path("README.md").read_text()
tmpl = pathlib.Path("site/index.template.html").read_text()
# Inline the stylesheets: a few KB in the document beats a render-blocking request.
tmpl = tmpl.replace("@@FONTS_CSS@@", pathlib.Path("site/fonts.css").read_text().strip())
tmpl = tmpl.replace("@@SITE_CSS@@", pathlib.Path("site/site.css").read_text().strip())

def block(name):
    m = re.search(rf"<!-- {name}:start -->\n(.*?)\n<!-- {name}:end -->", readme, re.S)
    if not m:
        raise SystemExit(f"README.md has no <!-- {name}:start --> block")
    return m.group(1).strip()

def inline_md(s):
    s = html.escape(s, quote=False)
    s = re.sub(r"`([^`]+)`", r"<code>\1</code>", s)
    s = re.sub(r"\*\*([^*]+)\*\*", r"<strong>\1</strong>", s)
    return s

# hero: the <p align="center"><strong>…</strong></p> line → its text
hero = re.sub(r"<[^>]+>", "", block("hero")).strip()
# install: fenced code → text; the trailing "# comment" on each line drawn dim
install = re.sub(r"^```\n|\n```$", "", block("install").strip("`\n"))
install = re.sub(r"(#.*)$", r'<span class="c">\1</span>', html.escape(install), flags=re.M)
# themes: name + the four signal colours, on the theme's own background
themes = []
for path in sorted(glob.glob("themes/*.toml")):
    kv = dict(re.findall(r'^(\w+)\s*=\s*"([^"]+)"', pathlib.Path(path).read_text(), re.M))
    sw = "".join(f'<i style="background:{kv[k]}"></i>' for k in ("accent", "ok", "warn", "crit"))
    bar = f'<span class="bar" style="color:{kv["accent"]}">▇▇▇▇▇<span style="color:{kv["dim"]}">▁▁▁</span></span>'
    themes.append(f'<div class="theme" style="background:{kv["bg"]};color:{kv["fg"]};border-color:{kv["border"]}">{kv["name"]}{bar}{sw}</div>')
# mockup: the fenced block after "## What it looks like" (the terminal view):
# the ledger's digits become callouts that the legend below repeats, the four
# lights' names and the nudge are marked, the key line is dimmed
m = re.search(r"## What it looks like\n\n```\n(.*?)\n```", readme, re.S)
mockup = html.escape(m.group(1)) if m else ""
mockup = re.sub(r"^ (\d) ([A-Z][a-z]+) ", r' <b class="co">\1</b> <span class="t">\2</span> ', mockup, flags=re.M)
mockup = re.sub(r"([○◐●◆]) (context|cache|limits|rework)\b", r'\1 <span class="t">\2</span>', mockup)
mockup = re.sub(r"^( ▸ .*)$", r'<b class="nudge">\1</b>', mockup, flags=re.M)
mockup = re.sub(r"^( \?help .*)$", r'<span class="d">\1</span>', mockup, flags=re.M)
# panel mockup: the fenced block under "**Panel view.**" in the same section
m = re.search(r"\*\*Panel view\.\*\*.*?\n```\n(.*?)\n```", readme, re.S)
panel_mockup = html.escape(m.group(1)) if m else ""
panel_mockup = re.sub(r"╭(coach|[○◐●] \w+)", r'╭<span class="t">\1</span>', panel_mockup)

# The demo block needs assets produced by `make demo`; drop it until they exist.
if not pathlib.Path("site/assets/two-pane.png").exists():
    tmpl = re.sub(r'\s*<figure>\s*<div class="plate">.*?</figure>\n', "\n", tmpl, count=1, flags=re.S)
out = (tmpl.replace("@HEADLINE@", html.escape(hero))
           .replace("@INSTALL@", install)
           .replace("@THEMES@", "\n".join(themes))
           .replace("@MOCKUP@", mockup)
           .replace("@PANEL_MOCKUP@", panel_mockup))
pathlib.Path("site/index.html").write_text(out)

# metrics.html — the appendix: how each number is measured, from docs/metrics.md.
# The registry's anchors and identifiers stay out of the page; the identifier
# becomes the row's id so guide links keep working.
SOURCES = {
    "D1": "session registry", "D2": "transcript", "D2a": "subagent transcripts",
    "D3": "status line (after cctop install)", "D4": "hook events (after cctop install)",
    "D5": "process tree", "D7": "git", "D8": "file tool calls", "D9": "price table",
    "D10": "OpenTelemetry", "D11": "Claude Code's cost record",
}
EXACT = {
    "never": "exact",
    "≈ always": "always an estimate",
    "≈ (always priced from the table)": "always an estimate — priced from the list table",
    "≈ when any part is estimated": "≈ when any part is estimated",
    "≈ per turn": "exact per session, ≈ per turn",
    "≈ until hook timings replace them": "≈ until hook timings replace it",
    "≈ without OTel": "≈ unless OpenTelemetry is on",
    "est when computed from the transcript alone": "≈ when read from the transcript alone",
    "est without the status-line shim": "≈ until cctop install",
    "est until a compaction has been observed": "≈ until the first compaction has been seen",
}
md = pathlib.Path("docs/metrics.md").read_text().splitlines()
# a formula may name another reading by its identifier; spell it out
NAMES = {m.group(2): m.group(1).lower() for m in re.finditer(r"\*\*(.+?)\*\* <a id=\"(.+?)\"></a>", "\n".join(md))}
def spell(cell):
    for mid, name in NAMES.items():
        cell = cell.replace(f"`{mid}`", name)
    parts = cell.split("`")  # identifiers inside backticks are transcript fields, left alone
    for i in range(0, len(parts), 2):
        for mid, name in NAMES.items():
            parts[i] = re.sub(rf"\b{mid}\b", name, parts[i])
    return "`".join(parts)
body, table = [], []
def flush():
    global table
    if not table:
        return
    rows = [r for r in table if not re.match(r"^\|[-| ]+\|$", r)]
    head = [c.strip() for c in rows[0].strip("|").split("|")]
    head = ["Reading", "Unit", "How it is measured", "From", "Caveats", "Exact?"][:len(head)]
    body.append('<div class="tbl"><table><colgroup>' + "".join(f'<col class="c{i + 1}">' for i in range(len(head))) + '</colgroup><tr>' + "".join(f"<th>{html.escape(h)}</th>" for h in head) + "</tr>")
    for r in rows[1:]:
        cells = [c.strip() for c in re.split(r"(?<!\\)\|", r.strip("|"))]
        if len(cells) == len(head) + 1:  # an unescaped "|" inside the unit, e.g. tokens|seconds
            cells[1:3] = [cells[1] + " or " + cells[2]]
        m = re.match(r"\*\*(.+?)\*\* <a id=\"(.+?)\"></a> `(.+?)`", cells[0])
        rid = ""
        if m:
            cells[0] = m.group(1)
            rid = f' id="{m.group(2)}"'
        if len(cells) >= 5:
            cells[2], cells[4] = spell(cells[2]), spell(cells[4])
        if len(cells) >= 4:
            cells[3] = " · ".join(SOURCES.get(s, s) for s in cells[3].split())
        if len(cells) >= 6:
            cells[5] = EXACT.get(cells[5], cells[5])
        body.append(f"<tr{rid}>" + "".join(f"<td>{inline_md(c.replace(chr(92)+'|','|'))}</td>" for c in cells) + "</tr>")
    body.append("</table></div>")
    table = []
for line in md:
    if line.startswith("|"):
        table.append(line); continue
    flush()
    if line.startswith("# ") or line.startswith("Generated by") or line.startswith("Sources:"):
        continue
    if line.startswith("## "):
        body.append(f"<h2 id=\"{re.sub(r'[^a-z]+', '-', line[3:].strip().lower()).strip('-')}\">{inline_md(line[3:])}</h2>")
    elif line.strip():
        body.append(f"<p>{inline_md(line)}</p>")
flush()
head_html = tmpl.split('<section class="cover">')[0]
head_html = head_html.replace("<title>cctop — see what Claude Code is doing</title>", "<title>How each number is measured — cctop</title>")
head_html = re.sub(r'<meta name="description" content="[^"]*">', '<meta name="description" content="Every reading on the cctop dashboard: its unit, how it is measured, where the data comes from, and when it is an estimate.">', head_html)
head_html = head_html.replace('<link rel="preload" href="assets/two-pane.webp" as="image" fetchpriority="high">\n', "")
head_html = head_html.replace('<a href="guide/">Panel guide</a>', '<a href="guide/">Panel guide</a>\n    <a href="metrics.html" aria-current="page">Reference</a>')
metrics_page = head_html + '''<p class="crumbs"><a href="./">cctop</a> / Reference</p>
<section class="sec first">
  <div class="mg"><span class="no"><small>§</small>A</span><span class="lbl">Appendix</span>
    <span class="note"><b>Plain words first</b>What each reading means and what to do about it is in <a href="guide/">the panel guide</a>. This page is the fine print.</span></div>
  <div class="body">
    <h1 style="font-size:clamp(30px,3.6vw,44px);line-height:1.02;margin-bottom:14px">How each number is measured</h1>
    <p>Every reading on the dashboard, panel by panel: its unit, how cctop works it out, which of Claude Code's own records it comes from, what to keep in mind, and whether it is exact or carries the <code>≈</code> mark. Most estimates become exact after <code>cctop install</code>, which adds the status line and hook events as sources.</p>
''' + "\n".join(body) + '''
  </div>
</section>
<footer class="colophon">
  <a class="brand" href="./"><b>cctop</b></a>
  <div><nav aria-label="Footer"><a href="./">Home</a><a href="guide/">Panel guide</a><a href="https://github.com/tomstagl/cctop">GitHub</a></nav><span>© 2026 Thomas Stagl</span></div>
</footer>
</div>
</body>
</html>
'''
pathlib.Path("site/metrics.html").write_text(metrics_page)
print("site/index.html and site/metrics.html written")
PY
