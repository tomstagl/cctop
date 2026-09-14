import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { ProcessRunResult, RenderElement } from 'claude-code';
import { fakeEngine, fakeElements, fakeOn, paneRender, type FakeEngine, type ProcessScript } from './harness';
// The engine as the tests see it: fullscreen at 160 columns, the pane docked 72 wide.
const SURFACE = { columns: 160, bodyColumns: 72 };
import { renderToText } from './render';
import { QUERY_FIXTURES, fixture } from './fixture';
import { register } from '../../plugin/hooks/pane';

// US-008 through the headless harness with the manual clock: the redraw
// throttle, the timers' life with the pane, the store round-trip at
// session.start, the marker file and a render that throws.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const SESSION = 'fake-session';
const MARKER = `/home/user/.cctop/pane/${SESSION}.json`;
const MANIFEST = '/plugins/cctop/.claude-plugin/plugin.json';
const manifest = readFileSync(join(__dirname, '..', '..', '..', 'plugin', '.claude-plugin', 'plugin.json'), 'utf8');
const PLUGIN_VERSION = (JSON.parse(manifest) as { version: string }).version;

const HELP = `Commands:\n${['summary', 'tools', 'files', 'agents', 'advice', 'events'].map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '' });

function binaryScripts(): Record<string, ProcessScript> {
  const scripts: Record<string, ProcessScript> = { 'cctop --version': ok('cctop 0.9.0\n'), 'cctop query --help': ok(HELP) };
  for (const verb of QUERY_FIXTURES) scripts[`cctop query ${verb} --session ${SESSION}`] = ok(JSON.stringify(fixture(verb)));
  return scripts;
}

const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

type Origin = 'plugin' | 'person' | 'unload';

function boot(opts: { binary?: boolean; store?: Record<string, unknown>; isInteractive?: boolean } = {}) {
  const $ = fakeEngine({
    now: T0,
    env: { HOME: '/home/user' },
    process: opts.binary === false ? { 'cctop --version': new Error('ENOENT') } : binaryScripts(),
    store: opts.store,
    files: { [MANIFEST]: manifest },
  });
  const { on, dispatch, registrations } = fakeOn($, { surface: SURFACE });
  register(on, {});
  const start = () =>
    dispatch(
      'session.start',
      { cwd: '/home/user/project', surface: 'terminal', isInteractive: opts.isInteractive ?? true },
      () => ({ cwd: '/home/user/project' }),
    );
  const command = (args = '') => dispatch('command.run', { command: 'cctop-pane', args, origin: 'person' });
  const close = (kind: Origin, id = 'cctop') => dispatch('ui.close', { id, origin: { kind } });
  const turnStart = () => dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  const turnComplete = () =>
    dispatch('turn.complete', { answer: 'ok', durationMs: 2900, isAborted: false, turnId: 't1', reason: 'answer' }, () => ({
      text: 'ok',
    }));
  const compact = () => dispatch('session.compact', { trigger: 'auto', messages: [] }, () => ({}));
  const render = (columns = 80) => dispatch<RenderElement>('ui.render', paneRender('cctop', columns));
  const invalidates = () => $.ui.invalidates['ui.render'] ?? 0;
  const marker = () => JSON.parse($.fs.files.get(MARKER) ?? 'null') as Record<string, unknown> | null;
  const queries = () => $.process.calls.filter((argv) => argv[1] === 'query' && argv[2] !== '--help');
  return { $, registrations, start, command, close, turnStart, turnComplete, compact, render, invalidates, marker, queries };
}

function stored($: FakeEngine): unknown {
  return $.store.map.get('pane');
}

test('20 model changes in 1 s produce at most 4 invalidates plus one trailing', async () => {
  const { $, start, command, compact, invalidates } = boot({ binary: false });
  await start();
  await settle();
  await command();
  await settle();
  // The open pane is idle and quiet: the first change after a gap draws at once.
  $.clock.tick(1000);
  const before = invalidates();
  await compact();
  assert.equal(invalidates(), before + 1, 'a lone change invalidates at once');

  $.clock.tick(1000);
  const base = invalidates();
  for (let i = 0; i < 20; i++) {
    if (i > 0) $.clock.tick(50);
    await compact();
  }
  const burst = invalidates() - base;
  assert.ok(burst <= 4, `${burst} invalidates inside one second`);
  assert.ok(burst >= 3, `${burst} invalidates: the throttle starves the pane`);
  // The surface draws on its frame, which also stands the hidden watch down.
  await settle();
  assert.equal($.clock.pending(), 1, 'one trailing call waits');
  $.clock.tick(250);
  assert.equal(invalidates() - base, burst + 1, 'the trailing call lands once the gap has passed');
  await settle();
  assert.equal($.clock.pending(), 0);
  $.clock.tick(5000);
  assert.equal(invalidates() - base, burst + 1, 'nothing more without a change');
});

test('no invalidate while the pane is closed', async () => {
  const { $, start, compact, turnStart, invalidates } = boot({ binary: false });
  await start();
  await settle();
  await turnStart();
  await compact();
  $.clock.tick(1000);
  assert.equal(invalidates(), 0);
});

for (const origin of ['plugin', 'person', 'unload'] as Origin[]) {
  test(`timers and the poller stop on ui.close from ${origin}, and only the books are kept after`, async () => {
    const { $, start, command, close, turnStart, turnComplete, queries, render } = boot();
    await start();
    await settle();
    await command();
    await settle();
    // A second of quiet lets the trailing redraw of the first tick land.
    $.clock.tick(1000);
    await turnStart();
    await settle();
    assert.equal($.clock.pending(), 2, 'usage timer and poller timer while open and busy');
    const polled = queries().length;
    assert.ok(polled > 0);
    const before = $.process.calls.length;

    await close(origin);
    await settle();
    assert.equal($.clock.pending(), 0, 'every timer is cancelled on close');
    assert.deepEqual(stored($), { open: false, view: 'overview' });

    // Closed: the turn hooks keep the books, but no timer, poll or usage read.
    await turnComplete();
    await turnStart();
    $.clock.tick(30000);
    await settle();
    assert.equal($.clock.pending(), 0);
    assert.equal($.process.calls.length, before, 'no query while closed');

    // Reopening shows the books were kept: the second turn is on the header.
    await command();
    await settle();
    const rows = renderToText(await render(80), 80);
    assert.ok(rows.some((r) => r.includes('turn 2')), JSON.stringify(rows));
    assert.ok(queries().length > polled, 'the poller runs again once reopened');
  });
}

test('a close of another pane is not ours', async () => {
  const { $, start, command, close, turnStart } = boot({ binary: false });
  await start();
  await settle();
  await command();
  await settle();
  $.clock.tick(1000);
  await turnStart();
  await close('person', 'other');
  await settle();
  assert.deepEqual(stored($), { open: true, view: 'overview' });
  assert.equal($.clock.pending(), 1, 'the usage timer keeps running');
});

test('store round-trip: an interactive session.start reopens the pane on the saved view after the binary check', async () => {
  const { $, start, queries, render } = boot({ store: { pane: { open: true, view: 'files' } } });
  await start();
  assert.deepEqual($.ui.opens, [], 'the restore waits for the binary check');
  await settle();
  await settle();
  assert.deepEqual($.ui.opens, [{ id: 'cctop', title: 'cctop' }]);
  assert.equal($.process.calls[0].join(' '), 'cctop --version');
  assert.ok(queries().length > 0, 'the poller starts with the restored pane');
  assert.deepEqual(stored($), { open: true, view: 'files' });
  const rows = renderToText(await render(80), 80);
  assert.match(rows[2], /^│ FILE\s+TOUCHES/, JSON.stringify(rows));
});

test('store round-trip: no reopen for a closed pane, a -p run or an empty store', async () => {
  for (const opts of [
    { store: { pane: { open: false, view: 'files' } } },
    { store: { pane: { open: true, view: 'files' } }, isInteractive: false },
    {},
  ]) {
    const { $, start } = boot(opts);
    await start();
    await settle();
    await settle();
    assert.deepEqual($.ui.opens, [], JSON.stringify(opts));
  }
  // A saved view the pane does not know falls back to the Overview.
  const { $, start } = boot({ store: { pane: { open: true, view: 'bogus' } } });
  await start();
  await settle();
  await settle();
  assert.deepEqual(stored($), { open: true, view: 'overview' });
});

// The marker as the module writes it: the session and load facts are fixed
// per boot, the rest changes with the pane.
function expectedMarker(rest: Record<string, unknown>): Record<string, unknown> {
  return {
    version: PLUGIN_VERSION,
    sessionId: SESSION,
    loaded: true,
    loadedAt: new Date(T0).toISOString(),
    placement: 'dock',
    ...rest,
  };
}

test('marker: written at load, on open, refreshed by the tick, open:false on close, never removed', async () => {
  const { $, start, command, close, marker } = boot();
  await start();
  await settle();
  // Written as soon as the session id is known: `loaded` proves the module
  // runs in this session before any pane opens (cctop pane status reads it).
  assert.deepEqual(
    marker(),
    expectedMarker({
      openedAt: null,
      heartbeatAt: new Date(T0).toISOString(),
      open: false,
      visibility: 'unknown',
      bodyColumns: null,
      viewportColumns: null,
    }),
    'loaded, not open, before the pane opens',
  );

  await command();
  await settle();
  assert.deepEqual(
    marker(),
    expectedMarker({
      openedAt: new Date(T0).toISOString(),
      heartbeatAt: new Date(T0).toISOString(),
      open: true,
      visibility: 'visible',
      bodyColumns: 72,
      viewportColumns: 160,
    }),
  );

  $.clock.tick(10000);
  await settle();
  const ticked = marker();
  assert.equal(ticked?.heartbeatAt, new Date(T0 + 10000).toISOString(), JSON.stringify(ticked));
  assert.equal(ticked?.openedAt, new Date(T0).toISOString(), 'openedAt is the open, not the tick');
  assert.equal(ticked?.open, true);

  $.clock.tick(500);
  await close('person');
  await settle();
  assert.deepEqual(
    marker(),
    expectedMarker({
      openedAt: new Date(T0).toISOString(),
      heartbeatAt: new Date(T0 + 10500).toISOString(),
      open: false,
      visibility: 'unknown',
      bodyColumns: 72,
      viewportColumns: 160,
    }),
  );
  $.clock.tick(30000);
  await settle();
  assert.ok($.fs.files.has(MARKER), 'the file stays: $.fs cannot delete');
  assert.equal(marker()?.heartbeatAt, new Date(T0 + 10500).toISOString(), 'no heartbeat once closed');
});

test('marker: the command close writes open:false too', async () => {
  const { $, start, command, marker } = boot();
  await start();
  await settle();
  await command();
  await settle();
  $.clock.tick(300);
  await command('close');
  await settle();
  assert.equal(marker()?.open, false);
  assert.equal(marker()?.heartbeatAt, new Date(T0 + 300).toISOString());
  assert.equal($.clock.pending(), 0);
  assert.deepEqual(stored($), { open: false, view: 'overview' });
});

test('a throwing view renders the error line above the previous tree, and the hook has a .catch', async () => {
  const { $, registrations, start, command, render } = boot();
  await start();
  await settle();
  await command('tools');
  await settle();
  const good = await render(80);
  const goodRows = renderToText(good, 80);
  assert.match(goodRows[2], /^│ TOOL\s+N\s+ERR/, JSON.stringify(goodRows));

  // The view bar's Button throws from now on: the view cannot build.
  const table = fakeElements(new Map()) as unknown as Record<string, unknown>;
  $.ui.resolve = (() => ({
    ...table,
    Button: () => {
      throw new Error('boom');
    },
  })) as unknown as FakeEngine['ui']['resolve'];
  const bad = await render(80);
  const rows = renderToText(bad, 80);
  assert.equal(rows[0], 'cctop render error: boom');
  assert.deepEqual(rows.slice(1), goodRows, 'the last good tree follows the error line');
  const line = (bad as { children?: unknown[] }).children?.[0] as { type: string; props: Record<string, unknown> };
  assert.equal(line.type, 'Text');
  assert.equal(line.props.color, 'red');
  assert.ok($.ui.logs.some((l) => l.includes('render error: boom')), JSON.stringify($.ui.logs));

  const reg = registrations.find((r) => r.event === 'ui.render');
  assert.ok(reg?.catchHandler !== undefined, 'ui.render has a .catch');
});
