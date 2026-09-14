# Site redesign — design canvas sources

Canvas: https://claude.ai/code/artifact/c56e41af-185f-472e-94ad-91694cf9d0dd

- `Main.dc.html`, `Mobile.dc.html`, `Guide.dc.html` — the built site pages as artboards
  (home at desktop and phone width, the Context guide page). Generated: run `make site`,
  then `python3 tasks/design-site/make-artboards.py`.
- `DirectionB.dc.html`, `DirectionC.dc.html` — the two low-fi alternates that were not built.
- `canvas.json` — artboard positions and the sticky notes.

The seeded canvas (`cctop-site-redesign.html`, ~2.7 MB) is not committed; `/design` re-seeds it
from these files with `--image site/assets/two-pane.webp --canvas canvas.json`.
