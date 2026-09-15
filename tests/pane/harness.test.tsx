import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { EngineInterface, On } from 'claude-code';
import { fakeEngine, fakeOn, textOf } from './harness';

test('clock: every/after fire on tick in due order and cancel; now resolves the time of the call', async () => {
  const $ = fakeEngine({ now: 1000 });
  const fired: string[] = [];
  // `now` is a Promise (2.1.271+), read at the moment the timer fires.
  const stamp = (label: string) => void $.clock.now().then((at) => fired.push(`${label}@${at}`));
  const every = $.clock.every(1000, () => stamp('every'));
  $.clock.after(1500, () => stamp('after'));
  $.clock.tick(2500);
  assert.deepEqual(fired, [], 'the readings land after the tick, not inside it');
  await Promise.resolve();
  assert.deepEqual(fired, ['every@2000', 'after@2500', 'every@3000']);
  assert.equal(await $.clock.now(), 3500);
  assert.equal($.clock.pending(), 1);
  every.cancel();
  $.clock.tick(5000);
  await Promise.resolve();
  assert.equal(fired.length, 3);
  assert.equal($.clock.pending(), 0);
});

test('process.run answers from the scripted map and rejects otherwise', async () => {
  const $ = fakeEngine({
    process: {
      'cctop --version': { exitCode: 0, stdout: 'cctop 0.1.1\n', stderr: '' },
      'cctop boom': new Error('spawn failed'),
    },
  });
  assert.equal((await $.process.run(['cctop', '--version'])).stdout, 'cctop 0.1.1\n');
  await assert.rejects($.process.run(['cctop', 'boom']));
  await assert.rejects($.process.run(['cctop', 'query', 'summary']));
  assert.deepEqual($.process.calls, [['cctop', '--version'], ['cctop', 'boom'], ['cctop', 'query', 'summary']]);
});

test('store, fs, env, session and ui capture', async () => {
  const $ = fakeEngine({ env: { HOME: '/home/user' }, session: { id: 'abc', turns: 3 } });
  await $.store.set('pane', { open: true, view: 'tools' });
  assert.deepEqual(await $.store.get('pane'), { open: true, view: 'tools' });
  assert.deepEqual($.store.map.get('pane'), { open: true, view: 'tools' });
  await $.fs.write('/home/user/.cctop/pane/abc.json', '{}');
  assert.equal(await $.fs.exists('/home/user/.cctop/pane/abc.json'), true);
  assert.equal($.fs.files.size, 1);
  assert.equal(await $.env.get('HOME'), '/home/user');
  assert.equal(await $.session.id(), 'abc');
  assert.equal(await $.session.turns(), 3);
  $.ui.invalidate('ui.render');
  $.ui.invalidate('ui.render');
  $.ui.log('hello');
  assert.equal($.ui.invalidates['ui.render'], 2);
  assert.deepEqual($.ui.logs, ['hello']);
});

test('elements are tagged factories usable through h', () => {
  const $ = fakeEngine();
  const { Box, Text, Button } = $.ui.resolve({ surface: 'terminal', component: 'Pane' });
  let pressed = 0;
  const tree = (
    <Box flexDirection="row">
      <Text wrap="truncate">hi {1}</Text>
      {false}
      <Button hotkey="1" onPress={() => pressed++}>
        Overview
      </Button>
    </Box>
  );
  assert.ok(tree.type === 'Box');
  assert.deepEqual(tree.props, { flexDirection: 'row' });
  const [t, b] = tree.children ?? [];
  assert.equal(textOf(t), 'hi 1');
  assert.equal(typeof b === 'object' && b.type === 'Button' && b.props.label, 'Overview');
  $.ui.press('Overview');
  assert.equal(pressed, 1);
});

test('fakeOn records registrations and dispatches a chain with next and core', async () => {
  const { on, dispatch, registrations } = fakeOn();
  const trail: string[] = [];
  const loose = on as unknown as (event: string, ...rest: unknown[]) => { catch: (h: unknown) => void };
  loose('tool.call', { tool: 'Bash' }, async (_$: EngineInterface, e: { tool: string }, next: (e: unknown) => Promise<unknown>) => {
    trail.push('outer');
    const r = await next(e);
    trail.push('outer-after');
    return r;
  });
  loose('tool.*', async (_$: EngineInterface, e: unknown, next: (e: unknown) => Promise<unknown>) => {
    trail.push('inner');
    return next(e);
  });
  loose('turn.start', async () => trail.push('never'));
  assert.equal(registrations.length, 3);
  const result = await dispatch('tool.call', { tool: 'Bash' }, () => 'core');
  assert.equal(result, 'core');
  assert.deepEqual(trail, ['outer', 'inner', 'outer-after']);
  trail.length = 0;
  await dispatch('tool.call', { tool: 'Read' });
  assert.deepEqual(trail, ['inner']);
});

test('fakeOn runs the .catch handler when a hook throws', async () => {
  const { on, dispatch, $ } = fakeOn();
  const typed: On = on;
  typed('session.start', () => {
    throw new Error('kaput');
  }).catch(($, e, next) => {
    $.ui.log(`caught ${next.error.message ?? next.error.kind}`);
    return next(e);
  });
  const result = await dispatch('session.start', { cwd: '/x', surface: 'terminal', isInteractive: true }, () => ({ cwd: '/x' }));
  assert.deepEqual(result, { cwd: '/x' });
  assert.deepEqual($.ui.logs, ['caught kaput']);
});
