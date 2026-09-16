import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { RenderElement, RenderNode } from 'claude-code';
import { fakeElements, textOf } from './harness';
import { body, frameTitles, isBorder, renderToText } from './render';
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

// The rows of a view as its frame holds them (`body` strips the borders and
// the `│ … │`), so the assertions read the content the TUI would show.
function rows(view: View, model: Model, columns: number): string[] {
  return body(rawRows(view, model, columns));
}
function rawRows(view: View, model: Model, columns: number): string[] {
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
  const visit = (node: RenderNode | undefined): void => {
    if (node === undefined || node === null || typeof node !== 'object' || node.type === 'engine') return;
    const n = node as Node;
    // A frame's top and bottom rows are chrome (the hotkey there is accent-coloured by design).
    if (n.type === 'Text' && /^[╭╰]/.test(textOf(n.children))) return;
    if (n.type === 'Text' && typeof n.props?.color === 'string') {
      const text = textOf(n.children).trim();
      // Bars and gauges are chrome too, not a coloured value.
      if (text !== '' && !/^[│─▇▁]+$/.test(text)) out.push([text, ROLE[n.props.color] ?? n.props.color]);
    }
    for (const c of n.children ?? []) visit(c);
  };
  visit(tree);
  return out;
}

// The theme keys the views draw with, back to the TUI's roles the
// assertions name (views/frame.tsx THEME).
const ROLE: Record<string, string> = { success: 'green', warning: 'yellow', error: 'red', suggestion: 'cyan' };
const THEME_KEYS = new Set(Object.keys(ROLE));

function colorOf(tree: RenderElement, text: string): string | undefined {
  return coloured(tree).find(([t]) => t === text)?.[1];
}

for (const columns of [50, 80]) {
  test(`tools at ${columns} columns: the fixture table, the running tool and the top consumers`, () => {
    const lines = rows('tools', build(), columns);
    fits(lines, columns);
    has(lines, /^TOOL\s+N\s+ERR\s+p50\s+p95\s+→CTX$/);
    assert.deepEqual(frameTitles(rawRows('tools', build(), columns)), ['5 Tools ─ 257 calls']);
    // The chrome MCP server: 214 calls, 2 errors, p50/p95 estimated, 152k tokens into the
    // context (87 screenshots at ~1 500 tokens each on top of their text).
    has(lines, /^mcp:claude-i.*\s214\s+2\s+≈1\.6s\s+≈25\.4s\s+≈152k$/);
    has(lines, /^Bash ▶0:46\s+15\s+3\s+≈2\.0s\s+≈27\.2s\s+≈4k$/);
    has(lines, /^RemoteTrigger\s+16\s+0\s+≈4\.3s\s+≈10\.0s\s+≈9k$/);
    // The sort is the query's: by calls, descending.
    const order = ['mcp:claude-i', 'RemoteTrigger', 'Bash', 'ToolSearch'].map((t) => lines.findIndex((r) => r.startsWith(t)));
    assert.deepEqual([...order].sort((a, b) => a - b), order, JSON.stringify(lines));
    has(lines, 'top ctx');
    // The top consumers are screenshot results (text + one image each); the
    // input is cut to the room left at 50 columns.
    has(lines, /^mcp:claude-.*\s+t1[24]\s+≈2k$/);
    assert.equal(lines.filter((r) => /\st\d+\s+≈\d+k?$/.test(r)).length, 5, 'five top consumers');
  });

  test(`agents at ${columns} columns: the fixture agent and the missing MCP hint`, () => {
    const lines = rows('agents', build(), columns);
    fits(lines, columns);
    // The TUI's agents view columns: type, model, time, tokens, ≈$, ret,
    // waste; below 58 body columns the model and ret make way.
    if (columns >= 60) has(lines, /^✓ fork\s+sonnet\s+0:42\s+485k\s+0\.13\s+—\s+0\.00$/);
    else has(lines, /^✓ fork\s+0:42\s+485k\s+0\.13\s+0\.00$/);
    has(lines, 'mcp: no live process (fixture)');
    assert.equal(lines.filter((r) => r !== '').length, 2, JSON.stringify(lines));
    assert.deepEqual(frameTitles(rawRows('agents', build(), columns)), ['6 Agents & MCP ─ 0/1 agents']);
  });

  test(`files at ${columns} columns: paths, touches and no diff`, () => {
    const lines = rows('files', build(), columns);
    fits(lines, columns);
    has(lines, /^FILE\s+TOUCHES\s+\+\s+−$/);
    has(lines, /ad450c\.rs\s+W×1\s+—$/);
    has(lines, /fc57f7\.rs\s+R×1\s+—$/);
    // The path keeps its tail when cut.
    if (columns === 50) has(lines, /^…ad450c\.rs\s/);
    assert.equal(lines.filter((r) => r !== '').length, 5, JSON.stringify(lines));
    assert.deepEqual(frameTitles(rawRows('files', build(), columns)), ['7 Files ─ 4 touched']);
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

  test(`advisor at ${columns} columns: the slot occupant expanded, then the queue`, () => {
    const advice = fixture<{ items: Record<string, string>[]; primary: Record<string, string> }>('advice');
    const first: Record<string, string> = { ...advice.items[0], class: 'NEXT', rule: 'A25', headline: 'EXPLORING ×8 · +31k ctx this run', saving: '~31k/turn', action_kind: 'prompt', action_text: 'Use an Explore subagent for the rest.' };
    const second: Record<string, string> = { ...advice.items[0], class: 'LATER', rule: 'A03', headline: 'second headline', saving: '~12/turn' };
    const model = build({ query: { advice: { schema: 2, session_mode: 'interactive', primary: first, items: [first, second], snoozed: [{ rule: 'A17', until_turn: 9 }] } } });
    const lines = rows('advisor', model, columns);
    fits(lines, columns);
    has(lines, columns >= 80 ? /^\s+▸\s+NEXT\s+A25\s+EXPLORING ×8 · \+31k ctx this run\s+~31k\/turn$/ : /^\s+▸\s+NEXT\s+A25\s+EXPLORING.*~31k\/turn$/);
    has(lines, /^\s+2\.\s+LATER\s+A03\s+second headline\s+~12\/turn$/);
    assert.deepEqual(frameTitles(rawRows('advisor', model, columns)), ['9 Advisor ─ 1 of 2 · 1 snoozed']);
    const text = lines.join(' ').replace(/\s+/g, ' ');
    for (const field of ['evidence', 'action', 'prompt', 'saving', 'retires', 'why']) has(lines, new RegExp(`^\\s+${field}\\s`));
    assert.ok(text.includes(first.evidence), text);
    assert.ok(text.includes(first.action), text);
    assert.ok(text.includes(first.action_text), text);
    assert.ok(text.includes(first.explain), text);
    assert.ok(text.includes('snoozed: A17'), text);
    // The expansion sits between the first and the second row.
    const top = lines.findIndex((r) => r.includes('▸'));
    const next = lines.findIndex((r) => r.includes('second headline'));
    const why = lines.findIndex((r) => /^\s+why\s/.test(r));
    assert.ok(top < why && why < next, JSON.stringify(lines));
    // The pre-schema-2 bare array still renders, without a slot marker.
    const legacy = rows('advisor', build({ query: { advice: [second] } }), columns);
    has(legacy, /^\s+1\.\s+LATER\s+A03\s+second headline/);
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

test('agents on fixture C: the columns of the TUI agents view, waste with its reason', () => {
  // `cctop query agents` on fixture C (tests/pane/fixtures/agents-c.json):
  // a killed Explore agent with a partial result and the fork that
  // returned 2 069 characters; the shell task's notification made no row.
  const data = fixture<Record<string, unknown>>('agents-c');
  const model = build({ query: { agents: data } });
  const lines = rows('agents', model, 80);
  fits(lines, 80);
  // (354 828 tokens: the pane's formatTokens rounds to 355k where the
  // TUI's fmt::tokens truncates to 354k — a pre-existing difference.)
  has(lines, /^✗ Explore\s+sonnet\s+2:32\s+355k\s+0\.18\s+41\s+0\.18 killed$/);
  has(lines, /^✓ fork\s+sonnet\s+0:42\s+485k\s+0\.13\s+517\s+0\.00$/);
  assert.equal(lines.filter((r) => /^[✗✓◐] /.test(r)).length, 2);
  assert.deepEqual(frameTitles(rawRows('agents', model, 80)), ['6 Agents & MCP ─ 0/2 agents']);
  // The rows are sorted by spend, as the query returns them; the totals
  // carry the same figures the TUI's footer prints.
  const totals = data.totals as Record<string, { value: number }>;
  assert.equal(totals.classified as unknown as number, 1);
  assert.ok(Math.abs(totals.waste.value - 0.18) < 0.005);
  const narrow = rows('agents', model, 50);
  fits(narrow, 50);
  has(narrow, /^✗ Explore\s+2:32\s+355k\s+0\.18\s+0\.18 killed$/);
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
    if (columns >= 60) {
      has(lines, /^◐ general\s+opus\s+0:09\s+1k\s+—\s+\.\.\.\s+0\.00$/);
      has(lines, /^✗ Explore\s+sonnet\s+—\s+0\s+—\s+—\s+0\.00$/);
    } else {
      has(lines, /^◐ general\s+0:09\s+1k\s+—\s+0\.00$/);
      has(lines, /^✗ Explore\s+—\s+0\s+—\s+0\.00$/);
    }
    has(lines, /^mcp playwright\s+188 MB\s+7 calls\s+↻2$/);
    has(lines, /^mcp github\s+512 kB\s+0 calls$/);
    has(lines, /^bg\s+bash\s+cargo bui.*\s1:58\s+b7f3a9c0 running$/);
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
    if (n.type === 'Text' && n.props?.key === 'file_rereads' && textOf(n.children).trim() !== '') keys.push(textOf(n.children).trim());
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
  const lines = body(renderToText(tree, 80));
  assert.equal(lines[0], '12:00:00 tool    Read x ▶');
  assert.equal(lines[5], '12:00:05 away    idle');
  assert.deepEqual(frameTitles(renderToText(tree, 80)), ['8 Events ─ 6']);
  assert.deepEqual(rows('events', build({ query: { events: [] } }), 50), ['no events yet']);
});

test('every view caps its frame at MAX_ROWS rows: 1000 events draw 50, 1000 files MAX_ROWS', () => {
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
    const lines = rawRows(view, model, 80);
    // The frame's two borders sit outside the cap.
    assert.ok(lines.length <= MAX_ROWS + 2, `${view}: ${lines.length} rows`);
    assert.ok(isBorder(lines[0]) && isBorder(lines[lines.length - 1]), view);
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
  assert.match(first.get('overview')!, /^ cctop  /);
  assert.match(first.get('tools')!, /^TOOL/);
  assert.match(first.get('events')!, /^\d\d:\d\d:\d\d /);
  for (const view of DETAIL) {
    const inline = renderToText(renderView({ ...model, view }, el, 80, 'inline', NOW), 80);
    assert.match(inline[0], /^● BUSY/, view);
    assert.ok(inline.some((r) => r.includes('Limits')), view);
  }
  // The functions the dispatcher calls are the ones the views export.
  assert.deepEqual(renderToText(renderTools(model, el, 80, NOW), 80), rawRows('tools', model, 80));
  assert.deepEqual(renderToText(renderAgents(model, el, 80, NOW), 80), rawRows('agents', model, 80));
  assert.deepEqual(renderToText(renderFiles(model, el, 80), 80), rawRows('files', model, 80));
  assert.deepEqual(renderToText(renderEvents(model, el, 80), 80), rawRows('events', model, 80));
  assert.deepEqual(renderToText(renderAdvisor(model, el, 80), 80), rawRows('advisor', model, 80));
});

// Every row Text (a Box's child) truncates; a Text inside a Text is an
// inline segment and inherits its row's wrap. Colours are theme keys.
function auditTexts(tree: RenderElement, label: string): { texts: number; keys: Set<string> } {
  let texts = 0;
  const keys = new Set<string>();
  const visit = (node: RenderNode | undefined, inText: boolean): void => {
    if (node === undefined || node === null || typeof node !== 'object' || node.type === 'engine') return;
    const n = node as Node;
    if (typeof n.props?.key === 'string') keys.add(n.props.key);
    if (n.type === 'Text') {
      texts += 1;
      if (!inText) assert.match(String(n.props?.wrap), /^truncate/, `${label}: ${JSON.stringify(n.props)}`);
      const c = n.props?.color;
      if (c !== undefined) assert.ok(THEME_KEYS.has(String(c)), `${label}: colour ${String(c)} is not a theme key`);
    }
    for (const child of n.children ?? []) visit(child, inText || n.type === 'Text');
  };
  visit(tree, false);
  return { texts, keys };
}

test('every row Text truncates, colours are theme keys and keys are metric ids', () => {
  const doc = readFileSync(join(repo, 'docs', 'metrics.md'), 'utf8');
  const ids = new Set([...doc.matchAll(/<a id="([a-z0-9_]+)"><\/a>/g)].map((m) => m[1]));
  const model = build();
  for (const view of DETAIL) {
    const { texts, keys } = auditTexts(draw(view, model, 80), view);
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
