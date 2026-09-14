#!/usr/bin/env bash
# Build site/index.html and site/metrics.html from the README's marked blocks,
# docs/metrics.md and the themes. No dependencies beyond python3.
set -euo pipefail
cd "$(dirname "$0")/.."
python3 - <<'PY'
import re, html, pathlib, glob

readme = pathlib.Path("README.md").read_text()
tmpl = pathlib.Path("site/index.template.html").read_text()
# Inline the stylesheets: 4 KB in the document beats a render-blocking request.
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
lede = inline_md(block("lede").replace("\n", " "))
# install: fenced code → text
install = re.sub(r"^```\n|\n```$", "", block("install").strip("`\n"))
install = html.escape(install)
# panels: markdown table → cards
cards = []
for line in block("panels").splitlines()[2:]:
    cells = [c.strip() for c in line.strip("|").split("|")]
    if len(cells) < 3:
        continue
    n, name, answers = cells[0], re.sub(r"\*", "", cells[1]), cells[2]
    cards.append(f'<div class="pnl"><h3>{n} {html.escape(name)}</h3><p>{inline_md(answers)}</p></div>')
# themes
themes = []
for path in sorted(glob.glob("themes/*.toml")):
    kv = dict(re.findall(r'^(\w+)\s*=\s*"([^"]+)"', pathlib.Path(path).read_text(), re.M))
    sw = "".join(f'<i style="background:{kv[k]}"></i>' for k in ("accent", "ok", "warn", "crit"))
    themes.append(f'<div class="theme" style="background:{kv["bg"]};color:{kv["fg"]};border-color:{kv["border"]}">{kv["name"]}<br>{sw}</div>')
# mockup: the fenced block after "## What it looks like" (the terminal view)
m = re.search(r"## What it looks like\n\n```\n(.*?)\n```", readme, re.S)
mockup = html.escape(m.group(1)) if m else ""
# panel mockup: the fenced block under "**Panel view.**" in the same section
m = re.search(r"\*\*Panel view\.\*\*.*?\n```\n(.*?)\n```", readme, re.S)
panel_mockup = html.escape(m.group(1)) if m else ""

# The demo block needs assets produced by `make demo`; drop it until they exist.
if not pathlib.Path("site/assets/two-pane.png").exists():
    tmpl = re.sub(r'\s*<div class="demo">.*?</div>\n', "\n", tmpl, flags=re.S)
out = (tmpl.replace("@HERO@", lede)
           .replace("@HEADLINE@", html.escape(hero))
           .replace("@INSTALL@", install)
           .replace("@PANELS@", "\n".join(cards))
           .replace("@THEMES@", "\n".join(themes))
           .replace("@MOCKUP@", mockup)
           .replace("@PANEL_MOCKUP@", panel_mockup))
pathlib.Path("site/index.html").write_text(out)

# metrics.html from docs/metrics.md (headings, paragraphs, tables)
md = pathlib.Path("docs/metrics.md").read_text().splitlines()
body, table = [], []
def flush():
    global table
    if not table:
        return
    rows = [r for r in table if not re.match(r"^\|[-| ]+\|$", r)]
    head = [c.strip() for c in rows[0].strip("|").split("|")]
    body.append('<div class="tbl"><table><tr>' + "".join(f"<th>{html.escape(h)}</th>" for h in head) + "</tr>")
    for r in rows[1:]:
        cells = [c.strip() for c in re.split(r"(?<!\\)\|", r.strip("|"))]
        body.append("<tr>" + "".join(f"<td>{inline_md(c.replace(chr(92)+'|','|'))}</td>" for c in cells) + "</tr>")
    body.append("</table></div>")
    table = []
for line in md:
    if line.startswith("|"):
        table.append(line); continue
    flush()
    if line.startswith("# "):
        body.append(f"<h1>{inline_md(line[2:])}</h1>")
    elif line.startswith("## "):
        body.append(f"<h2>{inline_md(line[3:])}</h2>")
    elif line.strip():
        body.append(f"<p>{inline_md(line)}</p>")
flush()
metrics_page = tmpl.split("<section class=\"hero\">")[0].replace("<title>cctop — see what Claude Code is doing</title>", "<title>cctop metrics</title>") + \
    '<main class="metrics">' + "\n".join(body) + '</main>\n<footer><a href="./">← cctop</a><a href="https://github.com/tomstagl/cctop">GitHub</a></footer>\n</div>\n</body>\n</html>\n'
pathlib.Path("site/metrics.html").write_text(metrics_page)
print("site/index.html and site/metrics.html written")
PY
