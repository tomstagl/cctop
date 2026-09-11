# cctop brand assets

Chosen mark: **gauge beard** — five terracotta meter bars form a ZZ Top beard under round shades. Reads as a btop gauge and as a bearded face.

| Use | File |
|---|---|
| Mark, transparent, light grounds (black frames) | `svg/cctop-mark.svg` |
| App icon tile (charcoal, light frames) | `svg/cctop-icon.svg`, `png/icon-{192,512,1024}.png`, `png/apple-touch-icon.png` |
| Monochrome (`currentColor`) — terminal splash, stamps | `svg/cctop-mark-mono.svg` |
| Favicon (simplified for 16 px) | `svg/favicon.svg`, `svg/favicon-dark.svg`, `png/favicon.ico`, `png/favicon-{16,32,64}.png` |
| Horizontal lockup | `svg/cctop-lockup-horizontal-{light,dark}.svg`, `png/lockup-horizontal-*.png` |
| Vertical lockup | `svg/cctop-lockup-vertical-{light,dark}.svg`, `png/lockup-vertical-*.png` |
| Wordmark only | `svg/cctop-wordmark-{light,dark}.svg` |
| OpenGraph 1200×630 | `svg/cctop-og.svg`, `png/og-image.png` |
| GitHub social preview / README banner 1280×400 | `svg/cctop-banner.svg`, `png/banner.png` |

Palette: terracotta `#D97757` (beard), black `#111111` frames on light grounds / `#D6DEE8` on dark grounds, lens `#3A3F47`, charcoal `#0E1318`, paper `#F1F3F6`, ink `#1A212C`.

Wordmark is IBM Plex Sans Condensed Bold converted to outlines (fonts in `fonts/`, SIL OFL) — the SVGs have no font dependency.

Rebuild everything: `cd ../studio-master && NODE_PATH=$PWD/node_modules ./node_modules/.bin/tsx ../cctop/brand/build.ts` (uses fontkit + sharp from that repo). `candidates/` holds the kie.ai references from `gen-logo.ts`.
