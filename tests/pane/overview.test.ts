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
  ACT_FULL,
  ACT_TAIL,
  MID_CELL,
  NEEDS_BINARY,
  SCHEMA,
  THREE_CELLS,
  cellForm,
  dashboardOf,
  renderOverview,
  schemaMismatch,
  schemaOf,
  wrap,
  type OverviewActions,
} from '../../plugin/hooks/views/overview';

// Console (PRD dashboard-v2 §4) from the fixtures: `cctop query dashboard`
// (schema 2) on fixture A (tests/pane/fixtures/dashboard.json) and fixture B
// (dashboard-b.json), drawn at the widths the dock actually gives — 54, 67,
// 85 — and the standalone 122; a turn started at T0 with Bash running since
// T0 + 2 s, drawn 48 s in, for the engine-side rows.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const NOW = T0 + 48_000;
const presses = new Map<string, () => void>();
const el = fakeElements(presses);
const repo = join(__dirname, '..', '..', '..');

type Options = { usage?: boolean; summary?: boolean; dashboard?: boolean | unknown; binary?: Binary; body?: string };

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
  if (opts.body !== undefined) actions.push({ type: 'overview.body', id: opts.body });
  return actions.reduce(reduce, initialModel());
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

test('the dashboard fixture parses into six cells, an act line and eight bodies (schema 2)', () => {
  const raw = fixture('dashboard') as Record<string, unknown>;
  assert.equal(schemaOf(raw), SCHEMA);
  const d = dashboardOf(raw)!;
  assert.equal(d.cells.length, 6);
  assert.deepEqual(
    d.cells.map((c) => c.key),
    ['1', '2', '3', '4', '5', '6'],
  );
  assert.deepEqual(
    d.cells.map((c) => c.opens),
    ['context', 'limits', 'cache', 'cost', 'work', 'tools'],
  );
  for (const c of d.cells) {
    assert.ok(c.short.length > 0, c.id);
    assert.ok(textOf(c.short.map((s) => s.text)).length <= textOf(c.label.map((s) => s.text)).length, c.id);
  }
  assert.equal(d.act.key, 'a');
  assert.equal(d.act.opens, 'advisor');
  assert.equal(d.bodies.length, 8);
  assert.deepEqual(
    d.bodies.map((b) => b.key),
    ['1', '2', '3', '4', '5', '6', 'a', '0'],
  );
  const ctx = d.bodies.find((b) => b.id === 'context')!;
  assert.deepEqual(
    ctx.slices.map((s) => [s.label, s.step]),
    [
      ['prefix', 0],
      ['harness', 0],
      ['thinking', 1],
      ['inputs', 2],
      ['results', 2],
    ],
  );
  // A step is carried by weight, never a colour: s0 dim, s2 bold.
  const barRow = ctx.rows.find((r) => r.some((s) => s.text.startsWith('▇')))!;
  const fill = barRow.find((s) => s.text.startsWith('▇'))!;
  assert.equal(fill.color, undefined);
  assert.ok(fill.dim === true || fill.bold === true || (fill.dim === undefined && fill.bold === undefined));
  assert.equal(d.phase.word, 'IDLE');
  assert.ok(d.headerLine.startsWith('cctop  claude-sonnet-5 · turn 14'), d.headerLine);
});

test('another schema draws one line naming the cctop this pane needs, never an indefinite wait (FR-16)', () => {
  const old = { ...(fixture('dashboard') as Record<string, unknown>), schema: 1, tiles: [], rows: [] };
  assert.equal(dashboardOf(old), null);
  assert.equal(schemaOf(old), 1);
  const lines = raw(build({ dashboard: old }), 85);
  has(lines, /cctop 0\.6\.0 or newer needed: the binary sends dashboard schema 1, this pane draws 2/);
  assert.ok(!lines.some((l) => l.includes('waiting for cctop')), JSON.stringify(lines));
  assert.ok(schemaMismatch(3).startsWith('cctop newer than this pane needed'));
  // No object at all: the wait, or the missing binary.
  has(raw(build({ dashboard: false }), 85), 'waiting for cctop query dashboard…');
  has(raw(build({ dashboard: false, binary: 'missing' }), 85), NEEDS_BINARY);
});

test('the width ladder: three cells per row at 80, two below; wide, mid and short forms; every row fits', () => {
  const d = dashboardOf(fixture('dashboard'))!;
  const model = build();
  for (const columns of [122, 85, 67, 54, 35]) {
    const lines = raw(model, columns);
    fits(lines, columns);
    const perRow = columns >= THREE_CELLS ? 3 : 2;
    const cw = Math.floor((columns - 2) / perRow);
    const form = cellForm(columns, cw);
    // Row 2 starts with cell 1 in the form the width gives.
    const cell1 = textOf(form(d.cells[0]).map((s) => s.text));
    assert.ok(lines[1].startsWith(` 1: ${[...cell1].slice(0, Math.max(1, cw - 4)).join('')}`), `${columns}: ${lines[1]}`);
    // The rows the cells take: two at three per row, three at two.
    const cellRows = lines.filter((l) => /^ [1-6]: /.test(l)).length;
    assert.equal(cellRows, perRow === 3 ? 2 : 3, `${columns}: ${cellRows} cell rows`);
    if (columns >= THREE_CELLS) assert.equal(form(d.cells[0]), d.cells[0].label);
    else if (cw >= MID_CELL) assert.equal(form(d.cells[0]), d.cells[0].mid);
    else assert.equal(form(d.cells[0]), d.cells[0].short);
    // The rule line names the open body, events by default, with `0: home`.
    const rule = lines.find((l) => l.startsWith('─── events '))!;
    assert.ok(rule !== undefined && rule.includes('0: home'), `${columns}: ${rule}`);
    assert.ok([...rule].length <= columns);
  }
});

test('cells are keyed Boxes of plain Buttons sharing one scope and one press; the open cell is Text; a press opens the body', () => {
  const opened: string[] = [];
  const actions: OverviewActions = { open: (id) => opened.push(id) };
  // With tools open: five cell Buttons (the open cell is Text), `a` and `0`.
  const tree = renderOverview(build({ body: 'tools' }), el, 85, 'dock', NOW, { el, actions });
  const bs = buttons(tree);
  const hotkeys = bs.map((b) => b.props?.hotkey).filter((h): h is string => typeof h === 'string');
  assert.deepEqual(hotkeys.sort(), ['0', '1', '2', '3', '4', '5', 'a']);
  // Home open: every cell is a Button — contiguous 1–6, one per cell (FR-15) — and `0` is Text.
  const home = renderOverview(build(), el, 85, 'dock', NOW, { el, actions });
  const homeKeys = buttons(home).map((b) => b.props?.hotkey).filter((h): h is string => typeof h === 'string');
  assert.deepEqual(homeKeys.sort(), ['1', '2', '3', '4', '5', '6', 'a']);
  for (const b of bs) assert.equal(b.props?.plain, true, JSON.stringify(b.props));
  // A cell's Buttons — the hotkeyed one, the rest of the label, the padding — share the scope.
  const scoped = new Map<string, Node[]>();
  walk(tree, (n) => {
    if (n.type === 'Box' && typeof (n.props?.hover as { scope?: string } | undefined)?.scope === 'string') {
      const scope = (n.props!.hover as { scope: string }).scope;
      scoped.set(scope, buttons(n as unknown as RenderElement));
    }
  });
  assert.deepEqual(
    [...scoped.keys()].sort(),
    ['cctop-cell-advisor', 'cctop-cell-cache', 'cctop-cell-context', 'cctop-cell-cost', 'cctop-cell-limits', 'cctop-cell-work'],
  );
  for (const [scope, group] of scoped) {
    assert.ok(group.length >= 2, `${scope} has ${group.length} Buttons`);
    // The padding presses too: the last Button's label is spaces.
    if (scope !== 'cctop-cell-advisor') assert.match(String(group[group.length - 1].props?.label), /^ +$/, scope);
  }
  // No cctop-drawn digit chrome (FR-14): the only Text that starts with a
  // digit and a colon is the open cell's, the one state that is no target.
  walk(tree, (n) => {
    if (n.type === 'Text') assert.doesNotMatch(textOf(n.children), /^[1-5]: /, textOf(n.children));
  });
  // A press on any part of the cost cell opens cost (the padding here); on
  // the act line, advisor; on home, events. The harness keeps `onPress` by key.
  const cost = scoped.get('cctop-cell-cost')!;
  presses.get(String(cost[cost.length - 1].props!.key))!();
  presses.get(String(scoped.get('cctop-cell-advisor')![0].props!.key))!();
  presses.get('cell-home')!();
  assert.deepEqual(opened, ['cost', 'advisor', 'events']);
  // The open cell is Text in green, bold — not a target — and the rule names its body.
  assert.ok(keyed(tree).get('cell_tools')?.startsWith('6: '), keyed(tree).get('cell_tools'));
  has(renderToText(tree, 85), /^─── tools /);
  assert.ok(!buttons(home).some((b) => b.props?.hotkey === '0'));
});

test('the act line wraps rather than cuts, carries `a: advisor` at 66 columns, the short copy below 72, and a wait paints it red', () => {
  const lines85 = raw(build(), 85);
  const act = lines85.find((l) => l.startsWith(' ▸ '))!;
  assert.ok(act.endsWith('a: advisor'), act);
  assert.ok(act.includes('NEXT · turn 14') || lines85[lines85.indexOf(act) + 1].includes('NEXT · turn 14'), act);
  const lines54 = raw(build(), 54);
  const short = lines54.find((l) => l.startsWith(' ▸ '))!;
  assert.ok(!short.includes('a: advisor'), short);
  assert.ok(!short.includes(' — '), `short copy has the headline only: ${short}`);
  assert.ok(ACT_TAIL < ACT_FULL);
  // Wrapped at a space, indented, never mid-word; at most two rows.
  const rows = wrap([{ text: 'alpha beta gamma delta epsilon' }], 12, 3);
  assert.deepEqual(
    rows.map((r) => textOf(r.map((s) => s.text))),
    ['alpha beta ', '   gamma ', '   delta ', '   epsilon'],
  );
  const blocked = { ...(fixture('dashboard') as Record<string, unknown>), act: { key: 'a', opens: 'advisor', line: [{ text: '◆ ', tone: 'crit' }, { text: 'WAITING 1:56', tone: 'crit' }, { text: ' — ', tone: 'dim' }, { text: 'permission prompt open', tone: 'fg' }], short: [{ text: '◆ ', tone: 'crit' }, { text: 'WAITING 1:56', tone: 'crit' }], tag: '', blocked: true, acting: false } };
  const tree = renderOverview(build({ dashboard: blocked }), el, 85, 'dock', NOW);
  const reds: string[] = [];
  walk(tree, (n) => {
    if (n.type === 'Text' && n.props?.color === 'error') reds.push(textOf(n.children));
  });
  assert.ok(reds.some((t) => t.includes('WAITING 1:56')), JSON.stringify(reds));
  assert.equal(dashboardOf(blocked)!.act.blocked, true);
});

test('the inline form is the strip: the status line, the engine usage line, the coach line, the act line', () => {
  const lines = raw(build(), 72, 'inline');
  assert.equal(lines.length, 4, JSON.stringify(lines));
  assert.match(lines[0], /^● BUSY · turn 1 · 0:48 · claude-sonnet-5/);
  assert.ok(lines[0].includes('bin shim'), lines[0]);
  assert.match(lines[1], /^ context 396k \/ 1\.0M \(40 %\) · 5h 42 %, resets in/);
  assert.match(lines[2], /^ [○◐●]/);
  assert.match(lines[3], /^ ▸ /);
  fits(lines, 72);
  // Without the engine's usage read: the status line alone before the object.
  assert.equal(raw(build({ usage: false, dashboard: false }), 72, 'inline').length, 1);
});

test('every keyed metric row is a metric id from docs/metrics.md and colours are theme keys only', () => {
  const doc = readFileSync(join(repo, 'docs', 'metrics.md'), 'utf8');
  const ids = new Set([...doc.matchAll(/<a id="([a-z0-9_]+)"><\/a>/g)].map((m) => m[1]));
  assert.ok(ids.size > 40, `metrics.md parsed ${ids.size} ids`);
  const tree = renderOverview(build(), el, 85, 'dock', NOW);
  const keys = [...keyed(tree).keys()].filter((k) => !/^(cell|cells|rule|body|act)_/.test(k));
  assert.ok(keys.length >= 2, `only ${keys.length} keyed rows: ${keys.join(', ')}`);
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

// The row-identity harness (US-109): fixture B's Console as the TUI renders
// it (the insta snapshots at 54 / 67 / 85 columns, src/snapshots/) and as the
// pane draws the same object (dashboard-b.json), row for row. The TUI's last
// row is its footer, which the pane does not draw.
test('fixture B renders row-identical on both surfaces at 54, 67 and 85 columns', () => {
  const model = build({ dashboard: fixture('dashboard-b') });
  for (const columns of [54, 67, 85]) {
    const snap = readFileSync(join(repo, 'src', 'snapshots', `cctop__theme__fixture_b_snapshots__fixture_b_dashboard_${columns}x24.snap`), 'utf8');
    const tui = snap
      .split('\n---\n')[1]
      .split('\n')
      .map((l) => l.replace(/\s+$/, ''));
    while (tui.length > 0 && tui[tui.length - 1] === '') tui.pop();
    tui.pop(); // the footer
    const pane = raw(model, columns, 'dock', { open: () => undefined }).map((l) => l.replace(/\s+$/, ''));
    assert.ok(tui.length >= 10, `${columns}: ${tui.length} TUI rows`);
    for (let i = 0; i < tui.length; i++) {
      assert.equal(pane[i], tui[i], `${columns} columns, row ${i + 1}\n  tui:  ${JSON.stringify(tui[i])}\n  pane: ${JSON.stringify(pane[i])}`);
    }
  }
});
