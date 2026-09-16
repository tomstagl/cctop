import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { CommandRunResult, RenderElement, RenderNode, SkillPromptResult } from 'claude-code';
import { fakeEngine, fakeOn, paneRender, textOf, type FakeEngine } from './harness';
// The engine as the tests see it: fullscreen at 160 columns, the pane docked 72 wide.
const SURFACE = { columns: 160, bodyColumns: 72 };
import { renderToText } from './render';
import { register } from '../../plugin/hooks/pane';

// /cctop-pane and the view bar through the headless harness: the command's
// arguments open, toggle, switch and close the pane, a Button press switches
// the view, and the inline placement draws the short form without the bar.
const USAGE = 'usage: /cctop-pane [coach|overview|tools|agents|files|events|advisor|close]';
// What an open answers on the test surface (160 columns, a 72-wide dock).
const DOCKED = 'docked beside the transcript (72 columns): click a view in its bar to switch (or ctrl+x tab, then tab and enter), ctrl+x x closes it.';

// Lets the promise chains a hook left behind (binary detection) settle.
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function boot(opts: { binary?: 'present' | 'missing' } = {}) {
  const $ = fakeEngine({
    process: opts.binary === 'present' ? { 'cctop --version': { exitCode: 0, stdout: 'cctop 0.2.0\n', stderr: '' } } : {},
  });
  const { on, dispatch } = fakeOn($, { surface: SURFACE });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({
    cwd: '/home/user/project',
  }));
  await settle();
  const run = (args: string) =>
    dispatch<CommandRunResult>('command.run', { command: 'cctop-pane', args, origin: { kind: 'composer' } });
  const render = (columns: number, placement: 'dock' | 'inline' = 'dock') =>
    dispatch<RenderElement>('ui.render', paneRender('cctop', columns, { placement })).then((tree) => ({
      tree,
      rows: renderToText(tree, columns),
    }));
  return { $, dispatch, run, render };
}

function stored($: FakeEngine): unknown {
  return $.store.map.get('pane');
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };

function buttons(node: RenderNode | undefined, out: Node[] = []): Node[] {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return out;
  const n = node as Node;
  if (n.type === 'Button') out.push(n);
  for (const child of n.children ?? []) buttons(child, out);
  return out;
}

/** The text of every `inverse` Text in the tree (the view bar's current tab). */
function inverseTexts(node: RenderNode | undefined, out: string[] = []): string[] {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return out;
  const n = node as Node;
  if (n.type === 'Text' && n.props?.inverse === true) out.push(textOf(n.children));
  for (const child of n.children ?? []) inverseTexts(child, out);
  return out;
}

test('no args toggles the pane and remembers it', async () => {
  const { $, run } = await boot();
  assert.equal(stored($), undefined);

  const opened = await run('');
  assert.equal(opened.text, `cctop pane ${DOCKED}`);
  assert.deepEqual($.ui.opens, [{ id: 'cctop', title: 'cctop' }]);
  assert.ok(!('focus' in $.ui.opens[0]), 'open asked for focus');
  assert.deepEqual($.ui.closes, []);
  assert.deepEqual(stored($), { open: true, view: 'overview' });

  const closed = await run('');
  assert.equal(closed.text, 'cctop pane closed');
  assert.deepEqual($.ui.closes, [{ id: 'cctop' }]);
  assert.equal($.ui.opens.length, 1);
  assert.deepEqual(stored($), { open: false, view: 'overview' });

  const again = await run('');
  assert.equal(again.text, `cctop pane ${DOCKED}`);
  assert.equal($.ui.opens.length, 2);
  assert.ok(!('focus' in $.ui.opens[1]), 'open asked for focus');
});

test('a view name opens the pane on that view', async () => {
  const { $, run, render } = await boot({ binary: 'present' });
  const result = await run('tools');
  assert.equal(result.text, `cctop pane on Tools ${DOCKED}`);
  assert.deepEqual($.ui.opens, [{ id: 'cctop', title: 'cctop' }]);
  assert.deepEqual(stored($), { open: true, view: 'tools' });
  const { rows } = await render(80);
  assert.match(rows[1], /^╭5 Tools/, JSON.stringify(rows));
  assert.match(rows[2], /^│ TOOL\s+N\s+ERR/, JSON.stringify(rows));

  // A second view while open switches without a close; whitespace is ignored.
  const files = await run('  files ');
  assert.equal(files.text, `cctop pane on Files ${DOCKED}`);
  assert.deepEqual($.ui.closes, []);
  assert.deepEqual(stored($), { open: true, view: 'files' });
  assert.match((await render(80)).rows[2], /^│ FILE\s+TOUCHES/);
});

test('close closes the pane', async () => {
  const { $, run } = await boot();
  await run('advisor');
  assert.deepEqual(stored($), { open: true, view: 'advisor' });
  const result = await run('close');
  assert.equal(result.text, 'cctop pane closed');
  assert.deepEqual($.ui.closes, [{ id: 'cctop' }]);
  assert.deepEqual(stored($), { open: false, view: 'advisor' });
  // Closing again is harmless: the engine leaves an id that is not open alone.
  await run('close');
  assert.equal($.ui.closes.length, 2);
  assert.equal($.ui.opens.length, 1);
});

test('an unknown argument answers the usage line', async () => {
  const { $, run } = await boot();
  for (const args of ['bogus', 'open', 'Tools']) {
    const result = await run(args);
    assert.equal(result.text, USAGE, args);
  }
  assert.deepEqual($.ui.opens, []);
  assert.deepEqual($.ui.closes, []);
  assert.equal(stored($), undefined);
});

test('the view bar names the views, the current one inverse, and a press switches the view', async () => {
  const { $, run, render } = await boot({ binary: 'present' });
  await run('');
  const { tree, rows } = await render(80);
  // The current view is not a Button: nothing to press. No hotkeys: the
  // docked pane never reads them, and the band that carried them is gone.
  const bar = buttons(tree);
  assert.deepEqual(
    bar.map((b) => [b.props?.hotkey, b.props?.label, b.props?.plain, b.props?.key]),
    [
      [undefined, 'Coach', true, 'coach'],
      [undefined, 'Tools', true, 'tools'],
      [undefined, 'Agents', true, 'agents'],
      [undefined, 'Files', true, 'files'],
      [undefined, 'Events', true, 'events'],
      [undefined, 'Advisor', true, 'advisor'],
    ],
  );
  assert.equal(rows[0], 'cctop  Coach  Overview  Tools  Agents  Files  Events  Advisor');
  assert.ok(inverseTexts(tree).includes(' Overview '), 'the current view is drawn inverse');

  const before = $.ui.invalidates['ui.render'] ?? 0;
  $.ui.press('tools');
  // The redraw is asked for once the clock (a Promise) has answered.
  await settle();
  assert.equal($.ui.invalidates['ui.render'], before + 1);
  assert.deepEqual(stored($), { open: true, view: 'tools' });
  const after = await render(80);
  assert.match(after.rows[2], /^│ TOOL\s+N\s+ERR/);
  assert.equal(after.rows[0], 'cctop  Coach  Overview  Tools  Agents  Files  Events  Advisor');
  assert.ok(inverseTexts(after.tree).includes(' Tools '));
});

test('the view bar wraps at narrow widths and never overflows', async () => {
  const { run, render } = await boot();
  await run('');
  for (const columns of [40, 50, 60, 80]) {
    const { rows } = await render(columns);
    for (const row of rows) assert.ok(row.length <= columns, `row wider than ${columns}: ${JSON.stringify(row)}`);
    assert.ok(rows[0].startsWith('cctop  Coach  Overview'), rows[0]);
    // The harness draws a plain Button as the engine does, the bare label;
    // the bar still reserves the `[ ]` a surface may draw, so it wraps
    // into two rows below 73 columns.
    const barRows = rows.filter((r) => /^(cctop  )?(Coach|Overview|Tools|Agents|Files|Events|Advisor)\b/.test(r) && !r.startsWith('╭') && !r.startsWith(' cctop'));
    assert.equal(barRows.length, columns >= 73 ? 1 : 2, JSON.stringify(barRows));
    assert.ok(barRows.some((r) => r.includes('Advisor')), JSON.stringify(barRows));
  }
});

test('inline placement draws the strip without the view bar', async () => {
  const { run, render } = await boot();
  await run('tools');
  const { tree, rows } = await render(80, 'inline');
  assert.deepEqual(buttons(tree), []);
  assert.match(rows[0], /^○ IDLE · turn 0/, JSON.stringify(rows));
  assert.ok(
    !rows.some((r) => /^(cctop  )?(Coach|Overview|Tools)\b/.test(r) || r.startsWith('TOOL') || r.includes('Tokens & Cost') || r.includes('waiting on')),
    JSON.stringify(rows),
  );
  // The docked form of the same model draws the bar and the Tools view.
  const dock = await render(80);
  assert.ok(buttons(dock.tree).length === 6);
});

test('the /cctop skill prompt opens the pane and hands the model the outcome to relay', async () => {
  const { $, dispatch } = await boot();
  assert.deepEqual($.ui.opens, []);
  const result = await dispatch<SkillPromptResult>('skill.prompt', { skill: 'cctop', text: 'original skill text' });
  assert.deepEqual($.ui.opens, [{ id: 'cctop', title: 'cctop' }]);
  assert.equal(
    result.text,
    `The cctop pane was just opened by the plugin; its state is: "cctop pane ${DOCKED}" Reply with exactly that sentence and nothing else. Do not run any tool.`,
  );
});

test('a Pane render for another requestId passes through', async () => {
  const { dispatch, run } = await boot();
  await run('');
  const core: RenderElement = { type: 'Text', props: {}, children: ['other'] };
  const tree = await dispatch<RenderElement>('ui.render', paneRender('other', 80), () => core);
  assert.equal(tree, core);
  const inline = await dispatch<RenderElement>('ui.render', paneRender('other', 80, { placement: 'inline' }), () => core);
  assert.equal(inline, core);
});
