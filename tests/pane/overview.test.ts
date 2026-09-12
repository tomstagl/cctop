import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import type { RenderElement, RenderNode, SessionUsage } from 'claude-code';
import { fakeElements, textOf } from './harness';
import { renderToText } from './render';
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

function rows(model: Model, columns: number, placement: 'dock' | 'inline' = 'dock'): string[] {
  return renderToText(renderOverview(model, el, columns, placement, NOW), columns);
}

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

// The text of every keyed row Box, by key.
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
    const lines = rows(build(), columns);
    for (const row of lines) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
    for (const text of EXPECTED) has(lines, text);
    has(lines, /^● busy · turn 1 · 0:48 · claude-so/);
    // The header truncates before the badges: model and effort fit from 80.
    if (columns >= 80) has(lines, /· claude-sonnet-5 · medium/);
    has(lines, /bin shim hooks$/);
    for (const title of ['Context', 'Tokens & Cost', 'Limits', 'Turn']) has(lines, new RegExp(`(^|\\s)${title.replace('&', '&')}(\\s|$)`));
  });

  test(`overview at ${columns} columns: two-column rows only from 60`, () => {
    const lines = rows(build(), columns);
    const paired = lines.some((r) => r.includes('Context') && r.includes('Tokens & Cost'));
    assert.equal(paired, columns >= 60, JSON.stringify(lines));
    if (columns < 60) {
      const order = ['Context', 'Tokens & Cost', 'Limits', 'Turn'].map((t) => lines.findIndex((r) => r === t));
      assert.deepEqual([...order].sort((a, b) => a - b), order, 'blocks stack in order');
    }
  });
}

test('values are right-aligned in one column per block', () => {
  const lines = rows(build(), 50);
  const tokens = lines.slice(lines.indexOf('Tokens & Cost') + 1, lines.indexOf('Limits'));
  const ends = new Set(tokens.map((r) => r.length));
  assert.equal(ends.size, 1, `token rows end at different columns: ${JSON.stringify(tokens)}`);
  assert.match(tokens[0], /^cache read\s+33\.0M$/);
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

test('every Text truncates and colours are palette names only', () => {
  const tree = renderOverview(build({ advice: [{ severity: 'high', headline: 'x' }] }), el, 80, 'dock', NOW);
  let texts = 0;
  walk(tree, (n) => {
    if (n.type !== 'Text') return;
    texts += 1;
    assert.equal(n.props?.wrap, 'truncate', JSON.stringify(n.props));
    for (const prop of ['color', 'backgroundColor']) {
      const c = n.props?.[prop];
      if (c !== undefined) assert.match(String(c), /^[a-z]+$/i, `${prop} ${String(c)} is not a palette name`);
    }
  });
  assert.ok(texts > 20, `only ${texts} Texts`);
  const dir = join(repo, 'plugin', 'hooks', 'views');
  for (const file of readdirSync(dir)) {
    const src = readFileSync(join(dir, file), 'utf8');
    assert.doesNotMatch(src, /['"`]#[0-9a-f]{3,8}\b|rgb\(|ansi256/i, `${file} uses a non-palette colour`);
  }
});

test('the header dims the badges that are off', () => {
  const on = new Map<string, boolean>();
  walk(renderOverview(build(), el, 80, 'dock', NOW), (n) => {
    if (n.type === 'Text' && typeof n.props?.key === 'string') on.set(n.props.key, n.props.dimColor !== true);
  });
  // The fixture summary has no shim (limits missing) and no hooks.
  assert.deepEqual([...on.entries()], [['bin', true], ['shim', false], ['hooks', false]]);
});

test('a high-severity Advisor headline is the last row', () => {
  const advice = fixture<Record<string, unknown>[]>('advice');
  const none = rows(build({ advice }), 80);
  assert.ok(!none.some((r) => r.includes(String(advice[0].headline))), 'no row without a severity');
  const lines = rows(build({ advice: [{ ...advice[0], severity: 'high' }] }), 80);
  const last = lines.filter((r) => r !== '').at(-1)!;
  assert.ok(last.includes('`Bash make check` failed 3× with the same input'), last);
  assert.ok(!rows(build({ advice: [{ ...advice[0], severity: 'medium' }] }), 80).includes(last));
});

test('a missing binary draws one line per binary-backed section and keeps the engine rows', () => {
  for (const columns of [50, 80]) {
    const lines = rows(build({ binary: 'missing', summary: false }), columns);
    // Velocity, the token breakdown and the Advisor: one line each (two share a row from 60 columns).
    assert.equal(lines.join('\n').split(NEEDS_BINARY).length - 1, 3, JSON.stringify(lines));
    for (const text of ['396k / 1.0M (40 %)', '$9.90', '42 %', '17 %', '2h 29m', 'Bash 0:46']) has(lines, text);
    for (const gone of ['cache read', 'velocity', 'burn rate']) assert.ok(!lines.some((r) => r.includes(gone)), gone);
    has(lines, /bin shim hooks$/);
    for (const row of lines) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
  }
});

test('without any source the rows draw — and the header turn 0', () => {
  const lines = renderToText(renderOverview(initialModel(), el, 50, 'dock', NOW), 50);
  has(lines, /^○ idle · turn 0 · — · — · —/);
  has(lines, /^context\s+—$/);
  has(lines, /^cache read\s+—$/);
  has(lines, /^5 h\s+—$/);
  has(lines, /^waiting on\s+—$/);
  assert.ok(!lines.some((r) => r.includes(NEEDS_BINARY)), 'unknown is not missing');
});

test('inline placement draws the header, Context and Limits only', () => {
  const lines = rows(build(), 80, 'inline');
  has(lines, /^● busy/);
  has(lines, /(^|\s)Context(\s|$)/);
  has(lines, /(^|\s)Limits(\s|$)/);
  assert.ok(!lines.some((r) => r.includes('Tokens & Cost') || r.includes('cache read') || r.includes('waiting on')), JSON.stringify(lines));
});
