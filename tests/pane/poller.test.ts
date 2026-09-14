import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { ProcessRunResult, RenderElement } from 'claude-code';
import { fakeEngine, fakeOn, paneRender, type FakeEngine, type ProcessScript } from './harness';
// The engine as the tests see it: fullscreen at 160 columns, the pane docked 72 wide.
const SURFACE = { columns: 160, bodyColumns: 72 };
import { renderToText } from './render';
import { QUERY_FIXTURES, fixture } from './fixture';
import { initialModel, reduce, QUERY_VERBS, type Model } from '../../plugin/hooks/model';
import { createPoller, parseQueryVerbs, writeMarker, type PollerEngine } from '../../plugin/hooks/poller';
import { register } from '../../plugin/hooks/pane';

const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const SESSION = 'fake-session';
const MARKER = `/home/user/.cctop/pane/${SESSION}.json`;

// `cctop query --help` as the 0.x binary prints it (the fixture generation
// binary); `advice` is dropped by tests that want an unsupported verb.
const HELP = `Print session metrics as JSON

Usage: cctop query [OPTIONS] <COMMAND>

Commands:
  summary   Session, context, tokens, cost, limits at a glance
  ledger    One row per turn
  tools     Per-tool statistics and the largest results
  files     Files touched
  agents    Subagents, MCP servers, background tasks
  advice    Ranked Advisor recommendations with explanations
  prefix    What rides on every request
  events    Event log
  help      Print this message or the help of the given subcommand(s)

Options:
      --session <SESSION>  Session id (or ≥ 8-char prefix), name, pid, or a fixture .jsonl path
  -h, --help               Print help
`;

const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '' });

// The scripted answers of a present binary: `--version`, `--help` and each
// verb answered from its fixture.
function binaryScripts(help = HELP): Record<string, ProcessScript> {
  const scripts: Record<string, ProcessScript> = {
    'cctop --version': ok('cctop 0.9.0\n'),
    'cctop query --help': ok(help),
  };
  for (const verb of QUERY_FIXTURES) {
    scripts[`cctop query ${verb} --session ${SESSION}`] = ok(JSON.stringify(fixture(verb)));
  }
  return scripts;
}

const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

// The queries spawned so far, `summary` alone or every verb.
function queries($: FakeEngine, verb?: string): string[][] {
  return $.process.calls.filter((argv) => argv[1] === 'query' && argv[2] !== '--help' && (verb === undefined || argv[2] === verb));
}

// The poller on its own: the model held by the test, the binary present.
function bootPoller(process: Record<string, ProcessScript> = binaryScripts(), from?: Model) {
  const $ = fakeEngine({ now: T0, process, env: { HOME: '/home/user' } });
  const engine: PollerEngine = { ...$, home: () => $.env.get('HOME') };
  let model: Model = from ?? reduce(initialModel(), { type: 'binary', binary: 'present' });
  const poller = createPoller(
    engine,
    () => model,
    (next) => {
      model = next;
    },
  );
  const busy = () => {
    model = reduce(model, { type: 'turn.start', at: $.clock.now() });
    poller.reschedule();
  };
  const idle = () => {
    model = reduce(model, { type: 'turn.complete', at: $.clock.now(), durationMs: 1, reason: 'answer' });
    poller.reschedule();
  };
  // Advances the clock in `step` slices so each due tick can settle before the next.
  const advance = async (ms: number, step = 1000) => {
    for (let left = ms; left > 0; left -= step) {
      $.clock.tick(Math.min(step, left));
      await settle();
    }
  };
  return { $, poller, model: () => model, busy, idle, advance };
}

test('parseQueryVerbs keeps the known verbs of the Commands block', () => {
  assert.deepEqual(parseQueryVerbs(HELP), ['summary', 'tools', 'files', 'agents', 'advice', 'events']);
  assert.deepEqual(parseQueryVerbs(HELP.replace(/^  advice .*\n/m, '')), ['summary', 'tools', 'files', 'agents', 'events']);
  assert.deepEqual(parseQueryVerbs('nothing here'), []);
  // Nothing after the block counts, and unknown verbs are dropped.
  assert.deepEqual(parseQueryVerbs('Commands:\n  events  x\n  pane  y\n\nOptions:\n  summary\n'), ['events']);
});

test('a tick reads the verbs once, runs every verb in order and parses the JSON', async () => {
  const { $, poller, model } = bootPoller();
  await poller.tick();
  assert.deepEqual(
    $.process.calls.map((argv) => argv.slice(1, 3).join(' ')),
    ['query --help', ...QUERY_VERBS.map((v) => `query ${v}`)],
  );
  for (const argv of queries($)) {
    assert.deepEqual(argv.slice(3), ['--session', SESSION]);
  }
  assert.deepEqual(model().verbs, [...QUERY_VERBS]);
  assert.equal(model().sessionId, SESSION);
  assert.deepEqual(model().query.summary, fixture('summary'));
  assert.equal((model().query.events as unknown[]).length, (fixture<unknown[]>('events')).length);
  assert.equal(model().queryAt, T0);
  assert.equal(model().stale, false);
  await poller.tick();
  assert.equal($.process.calls.filter((argv) => argv[2] === '--help').length, 1, '--help is read once');
});

test('cadence: 2 s while busy, 10 s idle, re-evaluated on each turn change', async () => {
  const { $, poller, busy, idle, advance } = bootPoller();
  poller.start();
  await settle();
  assert.equal(queries($, 'summary').length, 1, 'start runs a first tick');
  assert.equal($.clock.pending(), 1);

  await advance(9000);
  assert.equal(queries($, 'summary').length, 1, 'nothing before 10 s idle');
  await advance(1000);
  assert.equal(queries($, 'summary').length, 2, 'one tick at 10 s');
  await advance(10000);
  assert.equal(queries($, 'summary').length, 3);

  busy();
  assert.equal($.clock.pending(), 1, 'the idle timer is replaced, not added to');
  await advance(6000);
  assert.equal(queries($, 'summary').length, 6, 'one tick per 2 s while busy');

  idle();
  await advance(8000);
  assert.equal(queries($, 'summary').length, 6, 'back to 10 s once idle');
  await advance(2000);
  assert.equal(queries($, 'summary').length, 7);

  poller.stop();
  assert.equal($.clock.pending(), 0);
  idle();
  busy();
  assert.equal($.clock.pending(), 0, 'reschedule arms nothing once stopped');
  await advance(20000);
  assert.equal(queries($, 'summary').length, 7);
});

test('a slow summary blocks further ticks: one process at a time, no overlap', async () => {
  const scripts = binaryScripts();
  scripts[`cctop query summary --session ${SESSION}`] = () => new Promise<ProcessRunResult>(() => {});
  const { $, poller, busy, advance, model } = bootPoller(scripts);
  busy();
  poller.start();
  await settle();
  assert.equal(queries($).length, 1, 'summary started');
  await advance(20000);
  assert.equal(queries($).length, 1, 'no second summary and no tools while it hangs');
  const skipped = poller.tick();
  await skipped;
  assert.equal(queries($).length, 1, 'an explicit tick while one runs is skipped');
  await advance(12000);
  assert.equal(model().stale, true, 'a hung query still turns the pane stale after 30 s');
});

test('a failed or non-JSON call keeps the previous data; stale after 30 s of failures', async () => {
  const { $, poller, advance, model } = bootPoller();
  poller.start();
  await settle();
  const summary = model().query.summary;
  const tools = model().query.tools;
  assert.ok(summary !== undefined && tools !== undefined);

  // The fake copies the scripts at construction: later answers go through `$.process.script`.
  const scripts = $.process.script;
  scripts[`cctop query summary --session ${SESSION}`] = ok('not json {');
  scripts[`cctop query tools --session ${SESSION}`] = { exitCode: 2, stdout: '', stderr: 'no such session' };
  scripts[`cctop query files --session ${SESSION}`] = new Error('spawn failed');
  await advance(10000);
  assert.deepEqual(model().query.summary, summary, 'non-JSON keeps the previous summary');
  assert.deepEqual(model().query.tools, tools, 'a non-zero exit keeps the previous tools');
  assert.equal(model().queryAt, T0, 'a tick with a failure is not a successful one');
  assert.equal(model().stale, false, 'not stale yet');
  await advance(20000);
  assert.equal(model().stale, false, 'exactly 30 s is not older than 30 s');
  await advance(10000);
  assert.equal(model().stale, true, 'stale once the last success is older than 30 s');
  assert.equal($.ui.logs.filter((l) => l.includes('query summary failed')).length, 1, 'each failure logged once');

  Object.assign(scripts, binaryScripts());
  await advance(10000);
  assert.equal(model().stale, false, 'a full success clears stale');
  assert.equal(model().queryAt, T0 + 50000);
});

test('an unsupported verb is never called', async () => {
  const { $, poller, model } = bootPoller(binaryScripts(HELP.replace(/^  advice .*\n/m, '')));
  await poller.tick();
  assert.deepEqual(model().verbs, ['summary', 'tools', 'files', 'agents', 'events']);
  assert.equal(queries($, 'advice').length, 0);
  assert.equal(model().query.advice, undefined);
  assert.equal(model().stale, false);
  assert.equal(model().queryAt, T0, 'the tick counts as successful without it');
});

test('a binary that is not present spawns no query', async () => {
  const { $, poller, model } = bootPoller(binaryScripts(), initialModel());
  await poller.tick();
  assert.equal(model().binary, 'unknown');
  assert.equal($.process.calls.length, 0);
  poller.start();
  await settle();
  assert.equal($.process.calls.length, 0);
  poller.stop();
});

test('every tick refreshes the marker file', async () => {
  const { $, poller, advance, model } = bootPoller();
  await poller.tick();
  const first = JSON.parse($.fs.files.get(MARKER) ?? 'null') as Record<string, unknown>;
  assert.equal(first.sessionId, SESSION);
  assert.equal(first.heartbeatAt, new Date(T0).toISOString());
  assert.equal(first.open, false);
  poller.start();
  await advance(10000);
  const second = JSON.parse($.fs.files.get(MARKER) ?? 'null') as Record<string, unknown>;
  assert.equal(second.heartbeatAt, new Date(T0 + 10000).toISOString());
  // Without a session id there is nothing to name the file after.
  $.fs.files.clear();
  await writeMarker({ ...$, home: () => $.env.get('HOME') }, { ...model(), sessionId: null });
  assert.equal($.fs.files.size, 0);
});

// Through the hooks: session.start detects the binary after `next(e)`, the
// poller runs while the pane is open and follows the turn state.
function bootHooks(process: Record<string, ProcessScript>) {
  const $ = fakeEngine({ now: T0, process, env: { HOME: '/home/user' } });
  const { on, dispatch } = fakeOn($, { surface: SURFACE });
  register(on, {});
  const start = () =>
    dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({
      cwd: '/home/user/project',
    }));
  const open = () => dispatch('command.run', { command: 'cctop-pane', args: '', origin: 'person' });
  const turnStart = () => dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  const turnComplete = () =>
    dispatch(
      'turn.complete',
      { answer: 'ok', durationMs: 2900, isAborted: false, turnId: 't1', reason: 'answer' },
      () => ({ text: 'ok' }),
    );
  const render = async (columns = 80) =>
    renderToText(await dispatch<RenderElement>('ui.render', paneRender('cctop', columns)), columns);
  return { $, start, open, turnStart, turnComplete, render };
}

test('binary: missing shows the install hint and never spawns a query', async () => {
  const { $, start, open, turnStart, render } = bootHooks({ 'cctop --version': new Error('ENOENT') });
  await start();
  assert.deepEqual($.process.calls, [['cctop', '--version']]);
  await settle();
  await open();
  await turnStart();
  $.clock.tick(30000);
  await settle();
  assert.deepEqual($.process.calls, [['cctop', '--version']], 'no query without the binary');
  const rows = await render(80);
  assert.ok(rows.some((r) => r === 'needs the cctop binary: brew install tomstagl/tap/cctop'), JSON.stringify(rows));
});

test('a non-zero --version counts as missing too', async () => {
  const { $, start, open, render } = bootHooks({ 'cctop --version': { exitCode: 1, stdout: '', stderr: 'bad' } });
  await start();
  await settle();
  await open();
  $.clock.tick(30000);
  await settle();
  assert.equal(queries($).length, 0);
  assert.ok((await render(80)).some((r) => r.startsWith('needs the cctop binary')));
});

test('binary: present polls while the pane is open, at the cadence of the turn', async () => {
  const { $, start, open, turnStart, turnComplete, render } = bootHooks(binaryScripts());
  await start();
  assert.deepEqual($.process.calls, [['cctop', '--version']], '--version runs with session.start');
  await settle();
  assert.equal(queries($).length, 0, 'nothing is polled while the pane is closed');
  await open();
  await settle();
  assert.equal(queries($, 'summary').length, 1, 'opening the pane runs the first tick');
  assert.ok($.fs.files.has(MARKER));
  await turnStart();
  for (let i = 0; i < 3; i++) {
    $.clock.tick(2000);
    await settle();
  }
  assert.equal(queries($, 'summary').length, 4, 'every 2 s while busy');
  await turnComplete();
  await settle();
  for (let i = 0; i < 9; i++) {
    $.clock.tick(1000);
    await settle();
  }
  assert.equal(queries($, 'summary').length, 4, 'nothing before 10 s idle');
  $.clock.tick(1000);
  await settle();
  assert.equal(queries($, 'summary').length, 5);
  const rows = await render(80);
  assert.ok(!rows.some((r) => r.includes('needs the cctop binary')));
  assert.ok(!rows.some((r) => r.includes('unsupported')));
});

test('the pane names each unsupported verb', async () => {
  const { start, open, render } = bootHooks(binaryScripts(HELP.replace(/^  advice .*\n/m, '')));
  await start();
  await settle();
  await open();
  await settle();
  const rows = await render(80);
  assert.ok(rows.some((r) => r === 'advice: unsupported by this cctop version'), JSON.stringify(rows));
  assert.ok(!rows.some((r) => r.startsWith('events:')));
});

test('a pane opened before detection ends starts polling once the binary is found', async () => {
  let answer: (r: ProcessRunResult) => void = () => {};
  const scripts = binaryScripts();
  scripts['cctop --version'] = () => new Promise<ProcessRunResult>((resolve) => (answer = resolve));
  const { $, start, open } = bootHooks(scripts);
  await start();
  await open();
  await settle();
  assert.equal(queries($).length, 0);
  answer(ok('cctop 0.9.0\n'));
  await settle();
  assert.equal(queries($, 'summary').length, 1);
});
