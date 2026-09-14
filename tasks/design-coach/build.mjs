// Builds the coach design canvas: one .dc.html artboard per screen, plus
// canvas.json. Run `node build.mjs` here, then seed with the design helper.
import { writeFileSync } from 'node:fs';
import { THEME, artboard, screen } from './mock.mjs';
import { big, bigPanel, bigTerminal } from './dashboard.mjs';
import { navOptions, paneCoach, paneNarrow, statusForms, terminalAfter, tuiCoach, tuiCoachIdle, tuiCoachWide } from './screens.mjs';

const boards = [];
function add(file, title, blocks, opts) {
  const a = artboard(title, blocks, opts);
  writeFileSync(file, a.doc);
  boards.push({ file, title, w: a.width, h: a.height });
  console.log(`${file}: ${a.width}×${a.height}`);
}

const small = { w: 6.6, h: 14, font: 11 };

// 1 — the pane's Overview: direction B, the nav inside it.
add('Main.dc.html', 'Pane · Overview', [
  { ...screen(bigPanel(), 72), caption: '72 body columns, docked · the view bar is the pane’s first row · tiles two per row · the ledger rows are Buttons' },
], {
  head: 'Pane · Overview',
  sub: 'The four lights as block digits, the nudge, then the nine-row ledger. Rows 5–9 switch to their tab; rows 1–4 unfold their block inline. Same object as the terminal: cctop query dashboard.',
  subLines: 2,
});

// 2 — the pane's Coach view.
add('PaneCoach.dc.html', 'Pane · Coach', [
  { ...screen(paneCoach(), 72), caption: '72 body columns · the same card, then next / snoozed / lifecycle, then the red light’s detail' },
], {
  head: 'Pane · Coach',
  sub: 'Buttons replace the TUI’s keys: fill = $.prompt.fill (the person presses Enter), snooze, why. The detail frame follows the highest light; the four buttons switch it.',
  subLines: 2,
});

// 3 — the pane below 50 body columns.
add('PaneNarrow.dc.html', 'Pane · narrow', [
  { ...screen(paneNarrow(), 48), caption: '48 body columns · the bar wraps · each light drops its third figure · the nudge wraps' },
], { head: 'Pane · narrow' });

// 4 — the TUI dashboard: direction B.
add('TuiDashboard.dc.html', 'TUI · Dashboard', [
  { ...screen(bigTerminal(), 122), caption: '122 × 24 · the four lights as block digits, the nudge, then a borderless ledger: one row per panel, its second row dim · 1-9 open a panel full-screen' },
], {
  head: 'TUI · Dashboard',
  sub: 'Four figures a person can read from across the room, everything else demoted to a dim ledger without frames — 24 rows where the framed grid took 46. Chosen 2026-09-14 (direction B); plan: tasks/plan-dashboard-big-figures.md.',
  subLines: 2,
});

// 5 — the TUI coach view, mid-turn and idle.
add('TuiCoach.dc.html', 'TUI · Coach', [
  { ...screen(tuiCoach(), 56), caption: '56 × 17 · mid-turn: a NOW nudge holds the slot (PRD §6.2)' },
  { ...screen(tuiCoachIdle(), 56), caption: 'idle: absence of colour is the signal; the lifecycle rows show the coach acted and vanished' },
], {
  head: 'TUI · Coach (c)',
  sub: 'One nudge, the keystroke that resets the light, retired the moment the person acts.',
});

// 6 — the coach view on a wide terminal.
add('TuiCoachWide.dc.html', 'TUI · Coach, wide', [
  { ...screen(tuiCoachWide(), 122), caption: '122 columns · the card keeps its 56 · the why/lifecycle column is permanent instead of an overlay' },
], {
  head: 'TUI · Coach on a wide terminal',
  sub: 'At ≥ 100 columns the e (why) and l (log) overlays get a column of their own; the card never widens.',
});

// 7 — the compact forms.
{
  const f = statusForms();
  add('StatusLine.dc.html', 'Coach · one-line forms', [
    { ...screen(f.l0, 96), caption: 'L0 · ≥ 80 columns · $.ui.status under the prompt, cctop query coach --line, tmux status' },
    { ...screen(f.l1, 24), caption: 'L1 · the four glyph+number pairs and the ▸ marker' },
    { ...screen(f.l2, 8), caption: 'L2 · the four glyphs' },
  ], {
    head: 'The coach in one line',
    sub: 'Changes only when a light’s level or the slot occupant changes — never on a render tick.',
  });
}

// 8 — nav options.
{
  const n = navOptions();
  add('NavOptions.dc.html', 'Nav · options', [
    { ...screen(n.band, 120, { cell: small }), caption: 'today · the band above the prompt (removed): the only site where Claude Code honours a Button hotkey' },
    { ...screen(n.a, 72), caption: 'A · names only, active tab inverse · honest (no key switches views inside the pane) · one row from 61 columns' },
    { ...screen(n.b, 72), caption: 'B · the band’s key hints kept · digits that do nothing once the band is gone · wraps below 82 columns' },
  ], {
    head: 'Where the nav lives',
    sub: 'A is the draft in every pane artboard. Switching: click a tab, or ctrl+x tab then tab / enter, or /cctop-pane <view>.',
  });
}

// 9 — the whole terminal after the move.
add('Terminal.dc.html', 'Terminal · after', [
  { ...screen(terminalAfter(bigPanel()), 200, { cell: small }), caption: '200 columns · Claude Code fullscreen · the pane docked on its Overview · nothing above the prompt but the prompt' },
], {
  head: 'The terminal after the move',
  sub: 'The left column is Claude Code’s own transcript; the band that held “cctop 1: Overview …” above the prompt is gone.',
});

// 10 — the sync sheet.
writeFileSync('SyncMap.dc.html', syncSheet());
boards.push({ file: 'SyncMap.dc.html', title: 'Sync map', w: 980, h: 1330 });

// ---- the levers a terminal has instead of font sizes ------------------------
{
  const mono = "'IBM Plex Mono',ui-monospace,Menlo,Consolas,monospace";
  const dark = (inner, w, h) =>
    `<div style="background:${THEME.bg};color:${THEME.fg};font-family:${mono};font-size:13px;line-height:17px;width:${w}px;height:${h}px;box-sizing:border-box;padding:10px 12px;border-radius:6px;overflow:hidden;display:flex;flex-direction:row;align-items:baseline;gap:18px">${inner}</div>`;
  const weights = screen(
    [
      `Regular  {b:Bold}  {dim:dim}  {inv: inverse }  {ok:ok}  {warn:warn}  {crit:crit}  {acc:accent}`,
      `{dim:one cell size · weight, dimming, inverse and state colour are the whole ramp}`,
    ],
    80,
  );
  const digits = big('41', 'warn').map((r, i) => r + (i === 2 ? '{dim:%}  ' : '   ') + ['{warn:◐} {b:context}', '412k of 1.00M', '≈$.21/call'][i]);
  const block = screen(
    [...digits, '', `{b:41 %} {dim:◐ context · 412k of 1.00M}`, `{dim:the ASCII / short-terminal fallback: the same figure in bold}`],
    80,
  );
  const kitty = {
    width: 656,
    height: 74,
    html: dark(
      `<span style="font-size:26px;line-height:34px;color:${THEME.warn};font-weight:600;white-space:nowrap">41 %</span><span style="white-space:nowrap">◐ context · 412k of 1.00M</span><span style="color:${THEME.dim};white-space:nowrap">real 2× text: kitty ≥ 0.40 only — not ratatui, not the pane</span>`,
      656,
      74,
    ),
  };
  const fold = screen(
    [
      `{dim:▸} {acc b:5} {b:Tools} {bd:─} 257 calls {dim:·} {crit:3 err} {dim:·} gitread 12 (3 ✗) {bd:${'─'.repeat(28)}}`,
      `{dim:a folded panel is its title line: the TUI draws it when rows run out}`,
    ],
    80,
  );
  add('TypeLevers.dc.html', 'Type in a terminal', [
    { ...weights, caption: '1 · weight, dim, inverse, colour — what every terminal and the Claude Code pane can do' },
    { ...block, caption: '2 · large type from block glyphs: 3 rows tall, any terminal, drawn by cctop itself' },
    { ...kitty, caption: '3 · true font scaling exists in exactly one place today' },
    { ...fold, caption: '4 · disclosure: fold what is not needed now; whitespace between groups costs a row and buys hierarchy' },
  ], {
    head: 'Type in a terminal',
    sub: 'There is no font size in a terminal or in the pane: one cell grid. Hierarchy comes from weight, dimming, inverse, state colour, block-glyph digits and whitespace — the dashboard uses all of them.',
    subLines: 2,
  });
}

// ---- canvas.json ------------------------------------------------------------

const GAP_X = 100;
const GAP_Y = 160;
const rows = [
  ['Terminal.dc.html'],
  ['Main.dc.html', 'PaneCoach.dc.html', 'PaneNarrow.dc.html', 'NavOptions.dc.html'],
  ['TuiDashboard.dc.html', 'TuiCoach.dc.html', 'TuiCoachWide.dc.html'],
  ['TypeLevers.dc.html', 'StatusLine.dc.html', 'SyncMap.dc.html'],
];
const artboards = [];
const placed = {};
let y = 0;
for (const row of rows) {
  let x = 0;
  let tallest = 0;
  for (const file of row) {
    const b = boards.find((b) => b.file === file);
    artboards.push({ file, title: b.title, x, y, w: b.w, h: b.h });
    placed[file] = { x, y };
    x += b.w + GAP_X;
    tallest = Math.max(tallest, b.h);
  }
  y += tallest + GAP_Y;
}
const note = (id, file, w, text) => ({ id, x: placed[file].x, y: placed[file].y - 150, w, text });
const canvas = {
  artboards,
  annotations: [
    note('nav-moved', 'Terminal.dc.html', 420, 'The nav line “cctop 1: Overview …” leaves the band above the prompt and becomes the pane’s first row. Cost: the digit hotkeys typed into the empty composer stop working (the docked pane never reads Button.hotkey). Switching stays: click, ctrl+x tab + tab/enter, /cctop-pane <view>.'),
    note('one-object', 'Main.dc.html', 420, 'One coach object, drawn the same everywhere: state line → four lights → one nudge. The Overview’s tiles, the pane Coach view and the TUI coach view all come from cctop query coach; the ledger from cctop query dashboard.'),
    note('dashboard-b', 'TuiDashboard.dc.html', 460, 'Direction B (“Big figures”) chosen 2026-09-14 over A (fold) and C (screens), both retired. The ledger digits are the TUI’s panel ids on both surfaces; 1-9 open the panel full-screen — nothing is hidden any more. Implementation: tasks/plan-dashboard-big-figures.md.'),
  ],
  launch: { view: 'canvas' },
};
writeFileSync('canvas.json', JSON.stringify(canvas, null, 2));
console.log('canvas.json written');

// ---- the sync sheet ----------------------------------------------------------

function syncSheet() {
  const mono = "'IBM Plex Mono',ui-monospace,Menlo,monospace";
  const sans = "'IBM Plex Sans',system-ui,sans-serif";
  const cond = "'IBM Plex Sans Condensed','IBM Plex Sans',system-ui,sans-serif";
  const term = (t) =>
    `<code style="font-family:${mono};font-size:12px;background:#0E1318;color:#D6DEE8;padding:2px 6px;border-radius:3px;white-space:pre">${t}</code>`;
  const th = (t) => `<th style="text-align:left;font-family:${sans};font-weight:600;font-size:12px;color:#5A6577;padding:6px 10px 6px 0;border-bottom:1px solid #D5DAE2">${t}</th>`;
  const td = (t, extra = '') => `<td style="font-family:${sans};font-size:13px;color:#1A212C;padding:7px 10px 7px 0;border-bottom:1px solid #E6E9EF;vertical-align:top;${extra}">${t}</td>`;
  const h2 = (t) => `<h2 style="font-family:${cond};font-size:20px;line-height:26px;margin:0;color:#1A212C">${t}</h2>`;
  const p = (t) => `<p style="font-family:${sans};font-size:13px;line-height:20px;margin:0;color:#5A6577;max-width:68ch;text-wrap:pretty">${t}</p>`;
  const glyph = (g, color) => `<span style="font-family:${mono};color:${color};font-weight:600">${g}</span>`;

  const map = [
    ['—', 'dashboard · header line, <b>four tiles</b> (the lights as block digits), the nudge, the nine-row ledger', 'Overview · the same: state line, tiles two per row, nudge, ledger rows as Buttons', 'dashboard (+ coach for the tiles)'],
    ['c', 'coach view (<code>c</code>)', 'Coach · card + next / snoozed / lifecycle + light detail', 'coach'],
    ['1', 'Context', 'Overview', 'summary.context'],
    ['2', 'Tokens &amp; Cost', 'Overview', 'summary.tokens, cost'],
    ['3', 'Limits', 'Overview', 'summary.limits'],
    ['4', 'Turn', 'Overview', 'summary.turn'],
    ['5', 'Tools', 'Tools', 'tools'],
    ['6', 'Agents &amp; MCP', 'Agents', 'agents'],
    ['7', 'Files', 'Files', 'files'],
    ['8', 'Events', 'Events', 'events'],
    ['9', 'Advisor · the nudge first, then the ranked rest', 'Advisor', 'advice'],
  ];
  const mapRows = map
    .map(
      ([n, tui, pane, q]) =>
        `<tr>${td(`<span style="font-family:${mono};color:#177F86;font-weight:600">${n}</span>`)}${td(tui)}${td(pane)}${td(`<span style="font-family:${mono};font-size:12px">${q}</span>`)}</tr>`,
    )
    .join('');

  const vocab = [
    [`${glyph('○', '#5D6B7D')} ${glyph('◐', '#E2B04A')} ${glyph('●', '#E5605E')}`, 'light level quiet / watch / act', 'the glyph carries the level; colour sits on top', 'ASCII <code>-</code> <code>+</code> <code>!</code>'],
    [`${glyph('●', '#5FC77E')} BUSY  ${glyph('○', '#5D6B7D')} IDLE  ${glyph('◆', '#E2B04A')} WAITING`, 'phase pill', 'TUI header, pane card, state line, footer of Claude Code’s own status', '<code>*</code> <code>o</code> <code>?</code>'],
    [`${glyph('■', '#5D6B7D')} ENDED  ${glyph('⏸', '#E2B04A')} PAUSED  LOOP`, 'lifecycle pills (LOOP is the word alone)', 'header only', '<code>#</code> <code>=</code>'],
    [`${glyph('▸', '#E5605E')}`, 'the nudge', 'slot headline, the Advisor panel’s first row (<code>▸ NOW</code>), L0 line', '<code>&gt;</code>'],
    [`${glyph('✓', '#D6DEE8')}  ${glyph('✗', '#E5605E')}`, 'a check ran / a call failed', 'rework light, Turn, Events, evidence rows', '<code>v</code> <code>x</code>'],
    [`${glyph('↻', '#D6DEE8')}  ${glyph('≈', '#D6DEE8')}  ${glyph('—', '#D6DEE8')}`, 'resets in / estimated / absent', 'every number: <code>—</code> never 0, <code>≈</code> when the shim or hooks are missing', '<code>@</code> <code>~</code> <code>-</code>'],
    [`${glyph('▇▇▇▁▁▁', '#5FC77E')}  ${glyph('▂▃▅▆█', '#4CC2C2')}`, 'gauge / sparkline', 'gauges coloured by band (ok · warn · crit); sparklines in the accent', '<code>###---</code> <code>.:-=+</code>'],
  ];
  const vocabRows = vocab
    .map(([g, name, where, ascii]) => `<tr>${td(g, 'white-space:nowrap')}${td(name)}${td(where)}${td(ascii)}</tr>`)
    .join('');

  const roles = [
    ['fg', '#D6DEE8', 'text', 'values, labels'],
    ['dim', '#5D6B7D', 'dimColor', 'secondary text, quiet lights, empty gauge'],
    ['accent', '#4CC2C2', 'suggestion', 'panel digits, running tool, agents, sparklines, focused border'],
    ['ok', '#5FC77E', 'success', 'BUSY, badges on, gauges below warn'],
    ['warn', '#E2B04A', 'warning', 'WAITING, watch lights, gauges in the warn band'],
    ['crit', '#E5605E', 'error', 'act lights, failures, the NOW nudge'],
    ['border', '#2A3644', 'dim text', 'frames; accent when focused'],
  ];
  const roleRows = roles
    .map(
      ([r, hex, key, use]) =>
        `<tr>${td(`<span style="display:inline-block;width:12px;height:12px;border-radius:2px;background:${hex};vertical-align:-1px;margin-right:8px;border:1px solid #D5DAE2"></span><span style="font-family:${mono};font-size:12px">${r}</span>`, 'white-space:nowrap')}${td(`<span style="font-family:${mono};font-size:12px">${hex}</span>`)}${td(`<span style="font-family:${mono};font-size:12px">${key}</span>`)}${td(use)}</tr>`,
    )
    .join('');

  const forms = [
    ['Tiles', 'the four lights as 3-row block digits: figure in the level colour, unit dim on the baseline, glyph + name, two sub-lines', 'TUI dashboard · pane Overview (two per row) · L1 below 60 / 50 columns'],
    ['Full card', 'state line · four light rows · slot (headline, action, meta) · next · snoozed', 'TUI coach view (c) · pane Coach'],
    ['L0 · ≥ 80 cols', term('◐412k ≈$.21 · ○cache 41m · ○5h 62% · ●fails 3 · ▸ Esc, give the missing fact'), 'TUI header row 3 (labelled) · $.ui.status · cctop query coach --line · tmux'],
    ['L1', term('◐412k ○41m ○62% ●3 ▸'), '$.ui.status below 80 columns'],
    ['L2', term('◐○○●'), 'the narrowest status line'],
  ];
  const formRows = forms.map(([f, what, where]) => `<tr>${td(`<b>${f}</b>`, 'white-space:nowrap')}${td(what)}${td(where)}</tr>`).join('');

  const keys = [
    ['TUI', 'dashboard', '<code>c</code> coach · <code>1-9</code> open a panel full-screen · <code>Esc</code> back · <code>Enter</code> act on the nudge · <code>a</code> ask · <code>t</code> theme · <code>L</code> sessions · <code>q</code>'],
    ['TUI', 'coach view', '<code>Enter</code> act (the ask popup, pre-filled) · <code>x</code> / <code>X</code> snooze · <code>e</code> why · <code>n</code> / <code>N</code> peek · <code>1-4</code> light detail · <code>l</code> log · <code>$</code> units · <code>Esc</code> back'],
    ['pane', 'any view', 'click a tab · <code>ctrl+x tab</code> then <code>tab</code> / <code>enter</code> · <code>/cctop-pane &lt;view&gt;</code> · no digit hotkeys (the band is gone)'],
    ['pane', 'Overview', 'ledger rows are Buttons: <code>5</code>–<code>9</code> switch to that tab, <code>1</code>–<code>4</code> unfold their block inline'],
    ['pane', 'Coach', '<code>[ fill ]</code> = $.prompt.fill, the person presses Enter · <code>[ snooze ]</code> · <code>[ why ]</code> · <code>[ context ] [ cache ] [ limits ] [ rework ]</code> switch the detail frame'],
  ];
  const keyRows = keys.map(([s, v, k]) => `<tr>${td(`<b>${s}</b>`)}${td(v)}${td(k)}</tr>`).join('');

  return `<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <script src="./support.js"></script>
</head>
<body>
<x-dc>
<helmet>
  <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&amp;family=IBM+Plex+Sans:wght@400;600&amp;family=IBM+Plex+Sans+Condensed:wght@700&amp;display=swap">
  <style>
    body { margin: 0; background: #F1F3F6; }
    a { color: #177F86; } a:hover { color: #0f5a5f; }
    code { font-family: ${mono}; font-size: 0.92em; background: #E6E9EF; padding: 1px 5px; border-radius: 3px; }
    table { border-collapse: collapse; width: 100%; }
  </style>
</helmet>
<div style="background:#F1F3F6;width:980px;min-height:1330px;box-sizing:border-box;padding:36px 40px;display:flex;flex-direction:column;gap:28px">
  <div style="display:flex;flex-direction:column;gap:8px">
    <div style="font-family:${mono};font-size:12px;letter-spacing:.08em;text-transform:uppercase;color:#5A6577">cctop · coach · design sheet</div>
    <h1 style="font-family:${cond};font-size:34px;line-height:1.05;margin:0;color:#1A212C;letter-spacing:-.01em">One coach, three surfaces — what stays in sync</h1>
    ${p('The TUI and the pane draw one object from <code>cctop query coach</code>. This sheet is the contract between them: the same words, glyphs, colour roles, panel numbers and forms, so a person switching from the terminal to the pane reads the same screen.')}
  </div>

  <div style="display:flex;flex-direction:column;gap:10px">
    ${h2('Surface map')}
    ${p('Panel digits are the TUI’s ids on both surfaces: the ledger row, the full-screen panel and the pane tab share the number. The pane groups panels 1–4 under Overview because of its width, and adds Coach as a named view; everything else is one-to-one.')}
    <table><thead><tr>${th('#')}${th('TUI')}${th('Pane view')}${th('cctop query')}</tr></thead><tbody>${mapRows}</tbody></table>
  </div>

  <div style="display:flex;flex-direction:column;gap:10px">
    ${h2('Vocabulary')}
    <table><thead><tr>${th('glyphs')}${th('meaning')}${th('where')}${th('ASCII')}</tr></thead><tbody>${vocabRows}</tbody></table>
  </div>

  <div style="display:flex;flex-direction:column;gap:10px">
    ${h2('Colour roles')}
    ${p('Colour is for state only — never for a phase, a tool or a category. The TUI reads them from the theme file (default-dark shown); the pane maps each role to a Claude Code theme key and so follows light and dark.')}
    <table><thead><tr>${th('role')}${th('default-dark')}${th('pane theme key')}${th('used for')}</tr></thead><tbody>${roleRows}</tbody></table>
  </div>

  <div style="display:flex;flex-direction:column;gap:10px">
    ${h2('Forms of the coach object')}
    <table><thead><tr>${th('form')}${th('what')}${th('where')}</tr></thead><tbody>${formRows}</tbody></table>
  </div>

  <div style="display:flex;flex-direction:column;gap:10px">
    ${h2('Keys and buttons')}
    <table><thead><tr>${th('surface')}${th('view')}${th('bindings')}</tr></thead><tbody>${keyRows}</tbody></table>
  </div>

  <div style="display:flex;flex-direction:column;gap:10px">
    ${h2('Rules both surfaces obey')}
    ${p('One nudge at a time · NOW › NEXT › LATER · the slot changes only at a human-turn boundary, on a NOW event or on retirement, never on a render tick · at most one newly promoted nudge per human turn · lights re-evaluate every tick and are never rate-limited · every number renders <code>—</code> when its source is absent and <code>≈</code> when estimated · wording is Claude Code’s own where it has a word for the thing (footer phrases, error taxonomy, /context thresholds) · lines are cut at 52 cells in the query so every surface shows the same text.')}
  </div>
</div>
</x-dc>
</body>
</html>
`;
}
