import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { RenderElement, RenderNode } from 'claude-code';
import { fakeElements, textOf } from './harness';
import { renderToText } from './render';
import { fixture } from './fixture';
import { initialModel, reduce, type Action, type Binary, type Model, type QueryVerb, type View } from '../../plugin/hooks/model';
import { NEEDS_BINARY } from '../../plugin/hooks/views/overview';
import { renderView } from '../../plugin/hooks/views/index';
import { renderTools } from '../../plugin/hooks/views/tools';
import { renderAgents } from '../../plugin/hooks/views/agents';
import { renderFiles } from '../../plugin/hooks/views/files';
import { clock, EVENT_ROWS, renderEvents } from '../../plugin/hooks/views/events';
import { renderAdvisor } from '../../plugin/hooks/views/advisor';
import { MAX_ROWS, wrapWords } from '../../plugin/hooks/views/table';

// The detail views from the fixtures: a turn started at T0 with Bash running
// since T0 + 2 s, drawn 48 s in. Every figure the tests expect comes from
// tests/pane/fixtures/{tools,agents,files,events,advice}.json.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const NOW = T0 + 48_000;
const el = fakeElements(new Map());
const repo = join(__dirname, '..', '..', '..');
const DETAIL: View[] = ['tools', 'agents', 'files', 'events', 'advisor'];

type Options = { binary?: Binary; view?: View; running?: string | null; query?: Partial<Record<QueryVerb, unknown>> };

function build(opts: Options = {}): Model {
  const actions: Action[] = [
    { type: 'session.start', at: T0 - 1000 },
    { type: 'binary', binary: opts.binary ?? 'present' },
    { type: 'turn.start', at: T0 },
  ];
  const running = opts.running === undefined ? 'Bash' : opts.running;
  if (running !== null) actions.push({ type: 'tool.start', name: running, at: T0 + 2000 });
  const query = opts.query ?? {};
  for (const verb of ['tools', 'agents', 'files', 'events', 'advice'] as const) {
    if (!(verb in query)) actions.push({ type: 'query', verb, data: fixture(verb) });
  }
  for (const [verb, data] of Object.entries(query)) actions.push({ type: 'query', verb: verb as QueryVerb, data });
  const model = actions.reduce(reduce, initialModel());
  return { ...model, view: opts.view ?? 'overview' };
}

function draw(view: View, model: Model, columns: number): RenderElement {
  return renderView({ ...model, view }, el, columns, 'dock', NOW);
}

function rows(view: View, model: Model, columns: number): string[] {
  return renderToText(draw(view, model, columns), columns);
}

function has(lines: string[], text: string | RegExp): void {
  const hit = lines.some((r) => (typeof text === 'string' ? r.includes(text) : text.test(r)));
  assert.ok(hit, `no row has ${String(text)}: ${JSON.stringify(lines)}`);
}

function fits(lines: string[], columns: number): void {
  for (const row of lines) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };

function walk(node: RenderNode | undefined, visit: (n: Node) => void): void {
  if (node === undefined || node === null || typeof node !== 'object' || node.type === 'engine') return;
  const n = node as Node;
  visit(n);
  for (const c of n.children ?? []) walk(c, visit);
}

// Every Text with a colour, as `[text, color]`.
function coloured(tree: RenderElement): [string, string][] {
  const out: [string, string][] = [];
  walk(tree, (n) => {
    if (n.type === 'Text' && typeof n.props?.color === 'string') out.push([textOf(n.children), n.props.color]);
  });
  return out;
}

function colorOf(tree: RenderElement, text: string): string | undefined {
  return coloured(tree).find(([t]) => t === text)?.[1];
}

for (const columns of [50, 80]) {
  test(`tools at ${columns} columns: the fixture table, the running tool and the top consumers`, () => {
    const lines = rows('tools', build(), columns);
    fits(lines, columns);
    has(lines, /^TOOL\s+N\s+ERR\s+p50\s+p95\s+→CTX$/);
    // The chrome MCP server: 214 calls, 2 errors, p50/p95 estimated, 22k tokens into the context.
    has(lines, /^mcp:claude-in-ch.*\s214\s+2\s+≈1\.6s\s+≈25\.4s\s+≈22k$/);
    has(lines, /^Bash ▶0:46\s+15\s+3\s+≈2\.0s\s+≈27\.2s\s+≈4k$/);
    has(lines, /^RemoteTrigger\s+16\s+0\s+≈4\.3s\s+≈10\.0s\s+≈9k$/);
    // The sort is the query's: by calls, descending.
    const order = ['mcp:claude-in-ch', 'RemoteTrigger', 'Bash', 'ToolSearch'].map((t) => lines.findIndex((r) => r.startsWith(t)));
    assert.deepEqual([...order].sort((a, b) => a - b), order, JSON.stringify(lines));
    has(lines, 'top ctx');
    // The input is cut to the room left at 50 columns.
    has(lines, /^Read\s+\/home\/user\/project\/src\/a3.*\s+t1\s+≈1k$/);
    has(lines, /^Bash\s+make check\s+t2\s+≈888$/);
    assert.equal(lines.filter((r) => /\st\d\s+≈\d+k?$/.test(r)).length, 5, 'five top consumers');
  });

  test(`agents at ${columns} columns: the fixture agent and the missing MCP hint`, () => {
    const lines = rows('agents', build(), columns);
    fits(lines, columns);
    has(lines, /^✓ fork\s+Check whether a setup.*\s0:42\s+549k$/);
    has(lines, 'mcp: no live process (fixture)');
    assert.equal(lines.filter((r) => r !== '').length, 2, JSON.stringify(lines));
  });

  test(`files at ${columns} columns: paths, touches and no diff`, () => {
    const lines = rows('files', build(), columns);
    fits(lines, columns);
    has(lines, /^FILE\s+TOUCHES\s+\+\s+−$/);
    has(lines, /ad450c\.rs\s+W×1\s+—$/);
    has(lines, /fc57f7\.rs\s+R×1\s+—$/);
    // The path keeps its tail when cut.
    if (columns === 50) has(lines, /^…src\/ad450c\.rs\s/);
    assert.equal(lines.filter((r) => r !== '').length, 5, JSON.stringify(lines));
  });

  test(`events at ${columns} columns: the last 50, newest at the bottom`, () => {
    const events = fixture<{ at_ms: number; kind: string; text: string }[]>('events');
    const lines = rows('events', build(), columns);
    fits(lines, columns);
    assert.equal(lines.length, EVENT_ROWS);
    const first = events[events.length - EVENT_ROWS];
    const last = events[events.length - 1];
    assert.match(lines[0], new RegExp(`^${clock(first.at_ms)} ${first.kind}\\s+${first.text.slice(0, 10).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`));
    assert.match(lines[EVENT_ROWS - 1], new RegExp(`^${clock(last.at_ms)} ${last.kind}\\s+`));
    assert.ok(lines[EVENT_ROWS - 1].includes(last.text.slice(0, 10)), lines[EVENT_ROWS - 1]);
    assert.equal(clock(last.at_ms), new Date(last.at_ms).toISOString().slice(11, 19));
  });

  test(`advisor at ${columns} columns: the ranked list with the top item expanded`, () => {
    const advice = fixture<Record<string, string>[]>('advice');
    const second = { ...advice[0], rule: 'A03', headline: 'second headline', saving: '~12/turn' };
    const model = build({ query: { advice: [advice[0], second] } });
    const lines = rows('advisor', model, columns);
    fits(lines, columns);
    has(lines, columns >= 80 ? /^\s+▸\s+A15\s+`Bash make check` failed 3× with the same input\s+~481\/turn$/ : /^\s+▸\s+A15\s+`Bash make check` failed 3×.*~481\/turn$/);
    has(lines, /^\s+2\.\s+A03\s+second headline\s+~12\/turn$/);
    const text = lines.join(' ').replace(/\s+/g, ' ');
    for (const field of ['evidence', 'action', 'saving', 'why']) has(lines, new RegExp(`^\\s+${field}\\s`));
    assert.ok(text.includes(advice[0].evidence), text);
    assert.ok(text.includes(advice[0].action), text);
    assert.ok(text.includes(advice[0].explain), text);
    // The expansion sits between the first and the second row.
    const top = lines.findIndex((r) => r.includes('▸'));
    const next = lines.findIndex((r) => r.includes('second headline'));
    const why = lines.findIndex((r) => /^\s+why\s/.test(r));
    assert.ok(top < why && why < next, JSON.stringify(lines));
  });
}

test('tools: errors and the running tool carry the TUI colours', () => {
  const tree = draw('tools', build(), 80);
  const errors = coloured(tree).filter(([, c]) => c === 'red' || c === 'yellow');
  // Bash failed 3× (crit), the chrome server twice (warn); zero errors are uncoloured.
  assert.deepEqual(errors, [
    ['2', 'yellow'],
    ['3', 'red'],
  ]);
  assert.equal(colorOf(tree, 'Bash ▶0:46'), 'cyan');
  assert.equal(colorOf(tree, 'RemoteTrigger'), undefined);
});

test('tools: a running tool the query does not list yet is drawn from the engine stats', () => {
  const base = build({ running: null });
  const timed: Action[] = [
    { type: 'tool.start', name: 'Grep', at: T0 + 1000 },
    { type: 'tool.end', name: 'Grep', startedAt: T0 + 1000, at: T0 + 1250, isError: false, resultChars: 400 },
    { type: 'tool.start', name: 'Grep', at: T0 + 2000 },
  ];
  const model = timed.reduce(reduce, base);
  const lines = rows('tools', model, 80);
  assert.match(lines[1], /^Grep ▶0:46\s+1\s+0\s+250ms\s+250ms\s+≈100$/);
  // Without any stats the row still shows the tool as running.
  const fresh = rows('tools', reduce(base, { type: 'tool.start', name: 'Glob', at: T0 + 2000 }), 80);
  assert.match(fresh[1], /^Glob ▶0:46\s+0\s+0\s+—\s+—\s+—$/);
});

test('tools: ≈ marks exactly the approx values of the fixture', () => {
  const tools = fixture<{ tools: Record<string, { approx: boolean }>[] }>('tools').tools;
  const keys = new Map<string, string[]>();
  walk(draw('tools', build({ running: null }), 80), (n) => {
    if (n.type === 'Text' && typeof n.props?.key === 'string') keys.set(n.props.key, [...(keys.get(n.props.key) ?? []), textOf(n.children)]);
  });
  for (const [key, field] of [
    ['tool_calls', 'calls'],
    ['tool_errors', 'errors'],
    ['tool_p50', 'p50'],
    ['tool_p95', 'p95'],
    ['tokens_to_ctx', 'tokens_to_ctx'],
  ] as const) {
    const texts = keys.get(key)!;
    assert.equal(texts.length, tools.length, key);
    texts.forEach((t, i) => assert.equal(t.includes('≈'), tools[i][field].approx, `${key}[${i}] ${t}`));
  }
});

test('agents: MCP servers, restarts and background tasks', () => {
  const agents = fixture<Record<string, unknown>>('agents');
  const data = {
    ...agents,
    agents: [
      ...(agents.agents as unknown[]),
      { id: 'b1', type: 'general', description: 'Find the callers', model: 'claude-opus-5', state: 'running', elapsed: { value: 9000, unit: 'ms', metric_id: 'agent_state', approx: false }, tokens: { value: 1200, unit: 'tokens', metric_id: 'agent_tokens', approx: false } },
      { id: 'c1', type: 'Explore', description: 'Broken', model: 'claude-sonnet-5', state: 'failed', elapsed: null, tokens: { value: 0, unit: 'tokens', metric_id: 'agent_tokens', approx: false } },
    ],
    mcp: [
      { name: 'playwright', pid: 8244, rss: { value: 188 * 1024 * 1024, unit: 'bytes', metric_id: 'mcp_rss', approx: false }, restarts: 2, calls: { value: 7, unit: 'count', metric_id: 'mcp_calls', approx: false } },
      { name: 'github', pid: 8250, rss: { value: 512 * 1024, unit: 'bytes', metric_id: 'mcp_rss', approx: false }, restarts: 0, calls: { value: 0, unit: 'count', metric_id: 'mcp_calls', approx: false } },
    ],
    tasks: [{ id: 'b7f3a9c0ff', kind: 'bash', description: 'cargo build --release', started_at_ms: NOW - 118_000, status: 'running' }],
  };
  const model = build({ query: { agents: data } });
  for (const columns of [50, 80]) {
    const lines = rows('agents', model, columns);
    fits(lines, columns);
    has(lines, /^◐ general\s+Find the callers\s+0:09\s+1k$/);
    has(lines, /^✗ Explore\s+Broken\s+—\s+0$/);
    has(lines, /^mcp playwright\s+188 MB\s+7 calls\s+↻2$/);
    has(lines, /^mcp github\s+512 kB\s+0 calls$/);
    has(lines, /^bg\s+bash\s+cargo build --.*\s1:58\s+b7f3a9c0 running$/);
    assert.ok(!lines.some((r) => r.includes('no live process')), 'the hint goes once mcp is a list');
  }
  const tree = draw('agents', model, 80);
  assert.equal(colorOf(tree, '◐ general'), 'cyan');
  assert.equal(colorOf(tree, '✓ fork'), 'green');
  assert.equal(colorOf(tree, '✗ Explore'), 'red');
  assert.equal(colorOf(tree, '↻2'), 'yellow');
  const empty = rows('agents', build({ query: { agents: { agents: [], mcp: [], tasks: [] } } }), 50);
  assert.deepEqual(empty, ['no subagents, MCP servers or background tasks']);
});

test('files: lines ± when git knows them and the re-read marker', () => {
  const files = fixture<Record<string, unknown>[]>('files');
  const m = (value: number) => ({ value, unit: 'lines', metric_id: 'file_lines', approx: false });
  const data = [
    { ...files[0], path: '/home/user/project/src/main.rs', edits: 2, reads: 3, writes: 0, lines_added: m(12), lines_removed: m(3), reread_warning: true },
    ...files.slice(1),
  ];
  const model = build({ query: { files: data } });
  for (const columns of [50, 80]) {
    const lines = rows('files', model, columns);
    fits(lines, columns);
    has(lines, /main\.rs\s+R×3 E×2\s+\+12\s+−3\s+re-read ⚠$/);
    has(lines, /fc57f7\.rs\s+R×1\s+—$/);
  }
  const tree = draw('files', model, 80);
  assert.equal(colorOf(tree, '+12'), 'green');
  assert.equal(colorOf(tree, '−3'), 'red');
  assert.equal(colorOf(tree, 're-read ⚠'), 'yellow');
  const keys: string[] = [];
  walk(tree, (n) => {
    if (n.type === 'Text' && n.props?.key === 'file_rereads' && textOf(n.children) !== '') keys.push(textOf(n.children));
  });
  assert.deepEqual(keys, ['re-read ⚠']);
  assert.deepEqual(rows('files', build({ query: { files: [] } }), 50).slice(1), ['no files touched yet']);
});

test('events: kinds carry the TUI colours', () => {
  const data = [
    { at_ms: T0, kind: 'tool', text: 'Read x ▶' },
    { at_ms: T0 + 1000, kind: 'api', text: 'Bash ✗ error' },
    { at_ms: T0 + 2000, kind: 'hook', text: 'Stop hooks 1 ran' },
    { at_ms: T0 + 3000, kind: 'note', text: 'turn done in 0:03' },
    { at_ms: T0 + 4000, kind: 'perm', text: 'waiting' },
    { at_ms: T0 + 5000, kind: 'away', text: 'idle' },
  ];
  const tree = draw('events', build({ query: { events: data } }), 80);
  assert.deepEqual(coloured(tree), [
    ['tool', 'green'],
    ['api', 'red'],
    ['hook', 'cyan'],
    ['note', 'yellow'],
    ['perm', 'yellow'],
    ['away', 'yellow'],
  ]);
  const lines = renderToText(tree, 80);
  assert.equal(lines[0], '12:00:00 tool    Read x ▶');
  assert.equal(lines[5], '12:00:05 away    idle');
  assert.deepEqual(rows('events', build({ query: { events: [] } }), 50), ['no events yet']);
});

test('every view caps its tree at 400 rows: 1000 events draw 50, 1000 files 400', () => {
  const events = Array.from({ length: 1000 }, (_, i) => ({ at_ms: T0 + i * 1000, kind: 'tool', text: `event ${i}` }));
  const files = Array.from({ length: 1000 }, (_, i) => ({ path: `/p/f${i}.rs`, reads: 1, edits: 0, writes: 0, touches: { value: 1, unit: 'count', metric_id: 'file_touches', approx: false }, lines_added: { source: 'missing' }, lines_removed: { source: 'missing' }, reread_warning: false }));
  const tools = { tools: Array.from({ length: 1000 }, (_, i) => ({ tool: `t${i}`, calls: { value: 1, unit: 'count', metric_id: 'tool_calls', approx: false }, errors: { value: 0, unit: 'count', metric_id: 'tool_errors', approx: false }, p50: null, p95: null, tokens_to_ctx: { value: 1, unit: 'tokens', metric_id: 'tokens_to_ctx', approx: true } })), top_ctx: fixture<{ top_ctx: unknown[] }>('tools').top_ctx };
  const agents = { agents: Array.from({ length: 1000 }, (_, i) => ({ id: `a${i}`, type: 'fork', description: `d${i}`, state: 'done', elapsed: null, tokens: { value: 1, unit: 'tokens', metric_id: 'agent_tokens', approx: false } })), mcp: [], tasks: [] };
  const advice = Array.from({ length: 1000 }, (_, i) => ({ rule: 'A01', headline: `h${i}`, saving: '~1/turn', evidence: 'e', action: 'a', explain: 'x', doc_key: 'A01' }));
  const model = build({ query: { events, files, tools, agents, advice } });
  const eventLines = rows('events', model, 80);
  assert.equal(eventLines.length, EVENT_ROWS);
  assert.ok(eventLines[EVENT_ROWS - 1].endsWith('event 999'));
  for (const view of DETAIL) {
    const lines = renderToText(draw(view, model, 80), 80);
    assert.ok(lines.length <= MAX_ROWS, `${view}: ${lines.length} rows`);
  }
  assert.equal(rows('files', model, 80).length, MAX_ROWS);
  assert.equal(rows('tools', model, 80).length, MAX_ROWS);
  assert.equal(rows('agents', model, 80).length, MAX_ROWS);
  assert.equal(rows('advisor', model, 80).length, MAX_ROWS);
});

test('a missing binary draws the single line in every detail view', () => {
  for (const view of DETAIL) {
    for (const columns of [50, 80]) {
      assert.deepEqual(rows(view, build({ binary: 'missing' }), columns), [NEEDS_BINARY], view);
    }
  }
});

test('renderView dispatches on model.view and draws the Overview inline', () => {
  const model = build();
  const first = new Map<View, string>();
  for (const view of [...DETAIL, 'overview'] as View[]) first.set(view, rows(view, model, 80)[0]);
  assert.equal(new Set(first.values()).size, 6, JSON.stringify([...first]));
  assert.match(first.get('overview')!, /^● busy/);
  assert.match(first.get('tools')!, /^TOOL/);
  assert.match(first.get('events')!, /^\d\d:\d\d:\d\d /);
  for (const view of DETAIL) {
    const inline = renderToText(renderView({ ...model, view }, el, 80, 'inline', NOW), 80);
    assert.match(inline[0], /^● busy/, view);
    assert.ok(inline.some((r) => r.includes('Limits')), view);
  }
  // The functions the dispatcher calls are the ones the views export.
  assert.deepEqual(renderToText(renderTools(model, el, NOW), 80), rows('tools', model, 80));
  assert.deepEqual(renderToText(renderAgents(model, el, NOW), 80), rows('agents', model, 80));
  assert.deepEqual(renderToText(renderFiles(model, el), 80), rows('files', model, 80));
  assert.deepEqual(renderToText(renderEvents(model, el), 80), rows('events', model, 80));
  assert.deepEqual(renderToText(renderAdvisor(model, el, 80), 80), rows('advisor', model, 80));
});

test('every Text truncates, colours are palette names and keys are metric ids', () => {
  const doc = readFileSync(join(repo, 'docs', 'metrics.md'), 'utf8');
  const ids = new Set([...doc.matchAll(/<a id="([a-z0-9_]+)"><\/a>/g)].map((m) => m[1]));
  const model = build();
  for (const view of DETAIL) {
    let texts = 0;
    const keys = new Set<string>();
    walk(draw(view, model, 80), (n) => {
      if (typeof n.props?.key === 'string') keys.add(n.props.key);
      if (n.type !== 'Text') return;
      texts += 1;
      assert.match(String(n.props?.wrap), /^truncate/, `${view}: ${JSON.stringify(n.props)}`);
      const c = n.props?.color;
      if (c !== undefined) assert.match(String(c), /^[a-z]+$/, `${view}: colour ${String(c)}`);
    });
    assert.ok(texts > 3, `${view}: only ${texts} Texts`);
    // Events are a log, not metrics: the one view without a keyed element.
    assert.equal(keys.size > 0, view !== 'events', `${view}: ${keys.size} keyed elements`);
    for (const key of keys) assert.ok(ids.has(key), `${view}: key ${key} is not a metric id`);
  }
});

test('wrapWords splits by word and never past the width', () => {
  assert.deepEqual(wrapWords('a bb ccc dddd', 6), ['a bb', 'ccc', 'dddd']);
  assert.deepEqual(wrapWords('', 6), []);
  assert.deepEqual(wrapWords('toolongword x', 4), ['toolongword', 'x']);
});
