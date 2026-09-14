import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { CommandRunResult, RenderElement } from 'claude-code';
import { fakeEngine, fakeOn, paneRender } from './harness';
// The engine as the tests see it: fullscreen at 160 columns, the pane docked 72 wide.
const SURFACE = { columns: 160, bodyColumns: 72 };
import { renderToText } from './render';
import { register } from '../../plugin/hooks/pane';

// The US-001 skeleton through the headless harness: session.start registers
// the command, /cctop-pane opens the pane, ui.render draws the header.
async function boot() {
  const $ = fakeEngine();
  const { on, dispatch } = fakeOn($, { surface: SURFACE });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({
    cwd: '/home/user/project',
  }));
  return { $, dispatch };
}

test('session.start registers /cctop-pane', async () => {
  const { $ } = await boot();
  assert.deepEqual(
    $.command.registered.map((c) => c.name),
    ['cctop-pane'],
  );
});

test('/cctop-pane opens the pane', async () => {
  const { $, dispatch } = await boot();
  const result = await dispatch<CommandRunResult>('command.run', { command: 'cctop-pane', args: '', origin: 'person' });
  assert.match(result.text ?? "", /^cctop pane docked beside the transcript/);
  assert.deepEqual($.ui.opens, [{ id: 'cctop', title: 'cctop' }]);
});

for (const columns of [50, 80]) {
  test(`renders the pane at ${columns} columns`, async () => {
    const { dispatch } = await boot();
    const tree = await dispatch<RenderElement>('ui.render', paneRender('cctop', columns));
    const rows = renderToText(tree, columns);
    assert.ok(rows.some((r) => r.includes('turn 0')), `no header row: ${JSON.stringify(rows)}`);
    for (const row of rows) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
  });
}

test('a Pane render for another requestId passes through', async () => {
  const { dispatch } = await boot();
  const core: RenderElement = { type: 'Text', props: {}, children: ['other'] };
  const tree = await dispatch<RenderElement>('ui.render', paneRender('other', 80), () => core);
  assert.equal(tree, core);
});
