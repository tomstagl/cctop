#!/usr/bin/env python3
"""Assemble prototype.html from the shell and the three renderer modules.

    python3 build-prototype.py

`proto/terminal.js` is the character-grid renderer (segments, padding, bars,
the cut that mirrors dashboard::cut). `proto/views.js` holds the five view
bodies every direction shares. `proto/directions.js` holds one screen function
per direction. Edit those; this file only concatenates them into the shell in
`prototype.shell.html` and writes `prototype.html`.

Verify with:  node drive-protos.mjs      (drives every click target, both widths)
"""
import pathlib, sys

here = pathlib.Path(__file__).resolve().parent
shell = (here / "prototype.shell.html").read_text()
parts = {
    "__TERMINAL__": (here / "proto/terminal.js").read_text(),
    "__VIEWS__": (here / "proto/views.js").read_text(),
    "__DIRECTIONS__": (here / "proto/directions.js").read_text(),
}
for token, js in parts.items():
    if token not in shell:
        sys.exit(f"prototype.shell.html has no {token} placeholder")
    shell = shell.replace(token, js)
out = here / "prototype.html"
out.write_text(shell)
print(f"wrote {out.relative_to(here.parent.parent)} ({len(shell) // 1024} KB)")
