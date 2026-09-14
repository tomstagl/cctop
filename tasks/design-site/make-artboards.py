#!/usr/bin/env python3
"""Turn the built site pages into design-canvas artboards (Main = home,
Guide = the Context panel page, Mobile = home at phone width). Re-run after
`make site`, then re-seed the canvas."""
import re, pathlib
ROOT = pathlib.Path(__file__).resolve().parent.parent.parent
OUT = pathlib.Path(__file__).resolve().parent
FONTS = '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500;600&amp;family=IBM+Plex+Sans+Condensed:wght@700&amp;family=IBM+Plex+Sans:wght@400;500;600&amp;display=swap">'
SITE_CSS = (ROOT / "site/site.css").read_text().strip()

def artboard(page_html, name, extra_css=""):
    body = re.search(r"<body>\n(.*?)\n<script>", page_html, re.S)
    body = body.group(1) if body else re.search(r"<body>\n(.*?)\n</body>", page_html, re.S).group(1)
    # the demo video cannot ride along; the still does
    body = re.sub(r"<video.*?</video>\n\s*", "", body, flags=re.S)
    body = body.replace('src="assets/two-pane.webp"', 'src="two-pane.webp"')
    body = body.replace("<button id=\"copy\" type=\"button\" hidden>copy</button>", "<button id=\"copy\" type=\"button\">copy</button>")
    # local links point nowhere on the canvas; keep them as text-styled anchors
    html = f"""<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <script src="./support.js"></script>
</head>
<body>
<x-dc>
<helmet>
  {FONTS}
  <style>
{SITE_CSS}
.plate img.still{{display:block !important}}
{extra_css}
  </style>
</helmet>
{body}
</x-dc>
</body>
</html>
"""
    (OUT / f"{name}.dc.html").write_text(html)
    print(name, len(html))

home = (ROOT / "site/index.html").read_text()
artboard(home, "Main")
artboard(home, "Mobile")
artboard((ROOT / "site/guide/context.html").read_text(), "Guide")
