import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { CommandRunResult, RenderElement, RenderInput, RenderNode } from 'claude-code';
import { fakeEngine, fakeOn, paneRender, type FakeSurface } from './harness';
import { renderToText } from './render';
import { OPEN_SETTLE_MS, register } from '../../plugin/hooks/pane';

// The band above the prompt: the one site whose Button hotkeys the engine
// honours (a digit typed into the empty composer presses one), so the view
// bar is drawn there while the pane is docked and visible, and nowhere else
// — a bar nobody can see would still eat the digits.
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

function bandRender(columns = 120, extra: { hasSurvey?: boolean } = {}): RenderInput<'AbovePrompt', 'terminal'> {
  return {
    surface: 'terminal',
    component: 'AbovePrompt',
    requestId: 'band',
    viewport: { columns, rows: 40 },
    props: { hasSurvey: extra.hasSurvey ?? false, isWorking: false, maxRows: 8, scroll: { offset: 0, bodyRows: 8 } },
  };
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };

function buttons(node: RenderNode | undefined, out: Node[] = []): Node[] {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return out;
  const n = node as Node;
  if (n.type === 'Button') out.push(n);
  for (const child of n.children ?? []) buttons(child, out);
  return out;
}

async function boot(surface: FakeSurface) {
  const $ = fakeEngine({ env: { HOME: '/home/user' } });
  const { on, dispatch } = fakeOn($, { surface });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({
    cwd: '/home/user/project',
  }));
  await settle();
  const run = (args = '') =>
    dispatch<CommandRunResult>('command.run', { command: 'cctop-pane', args, origin: { kind: 'composer' } });
  // The engine's own answer for the band: nothing of its own.
  const core: RenderElement = { type: 'Box', props: {}, children: [] } as unknown as RenderElement;
  const band = (columns = 120, extra: { hasSurvey?: boolean } = {}) =>
    dispatch<RenderElement>('ui.render', bandRender(columns, extra), () => core);
  return { $, dispatch, run, band, core };
}

test('the band draws the view bar only while the pane is docked and visible', async () => {
  const { $, run, band, core } = await boot({ columns: 162, bodyColumns: 72 });
  // Closed: the engine's tree passes through untouched.
  assert.equal(await band(), core);

  await run('');
  const tree = await band();
  assert.notEqual(tree, core);
  const bar = buttons(tree);
  assert.deepEqual(
    bar.map((b) => [b.props?.hotkey, b.props?.label, b.props?.plain, b.props?.key]),
    [
      ['1', 'Overview', true, 'overview'],
      ['2', 'Tools', true, 'tools'],
      ['3', 'Agents', true, 'agents'],
      ['4', 'Files', true, 'files'],
      ['5', 'Events', true, 'events'],
      ['6', 'Advisor', true, 'advisor'],
    ],
  );
  const rows = renderToText(tree, 120);
  assert.equal(rows[0], 'cctop [1 Overview]  [2 Tools]  [3 Agents]  [4 Files]  [5 Events]  [6 Advisor]');
  assert.equal(rows.length, 1, JSON.stringify(rows));

  // The engine's own tree keeps its place beneath the bar.
  assert.ok(JSON.stringify(tree).includes(JSON.stringify(core)));

  // A survey holds the band: the bar yields.
  assert.equal(await band(120, { hasSurvey: true }), core);

  await run('close');
  await settle();
  assert.equal(await band(), core);
});

test('a digit pressed in the band switches the pane to that view', async () => {
  const { $, run, band, dispatch } = await boot({ columns: 162, bodyColumns: 72 });
  await run('');
  await band();
  // The engine presses the Button whose hotkey the person typed: `ui.press`
  // reaches onPress, which is selectView.
  $.ui.press('tools');
  await settle();
  assert.deepEqual($.store.map.get('pane'), { open: true, view: 'tools' });
  const pane = await dispatch<RenderElement>('ui.render', paneRender('cctop', 162, { bodyColumns: 72 }));
  assert.match(renderToText(pane, 72)[1], /^╭2 Tools/);
});

test('no bar while the pane is hidden behind the diff panel or inline above the prompt', async () => {
  const hidden = await boot({ columns: 162, hidden: true });
  const pending = hidden.run('');
  await settle();
  hidden.$.clock.tick(OPEN_SETTLE_MS);
  await pending;
  assert.equal(await hidden.band(), hidden.core, 'hidden: the hotkeys would switch a view nobody sees');
  // The hidden watch and the close each ask the band to redraw.
  assert.ok((hidden.$.ui.invalidates['ui.render'] ?? 0) >= 2);

  const inline = await boot({ columns: 100, placement: 'inline', bodyColumns: 96 });
  await inline.run('');
  assert.equal(await inline.band(), inline.core, 'inline: the pane itself sits above the prompt');
});

test('the band bar names cctop and wraps to the band width', async () => {
  const { run, band } = await boot({ columns: 162, bodyColumns: 72 });
  await run('');
  const narrow = renderToText(await band(50), 50);
  assert.ok(narrow.length >= 2, JSON.stringify(narrow));
  for (const row of narrow) assert.ok(row.length <= 50, row);
  assert.ok(narrow[0].startsWith('cctop [1 Overview]'), narrow[0]);
  const wide = renderToText(await band(200), 200);
  assert.equal(wide.length, 1);
  assert.ok(wide[0].startsWith('cctop [1 Overview]'), wide[0]);
});
