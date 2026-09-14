import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import type { RenderElement, RenderNode, SessionUsage } from 'claude-code';
import { fakeElements, textOf } from './harness';
import { body, frameTitles, renderToText } from './render';
import { fixture } from './fixture';
import { initialModel, reduce, type Action, type Binary, type Model } from '../../plugin/hooks/model';
import { NEEDS_BINARY, renderOverview } from '../../plugin/hooks/views/overview';

// The Overview from the fixtures: a turn started at T0 with Bash running
// since T0 + 2 s, drawn 48 s in. Every figure the tests expect comes from
// tests/pane/fixtures/{summary,usage,advice}.json.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const NOW = T0 + 48_000;
const el = fakeElements(new Map());
const repo = join(__dirname, '..', '..', '..');

type Options = { usage?: boolean; summary?: boolean; binary?: Binary; advice?: unknown };

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
  if (opts.advice !== undefined) actions.push({ type: 'query', verb: 'advice', data: opts.advice });
  return actions.reduce(reduce, initialModel());
}

// The rendered lines; `rows` gives the frames' content (borders and `│`
// stripped, side-by-side frames joined with a space), `raw` the screen.
function raw(model: Model, columns: number, placement: 'dock' | 'inline' = 'dock'): string[] {
  return renderToText(renderOverview(model, el, columns, placement, NOW), columns);
}
function rows(model: Model, columns: number, placement: 'dock' | 'inline' = 'dock'): string[] {
  return body(raw(model, columns, placement));
}

// The theme keys the views draw with (views/frame.tsx THEME) and the TUI roles they stand for.
const THEME_KEYS: Record<string, string> = { success: 'green', warning: 'yellow', error: 'red', suggestion: 'cyan' };

function has(lines: string[], text: string | RegExp): void {
  const hit = lines.some((r) => (typeof text === 'string' ? r.includes(text) : text.test(r)));
  assert.ok(hit, `no row has ${String(text)}: ${JSON.stringify(lines)}`);
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };

function walk(node: RenderNode | undefined, visit: (n: Node) => void): void {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return;
  const n = node as Node;
  visit(n);
  for (const c of n.children ?? []) walk(c, visit);
}

// The text of every keyed row Box, by key (the row that shows the metric).
function keyed(tree: RenderElement): Map<string, string> {
  const out = new Map<string, string>();
  walk(tree, (n) => {
    if (n.type === 'Box' && typeof n.props?.key === 'string') out.set(n.props.key, textOf(n.children));
  });
  return out;
}

const EXPECTED = [
  '396k / 1.0M (40 %)',
  '+50k/turn',
  '≈9 turns',
  '33.0M',
  '597k',
  '286',
  '67k',
  '31k',
  '98 %',
  '1h',
  '$9.90',
  '≈$26.0/h',
  '42 %',
  '17 %',
  '2h 29m',
  'Bash 0:46',
  '≈0:02 / 0:46',
];

for (const columns of [50, 60, 80]) {
  test(`overview at ${columns} columns shows the fixture values within the width`, () => {
    const screen = raw(build(), columns);
    for (const row of screen) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
    // Every frame row is exactly as wide as its frame: the right border lands.
    for (const row of screen) if (row.startsWith('│')) assert.ok(row.endsWith('│'), `open frame row: ${JSON.stringify(row)}`);
    const lines = rows(build(), columns);
    for (const text of EXPECTED) has(lines, text);
    // The header frame: the status pill, the turn and its elapsed; the model in the title.
    has(lines, /^● BUSY  turn 1  0:48$/);
    has(lines, /^auto · medium · \$9\.90\s+bin shim hooks 2\.1\.270$/);
    const titles = frameTitles(screen);
    assert.equal(titles[0], 'cctop ─ claude-sonnet-5');
    // The blocks carry the TUI's panel digits, as the guide numbers them.
    // In a 30-column frame (two columns at 60) the Limits title keeps its
    // 5 h figure and drops the 7 d one rather than clipping it.
    const limits = columns === 60 ? '3 Limits ─ 5h 42 %' : '3 Limits ─ 5h 42 % · 7d 17 %';
    for (const title of ['1 Context ─ 40 %', '2 Tokens & Cost ─ 33.6M', limits, '4 Turn ─ 0:48']) {
      assert.ok(titles.includes(title), `no frame ${title}: ${JSON.stringify(titles)}`);
    }
  });

  test(`overview at ${columns} columns: two-column rows only from 60`, () => {
    const screen = raw(build(), columns);
    const paired = screen.some((r) => r.includes('╭1 Context') && r.includes('╭2 Tokens & Cost'));
    assert.equal(paired, columns >= 60, JSON.stringify(screen));
    const order = ['1 Context', '2 Tokens & Cost', '3 Limits', '4 Turn'].map((t) => screen.findIndex((r) => r.includes(`╭${t} `)));
    assert.deepEqual([...order].sort((a, b) => a - b), order, 'blocks in order');
    if (columns >= 60) {
      // Paired frames close on the same line.
      const closes = screen.filter((r) => /^╰.*╯╰.*╯$/.test(r));
      assert.equal(closes.length, 2, JSON.stringify(screen));
    }
  });
}

test('values are right-aligned in one column per block, gauges between label and value', () => {
  const screen = raw(build(), 50);
  const start = screen.findIndex((r) => r.startsWith('╭2 Tokens & Cost'));
  const end = screen.findIndex((r, i) => i > start && r.startsWith('╰'));
  const tokens = body(screen.slice(start + 1, end));
  const ends = new Set(tokens.map((r) => r.length));
  assert.equal(ends.size, 1, `token rows end at different columns: ${JSON.stringify(tokens)}`);
  assert.match(tokens[0], /^cache read\s+▇+\s+33\.0M$/);
  assert.match(tokens[1], /^cache write\s+▁+\s+597k$/);
  // The context gauge takes its own row above the bare figure, as the TUI's.
  const context = rows(build(), 50);
  const gaugeRow = context.findIndex((r) => /^▇+▁+$/.test(r));
  assert.ok(gaugeRow >= 0, JSON.stringify(context));
  assert.match(context[gaugeRow + 1], /^396k \/ 1\.0M \(40 %\)$/);
  // Limits carry their gauge inline.
  has(context, /^5 h\s+▇+▁+\s+42 %$/);
});

test('≈ marks exactly the values the fixture calls approx', () => {
  const summary = fixture<Record<string, Record<string, { approx: boolean }>>>('summary');
  const byKey = keyed(renderOverview(build({ usage: false }), el, 80, 'dock', NOW));
  const checks: [string, boolean][] = [
    ['context_size', summary.context.size.approx],
    ['context_velocity', summary.context.velocity.approx],
    ['turns_until_compaction', summary.context.turns_until_compaction.approx],
    ['cache_read', summary.tokens.cache_read.approx],
    ['cache_hit_ratio', summary.tokens.cache_hit_ratio.approx],
    ['cost', (summary.cost as unknown as { approx: boolean }).approx],
    ['burn_rate', (summary.burn_rate as unknown as { approx: boolean }).approx],
    ['queued_prompts', (summary.queued_prompts as unknown as { approx: boolean }).approx],
  ];
  for (const [key, approx] of checks) {
    const text = byKey.get(key);
    assert.ok(text !== undefined, `no row keyed ${key}`);
    assert.equal(text.includes('≈'), approx, `${key}: ${JSON.stringify(text)}`);
  }
  assert.ok(byKey.get('context_size')?.includes('≈396k / 1.0M (40 %)'), byKey.get('context_size'));
  // The engine's own context figure is exact: no marker once usage is present.
  assert.ok(!keyed(renderOverview(build(), el, 80, 'dock', NOW)).get('context_size')?.includes('≈'));
});

test('every metric row is keyed by an id from docs/metrics.md', () => {
  const doc = readFileSync(join(repo, 'docs', 'metrics.md'), 'utf8');
  const ids = new Set([...doc.matchAll(/<a id="([a-z0-9_]+)"><\/a>/g)].map((m) => m[1]));
  assert.ok(ids.size > 40, `metrics.md parsed ${ids.size} ids`);
  const keys = [...keyed(renderOverview(build({ advice: [{ severity: 'high', headline: 'x' }] }), el, 80, 'dock', NOW)).keys()];
  assert.ok(keys.length >= 20, `only ${keys.length} keyed rows`);
  for (const key of keys) assert.ok(ids.has(key), `row key ${key} is not a metric id`);
});

test('every row Text truncates and colours are theme keys only', () => {
  const tree = renderOverview(build({ advice: [{ severity: 'high', headline: 'x' }] }), el, 80, 'dock', NOW);
  let texts = 0;
  const visit = (node: RenderNode | undefined, inText: boolean): void => {
    if (node === undefined || typeof node === 'string' || node.type === 'engine') return;
    const n = node as Node;
    if (n.type === 'Text') {
      texts += 1;
      // A row Text truncates; the segments inside it inherit that.
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

test('the header dims the badges that are off and colours the ones on', () => {
  const badges = new Map<string, { dim: boolean; color: unknown }>();
  walk(renderOverview(build(), el, 80, 'dock', NOW), (n) => {
    if (n.type !== 'Text') return;
    const text = textOf(n.children).trim();
    if (text === 'bin' || text === 'shim' || text.startsWith('hooks ')) badges.set(text.split(' ')[0], { dim: n.props?.dimColor === true, color: n.props?.color });
  });
  // The fixture summary has no shim (limits missing) and no hooks.
  assert.deepEqual(
    [...badges.entries()],
    [
      ['bin', { dim: false, color: 'success' }],
      ['shim', { dim: true, color: undefined }],
      ['hooks', { dim: true, color: undefined }],
    ],
  );
});

test('a high-severity Advisor headline is the last row', () => {
  const advice = fixture<Record<string, unknown>[]>('advice');
  const none = rows(build({ advice }), 80);
  assert.ok(!none.some((r) => r.includes(String(advice[0].headline))), 'no row without a severity');
  const lines = rows(build({ advice: [{ ...advice[0], severity: 'high' }] }), 80);
  const last = lines.filter((r) => r !== '').at(-1)!;
  assert.ok(last.includes('`Bash make check` failed 3× with the same input'), last);
  assert.ok(!rows(build({ advice: [{ ...advice[0], severity: 'medium' }] }), 80).includes(last));
  // In its own frame, the last one on the screen.
  const titles = frameTitles(raw(build({ advice: [{ ...advice[0], severity: 'high' }] }), 80));
  assert.equal(titles.at(-1), '9 Advisor');
});

test('a missing binary draws one line per binary-backed section and keeps the engine rows', () => {
  for (const columns of [50, 80]) {
    const lines = rows(build({ binary: 'missing', summary: false }), columns);
    // Velocity, the token breakdown and the Advisor: one line each (two share a row from 60 columns).
    assert.equal(lines.join('\n').split(NEEDS_BINARY).length - 1, 3, JSON.stringify(lines));
    for (const text of ['396k / 1.0M (40 %)', '$9.90', '42 %', '17 %', '2h 29m', 'Bash 0:46']) has(lines, text);
    for (const gone of ['cache read', 'velocity', 'burn rate']) assert.ok(!lines.some((r) => r.includes(gone)), gone);
    has(lines, /bin shim hooks 2\.1\.270$/);
    for (const row of raw(build({ binary: 'missing', summary: false }), columns)) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
  }
});

test('without any source the rows draw — and the header turn 0', () => {
  const lines = rows(initialModel(), 50);
  has(lines, /^○ IDLE  turn 0  —$/);
  // Effort and cost unknown, the badges still drawn; the bare context figure.
  has(lines, /^— · —\s+bin shim hooks 2\.1\.270$/);
  has(lines, /^—$/);
  has(lines, /^cache read\s+—$/);
  has(lines, /^5 h\s+—$/);
  has(lines, /^waiting on\s+—$/);
  assert.ok(!lines.some((r) => r.includes(NEEDS_BINARY)), 'unknown is not missing');
  assert.deepEqual(frameTitles(raw(initialModel(), 50)), ['cctop ─ —', '1 Context', '2 Tokens & Cost', '3 Limits', '4 Turn']);
});

test('inline placement draws the header, Context and Limits only, without frames', () => {
  const lines = raw(build(), 80, 'inline');
  has(lines, /^● BUSY · turn 1 · 0:48 · claude-sonnet-5 · medium\s+bin shim hooks 2\.1\.270$/);
  has(lines, /^Context$/);
  has(lines, /^Limits$/);
  assert.ok(!lines.some((r) => /[╭╰│]/.test(r)), 'no frames in the inline form above the prompt');
  assert.ok(!lines.some((r) => r.includes('Tokens & Cost') || r.includes('cache read') || r.includes('waiting on')), JSON.stringify(lines));
});
