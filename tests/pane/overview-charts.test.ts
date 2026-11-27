import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { CommandRunResult, ProcessRunResult, RenderElement, RenderNode } from 'claude-code';
import { fakeElements, fakeEngine, fakeOn, paneRender, textOf, type FakeEngine, type ProcessScript } from './harness';
import { chartsRenderer, register } from '../../plugin/hooks/pane';
import { initialModel, reduce } from '../../plugin/hooks/model';
import { renderToText } from './render';
import { fixture } from './fixture';
import { drawPicture, instantiateCharts, type ChartRenderer, type ChartsMode, type Picture } from '../../plugin/hooks/charts';
import {
  PAIRS,
  SPARKLINE,
  barKey,
  chartKey,
  dims,
  layout,
  maxTurnsFor,
  overviewOf,
  pictureJobs,
  renderOverviewCharts,
  sectionWidth,
  turnAreas,
  turnNumbers,
  turnScope,
  turnsIn,
  type ChartElements,
  type Overview,
  type Pictures,
} from '../../plugin/hooks/views/overview-charts';

// The overview in the pane (mod-pane US-007) from `cctop query overview` on
// fixture B (tests/pane/fixtures/overview-b.json) and on a synthetic 300-turn
// session at the turns that fit 105 columns (overview-long.json), the
// pictures drawn by the shipped charts.wasm the way pane.tsx draws them.

const repo = join(__dirname, '..', '..', '..');
const presses = new Map<string, () => void>();
const el = fakeElements(presses) as unknown as ChartElements;

let rendererP: Promise<ChartRenderer> | null = null;
function renderer(): Promise<ChartRenderer> {
  rendererP ??= (async () => {
    const bytes = readFileSync(join(repo, 'plugin', 'hooks', 'charts.wasm'));
    const wa = (globalThis as unknown as { WebAssembly: { compile(b: Uint8Array): Promise<WebAssembly.Module> } }).WebAssembly;
    return instantiateCharts(await wa.compile(bytes));
  })();
  return rendererP;
}

/** The pictures pane.tsx would hand the view at `columns` in `mode`. */
async function picturesAt(o: Overview, columns: number, mode: ChartsMode): Promise<Map<string, Picture>> {
  const wasm = await renderer();
  const out = new Map<string, Picture>();
  for (const job of pictureJobs(o, columns)) {
    const p = drawPicture(wasm, job, mode);
    if (p !== null) out.set(job.key, p);
  }
  return out;
}

const asPictures = (m: Map<string, Picture>): Pictures => ({ get: (k) => m.get(k) });

type Opts = { mode?: ChartsMode; readouts?: Record<string, number>; picked?: [string, number][] };

async function draw(o: Overview, columns: number, opts: Opts = {}): Promise<{ tree: RenderElement; lines: string[] }> {
  const pics = await picturesAt(o, columns, opts.mode ?? 'cells');
  const picked = opts.picked ?? [];
  const tree = renderOverviewCharts(o, columns, {
    el,
    pictures: asPictures(pics),
    readouts: opts.readouts ?? {},
    actions: { readout: (chart, turn) => picked.push([chart, turn]) },
  });
  return { tree, lines: renderToText(tree, columns) };
}

const B = (): Overview => overviewOf(fixture('overview-b'))!;
const LONG = (): Overview => overviewOf(fixture('overview-long'))!;

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };

function walk(node: RenderNode | undefined, visit: (n: Node) => void): void {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return;
  const n = node as Node;
  visit(n);
  for (const c of n.children ?? []) walk(c, visit);
}

function all(tree: RenderElement, type: string): Node[] {
  const out: Node[] = [];
  walk(tree, (n) => {
    if (n.type === type) out.push(n);
  });
  return out;
}

const pictureOf = (tree: RenderElement, key: string): Node | undefined =>
  [...all(tree, 'Raster'), ...all(tree, 'Image')].find((n) => n.props?.key === key);

function fits(lines: string[], columns: number): void {
  for (const row of lines) assert.ok([...row].length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
}

/** Is `want` a subsequence of `have`? Returns the first line of `want` not found in order. */
function missing(want: string[], have: string[]): string | null {
  let i = 0;
  for (const w of want) {
    while (i < have.length && have[i] !== w) i++;
    if (i >= have.length) return w;
    i++;
  }
  return null;
}

/** Lines split into what spans the width and the two columns of a pair (` │ ` after `left` cells). */
function streams(lines: string[], columns: number): { full: string[]; left: string[]; right: string[] } {
  const l = layout(columns);
  const at = 1 + l.left;
  const out = { full: [] as string[], left: [] as string[], right: [] as string[] };
  for (const line of lines) {
    if (l.two && line.slice(at, at + 3) === ' │ ') {
      out.left.push(line.slice(0, at).replace(/\s+$/, ''));
      out.right.push(line.slice(at + 3).replace(/\s+$/, ''));
    } else out.full.push(line.replace(/\s+$/, ''));
  }
  return out;
}

// Row identity with the TUI (src/ui/overview_view.rs, snapshots overview_b_*):
// at the same width the pane draws every row the TUI draws — the charts cell
// for cell, at the same sizes, the labels, verdicts, legends and axes — in
// the same order; the pane only adds the readout rows its hover reveals into.
test('fixture B: the pane draws the TUI\'s rows at 80, 122 and 190 columns', async () => {
  for (const columns of [80, 122, 190]) {
    const snap = readFileSync(join(repo, 'src', 'ui', 'snapshots', `cctop__ui__overview_view__tests__overview_b_${columns}.snap`), 'utf8');
    const tui = snap.split('\n---\n')[1].split('\n').map((l) => l.replace(/\s+$/, ''));
    while (tui.length > 0 && tui[tui.length - 1] === '') tui.pop();
    const { lines } = await draw(B(), columns);
    fits(lines, columns);
    const want = streams(tui, columns);
    const have = streams(lines, columns);
    for (const part of ['full', 'left', 'right'] as const) {
      const gone = missing(want[part], have[part]);
      assert.equal(gone, null, `${columns} columns, ${part}: the TUI row ${JSON.stringify(gone)} is not drawn\n${JSON.stringify(have[part], null, 1)}`);
    }
    assert.ok(want.left.length > 10 === layout(columns).two, `${columns}: pairs side by side from ${PAIRS}`);
  }
});

test('the turns that fit are the TUI\'s: two cells a turn beside the heat map\'s names', () => {
  // overview_t_122 (fixture T, 56 turns) reads "last 23 turns" at 122 columns.
  assert.equal(maxTurnsFor(122), 23);
  assert.equal(maxTurnsFor(105), 18);
  assert.equal(maxTurnsFor(99), 42);
  assert.equal(maxTurnsFor(59), 22);
  for (const columns of [59, 80, 99, 105, 122, 190]) {
    const l = layout(columns);
    const narrowest = l.two ? l.right : l.inner;
    assert.ok(maxTurnsFor(columns) * 2 + 13 <= narrowest, `${columns}`);
  }
});

test('width forms: two columns at 105, stacked at 99, one row of cells at 59; nothing overflows', async () => {
  const o = B();
  const at105 = await draw(o, 105);
  const at99 = await draw(o, 99);
  const at59 = await draw(o, 59);
  for (const [columns, d] of [[105, at105], [99, at99], [59, at59]] as const) fits(d.lines, columns);
  assert.ok(at105.lines.some((l) => l.includes(' │ ')), 'a pair at 105');
  assert.ok(!at99.lines.some((l) => l.includes(' │ ')), 'stacked at 99');
  assert.ok(!at59.lines.some((l) => l.includes(' │ ')), 'stacked at 59');
  // At 105 and 99 the charts are drawn at the TUI's sizes for their section.
  for (const [columns, d] of [[105, at105], [99, at99]] as const) {
    const l = layout(columns);
    for (const c of o.charts) {
      const want = dims(c, sectionWidth(l, c.id));
      const p = pictureOf(d.tree, chartKey(c.id));
      assert.ok(p !== undefined, `${columns} ${c.id}`);
      assert.deepEqual([p.props?.columns, p.props?.rows], [want.cols, want.rows], `${columns} ${c.id}`);
    }
    assert.equal(pictureOf(d.tree, chartKey('spend'))?.props?.rows, 8);
  }
  // Below 60: every chart one row of cells (a sparkline), even where pictures could be drawn.
  const imaged = await draw(o, 59, { mode: 'image' });
  for (const d of [at59, imaged]) {
    assert.equal(all(d.tree, 'Image').length, 0, 'cells below 60');
    const rasters = all(d.tree, 'Raster');
    assert.ok(rasters.length >= 5);
    for (const r of rasters) assert.equal(r.props?.rows, 1, String(r.props?.key));
  }
  assert.ok(SPARKLINE === 60 && PAIRS === 100);
});

test('pictures where the terminal draws them: one keyed Image per chart, at its cells, with an alt; no text inside', async () => {
  const { tree, lines } = await draw(B(), 122, { mode: 'image' });
  const images = all(tree, 'Image');
  assert.deepEqual(
    images.map((i) => i.props?.key).sort(),
    [barKey('cache'), barKey('context'), ...['context', 'spend', 'cost_per_turn', 'cache', 'tool_calls'].map(chartKey)].sort(),
  );
  for (const i of images) {
    const source = i.props?.source as { png?: string };
    assert.deepEqual([...Buffer.from(source.png ?? '', 'base64').slice(0, 4)], [0x89, 0x50, 0x4e, 0x47], String(i.props?.key));
    assert.ok(typeof i.props?.alt === 'string' && (i.props.alt as string).length > 0, String(i.props?.key));
  }
  assert.equal(all(tree, 'Raster').length, 0);
  // The labels stay text beside the picture: the axis and the title are drawn as rows.
  assert.ok(lines.some((l) => l.startsWith(' Context window  142k of 1M')));
  assert.ok(lines.some((l) => /^ 1\.00M\s+▒+$/.test(l)), JSON.stringify(lines.slice(0, 14)));
});

test('the query\'s cells when the wasm is not there, drawn at their own size', async () => {
  const o = B();
  const pics = new Map<string, Picture>();
  for (const job of pictureJobs(o, 122)) {
    const p = drawPicture(null, job, 'image');
    if (p !== null) pics.set(job.key, p);
  }
  const tree = renderOverviewCharts(o, 122, { el, pictures: asPictures(pics) });
  const ctx = pictureOf(tree, chartKey('context'))!;
  assert.equal(ctx.type, 'Raster');
  const fallback = o.charts.find((c) => c.id === 'context')!.fallback!;
  assert.deepEqual([ctx.props?.columns, ctx.props?.rows, ctx.props?.cells], [fallback.cols, fallback.rows, fallback.cells]);
});

test('before the pictures are drawn: blank boxes of their size, the labels already there', () => {
  const o = B();
  const tree = renderOverviewCharts(o, 122, { el });
  assert.equal(all(tree, 'Raster').length + all(tree, 'Image').length, 0);
  const lines = renderToText(tree, 122);
  assert.ok(lines.some((l) => l.startsWith(' 1.00M')));
  assert.ok(lines.some((l) => l.startsWith(' Spend  $18.75')));
});

// Hover (spec §3): no code runs on hover, so every per-turn readout is drawn
// beforehand, hidden, and revealed through its turn's scope. The targets are
// in the flow (an absolute Box passes the pointer to its parent, per the
// d.ts): the turn-numbers row is one keyed Box per turn column.
test('hover: each turn column has an in-flow keyed Box that reveals its readout; its Button pins it', async () => {
  const o = B();
  const picked: [string, number][] = [];
  const { tree, lines } = await draw(o, 105, { picked });
  for (const id of ['cost_per_turn', 'cache', 'tool_calls']) {
    const c = o.charts.find((x) => x.id === id)!;
    const n = turnsIn(c.spec);
    const pic = pictureOf(tree, chartKey(id))!;
    const cols = pic.props?.columns as number;
    const areas = all(tree, 'Box').filter((b) => String(b.props?.key).startsWith(`hover-${id}-`));
    assert.equal(areas.length, n, `${id}: one area per turn`);
    let x = 0;
    let numbers = '';
    for (const [i, a] of areas.entries()) {
      const turn = c.spec.x.from + i;
      assert.notEqual(a.props?.position, 'absolute', `${id} turn ${turn}: in the flow`);
      assert.ok((a.props?.width as number) >= 2, `${id}: two cells a turn at least`);
      assert.deepEqual(a.props?.hover, { scope: turnScope(id, turn) });
      const button = (a.children ?? []).find((ch) => typeof ch === 'object' && ch.type === 'Button') as Node | undefined;
      assert.equal(button?.props?.key, `turn-${id}-${turn}`, `${id} turn ${turn}: a Button inside`);
      assert.equal((button?.props?.label as string).length, a.props?.width);
      numbers += button?.props?.label as string;
      x += a.props?.width as number;
    }
    assert.equal(x, cols, `${id}: the areas cover the chart's width`);
    // The row reads as the TUI's turn numbers.
    const want = turnNumbers(c.spec, cols).reduce((t, p) => t + ' '.repeat(p.at - t.length) + p.label, '');
    assert.equal(numbers.trimEnd(), want, id);
    // The readouts: hidden, revealed in the turn's scope, the readout's own text.
    const reveals = all(tree, 'Box').filter((b) => b.props?.display === 'none' && String((b.props?.hover as { scope?: string })?.scope).startsWith(`cctop-${id}-turn-`));
    assert.equal(reveals.length, c.readouts.length, id);
    for (const r of c.readouts) {
      const box = reveals.find((b) => (b.props?.hover as { scope: string }).scope === turnScope(id, r.x));
      assert.ok(box !== undefined, `${id} turn ${r.x}`);
      assert.deepEqual(box.props?.hover, { scope: turnScope(id, r.x), display: 'flex' });
      assert.equal(box.props?.key, `readout-${id}-${r.x}`);
      assert.ok(textOf(box.children).includes(r.text.slice(0, 20)), `${id} turn ${r.x}: ${textOf(box.children)}`);
    }
    presses.get(`turn-${id}-3`)?.();
  }
  assert.deepEqual(picked, [['cost_per_turn', 3], ['cache', 3], ['tool_calls', 3]]);
  // By default the latest turn's readout shows; a picked turn's replaces it.
  const cost = o.charts.find((c) => c.id === 'cost_per_turn')!;
  assert.ok(lines.some((l) => l.includes(cost.readouts.at(-1)!.text.slice(0, 30))));
  const pickedLines = (await draw(o, 105, { readouts: { cost_per_turn: 2 } })).lines;
  assert.ok(pickedLines.some((l) => l.includes('Turn 2 · $5.95. A costly turn')), JSON.stringify(pickedLines));
  assert.ok(!pickedLines.some((l) => l.includes(cost.readouts.at(-1)!.text.slice(0, 30))));
});

test('narrow columns: a turn without a whole cell shares an area with the next (spec §8)', () => {
  const spec = { ...B().charts.find((c) => c.id === 'cost_per_turn')!.spec };
  assert.equal(turnsIn(spec), 6);
  const two = turnAreas(spec, 4);
  assert.deepEqual(two.map((a) => a.turns), [[1, 2], [3, 4], [5, 6]]);
  assert.deepEqual(two.map((a) => [a.x0, a.x1]), [[0, 1], [1, 2], [2, 4]]);
  const one = turnAreas(spec, 12);
  assert.equal(one.length, 6);
  assert.deepEqual([one[0]!.x0, one.at(-1)!.x1], [0, 12]);
});

test('a 300-turn session: the per-turn charts show the last turns that fit at two cells each, labelled; context spans the whole', async () => {
  const o = LONG();
  const { tree, lines } = await draw(o, 105);
  fits(lines, 105);
  const n = maxTurnsFor(105);
  for (const id of ['cost_per_turn', 'cache', 'tool_calls']) {
    const c = o.charts.find((x) => x.id === id)!;
    assert.equal(c.label, `last ${n} turns`);
    assert.equal(turnsIn(c.spec), n);
    assert.equal(c.spec.x.from, 300 - n + 1);
    const pic = pictureOf(tree, chartKey(id))!;
    assert.ok((pic.props?.columns as number) >= 2 * n, `${id}: ${String(pic.props?.columns)} cells for ${n} turns`);
    assert.equal(all(tree, 'Box').filter((b) => String(b.props?.key).startsWith(`hover-${id}-`)).length, n);
    assert.ok(all(tree, 'Button').some((b) => b.props?.key === `turn-${id}-283`), `${id}: the first turn shown is numbered`);
  }
  assert.ok(lines.some((l) => /Cost per turn .* last 18 turns$/.test(l)), JSON.stringify(lines));
  assert.ok(lines.some((l) => /Prompt cache .* last 18 turns │ Tool calls per turn .* last 18 turns$/.test(l)), JSON.stringify(lines));
  // Context covers turn 1 to 300.
  assert.ok(lines.some((l) => /^ {7}turn 1 +300$/.test(l)), JSON.stringify(lines));
});

test('the footer says what the dollars are, wrapped rather than cut in a narrow pane', async () => {
  const { lines } = await draw(B(), 105);
  assert.equal(lines.at(-1), ' spent so far $18.75 · what this session would cost at API prices');
  const narrow = (await draw(B(), 59)).lines;
  assert.deepEqual(narrow.slice(-2), [' spent so far $18.75 · what this session would cost at', ' API prices']);
});

test('the whole Console with the overview stays under the engine\'s 2000 nodes at 190 columns', async () => {
  for (const o of [B(), LONG()]) {
    const { tree } = await draw(o, 190);
    let nodes = 0;
    walk(tree, () => {
      nodes += 1;
    });
    assert.ok(nodes < 1500, `${nodes} nodes`);
  }
});

// PRD 1–7 contributions the overview carries (US-011): each is drawn when the
// object has it and gone when it does not.
function edit(id: string, f: (c: Record<string, unknown>) => void): Overview {
  const raw = JSON.parse(JSON.stringify(fixture('overview-b'))) as { charts: Record<string, unknown>[] };
  f(raw.charts.find((c) => c.id === id)!);
  return overviewOf(raw)!;
}

test('PRD 1: located rebuilds come with the habit that avoids each, and the section is gone without them', async () => {
  const none = await draw(B(), 105);
  assert.ok(!none.lines.some((l) => l.includes('How to avoid each rebuild')));
  const o = edit('cache', (c) => {
    c.prevent = [{ turn: 4, kind: 'tools_changed', cause: 'a new MCP tool loaded', tokens: 112_000, habit: 'Connect the MCP servers you need before your first prompt.' }];
    (c.readouts as { x: number; text: string }[])[3] = { x: 4, text: 'Turn 4: rebuilt because a new MCP tool loaded (after settings change).' };
  });
  const { tree, lines } = await draw(o, 105);
  assert.ok(lines.some((l) => l.includes('How to avoid each rebuild')), JSON.stringify(lines));
  assert.ok(lines.some((l) => /^ turn 4 {3}a new MCP tool loaded · 112k/.test(l)), JSON.stringify(lines));
  assert.ok(lines.some((l) => l.startsWith(' ' + ' '.repeat(9) + 'Connect the MCP servers')), JSON.stringify(lines));
  const reveal = all(tree, 'Box').find((b) => (b.props?.hover as { scope?: string })?.scope === turnScope('cache', 4) && b.props?.display === 'none');
  assert.ok(textOf(reveal?.children).includes('after settings change'));
});

test('PRD 3: the composition bar with its age is drawn while a capture exists, and gone without one', async () => {
  const withBar = await draw(B(), 105);
  assert.ok(pictureOf(withBar.tree, barKey('context')) !== undefined);
  assert.ok(withBar.lines.some((l) => l.includes('per /context, 2m ago')));
  assert.ok(withBar.lines.some((l) => l.includes('■ System prompt 10k')));
  const o = edit('context', (c) => {
    delete c.bar;
    delete c.bar_legend;
    delete c.bar_fallback_cells;
    delete c.note;
  });
  const { tree, lines } = await draw(o, 105);
  assert.equal(pictureOf(tree, barKey('context')), undefined);
  assert.ok(!lines.some((l) => l.includes('per /context') || l.includes('System prompt')));
  assert.ok(!pictureJobs(o, 105).some((j) => j.key === barKey('context')));
});

test('PRD 2, 4, 7: the spend limit, the budget and the hook overhead reach the screen as the object words them', async () => {
  const o = edit('spend', (c) => {
    (c.legend as unknown[]).push({ role: 'crit', text: 'spend limit $50.00' }, { role: 'warn', text: 'budget $25.00 · halts in ~40 min' });
    (c.readouts as unknown[]).push({ x: 600, text: 'chain total $31.20' });
  });
  const tools = (o.charts.find((c) => c.id === 'tool_calls')!.verdict = { level: 'watch', text: 'Hooks add 820 ms a turn.', todo: 'Do: check the slow hook.', metricId: 'verdict_tool_calls' });
  const { lines } = await draw(o, 105);
  assert.ok(lines.some((l) => l.includes('■ spend limit $50.00')), JSON.stringify(lines));
  assert.ok(lines.some((l) => l.includes('budget $25.00 · halts in ~40 min')), JSON.stringify(lines));
  assert.ok(lines.some((l) => l.includes(`Watch ${tools.text}`)), JSON.stringify(lines));
  const plain = await draw(B(), 105);
  assert.ok(!plain.lines.some((l) => l.includes('spend limit') || l.includes('budget') || l.includes('Hooks add')));
});

test('PRD 2, 4, 5, 7: the markers\' legend rows and readout sentences are drawn when the object has them, and gone when it does not', async () => {
  const raw = JSON.parse(JSON.stringify(fixture('overview-b'))) as { charts: { id: string; legend: unknown[]; readouts: { x: number; text: string }[] }[] };
  const chart = (id: string) => raw.charts.find((c) => c.id === id)!;
  const readout = (id: string, x: number, text: string) => (chart(id).readouts.find((r) => r.x === x)!.text = text);
  chart('cache').legend.push({ role: 'warn', text: 'thinking dropped' });
  readout('cache', 4, 'Turn 4: the cache was reused. Thinking dropped: 3 blocks.');
  chart('cost_per_turn').legend.push({ role: 'warn', text: 'past 200k' });
  readout('cost_per_turn', 4, 'Turn 4 · $1.20. 2 retries, +3.0 s. 220k of context, past 200k.');
  chart('tool_calls').legend.push({ role: 's1', text: 'skill shell commands' });
  readout('tool_calls', 4, 'Turn 4: 3 calls, 2 skill shell.');
  const o = overviewOf(raw)!;
  const { tree, lines } = await draw(o, 105);
  const reveal = (id: string): string => textOf(all(tree, 'Box').find((b) => (b.props?.hover as { scope?: string })?.scope === turnScope(id, 4) && b.props?.display === 'none')?.children).replace(/\s+/g, ' ');
  for (const l of ['■ thinking dropped', '■ past 200k', '■ skill shell commands']) assert.ok(lines.some((x) => x.includes(l)), `${l}\n${JSON.stringify(lines)}`);
  assert.ok(reveal('cache').includes('Thinking dropped: 3 blocks.'), reveal('cache'));
  assert.ok(reveal('cost_per_turn').includes('2 retries, +3.0 s.') && reveal('cost_per_turn').includes('past 200k'));
  assert.ok(reveal('tool_calls').includes('2 skill shell'));
  const plain = await draw(B(), 105);
  for (const l of ['thinking dropped', 'past 200k', 'skill shell']) assert.ok(!plain.lines.some((x) => x.includes(l)), l);
  const ptree = plain.tree;
  const preveal = all(ptree, 'Box').filter((b) => b.props?.display === 'none').map((b) => textOf(b.children)).join('\n');
  for (const t of ['Thinking dropped', 'retries', 'past 200k', 'skill shell']) assert.ok(!preveal.includes(t), t);
});

test('every keyed text row of the overview is a metric id from docs/metrics.md', async () => {
  const doc = readFileSync(join(repo, 'docs', 'metrics.md'), 'utf8');
  const ids = new Set([...doc.matchAll(/<a id="([a-z0-9_]+)"><\/a>/g)].map((m) => m[1]));
  const { tree } = await draw(B(), 122);
  const rows = all(tree, 'Box').filter((b) => typeof b.props?.key === 'string' && (b.children ?? []).some((c) => typeof c === 'object' && c.type === 'Text'));
  const keys = rows.map((b) => String(b.props?.key)).filter((k) => !/^(hover|readout|plot|axis)-/.test(k));
  assert.deepEqual(keys.sort(), ['verdict_cache', 'verdict_context', 'verdict_cost_per_turn', 'verdict_spend', 'verdict_tool_calls']);
  for (const k of keys) assert.ok(ids.has(k), k);
});

// -- the pane end to end: pictures rendered off the render hook ----------

const SESSION = 'fake-session';
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const VERBS = ['summary', 'dashboard', 'overview', 'coach', 'tools', 'files', 'agents', 'advice', 'events'];
const HELP = `Commands:\n${VERBS.map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
const OVERVIEW_ARGV = (columns: number) => `cctop query overview --session ${SESSION} --surface pane --max-turns ${maxTurnsFor(columns)}`;

async function bootPane(env: Record<string, string>, columns = 122, view = 'overview') {
  const script: Record<string, ProcessScript> = {
    'cctop --version': ok('cctop 0.12.0\n'),
    'cctop query config': ok(JSON.stringify({ schema: 1, pane: { charts: 'auto' } })),
    'cctop query --help': ok(HELP),
    [OVERVIEW_ARGV(columns)]: ok(JSON.stringify(fixture('overview-b'))),
  };
  for (const verb of VERBS) {
    if (verb === 'overview') continue;
    const name = verb === 'dashboard' ? 'dashboard-b' : verb;
    script[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(fixture(name)));
  }
  const $ = fakeEngine({ env: { HOME: '/home/user', ...env }, process: script });
  const surface = { columns: 200, bodyColumns: columns };
  const { on, dispatch } = fakeOn($, { surface });
  register(on, {});
  await dispatch('session.start', { cwd: '/p', surface: 'terminal', isInteractive: false }, () => ({ cwd: '/p' }));
  for (let i = 0; i < 4; i++) await settle();
  await dispatch<CommandRunResult>('command.run', { command: 'cctop-pane', args: view, origin: { kind: 'composer' } });
  for (let i = 0; i < 4; i++) await settle();
  // Runs the timers due in `ms` and lets what they start settle.
  const advance = async (ms: number) => {
    $.clock.tick(ms);
    for (let i = 0; i < 6; i++) await settle();
  };
  const render = async () => dispatch<RenderElement>('ui.render', paneRender('cctop', surface.columns, { bodyColumns: surface.bodyColumns }));
  return { $, surface, advance, render };
}

const overviewQueries = ($: FakeEngine) => $.process.calls.filter((argv) => argv[2] === 'overview');

test('the pane draws only the charts whose spec changed, off the render hook, and swaps them with $.ui.blit', async () => {
  const { $, advance, render } = await bootPane({ TERM_PROGRAM: 'Apple_Terminal' }, 122, 'tools');
  await advance(1);
  assert.equal(overviewQueries($).length, 0, 'the overview is not read while its charts are not shown');
  const wasm = chartsRenderer();
  assert.ok(wasm !== null);
  assert.equal(wasm.renders, 0);
  // The Overview opens on its charts: the verb is read at once, at the turns that fit 122 columns.
  $.ui.press('overview');
  await advance(1);
  await advance(1);
  assert.deepEqual(overviewQueries($).at(-1), OVERVIEW_ARGV(122).split(' '));
  const jobs = pictureJobs(B(), 122);
  assert.equal(wasm.renders, jobs.length, 'one wasm call per picture');
  assert.equal($.ui.blits.length, 0, 'nothing mounted yet: a redraw, not a blit');
  const tree = await render();
  for (const j of jobs) assert.equal(pictureOf(tree, j.key)?.type, 'Raster', j.key);
  await render();
  assert.equal(wasm.renders, jobs.length, 'a render draws what was rendered; it calls no wasm');
  // The next tick answers the same object: nothing is drawn again.
  const queried = overviewQueries($).length;
  await advance(10_000);
  await advance(1);
  assert.ok(overviewQueries($).length > queried, 'the idle tick read the overview again');
  assert.equal(wasm.renders, jobs.length);
  assert.equal($.ui.blits.length, 0);
  // One spec moves: one wasm call, one blit of that Raster at its mounted size.
  const moved = JSON.parse(JSON.stringify(fixture('overview-b'))) as { charts: { id: string; spec: { series: { points: { y: number }[] }[] } }[] };
  moved.charts.find((c) => c.id === 'cost_per_turn')!.spec.series[0]!.points[5]!.y = 3.5;
  $.process.script[OVERVIEW_ARGV(122)] = ok(JSON.stringify(moved));
  await advance(10_000);
  await advance(1);
  assert.equal(wasm.renders, jobs.length + 1);
  assert.equal($.ui.blits.length, 1);
  const blit = $.ui.blits[0] as { requestId: string; key: string; cells?: string; columns?: number; rows?: number };
  const cost = jobs.find((j) => j.key === chartKey('cost_per_turn'))!;
  assert.deepEqual([blit.requestId, blit.key, blit.columns, blit.rows], ['cctop', cost.key, cost.cols, cost.rows]);
  assert.equal(typeof blit.cells, 'string');
  assert.ok(!$.ui.logs.some((l) => l.includes('failed')), JSON.stringify($.ui.logs));
});

test('pictures in a picture terminal: Images, the first one probed once it is mounted', async () => {
  const { $, advance, render } = await bootPane({ TERM_PROGRAM: 'ghostty' });
  await advance(1);
  await advance(1);
  const tree = await render();
  const images = all(tree, 'Image');
  assert.equal(images.length, pictureJobs(B(), 122).length);
  assert.equal($.ui.blits.length, 0, 'the probe waits for the Image to mount');
  await advance(200);
  assert.equal($.ui.blits.length, 1, 'one probe blit');
  const probe = $.ui.blits[0] as { key: string; source: { png: string } };
  const first = images[0]!;
  assert.equal(probe.key, first.props?.key, 'the first Image drawn');
  assert.equal(probe.source.png, (first.props?.source as { png: string }).png);
  await render();
  await advance(400);
  assert.equal($.ui.blits.length, 1, 'probed once');
});

test('a denied probe turns the session to cells: every picture drawn again as a Raster', async () => {
  const { $, advance, render } = await bootPane({ TERM_PROGRAM: 'ghostty', TMUX: '/tmp/tmux-1/default,1,0' });
  await advance(1);
  await advance(1);
  await render();
  Object.assign($.ui.blitAnswer, { deny: 'the Image draws its alt', fix: 'tmux-passthrough' });
  await advance(200);
  await advance(1);
  const tree = await render();
  assert.equal(all(tree, 'Image').length, 0);
  assert.equal(all(tree, 'Raster').length, pictureJobs(B(), 122).length);
});

test('a new width redraws the overview at the TUI\'s sizes for it and reads the turns that fit', async () => {
  const { $, surface, advance, render } = await bootPane({ TERM_PROGRAM: 'Apple_Terminal' });
  await advance(1);
  await advance(1);
  await render();
  $.process.script[OVERVIEW_ARGV(99)] = ok(JSON.stringify(fixture('overview-b')));
  surface.bodyColumns = 99;
  await render();
  await advance(1);
  await advance(1);
  assert.deepEqual(overviewQueries($).at(-1), OVERVIEW_ARGV(99).split(' '));
  const tree = await render();
  const l = layout(99);
  for (const c of B().charts) {
    const want = dims(c, sectionWidth(l, c.id));
    assert.deepEqual([pictureOf(tree, chartKey(c.id))?.props?.columns, pictureOf(tree, chartKey(c.id))?.props?.rows], [want.cols, want.rows], c.id);
  }
});

test('a turn number pressed in the pane shows that turn\'s readout', async () => {
  const { $, advance, render } = await bootPane({ TERM_PROGRAM: 'Apple_Terminal' });
  await advance(1);
  await advance(1);
  await render();
  $.ui.press('turn-cost_per_turn-2');
  const lines = renderToText(await render(), 122);
  assert.ok(lines.some((l) => l.includes('Turn 2 · $5.95. A costly turn')), JSON.stringify(lines));
});

test('a probe denied for a reason that is not the alt keeps pictures and tries again on a later render', async () => {
  const { $, advance, render } = await bootPane({ TERM_PROGRAM: 'ghostty' });
  await advance(1);
  await advance(1);
  await render();
  Object.assign($.ui.blitAnswer, { deny: 'nothing of this plugin is mounted there' });
  await advance(200);
  assert.equal($.ui.blits.length, 1);
  let tree = await render();
  assert.ok(all(tree, 'Image').length > 0, 'still pictures');
  delete ($.ui.blitAnswer as { deny?: string }).deny;
  await advance(200);
  assert.equal($.ui.blits.length, 2, 'probed again after the next render');
  tree = await render();
  await advance(400);
  assert.equal($.ui.blits.length, 2, 'the answer is in: no more probes');
  assert.ok(all(tree, 'Image').length > 0);
});

test('a picked readout: a second press on its turn, or a new turn, brings the latest back', () => {
  let m = initialModel();
  m = reduce(m, { type: 'overview.readout', chart: 'cost_per_turn', turn: 2 });
  m = reduce(m, { type: 'overview.readout', chart: 'cache', turn: 4 });
  assert.deepEqual(m.readouts, { cost_per_turn: 2, cache: 4 });
  m = reduce(m, { type: 'overview.readout', chart: 'cost_per_turn', turn: 2 });
  assert.deepEqual(m.readouts, { cache: 4 }, 'the same turn again clears it');
  m = reduce(m, { type: 'overview.readout', chart: 'cache', turn: 5 });
  assert.deepEqual(m.readouts, { cache: 5 });
  m = reduce(m, { type: 'turn.start', at: 1 });
  assert.deepEqual(m.readouts, {}, 'a new turn is the latest again');
});
