import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import type { RenderElement, RenderNode, SessionUsage } from 'claude-code';
import { fakeElements, textOf } from './harness';
import { renderToText } from './render';
import { fixture } from './fixture';
import { initialModel, reduce, type Action, type Binary, type Model } from '../../plugin/hooks/model';
import {
  L2_MAX,
  NEEDS_BINARY,
  TILES_MIN,
  dashboardOf,
  engineTiles,
  renderOverview,
  viewOfDigit,
  type OverviewActions,
} from '../../plugin/hooks/views/overview';
import { BIG_GLYPHS, bigDigits } from '../../plugin/hooks/views/frame';

// The Overview (plan B) from the fixtures: `cctop query dashboard` on
// fixture A (tests/pane/fixtures/dashboard.json) drawn at 40 / 50 / 72 / 100
// body columns; a turn started at T0 with Bash running since T0 + 2 s, drawn
// 48 s in, for the engine-side rows.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const NOW = T0 + 48_000;
const el = fakeElements(new Map());
const repo = join(__dirname, '..', '..', '..');

type Options = { usage?: boolean; summary?: boolean; dashboard?: boolean | unknown; binary?: Binary; unfolded?: number[] };

function build(opts: Options = {}): Model {
  const actions: Action[] = [
    { type: 'session.start', at: T0 - 1000 },
    { type: 'session.model', name: 'claude-sonnet-5' },
    { type: 'binary', binary: opts.binary ?? 'present' },
    { type: 'turn.start', at: T0 },
    { type: 'tool.start', name: 'Bash', at: T0 + 2000 },
  ];
  if (opts.usage ?? true) actions.push({ type: 'usage', usage: fixture<SessionUsage>('usage'), at: NOW });
  if (opts.summary ?? true) actions.push({ type: 'query', verb: 'summary', data: fixture('summary') });
  const dash = opts.dashboard ?? true;
  if (dash !== false) actions.push({ type: 'query', verb: 'dashboard', data: dash === true ? fixture('dashboard') : dash });
  const model = actions.reduce(reduce, initialModel());
  return opts.unfolded === undefined ? model : { ...model, unfolded: opts.unfolded };
}

function raw(model: Model, columns: number, placement: 'dock' | 'inline' = 'dock', actions?: OverviewActions): string[] {
  const buttons = actions === undefined ? undefined : { el, actions };
  return renderToText(renderOverview(model, el, columns, placement, NOW, buttons), columns);
}

function has(lines: string[], text: string | RegExp): void {
  const hit = lines.some((r) => (typeof text === 'string' ? r.includes(text) : text.test(r)));
  assert.ok(hit, `no row has ${String(text)}: ${JSON.stringify(lines)}`);
}

function fits(lines: string[], columns: number): void {
  for (const row of lines) assert.ok([...row].length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };

function walk(node: RenderNode | undefined, visit: (n: Node) => void): void {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return;
  const n = node as Node;
  visit(n);
  for (const c of n.children ?? []) walk(c, visit);
}

function keyed(tree: RenderElement): Map<string, string> {
  const out = new Map<string, string>();
  walk(tree, (n) => {
    if (n.type === 'Box' && typeof n.props?.key === 'string') out.set(n.props.key, textOf(n.children));
  });
  return out;
}

function buttons(tree: RenderElement): Node[] {
  const out: Node[] = [];
  walk(tree, (n) => {
    if (n.type === 'Button') out.push(n);
  });
  return out;
}

const THEME_KEYS: Record<string, string> = { success: 'green', warning: 'yellow', error: 'red', suggestion: 'cyan' };

test('the dashboard fixture parses into tiles, a nudge and nine rows', () => {
  const d = dashboardOf(fixture('dashboard'));
  assert.ok(d !== null);
  assert.equal(d.tiles.length, 4);
  assert.deepEqual(
    d.tiles.map((t) => t.id),
    ['context', 'cache', 'limits', 'rework'],
  );
  assert.equal(d.rows.length, 9);
  assert.deepEqual(
    d.rows.map((r) => r.digit),
    [1, 2, 3, 4, 5, 6, 7, 8, 9],
  );
  assert.ok(d.headerLine.startsWith('cctop  claude-sonnet-5 · turn 14'), d.headerLine);
  assert.equal(d.phase.word, 'IDLE');
  assert.ok(d.nudge !== null && d.nudge.line.startsWith('Edited 1 src file, no test/build run yet'), JSON.stringify(d.nudge));
  assert.equal(d.tiles[0].figure, '40');
  assert.equal(d.tiles[0].unit, '%');
  assert.equal(dashboardOf(null), null);
  assert.equal(dashboardOf({ tiles: 'x' }), null);
});

test('the block font is 3 × 3 per glyph, one cell between digits', () => {
  for (const [c, rows] of Object.entries(BIG_GLYPHS)) {
    assert.equal(rows.length, 3, c);
    for (const r of rows) assert.equal([...r].length, 3, `${c}: ${JSON.stringify(r)}`);
  }
  const lines = bigDigits('41', 'error');
  assert.deepEqual(
    lines.map((l) => l.map((s) => s.text).join('')),
    ['█ █  ▄█', '▀▀█   █', '  ▀   ▀'],
  );
  assert.equal(lines[0][0].color, 'error');
  assert.equal(bigDigits('—')[1][0].text, '▀▀▀');
});

for (const columns of [50, 72, 100]) {
  test(`overview at ${columns} columns: header, tiles two per row, the nudge, nine ledger rows`, () => {
    const model = build();
    const lines = raw(model, columns);
    fits(lines, columns);
    assert.ok(lines[0].startsWith(' cctop  claude-sonnet-5 · turn 14'), lines[0]);
    assert.ok(lines[0].includes('○ IDLE'), 'the phase cell: ' + lines[0]);
    // Two tiles per row: the context and cache names on one line, limits and rework further down.
    const names = lines.filter((l) => /[○◐●] (context|cache|limits|rework)/.test(l));
    assert.ok(names.length >= 2, JSON.stringify(lines));
    assert.ok(names[0].includes('context') && names[0].includes('cache'), names[0]);
    has(lines, /^ ▸ Edited 1 src file, no test\/build run yet/);
    for (let d = 1; d <= 9; d++) assert.ok(lines.some((l) => new RegExp(`^ ?${d} `).test(l)), `row ${d}: ${JSON.stringify(lines)}`);
    has(lines, /^ ?1 Context\s+▇/);
    has(lines, /^ ?9 Advisor\s+NEXT A32/);
    // The detail rows sit under their value rows from 50 columns.
    has(lines, /^ {13}\S/);
  });
}

test('below 50 columns the tiles collapse to the coach line, below 40 to its glyphs, and the rows keep their names', () => {
  const model = build();
  const d = dashboardOf(fixture('dashboard'))!;
  const narrow = raw(model, 48);
  fits(narrow, 48);
  assert.ok(!narrow.some((l) => l.includes('▀▀▀')), 'no block digits');
  has(narrow, ` ${d.lines.l1}`);
  const tiny = raw(model, 36);
  fits(tiny, 36);
  has(tiny, ` ${d.lines.l2}`);
  assert.ok(tiny.some((l) => /^ ?1 Context/.test(l)), JSON.stringify(tiny));
  assert.equal(TILES_MIN, 50);
  assert.equal(L2_MAX, 40);
});

test('ledger rows are plain Buttons: 5–9 open the view, 1–4 unfold their block and fold again', () => {
  const pressed: number[] = [];
  const actions: OverviewActions = { row: (d) => pressed.push(d) };
  const model = build();
  const tree = renderOverview(model, el, 72, 'dock', NOW, { el, actions });
  const rows = buttons(tree);
  assert.deepEqual(
    rows.map((b) => [b.props?.hotkey, b.props?.plain, b.props?.key]),
    [1, 2, 3, 4, 5, 6, 7, 8, 9].map((d) => [String(d), true, `ledger-${d}`]),
  );
  assert.deepEqual(rows.map((b) => String(b.props?.label).trim()), ['Context', 'Tokens', 'Limits', 'Turn', 'Tools', 'Agents', 'Files', 'Events', 'Advisor']);
  assert.equal(viewOfDigit(5), 'tools');
  assert.equal(viewOfDigit(9), 'advisor');
  assert.equal(viewOfDigit(1), null);
  // Unfolded rows 1 and 3 draw the Context and Limits frames beneath them.
  const unfolded = raw(build({ unfolded: [1, 3] }), 72);
  assert.ok(unfolded.some((l) => l.startsWith('╭1 Context')), JSON.stringify(unfolded));
  assert.ok(unfolded.some((l) => l.startsWith('╭3 Limits')), JSON.stringify(unfolded));
  assert.ok(!unfolded.some((l) => l.startsWith('╭2 Tokens')), JSON.stringify(unfolded));
  const folded = raw(build(), 72);
  assert.ok(!folded.some((l) => l.startsWith('╭')), 'no frames while folded');
  // The model toggles.
  const m1 = reduce(build(), { type: 'overview.toggle', digit: 2 });
  assert.deepEqual(m1.unfolded, [2]);
  assert.deepEqual(reduce(m1, { type: 'overview.toggle', digit: 2 }).unfolded, []);
  assert.equal(pressed.length, 0);
});

test('before the binary answers the tiles read the engine, and a missing binary says so', () => {
  const model = build({ dashboard: false });
  const tiles = engineTiles(model);
  assert.equal(tiles[0].figure, '40', 'the usage read: 396365 of 1M');
  assert.equal(tiles[0].sub1, '396k of 1.0M');
  assert.equal(tiles[1].figure, '—');
  const lines = raw(model, 72);
  fits(lines, 72);
  assert.ok(lines[0].startsWith(' cctop  claude-sonnet-5 · turn 1'), lines[0]);
  assert.ok(lines.some((l) => l.includes('◐ context')), JSON.stringify(lines));
  has(lines, 'waiting for cctop query dashboard…');
  const missing = raw(build({ dashboard: false, binary: 'missing' }), 72);
  has(missing, NEEDS_BINARY);
  // No usage read either: the context tile is `—`.
  assert.equal(engineTiles(build({ dashboard: false, usage: false }))[0].figure, '—');
});

test('the inline form keeps the flat header, gains the coach line, then Context and Limits', () => {
  const lines = raw(build(), 80, 'inline');
  fits(lines, 80);
  assert.match(lines[0], /^● BUSY · turn 1 · 0:48 · claude-sonnet-5/);
  const d = dashboardOf(fixture('dashboard'))!;
  assert.equal(lines[1], d.lines.l1);
  assert.ok(lines.some((l) => /^Context$/.test(l)), JSON.stringify(lines));
  assert.ok(lines.some((l) => /^Limits$/.test(l)), JSON.stringify(lines));
  assert.ok(!lines.some((l) => l.startsWith('╭') || /^ ?\d Context/.test(l)), 'no frames, no ledger inline');
});

test('every keyed row is a metric id from docs/metrics.md and colours are theme keys only', () => {
  const doc = readFileSync(join(repo, 'docs', 'metrics.md'), 'utf8');
  const ids = new Set([...doc.matchAll(/<a id="([a-z0-9_]+)"><\/a>/g)].map((m) => m[1]));
  assert.ok(ids.size > 40, `metrics.md parsed ${ids.size} ids`);
  const tree = renderOverview(build({ unfolded: [1, 2, 3, 4] }), el, 100, 'dock', NOW);
  const keys = [...keyed(tree).keys()].filter((k) => !k.startsWith('ledger_'));
  assert.ok(keys.length >= 10, `only ${keys.length} keyed rows`);
  for (const key of keys) assert.ok(ids.has(key), `row key ${key} is not a metric id`);
  let texts = 0;
  const visit = (node: RenderNode | undefined, inText: boolean): void => {
    if (node === undefined || typeof node === 'string' || node.type === 'engine') return;
    const n = node as Node;
    if (n.type === 'Text') {
      texts += 1;
      if (!inText) assert.equal(n.props?.wrap, 'truncate', JSON.stringify(n.props));
      for (const prop of ['color', 'backgroundColor']) {
        const c = n.props?.[prop];
        if (c !== undefined) assert.ok(String(c) in THEME_KEYS, `${prop} ${String(c)} is not a theme key`);
      }
    }
    for (const c of n.children ?? []) visit(c, inText || n.type === 'Text');
  };
  visit(tree, false);
  assert.ok(texts > 20, `only ${texts} Texts`);
  const dir = join(repo, 'plugin', 'hooks', 'views');
  for (const file of readdirSync(dir)) {
    const src = readFileSync(join(dir, file), 'utf8');
    assert.doesNotMatch(src, /['"`]#[0-9a-f]{3,8}\b|rgb\(|ansi256/i, `${file} uses a non-palette colour`);
  }
});

test('the nudge line carries its class tag and the tile levels colour the digits', () => {
  const tree = renderOverview(build(), el, 100, 'dock', NOW);
  const coloured: [string, string][] = [];
  walk(tree, (n) => {
    if (n.type === 'Text' && typeof n.props?.color === 'string') coloured.push([textOf(n.children).trim(), THEME_KEYS[n.props.color] ?? String(n.props.color)]);
  });
  // The context tile is watch-level on fixture A (396k of 1M): amber digits and name.
  assert.ok(coloured.some(([t, c]) => t.includes('◐ context') && c === 'yellow'), JSON.stringify(coloured));
  const lines = raw(build(), 100);
  const nudge = lines.find((l) => l.startsWith(' ▸ '))!;
  assert.ok(nudge.includes('NEXT · turn 14'), nudge);
  const next = lines[lines.indexOf(nudge) + 1];
  assert.ok(next.startsWith("   queue: 'run make check, fix failures' or /goal"), next);
});
