#!/usr/bin/env python3
"""Render the demo assets from the fixture session with a fixed clock.

  two-pane.png       Claude Code pane (fixture text) + cctop at 60x51
  theme-<name>.png   cctop at 60x30 per bundled theme
  demo.webm / .gif   frames as the fixture session progresses

Needs: the cctop binary, Google Chrome (headless screenshots), ffmpeg.
"""
import os, re, subprocess, sys, html, pathlib, shutil, tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
CCTOP = os.environ.get("CCTOP", str(ROOT / "target/release/cctop"))
FIXTURE = str(ROOT / "fixtures/session-a.jsonl")
OUT = ROOT / "site/assets"
FAKE_NOW = "1787824000000"  # 2026-08-27 09:46:40 UTC, mid-session
CHROME = next((p for p in [
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    shutil.which("google-chrome"), shutil.which("chromium"), shutil.which("chromium-browser"), shutil.which("chrome"),
] if p and os.path.exists(p)), None)

ANSI = re.compile(r"\x1b\[([0-9;]*)m")

def cctop(args):
    env = dict(os.environ, CCTOP_FAKE_NOW=FAKE_NOW, HOME=os.environ.get("HOME", ""))
    return subprocess.run([CCTOP, "run", "--once", "--session", FIXTURE, "--ansi", *args],
                          capture_output=True, text=True, env=env, check=True).stdout

def ansi_to_html(text):
    out, fg, bg, bold, rev = [], None, None, False, False
    pos = 0
    def style():
        f, b = fg, bg
        if rev: f, b = (b or "#0E1318"), (f or "#D6DEE8")
        css = []
        if f: css.append(f"color:{f}")
        if b: css.append(f"background:{b}")
        if bold: css.append("font-weight:600")
        return ";".join(css)
    for m in ANSI.finditer(text):
        seg = text[pos:m.start()]
        if seg:
            s = style()
            out.append(f'<span style="{s}">{html.escape(seg)}</span>' if s else html.escape(seg))
        pos = m.end()
        codes = [c for c in m.group(1).split(";") if c != ""] or ["0"]
        i = 0
        while i < len(codes):
            c = codes[i]
            if c == "0": fg = bg = None; bold = rev = False
            elif c == "1": bold = True
            elif c == "7": rev = True
            elif c == "39": fg = None
            elif c == "49": bg = None
            elif c in ("38", "48") and i + 4 < len(codes) and codes[i + 1] == "2":
                col = "#%02x%02x%02x" % tuple(int(x) for x in codes[i + 2:i + 5])
                if c == "38": fg = col
                else: bg = col
                i += 4
            i += 1
    seg = text[pos:]
    if seg: out.append(html.escape(seg))
    return "".join(out)

PAGE = """<!doctype html><meta charset="utf-8"><style>
html,body{{margin:0;background:#0E1318}}
body{{width:{w}px;height:{h}px;overflow:hidden;font-family:"IBM Plex Mono","SF Mono",Menlo,Consolas,monospace;font-size:{fs}px;line-height:1.3;color:#D6DEE8}}
.win{{position:absolute;inset:16px;display:flex;background:#0E1318}}
pre{{margin:0;font:inherit;white-space:pre;padding:10px 12px;flex:0 0 auto}}
.l{{border-right:1px solid #2A3644;color:#D6DEE8}}
.bar{{position:absolute;left:0;right:0;bottom:0;height:22px;background:#1C2632;color:#5D6B7D;font-size:12px;line-height:22px;padding:0 12px}}
.bar b{{color:#5FC77E;font-weight:500}}
</style><body><div class="win">{left}<pre>{right}</pre></div><div class="bar">[cctop] <b>0:claude*</b> 1:cctop</div></body>"""

def shoot(html_text, path, w, h):
    with tempfile.NamedTemporaryFile("w", suffix=".html", delete=False) as f:
        f.write(html_text); tmp = f.name
    subprocess.run([CHROME, "--headless=new", "--hide-scrollbars", f"--window-size={w},{h}",
                    "--screenshot=" + str(path), "file://" + tmp], check=True, capture_output=True)
    os.unlink(tmp)

def two_pane(path, lines=None, theme="default-dark"):
    args = ["--size", "60x36", "--theme", theme] + (["--lines", str(lines)] if lines else [])
    right = ansi_to_html(cctop(args))
    left = '<pre class="l">' + html.escape((ROOT / "site/demo/claude-pane.txt").read_text()) + "</pre>"
    shoot(PAGE.format(w=1280, h=640, fs=12, left=left, right=right), path, 1280, 640)

def main():
    if not CHROME: sys.exit("demo: Google Chrome / Chromium not found")
    if not shutil.which("ffmpeg"): sys.exit("demo: ffmpeg not found")
    if not os.path.exists(CCTOP): sys.exit(f"demo: {CCTOP} not built (cargo build --release)")
    OUT.mkdir(parents=True, exist_ok=True)
    two_pane(OUT / "two-pane.png")
    for theme in ["default-dark", "default-light", "btop", "nord", "gruvbox", "catppuccin-mocha"]:
        right = ansi_to_html(cctop(["--size", "60x30", "--theme", theme]))
        shoot(PAGE.format(w=560, h=560, fs=12, left="", right=right), OUT / f"theme-{theme}.png", 560, 560)
    # Frames: the session as it unfolds.
    frames = tempfile.mkdtemp(prefix="cctop-frames-")
    steps = [120, 260, 420, 600, 800, 1000, 1200, 1463]
    for i, n in enumerate(steps):
        two_pane(pathlib.Path(frames) / f"f{i:03d}.png", lines=n)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-framerate", "1", "-i", f"{frames}/f%03d.png",
                    "-c:v", "libvpx-vp9", "-b:v", "600k", "-pix_fmt", "yuv420p", str(OUT / "demo.webm")], check=True)
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-framerate", "1", "-i", f"{frames}/f%03d.png",
                    "-vf", "scale=960:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=128[p];[b][p]paletteuse=dither=bayer",
                    str(OUT / "demo.gif")], check=True)
    shutil.rmtree(frames)
    size = (OUT / "demo.webm").stat().st_size
    print(f"demo: two-pane.png, 6 theme PNGs, demo.webm ({size/1e6:.1f} MB), demo.gif in {OUT}")
    if size > 3_000_000: sys.exit("demo.webm exceeds 3 MB")

if __name__ == "__main__":
    main()
