// The screens of the coach design canvas, one scenario throughout: turn 14
// of a session on claude-opus-5, mid-turn, 18 calls into a silent run after
// editing render.rs five times, three consecutive gh/git failures, 412 k of
// a 1 M window, three agents running (PRD §6.2's first mockup).
import { SEP, beside, clip, frame, gauge, pad, stacked, vw } from './mock.mjs';

/** The first variant that fits `inner` cells. */
function fit(inner, ...variants) {
  for (const v of variants) if (vw(v) <= inner) return v;
  throw new Error(`nothing fits ${inner}: ${variants[variants.length - 1]}`);
}

// ---- the coach object, drawn the same on every surface ------------------

const LIGHT = { quiet: '{dim:○}', watch: '{warn:◐}', act: '{crit:●}' };

/** The four light rows at `inner` cells (PRD §6.3). */
export function lightRows(inner) {
  const ctx = fit(
    inner,
    `${LIGHT.watch} context  412k ${gauge(0.41, 10, 'warn')} {warn:41%}{dim: · }≈$.21/call`,
    `${LIGHT.watch} context  412k ${gauge(0.41, 10, 'warn')} {warn:41%}`,
  );
  const cache = `${LIGHT.quiet} cache    warm 41m {dim:(1h)}`;
  const limits = fit(
    inner,
    `${LIGHT.quiet} limits   5h 62%{dim: · }↻ 2h10{dim: · }agents 3 run{dim: · }86%`,
    `${LIGHT.quiet} limits   5h 62%{dim: · }↻ 2h10`,
  );
  const rework = fit(
    inner,
    `${LIGHT.act} rework   {crit:3 fails}{dim: ▸}gh pr view{dim: · }edits 3 {dim:✓ none 11m}`,
    `${LIGHT.act} rework   {crit:3 fails}{dim: ▸}gh pr view`,
  );
  return [ctx, cache, limits, rework];
}

// The phase pill carries the header's glyph where it fits; at 56 columns
// the PRD's glyph-less form is the one that measures 52.
export function stateLine(inner) {
  return fit(
    inner,
    `{ok b:● IMPLEMENTING}{dim: · }render.rs ×5{dim: · }18c +42k{dim: · }silent 3:50`,
    `{ok b:IMPLEMENTING}{dim: · }render.rs ×5{dim: · }18c +42k{dim: · }silent 3:50`,
    `{ok b:● IMPLEMENTING}{dim: · }18c +42k{dim: · }3:50`,
  );
}

/** The dashboard header's second row: mode, effort, cost, PR, badges. */
export function headerRow(inner) {
  const badges = `{ok:bin shim hooks 2.1.270}`;
  const left = fit(
    inner - 22 - 2,
    `auto{dim: · }high{dim: · }thinking{dim: · }$18.6{dim: · }$6.03/h{dim: · }PR #142{dim: · }prev. session $0.16 / 1m15`,
    `auto{dim: · }high{dim: · }thinking{dim: · }$18.6{dim: · }PR #142`,
  );
  return pad(left, inner - 22) + badges;
}

/** The slot: headline, action, evidence row. */
export function slotRows(inner, opts = {}) {
  const action = `  Esc, give the missing fact — or run it, paste tail`;
  // The TUI's row names its peek key; the pane passes its own meta (no keys there).
  const meta = opts.meta ?? `{dim:  NOW · fired at call 12 · +2 queued · n peek}`;
  // Narrow: the headline and the action wrap onto two rows each.
  if (inner < vw(action)) {
    return [
      `{crit:▸} {b:3 fails in a row: gh pr view ×2}`,
      `  {b:(8), git push (128)}`,
      `  Esc, give the missing fact — or`,
      `  run it, paste tail`,
    ];
  }
  const head = fit(
    inner,
    `{crit:▸} {b:3 fails in a row: gh pr view ×2 (8), git push (128)}`,
    `{crit:▸} {b:3 in a row: gh pr view ×2 (8), git push (128)}`,
  );
  return [head, action, meta];
}

/** The whole coach card (state line, lights, slot) at `w` cells. */
export function coachCard(w, opts = {}) {
  const inner = w - 4;
  const rows = [stateLine(inner)];
  // On the pane's Overview the card is also the header: mode, cost, badges.
  if (opts.header) rows.push(headerRow(inner));
  rows.push(SEP, ...lightRows(inner), SEP, ...slotRows(inner, opts));
  if (opts.tail) rows.push(...opts.tail);
  const summary =
    opts.summary ?? fit(w - 12, `cctop {dim:───} claude-opus-5 {dim:·} turn 14`, `opus-5 {dim:·} turn 14`);
  return frame(w, opts.title ?? 'coach', summary, rows, opts);
}

// ---- TUI: the coach view at 56 × 17 (PRD §6.2) ---------------------------

export function tuiCoach() {
  const w = 56;
  const rows = coachCard(w, {
    tail: ['', `{dim:next}     verify-gap → turn end{dim: · }edits 3 ✓ none 11m`, `{dim:snoozed}  cache-miss (4 turns){dim: · }prefix tip (session)`],
  });
  rows.push(`{dim: c dashboard  Enter act  x snooze  e why  1-4 light  q}`);
  return rows.map((r) => pad(r, w));
}

/** The idle form: nothing to act on, the lifecycle rows show the coach acted. */
export function tuiCoachIdle() {
  const w = 56;
  const inner = w - 4;
  const rows = [
    `{dim:○ IDLE 4m}{dim: · }cargo test ok 6m ago{dim: · }committed 3m ago`,
    SEP,
    `${LIGHT.quiet} context   96k ${gauge(0.1, 10, 'ok')} 10%{dim: · }≈$.05/call`,
    `${LIGHT.quiet} cache    warm 55m {dim:(1h)}`,
    `${LIGHT.quiet} limits   5h 31%{dim: · }↻ 3h40`,
    `${LIGHT.quiet} rework   edits 0{dim: · }✓ 6m ago{dim: · }clean{dim: · }commit 3m`,
    SEP,
    `{dim:  quiet · nothing to act on · 0 nudges this hour}`,
    `{dim:  20:41 acted   verify-gap → cargo test 31s later}`,
    `{dim:  20:20 retired plan-first · EnterPlanMode seen}`,
    `{dim:next}     —`,
    `{dim:snoozed}  —`,
  ];
  const out = frame(w, 'coach', `cctop {dim:───} claude-opus-5 {dim:·} turn 15`, rows);
  out.push(`{dim: c dashboard  Enter act  x snooze  e why  1-4 light  q}`);
  void inner;
  return out.map((r) => pad(r, w));
}

// ---- TUI: the coach view on a wide terminal (122 × 21) ------------------

export function tuiCoachWide() {
  const cardW = 56;
  const whyW = 122 - cardW - 2;
  const card = coachCard(cardW, {
    tail: ['', `{dim:next}     verify-gap → turn end{dim: · }edits 3 ✓ none 11m`, `{dim:snoozed}  cache-miss (4 turns){dim: · }prefix tip (session)`],
  });
  const why = frame(whyW, 'why', `failure-cascade {dim:(A38)} {dim:·} NOW`, [
    `3 consecutive {b:is_error} in the last 10 calls of the turn,`,
    `2 sharing the prefix {b:gh pr view}{dim: · }no Edit between`,
    `{dim:acted when}  a call with that prefix succeeds`,
    `{dim:precision}   right 10/10 last month{dim: · }fires ~14×/month`,
    `{dim:saving}      ≈2 blind retries{dim: · }≈$0.40 at this context`,
    `{dim:evidence}    #41 gh pr view 142 {crit:✗ 8}{dim: · }#42 gh pr view {crit:✗ 8}`,
    `             #43 git push {crit:✗ 128}`,
    SEP,
    `{dim:20:41} fired    failure-cascade {crit:NOW}{dim: · }call 12`,
    `{dim:20:41} acted    verify-gap → cargo test ran 31 s later`,
    `{dim:20:20} retired  plan-first{dim: · }EnterPlanMode seen`,
    `{dim:20:04} expired  explore-delegate{dim: · }Agent spawned`,
  ], { height: 13 });
  const rows = beside(card.map((r) => pad(r, cardW + 2)), why);
  rows.push(pad(`{dim: c dashboard  Enter act  x snooze  e why  n/N peek  1-4 light  l log  $ units  Esc back  q}`, 122));
  return rows.map((r) => pad(r, 122));
}

// ---- pane: the view bar ---------------------------------------------------

const VIEWS = ['Coach', 'Overview', 'Tools', 'Agents', 'Files', 'Events', 'Advisor'];

/** `cctop  Coach  Overview …` with `active` inverse; wraps at `cols`. */
export function viewBar(cols, active) {
  const lines = [];
  let line = '{b:cctop}';
  let used = 5;
  let prevActive = false;
  for (const v of VIEWS) {
    const isActive = v === active;
    // Two cells between tabs; the active tab's inverse padding is one of them.
    let sep = used === 0 ? 0 : prevActive || isActive ? 1 : 2;
    const w = v.length + (isActive ? 2 : 0);
    if (used + sep + w > cols) {
      lines.push(line);
      line = '';
      used = 0;
      sep = 0;
    }
    line += ' '.repeat(sep) + (isActive ? `{inv: ${v} }` : `{fg:${v}}`);
    used += sep + w;
    prevActive = isActive;
  }
  lines.push(line);
  return lines.map((l) => pad(l, cols));
}

// ---- pane: Coach at 72 body columns --------------------------------------

export function paneCoach() {
  const W = 72;
  const bar = viewBar(W, 'Coach');
  const card = coachCard(W, {
    meta: `{dim:  NOW · fired at call 12 · +2 queued}`,
    tail: [
      `  {dim:[} fill {dim:]}  {dim:[} snooze {dim:]}  {dim:[} why {dim:]}`,
      '',
      `{dim:next}     verify-gap → turn end{dim: · }edits 3 ✓ none 11m`,
      `{dim:snoozed}  cache-miss (4 turns){dim: · }prefix tip (session)`,
      SEP,
      `{dim:20:41} acted    verify-gap → cargo test ran 31 s later`,
      `{dim:20:20} retired  plan-first{dim: · }EnterPlanMode seen`,
    ],
  });
  const detail = frame(W, 'rework', `{crit:●} detail {dim:·} the light that is red`, [
    `{dim:last check}   cargo test{dim: · }ok{dim: · }11m ago (before the edits)`,
    `{dim:fails}        gh pr view ×2 (exit 8){dim: · }git push (exit 128)`,
    `{dim:denials}      automode-blocked gh api ×2`,
    `{dim:uncommitted}  +412 −87{dim: · }9 files{dim: · }18 edits{dim: · }42m`,
    `{dim:rewind}       3 checkpoints since #12{dim: · }bash writes not covered`,
    `{dim:[} context {dim:]}  {dim:[} cache {dim:]}  {dim:[} limits {dim:]}  {inv: rework }`,
  ]);
  const rows = [...bar, ...card, ...detail];
  return rows.map((r) => pad(r, W));
}

// ---- pane: narrow, 48 body columns ---------------------------------------

export function paneNarrow() {
  const W = 48;
  const bar = viewBar(W, 'Coach');
  const card = coachCard(W, { meta: `{dim:  NOW · call 12}` });
  const inner = W - 4;
  const context = frame(W, 'Context', `{warn:41 %}`, [
    stacked([[3, 'fg'], [4, 'dim'], [3, 'fg'], [2, 'dim'], [2, 'fg'], [1, 'dim'], [3, 'fg']], inner),
    `{warn:412k} / 1.00M {warn:(41 %)}{dim: · }+42k/turn`,
    `prefix 57k{dim: · }inputs 94k{dim: · }results 73k`,
    `autocompact 967k{dim: · }555k left`,
  ], { hot: 1 });
  const rows = [...bar, ...card, ...context];
  return rows.map((r) => pad(r, W));
}

// ---- the compact forms ----------------------------------------------------

export function statusForms() {
  const W = 96;
  const l0 = `${LIGHT.watch}412k ≈$.21{dim: · }${LIGHT.quiet}cache 41m{dim: · }${LIGHT.quiet}5h 62%{dim: · }${LIGHT.act}fails 3{dim: · }{crit:▸} Esc, give the missing fact`;
  const l1 = `${LIGHT.watch}412k ${LIGHT.quiet}41m ${LIGHT.quiet}62% ${LIGHT.act}3 {crit:▸}`;
  const l2 = `${LIGHT.watch}${LIGHT.quiet}${LIGHT.quiet}${LIGHT.act}`;
  return {
    l0: [pad(l0, W)],
    l1: [pad(l1, 24)],
    l2: [pad(l2, 8)],
  };
}

// ---- nav options ----------------------------------------------------------

export function navOptions() {
  const W = 72;
  const a = viewBar(W, 'Overview');
  // B: the band's form carried into the pane: `1: Overview` plain Buttons.
  const b = [];
  {
    const items = [['c', 'Coach'], ['1', 'Overview'], ['2', 'Tools'], ['3', 'Agents'], ['4', 'Files'], ['5', 'Events'], ['6', 'Advisor']];
    let line = '{b:cctop}';
    let used = 5;
    for (const [k, v] of items) {
      const w = k.length + 2 + v.length;
      if (used + 2 + w > W) {
        b.push(pad(line, W));
        line = '';
        used = 0;
      }
      line += (used === 0 ? '' : '  ') + `{acc:${k}}:{fg: ${v}}`;
      used += (used === 0 ? 0 : 2) + w;
    }
    b.push(pad(line, W));
  }
  // The band above the prompt as it is today, 120 columns wide.
  const band = [
    pad(`{dim:cctop} {acc:1}: Overview  {acc:2}: Tools  {acc:3}: Agents  {acc:4}: Files  {acc:5}: Events  {acc:6}: Advisor`, 117) + `{dim:[—]}`,
  ];
  return { a, b, band };
}

// ---- the whole terminal, 200 columns, after the move ---------------------

export function terminalAfter(paneRows) {
  const COLS = 200;
  const paneW = 72;
  const leftW = COLS - paneW - 1;
  const transcript = [
    ``,
    `  {ok b:✻} {b:Claude Code} {dim:v2.1.270}`,
    `    {dim:Opus 5 with max effort · Claude Max}`,
    `    {dim:~/code/cctop · /rc}`,
    ``,
    `{dim:❯} /rc`,
    ``,
    `{dim:❯} add the coach view behind {acc:c} and move the nav bar into the pane`,
    ``,
    `{dim:●} I'll read the pane's band code first, then move the view bar.`,
    ``,
    `{dim:●} {b:Read}(plugin/hooks/pane.tsx)`,
    `  {dim:⎿  Read 685 lines}`,
    ``,
    `{dim:●} {b:Edit}(plugin/hooks/pane.tsx)`,
    `  {dim:⎿  Updated plugin/hooks/pane.tsx with 41 additions and 58 removals}`,
    `       {ok:+  // The view bar lives in the pane: cctop  Coach  Overview …}`,
    `       {crit:-  // The band above the prompt while the pane is docked}`,
    ``,
    `{dim:●} {b:Bash}(npm test)`,
    `  {dim:⎿  Running…}`,
    ``,
    `{ok:✻} {dim:Crunching… (48s · ↑ 2.1k tokens · esc to interrupt)}`,
  ];
  const pane = paneRows.map((r) => pad(r, paneW));
  const H = 44;
  // The prompt box and footer at the bottom of the transcript column.
  const bottom = [
    `{bd:╭${'─'.repeat(leftW - 4)}╮}`,
    `{bd:│} {dim:❯} {inv: }` + ' '.repeat(leftW - 8) + `{bd:│}`,
    `{bd:╰${'─'.repeat(leftW - 4)}╯}`,
    `  {dim:~/code/cctop} {acc:git:main} {b:Opus 5} {dim:ctx:} {warn:41%} {dim:plan 5h:} 62% {dim:(resets 2h10)} {dim:plan 7d:} 11%`,
    `  {ok:⏵⏵ auto mode on} {dim:(shift+tab to cycle)}`,
  ];
  const rows = [];
  for (let i = 0; i < H; i++) {
    let l;
    if (i < transcript.length) l = transcript[i];
    else if (i >= H - bottom.length) l = bottom[i - (H - bottom.length)];
    else l = '';
    const r = pane[i] ?? '';
    rows.push(pad(clip(l, leftW), leftW) + `{bd:▏}` + pad(r, paneW));
  }
  return rows.map((r) => pad(r, COLS));
}
