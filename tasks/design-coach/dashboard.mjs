// The dashboard — direction B, "Big figures" (chosen 2026-09-14): the four
// lights as 3-row block digits (the terminal's only large type), the nudge,
// then a borderless ledger of nine rows. Drawn for the terminal (122
// columns) and the pane (72 body columns), same scenario as screens.mjs.
import { pad, stacked, vw } from './mock.mjs';
import { viewBar } from './screens.mjs';

// ---- the block-digit font: 3 rows × 3 cells per digit ---------------------

const FONT = {
  0: ['█▀█', '█ █', '▀▀▀'],
  1: ['▄█ ', ' █ ', ' ▀ '],
  2: ['▀▀█', '█▀▀', '▀▀▀'],
  3: ['▀▀█', '▀▀█', '▀▀▀'],
  4: ['█ █', '▀▀█', '  ▀'],
  5: ['█▀▀', '▀▀█', '▀▀▀'],
  6: ['█▀▀', '█▀█', '▀▀▀'],
  7: ['▀▀█', '  █', '  ▀'],
  8: ['█▀█', '█▀█', '▀▀▀'],
  9: ['█▀█', '▀▀█', '▀▀▀'],
};

/** `digits` as three tagged rows in `style`, one cell between digits. */
export function big(digits, style) {
  const rows = ['', '', ''];
  [...digits].forEach((d, i) => {
    for (let r = 0; r < 3; r++) rows[r] += (i > 0 ? ' ' : '') + FONT[d][r];
  });
  return rows.map((r) => `{${style}:${r}}`);
}

/**
 * A tile of `w` cells: the big figure, its unit on the baseline, and three
 * short lines beside it (the light's glyph and name, two sub-lines).
 */
function tile(w, digits, unit, digitStyle, glyph, name, sub1, sub2) {
  const d = big(digits, digitStyle);
  const col = digits.length * 4 + 2; // digits, a space, the unit's cell, a gap
  const r1 = pad(d[0], col) + `${glyph} {b:${name}}`;
  const r2 = pad(d[1], col) + sub1;
  const r3 = pad(d[2] + ` {dim:${unit}}`, col) + sub2;
  return [pad(r1, w), pad(r2, w), pad(r3, w)];
}

const Q = '{dim:○}';
const W_ = '{warn:◐}';
const A_ = '{crit:●}';

function tilesRow(tiles) {
  const out = ['', '', ''];
  for (const t of tiles) for (let r = 0; r < 3; r++) out[r] += t[r];
  return out;
}

const TILES = (w) => [
  tile(w, '41', '%', 'warn', W_, 'context', '412k of 1.00M', '≈$.21/call'),
  tile(w, '62', '%', 'fg', Q, 'limits', '5h{dim: · }↻ 2h10', 'agents 3{dim: · }86%'),
  tile(w, '41', 'm', 'fg', Q, 'cache', 'warm{dim: · }1h TTL', 'misses 2'),
  tile(w, '3', '', 'crit', A_, 'rework', '{crit:fails}{dim: · }gh pr view', 'edits 3 {dim:✓ none 11m}'),
];

// =============================================================================
// B · Big figures
// =============================================================================

/** A ledger row: the accent digit, a dim name, the values; a second dim row when given. */
function ledger(W, hot, name, values, more) {
  const head = ` {acc b:${hot}} ` + pad(`{dim:${name}}`, 10);
  const rows = [pad(head + values, W)];
  if (more) rows.push(pad(pad('', vw(head)) + `{dim:${more}}`, W));
  return rows;
}

export function bigTerminal() {
  const W = 122;
  const tiles = tilesRow(TILES(30)).map((r) => ' ' + r);
  const rows = [
    pad(` {b:cctop}  claude-opus-5{dim: · }turn 14{dim: · }1:12:08{dim: · }~/code/cctop{dim: · }PR #142`, W - 41) + `{ok b:● IMPLEMENTING}{dim: · }18c +42k{dim: · }silent 3:50`,
    '',
    ...tiles,
    '',
    ` {crit:▸} {b:3 fails in a row: gh pr view ×2 (8), git push (128)}{dim: — }Esc, give the missing fact — or run it, paste tail  {dim:NOW · call 12}`,
    '',
    ...ledger(W, 1, 'Context', `${stacked([[3, 'fg'], [5, 'dim'], [4, 'fg'], [2, 'dim'], [2, 'fg'], [1, 'dim'], [6, 'fg']], 40)} prefix 57k{dim: · }inputs 94k{dim: · }results 73k{dim: · }thinking 40k{dim: · }harness 40k`,
      '+42k/turn · autocompact 967k · 555k left · compactions 0 · since /clear 20:04 · re-reads 2'),
    ...ledger(W, 2, 'Tokens', `cache read 33.0M{dim: · }write 597k{dim: · }output 67k{dim: · }{ok:warm} 41:12{dim: · }misses 2 (model_changed 427k)`,
      '$18.6 · $6.03/h · $/call ≈$.21 · $/turn ≈$2.6 · next 30c ≈$6.3 · agents $4.10 (22 %) · code-review 31 %'),
    ...ledger(W, 3, 'Limits', `5h 62 % ↻ 2h10{dim: · }7d 11 % ↻ 4d3h{dim: · weight ×5 (opus) · long_context 34 % · other sessions 2}`),
    ...ledger(W, 4, 'Turn', `{ok:IMPLEMENTING} ×18{dim: · }api ≈1:02{dim: · }tools 2:48{dim: · steers 1 queued · background 0 · hooks p99 31ms · denials 2}`),
    ...ledger(W, 5, 'Tools', `257 calls{dim: · }{crit:3 err}{dim: · }explore 41{dim: · }gitread 12 ({crit:3 ✗}){dim: · }Edit 11{dim: · }Read 18{dim: · }Agent 4{dim: · top ctx Read state.rs 12k}`),
    ...ledger(W, 6, 'Agents', `3 run{dim: · }86 %{dim: · }Explore 1:12 71k{dim: · }fork 0:48 412k{dim: · }wf:review 0:31 38k{dim: · $4.10 · }{crit:2 failed}{dim: · }{warn:! auth}`),
    ...ledger(W, 7, 'Files', `9 touched{dim: · }render.rs {warn:E×5}{dim: · }coach_view.rs W×1 E×2 {warn:IDE edit}{dim: · +412/−87 · commit 42m ago}`),
    ...ledger(W, 8, 'Events', `{dim:20:41:05} coach fired failure-cascade {crit:NOW}{dim: · }{dim:20:41:05} tool git push {crit:✗ 128}{dim: · }{dim:20:41:02} tool gh pr view {crit:✗ 8}`),
    ...ledger(W, 9, 'Advisor', `{crit:NOW 1}{dim: · }next verify-gap → turn end{dim: · }snoozed cache-miss (4 turns){dim: · }prefix tip (session)`),
    '',
    `{dim: ?help  1-9 open a panel full-screen  c coach  a ask  t theme  q}`,
  ];
  return rows.map((r) => pad(r, W));
}

export function bigPanel() {
  const W = 72;
  const bar = viewBar(W, 'Overview');
  const t = TILES(35);
  const top = tilesRow([t[0], t[1]]).map((r) => ' ' + r);
  const bottom = tilesRow([t[2], t[3]]).map((r) => ' ' + r);
  const rows = [
    ...bar,
    pad(` {ok b:● IMPLEMENTING}{dim: · }render.rs ×5{dim: · }18c +42k{dim: · }silent 3:50`, W - 8) + `{dim:turn 14}`,
    '',
    ...top,
    '',
    ...bottom,
    '',
    ` {crit:▸} {b:3 fails in a row: gh pr view ×2 (8), git push (128)}`,
    `   Esc, give the missing fact — or run it, paste tail`,
    '',
    ...ledger(W, 1, 'Context', `412k / 1.00M{dim: · }+42k/turn{dim: · }autocompact 967k`, 'prefix 57k · inputs 94k · results 73k · think 40k'),
    ...ledger(W, 2, 'Tokens', `$18.6{dim: · }$6.03/h{dim: · }≈$.21/call{dim: · }{ok:warm} 41:12`, 'read 33.0M · write 597k · out 67k · misses 2'),
    ...ledger(W, 3, 'Limits', `5h 62 % ↻ 2h10{dim: · }7d 11 % ↻ 4d3h`),
    ...ledger(W, 4, 'Turn', `×18{dim: · }api ≈1:02{dim: · }tools 2:48{dim: · }steers 1{dim: · }denials 2`),
    ...ledger(W, 5, 'Tools', `257 calls{dim: · }{crit:3 err}{dim: · }explore 41{dim: · }gitread 12{dim: · }Edit 11`),
    ...ledger(W, 6, 'Agents', `3 run{dim: · }86 %{dim: · }$4.10{dim: · }{crit:2 failed}`),
    ...ledger(W, 7, 'Files', `9 touched{dim: · }render.rs {warn:E×5}{dim: · }+412/−87`),
    ...ledger(W, 8, 'Events', `{dim:20:41:05} git push {crit:✗ 128}{dim: · }{dim:20:41:02} gh pr view {crit:✗ 8}`),
    ...ledger(W, 9, 'Advisor', `{crit:NOW 1}{dim: · }next verify-gap{dim: · }snoozed 2`),
  ];
  return rows.map((r) => pad(r, W));
}
