#!/usr/bin/env tsx
// Builds every cctop brand asset from one hand-drawn mark + outlined wordmark.
// Run from studio-master (for fontkit + sharp):
//   ./node_modules/.bin/tsx ../cctop/brand/build.ts
import { writeFileSync, mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import fontkit from 'fontkit';
import sharp from 'sharp';

const ROOT = '/Users/tom/code/cctop/brand';
const SVG = join(ROOT, 'svg'); const PNG = join(ROOT, 'png');
mkdirSync(SVG, { recursive: true }); mkdirSync(PNG, { recursive: true });

// ---- palette
const TERRA = '#D97757', FRAME = '#111111', LENS = '#3A3F47', CHAR = '#0E1318', PAPER = '#F1F3F6', INK = '#1A212C';
const FRAME_ON_DARK = '#D6DEE8'; // black frames disappear on charcoal; light frames keep the shades readable

// ---- the mark, drawn in a 256×256 box, visually centred
// beard: five "meter bars" of decreasing width, moustache on the top slab, zig-zag tip
function beard(fill: string) {
  return `
  <g fill="${fill}">
    <path d="M128 111 C118 98 100 98 92 106 C87 111 91 118 98 116 C107 113 117 115 128 121 C139 115 149 113 158 116 C165 118 169 111 164 106 C156 98 138 98 128 111 Z"/>
    <rect x="78" y="118" width="100" height="30" rx="2"/>
    <rect x="84" y="155" width="88" height="22" rx="2"/>
    <rect x="90" y="184" width="76" height="20" rx="2"/>
    <path d="M96 211 H160 L156 230 L146 222 L136 240 L128 254 L120 240 L110 222 L100 230 Z"/>
  </g>`;
}
// sunglasses: two round lenses, bridge, temple stubs
function shades(frame: string, lens: string) {
  return `
  <g>
    <circle cx="88" cy="60" r="30" fill="${lens}" stroke="${frame}" stroke-width="9"/>
    <circle cx="168" cy="60" r="30" fill="${lens}" stroke="${frame}" stroke-width="9"/>
    <path d="M118 56 Q128 45 138 56" fill="none" stroke="${frame}" stroke-width="8" stroke-linecap="round"/>
    <path d="M58 55 L42 51 M198 55 L214 51" fill="none" stroke="${frame}" stroke-width="8" stroke-linecap="round"/>
  </g>`;
}
const markBody = () => beard(TERRA) + shades(FRAME, LENS);          // for light / transparent grounds
const markBodyDark = () => beard(TERRA) + shades(FRAME_ON_DARK, LENS); // for dark grounds
const svgDoc = (w: number, h: number, body: string, extra = '') =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${w} ${h}" width="${w}" height="${h}"${extra}>${body}\n</svg>\n`;

// ---- wordmark outlines from IBM Plex Sans Condensed Bold (OFL)
const font = fontkit.openSync(join(ROOT, 'fonts/PlexSansCondensed-Bold.ttf')) as any;
function outline(text: string, size: number): { d: string; width: number; ascent: number; descent: number } {
  const run = font.layout(text);
  const scale = size / font.unitsPerEm;
  let x = 0; const parts: string[] = [];
  for (let i = 0; i < run.glyphs.length; i++) {
    const g = run.glyphs[i]; const pos = run.positions[i];
    // glyph path is y-up in font units; flip and scale
    const p = g.path.scale(scale, -scale).translate(x + pos.xOffset * scale, 0);
    parts.push(p.toSVG());
    x += pos.xAdvance * scale;
  }
  return { d: parts.join(' '), width: x, ascent: font.ascent * scale, descent: -font.descent * scale };
}
const monoFont = fontkit.openSync(join(ROOT, 'fonts/PlexMono-Medium.ttf')) as any;
function outlineMono(text: string, size: number) {
  const run = monoFont.layout(text); const scale = size / monoFont.unitsPerEm; let x = 0; const parts: string[] = [];
  for (let i = 0; i < run.glyphs.length; i++) { const g = run.glyphs[i]; const pos = run.positions[i];
    parts.push(g.path.scale(scale, -scale).translate(x + pos.xOffset * scale, 0).toSVG()); x += pos.xAdvance * scale; }
  return { d: parts.join(' '), width: x };
}

const files: Record<string, string> = {};

// 1. mark, transparent
files['cctop-mark.svg'] = svgDoc(256, 256, markBody());
// 2. mark on charcoal app-icon tile (rounded)
files['cctop-icon.svg'] = svgDoc(256, 256, `<rect width="256" height="256" rx="56" fill="${CHAR}"/>` + `<g transform="translate(0 4) scale(0.86) translate(21 12)">${markBodyDark()}</g>`);
// 3. monochrome mark (currentColor) — terminal splash, stamps, single-colour print
files['cctop-mark-mono.svg'] = svgDoc(256, 256, beard('currentColor') + `
  <g>
    <circle cx="88" cy="60" r="30" fill="none" stroke="currentColor" stroke-width="9"/>
    <circle cx="168" cy="60" r="30" fill="none" stroke="currentColor" stroke-width="9"/>
    <path d="M118 56 Q128 45 138 56" fill="none" stroke="currentColor" stroke-width="8" stroke-linecap="round"/>
    <path d="M58 55 L42 51 M198 55 L214 51" fill="none" stroke="currentColor" stroke-width="8" stroke-linecap="round"/>
  </g>`, ' fill="currentColor" color="#1A212C"');
// 4. favicon: simplified for 16 px — thicker frames, three bars, no moustache curl
files['favicon.svg'] = svgDoc(64, 64, `
  <g fill="${TERRA}">
    <rect x="12" y="30" width="40" height="9" rx="1"/>
    <rect x="16" y="42" width="32" height="7" rx="1"/>
    <path d="M20 52 H44 L32 64 Z"/>
  </g>
  <circle cx="21" cy="17" r="9" fill="${LENS}" stroke="${FRAME}" stroke-width="4"/>
  <circle cx="43" cy="17" r="9" fill="${LENS}" stroke="${FRAME}" stroke-width="4"/>
  <path d="M30 16 Q32 12 34 16" fill="none" stroke="${FRAME}" stroke-width="3.5" stroke-linecap="round"/>`);

files['favicon-dark.svg'] = files['favicon.svg'].replace(new RegExp(`stroke="${FRAME}"`, 'g'), `stroke="${FRAME_ON_DARK}"`);
// 5. horizontal lockup: mark + "cctop" (light and dark text variants)
const wm = outline('cctop', 150);
const gap = 28, markH = 256, totalW = Math.round(markH + gap + wm.width) + 8;
const wmY = 128 + (wm.ascent - wm.descent) / 2 - wm.descent - 8; // optical centre against the mark
const horiz = (ink: string, dark = false) => svgDoc(totalW, 256, `<g>${dark ? markBodyDark() : markBody()}</g><path transform="translate(${markH + gap} ${wmY.toFixed(1)})" fill="${ink}" d="${wm.d}"/>`);
files['cctop-lockup-horizontal-dark.svg'] = horiz(PAPER, true);   // for dark grounds
files['cctop-lockup-horizontal-light.svg'] = horiz(INK);    // for light grounds
// 6. vertical lockup
const wmV = outline('cctop', 120);
const vW = 320, vH = 256 + 24 + 110;
const vert = (ink: string, dark = false) => svgDoc(vW, vH, `<g transform="translate(${(vW - 256) / 2} 0)">${dark ? markBodyDark() : markBody()}</g><path transform="translate(${((vW - wmV.width) / 2).toFixed(1)} ${(256 + 24 + wmV.ascent * 0.72).toFixed(1)})" fill="${ink}" d="${wmV.d}"/>`);
files['cctop-lockup-vertical-dark.svg'] = vert(PAPER, true);
files['cctop-lockup-vertical-light.svg'] = vert(INK);
// 7. wordmark alone
files['cctop-wordmark-dark.svg'] = svgDoc(Math.round(wm.width) + 8, 170, `<path transform="translate(4 ${(wm.ascent * 0.95).toFixed(1)})" fill="${PAPER}" d="${wm.d}"/>`);
files['cctop-wordmark-light.svg'] = svgDoc(Math.round(wm.width) + 8, 170, `<path transform="translate(4 ${(wm.ascent * 0.95).toFixed(1)})" fill="${INK}" d="${wm.d}"/>`);
// 8. OpenGraph / social card 1200×630
const tag = outlineMono('see what Claude Code is doing — live, in a pane beside it', 30);
const ogWm = outline('cctop', 190);
files['cctop-og.svg'] = svgDoc(1200, 630, `
  <rect width="1200" height="630" fill="${CHAR}"/>
  <g transform="translate(120 155) scale(1.25)">${markBodyDark()}</g>
  <path transform="translate(500 ${(240 + ogWm.ascent * 0.72).toFixed(1)})" fill="${PAPER}" d="${ogWm.d}"/>
  <path transform="translate(500 440)" fill="#8B95A7" d="${tag.d}"/>
  <g fill="none" stroke="#2A3644" stroke-width="2"><path d="M500 476 H1080"/></g>
  <path transform="translate(500 528)" fill="#4CC2C2" d="${outlineMono('brew install cctop', 30).d}"/>`);
// 9. GitHub social preview / README banner 1280×400 (dark)
files['cctop-banner.svg'] = svgDoc(1280, 400, `
  <rect width="1280" height="400" fill="${CHAR}"/>
  <g transform="translate(${(1280 - totalW) / 2} 72)">${markBodyDark()}<path transform="translate(${markH + gap} ${wmY.toFixed(1)})" fill="${PAPER}" d="${wm.d}"/></g>`);

for (const [name, svg] of Object.entries(files)) writeFileSync(join(SVG, name), svg);

// ---- rasters
const rasters: [string, string, number, number?][] = [
  ['cctop-mark.svg', 'cctop-mark-512.png', 512], ['cctop-mark.svg', 'cctop-mark-256.png', 256],
  ['cctop-icon.svg', 'icon-1024.png', 1024], ['cctop-icon.svg', 'icon-512.png', 512], ['cctop-icon.svg', 'icon-192.png', 192], ['cctop-icon.svg', 'apple-touch-icon.png', 180],
  ['favicon.svg', 'favicon-64.png', 64], ['favicon-dark.svg', 'favicon-dark-32.png', 32], ['favicon.svg', 'favicon-32.png', 32], ['favicon.svg', 'favicon-16.png', 16],
  ['cctop-lockup-horizontal-dark.svg', 'lockup-horizontal-dark.png', 1200], ['cctop-lockup-horizontal-light.svg', 'lockup-horizontal-light.png', 1200],
  ['cctop-lockup-vertical-dark.svg', 'lockup-vertical-dark.png', 640], ['cctop-lockup-vertical-light.svg', 'lockup-vertical-light.png', 640],
  ['cctop-og.svg', 'og-image.png', 1200], ['cctop-banner.svg', 'banner.png', 1280], ['cctop-mark-mono.svg', 'mark-mono-256.png', 256],
];
(async () => {
  for (const [src, out, w] of rasters) {
    const buf = readFileSync(join(SVG, src));
    await sharp(buf, { density: 600 }).resize({ width: w }).png().toFile(join(PNG, out));
  }
  // contact sheet for review
  const sheet = svgDoc(1400, 900, `<rect width="1400" height="900" fill="#FFFFFF"/>
    <rect x="700" width="700" height="900" fill="${CHAR}"/>
    <g transform="translate(60 40)">${markBody()}</g>
    <g transform="translate(760 40)">${markBodyDark()}</g>
    <g transform="translate(360 40) scale(1)">${readFileSync(join(SVG,'cctop-mark-mono.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(60 340)">${readFileSync(join(SVG,'cctop-lockup-horizontal-light.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(760 340)">${readFileSync(join(SVG,'cctop-lockup-horizontal-dark.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(60 640)">${readFileSync(join(SVG,'cctop-icon.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(360 640) scale(2)">${readFileSync(join(SVG,'favicon.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(520 640) scale(0.5)">${readFileSync(join(SVG,'favicon.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(580 640) scale(0.25)">${readFileSync(join(SVG,'favicon.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(1080 20) scale(0.55)">${readFileSync(join(SVG,'cctop-lockup-vertical-dark.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>
    <g transform="translate(760 640) scale(0.4)">${readFileSync(join(SVG,'cctop-og.svg'),'utf8').replace(/^[\s\S]*?>/,'').replace('</svg>','')}</g>`);
  writeFileSync(join(ROOT, 'contact-sheet.svg'), sheet);
  await sharp(Buffer.from(sheet), { density: 200 }).resize({ width: 1400 }).png().toFile(join(ROOT, 'contact-sheet.png'));
  console.log('wrote', Object.keys(files).length, 'svg +', rasters.length, 'png');
})();
