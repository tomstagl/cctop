import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { RenderElement, SessionUsage, ToolCallResult } from 'claude-code';
import { fakeEngine, fakeOn, paneRender, type FakeEngine } from './harness';
import { renderToText } from './render';
import { fixture } from './fixture';
import { initialModel, percentile, reduce, usageRows, type Action, type Model } from '../../plugin/hooks/model';
import { register } from '../../plugin/hooks/pane';

const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);

function run(actions: Action[], from: Model = initialModel()): Model {
  return actions.reduce(reduce, from);
}

// The scripted sequence the story prescribes: start, three tool calls (one
// an error; results of 400, 4000 and 40 chars), a compaction, completion.
const SEQUENCE: Action[] = [
  { type: 'session.start', at: T0 },
  { type: 'turn.start', at: T0 + 100 },
  { type: 'tool.start', name: 'Bash', at: T0 + 200 },
  { type: 'tool.end', name: 'Bash', startedAt: T0 + 200, at: T0 + 500, isError: false, resultChars: 400 },
  { type: 'tool.start', name: 'Bash', at: T0 + 600 },
  { type: 'tool.end', name: 'Bash', startedAt: T0 + 600, at: T0 + 2600, isError: true, resultChars: 4000 },
  { type: 'tool.start', name: 'Bash', at: T0 + 2700 },
  { type: 'tool.end', name: 'Bash', startedAt: T0 + 2700, at: T0 + 2750, isError: false, resultChars: 40 },
  { type: 'session.compact' },
  { type: 'turn.complete', at: T0 + 3000, durationMs: 2900, reason: 'answer' },
];

test('the scripted sequence yields the expected counts', () => {
  const model = run(SEQUENCE);
  const bash = model.tools.Bash;
  assert.equal(bash.calls, 3);
  assert.equal(bash.errors, 1);
  assert.deepEqual(bash.durationsMs, [300, 2000, 50]);
  assert.equal(percentile(bash.durationsMs, 0.5), 300);
  // ceil(400/4) + ceil(4000/4) + ceil(40/4)
  assert.equal(bash.tokensToCtx, 1110);
  assert.equal(model.compactions, 1);
  assert.equal(model.turn.number, 1);
  assert.equal(model.turn.state, 'idle');
  assert.equal(model.turn.lastDurationMs, 2900);
  assert.equal(model.turn.lastReason, 'answer');
  assert.equal(model.turn.runningTool, null);
});

test('reduce is pure: the input model is untouched', () => {
  const before = initialModel();
  const snapshot = JSON.stringify(before);
  run(SEQUENCE, before);
  assert.equal(JSON.stringify(before), snapshot);
});

test('the running tool is the latest started until its own end', () => {
  let model = run(SEQUENCE.slice(0, 2));
  model = reduce(model, { type: 'tool.start', name: 'Read', at: T0 + 200 });
  model = reduce(model, { type: 'tool.start', name: 'Grep', at: T0 + 210 });
  assert.deepEqual(model.turn.runningTool, { name: 'Grep', startedAt: T0 + 210 });
  // The earlier call ending does not clear the later one.
  model = reduce(model, { type: 'tool.end', name: 'Read', startedAt: T0 + 200, at: T0 + 300, isError: false, resultChars: 8 });
  assert.deepEqual(model.turn.runningTool, { name: 'Grep', startedAt: T0 + 210 });
  model = reduce(model, { type: 'tool.end', name: 'Grep', startedAt: T0 + 210, at: T0 + 400, isError: false, resultChars: 0 });
  assert.equal(model.turn.runningTool, null);
  assert.equal(model.tools.Read.tokensToCtx, 2);
  assert.equal(model.tools.Grep.tokensToCtx, 0);
});

test('percentile is nearest-rank', () => {
  assert.equal(percentile([], 0.5), 0);
  assert.equal(percentile([5], 0.5), 5);
  assert.equal(percentile([50, 2000, 300], 0.5), 300);
  assert.equal(percentile([1, 2, 3, 4], 0.95), 4);
});

test('usage rows from fixtures/usage.json', () => {
  const usage = fixture<SessionUsage>('usage');
  const model = reduce(initialModel(), { type: 'usage', usage, at: T0 });
  assert.equal(model.usageAt, T0);
  const rows = usageRows(model.usage!, T0);
  assert.deepEqual(rows, [
    { key: 'context_size', label: 'Context', value: '396k / 1.0M (40 %)' },
    { key: 'limit_5h', label: '5h', value: '42 %, resets in 2h 30m' },
    { key: 'limit_7d', label: '7d', value: '17 %, resets in 2d 12h' },
    { key: 'cost', label: 'Cost', value: '$9.90' },
  ]);
});

test('usage rows draw what the engine left out as ?', () => {
  const rows = usageRows({ context: { window: 200000 }, rateLimits: [] }, T0);
  assert.deepEqual(rows, [{ key: 'context_size', label: 'Context', value: '? / 200k (? %)' }]);
  const passed = usageRows(
    { context: { tokens: 50000, window: 200000 }, rateLimits: [{ kind: 'five_hour', percentUsed: 99.6, resetsAt: '2026-09-12T11:00:00Z' }] },
    T0,
  );
  assert.equal(passed[0].value, '50k / 200k (25 %)');
  assert.equal(passed[1].value, '100 %, resets in now');
});

// The hooks through the harness: a `$` whose `session.usage` counts its
// reads, a chain with a core that answers each event as the engine would.
function boot(usage?: SessionUsage) {
  const $: FakeEngine = fakeEngine({ now: T0, session: usage ? { usage } : {} });
  const reads = { count: 0 };
  const real = $.session.usage;
  $.session.usage = () => {
    reads.count += 1;
    return real();
  };
  const { on, dispatch } = fakeOn($);
  register(on, {});
  const start = () =>
    dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({
      cwd: '/home/user/project',
    }));
  const turnStart = () => dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  const turnComplete = (reason = 'answer') =>
    dispatch(
      'turn.complete',
      { answer: 'ok', durationMs: 2900, isAborted: false, turnId: 't1', reason },
      () => ({ text: 'ok' }),
    );
  const toolCall = (name: string, core: () => ToolCallResult | Promise<ToolCallResult>) =>
    dispatch<ToolCallResult>('tool.call', { tool: name, tool_use_id: `${name}-1`, command: 'true' }, core);
  const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
  return { $, dispatch, reads, start, turnStart, turnComplete, toolCall, settle };
}

test('usage is read once on open and once after turn.complete, and every second while busy', async () => {
  const { $, dispatch, reads, start, turnStart, turnComplete, settle } = boot();
  await start();
  await settle();
  assert.equal(reads.count, 0, 'nothing is read while the pane is closed');
  assert.equal($.clock.pending(), 0, 'no timer armed while idle');

  await dispatch('command.run', { command: 'cctop-pane', args: '', origin: 'person' });
  await settle();
  assert.equal(reads.count, 1, 'one read on open');
  // A second of quiet lets the trailing redraw of the open land.
  $.clock.tick(1000);
  assert.equal($.clock.pending(), 0, 'no timer armed while idle and open');

  await turnStart();
  assert.equal(reads.count, 1, 'turn.start reads nothing itself');
  assert.equal($.clock.pending(), 1, 'the usage timer is armed for the turn');
  $.clock.tick(3000);
  await settle();
  assert.equal(reads.count, 4, 'one read per second while busy');

  await turnComplete();
  await settle();
  assert.equal(reads.count, 5, 'one read after turn.complete');
  $.clock.tick(5000);
  await settle();
  assert.equal(reads.count, 5, 'no reads once idle');
  assert.equal($.clock.pending(), 0, 'the timer is cancelled at turn end');
});

test('hooks pass every result through unchanged', async () => {
  const { start, turnStart, turnComplete, toolCall, dispatch } = boot();
  assert.deepEqual(await start(), { cwd: '/home/user/project' });
  assert.deepEqual(await turnStart(), { turnId: 't1' });
  const answer: ToolCallResult = { ref: 1, result: { stdout: 'x' }, text: 'x'.repeat(400) };
  assert.equal(await toolCall('Bash', () => answer), answer);
  const compacted = { tokensBefore: 100, tokensAfter: 10 };
  assert.equal(await dispatch('session.compact', { trigger: 'auto', messages: [] }, () => compacted), compacted);
  assert.deepEqual(await turnComplete(), { text: 'ok' });
});

test('a tool that throws beneath the hook still rejects, and is logged', async () => {
  const { $, start, turnStart, toolCall } = boot();
  await start();
  await turnStart();
  await assert.rejects(toolCall('Bash', () => Promise.reject(new Error('boom'))));
  assert.ok($.ui.logs.some((l) => l.includes('tool.call failed')), JSON.stringify($.ui.logs));
});

test('the pane draws the turn, the running tool and the usage rows', async () => {
  const usage = fixture<SessionUsage>('usage');
  const { $, dispatch, start, turnStart, toolCall, settle } = boot(usage);
  await start();
  await settle();
  await dispatch('command.run', { command: 'cctop-pane', args: '', origin: 'person' });
  await turnStart();
  const before = $.ui.invalidates['ui.render'] ?? 0;
  // A slow tool: render while it runs, then let it end.
  let finish: (r: ToolCallResult) => void = () => {};
  const running = toolCall('Bash', () => new Promise<ToolCallResult>((resolve) => (finish = resolve)));
  await settle();
  $.clock.tick(2000);
  for (const columns of [50, 80]) {
    const tree = await dispatch<RenderElement>('ui.render', paneRender('cctop', columns));
    const rows = renderToText(tree, columns);
    assert.ok(rows.some((r) => r.includes('● busy · turn 1 · 0:02')), JSON.stringify(rows));
    assert.ok(rows.some((r) => r.includes('Bash 0:02')), JSON.stringify(rows));
    assert.ok(rows.some((r) => r.includes('396k / 1.0M (40 %)')), JSON.stringify(rows));
    assert.ok(rows.some((r) => /5 h\s+42 %/.test(r)), JSON.stringify(rows));
    assert.ok(rows.some((r) => /resets in\s+2h 29m/.test(r)), JSON.stringify(rows));
    assert.ok(rows.some((r) => /cost\s+\$9\.90/.test(r)), JSON.stringify(rows));
    for (const row of rows) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
  }
  finish({ ref: 1, result: {}, text: 'done' });
  await running;
  assert.ok(($.ui.invalidates['ui.render'] ?? 0) > before, 'model changes invalidate the open pane');
  const tree = await dispatch<RenderElement>('ui.render', paneRender('cctop', 80));
  assert.ok(!renderToText(tree, 80).some((r) => r.includes('Bash')), 'the tool goes once the call ends');
});
