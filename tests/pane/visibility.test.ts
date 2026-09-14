import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { CommandRunResult, SkillPromptResult } from 'claude-code';
import { fakeEngine, fakeOn, paneRender, type FakeSurface } from './harness';
import { HIDDEN_AFTER_MS, OPEN_SETTLE_MS, register } from '../../plugin/hooks/pane';
import { HIDDEN_STATUS, initialModel, MIN_DOCK_COLUMNS, outcomeText } from '../../plugin/hooks/model';

// Where the pane went, told to the person: the engine reports the renderer
// and the width through the first render's props, and says nothing at all
// while the /diff panel holds the dock — so that case is a missing render
// (docs/claude-code-panels.md §5.4). Every open answers with the outcome,
// and an open pane the engine stops drawing pins a status line.
const T0 = Date.UTC(2026, 8, 14, 12, 0, 0);
const MARKER = '/home/user/.cctop/pane/fake-session.json';
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function boot(surface: FakeSurface) {
  const $ = fakeEngine({ now: T0, env: { HOME: '/home/user' } });
  const { on, dispatch } = fakeOn($, { surface });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({
    cwd: '/home/user/project',
  }));
  await settle();
  const run = (args = '') =>
    dispatch<CommandRunResult>('command.run', { command: 'cctop-pane', args, origin: { kind: 'composer' } });
  const marker = () => JSON.parse($.fs.files.get(MARKER) ?? 'null') as Record<string, unknown> | null;
  return { $, dispatch, run, marker, surface };
}

test('outcomeText: docked, inline under 110 columns, inline in the classic renderer, hidden', () => {
  const base = initialModel();
  const docked = { ...base, visibility: 'visible' as const, placement: 'dock' as const, bodyColumns: 72, viewportColumns: 160 };
  assert.equal(
    outcomeText(docked),
    'cctop pane docked beside the transcript (72 columns): ctrl+x tab focuses it, 1-6 switch views, ctrl+x x closes it.',
  );
  assert.match(outcomeText(docked, 'Tools'), /^cctop pane on Tools docked beside the transcript \(72 columns\)/);

  const narrow = { ...docked, placement: 'inline' as const, viewportColumns: 96 };
  assert.equal(
    outcomeText(narrow),
    `cctop pane drawn above the prompt: the terminal is 96 columns wide, ${MIN_DOCK_COLUMNS} or more dock it beside the transcript.`,
  );

  const classic = { ...docked, placement: 'inline' as const, viewportColumns: 160 };
  assert.equal(outcomeText(classic), 'cctop pane drawn above the prompt: /tui fullscreen docks it beside the transcript.');
  const unmeasured = { ...classic, viewportColumns: null };
  assert.equal(outcomeText(unmeasured), outcomeText(classic), 'no viewport reads as wide enough');

  for (const visibility of ['unknown', 'hidden'] as const) {
    assert.equal(
      outcomeText({ ...docked, visibility }),
      'cctop pane is open but not shown: the /diff panel holds the side dock. Run /diff to hide it and cctop takes its place (needs /tui fullscreen and 110+ columns).',
      visibility,
    );
  }
});

test('an open the surface draws docked answers docked, with the body width', async () => {
  const { $, run, marker } = await boot({ columns: 162, bodyColumns: 72 });
  const result = await run('');
  assert.equal(
    result.text,
    'cctop pane docked beside the transcript (72 columns): ctrl+x tab focuses it, 1-6 switch views, ctrl+x x closes it.',
  );
  assert.deepEqual($.ui.statuses, [], 'no status line while the pane is drawn');
  const m = marker();
  assert.equal(m?.visibility, 'visible');
  assert.equal(m?.placement, 'dock');
  assert.equal(m?.bodyColumns, 72);
  assert.equal(m?.viewportColumns, 162);
});

test('an open the surface draws inline names the width or the renderer', async () => {
  const narrow = await boot({ columns: 100, placement: 'inline', bodyColumns: 96 });
  const at100 = await narrow.run('');
  assert.equal(
    at100.text,
    'cctop pane drawn above the prompt: the terminal is 100 columns wide, 110 or more dock it beside the transcript.',
  );
  assert.equal(narrow.marker()?.placement, 'inline');

  const classic = await boot({ columns: 160, placement: 'inline', bodyColumns: 156 });
  const wide = await classic.run('tools');
  assert.equal(wide.text, 'cctop pane on Tools drawn above the prompt: /tui fullscreen docks it beside the transcript.');
});

test('an open the surface never draws answers "not shown" after the settle window and pins the status', async () => {
  const { $, run, marker } = await boot({ columns: 162, hidden: true });
  const pending = run('');
  await settle();
  assert.equal($.ui.opens.length, 1, 'the pane is opened all the same');
  // Nothing has settled yet: the reply waits for a render or the window.
  let answered = false;
  void pending.then(() => {
    answered = true;
  });
  $.clock.tick(OPEN_SETTLE_MS - 1);
  await settle();
  assert.equal(answered, false, 'no answer before the settle window closes');
  $.clock.tick(1);
  const result = await pending;
  assert.equal(
    result.text,
    'cctop pane is open but not shown: the /diff panel holds the side dock. Run /diff to hide it and cctop takes its place (needs /tui fullscreen and 110+ columns).',
  );
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS], 'the status line says why nothing appeared');
  assert.equal(marker()?.open, true);
  assert.equal(marker()?.visibility, 'hidden');
  assert.deepEqual($.store.map.get('pane'), { open: true, view: 'overview' }, 'the pane stays open for /diff to reveal');
});

test('a pane the surface stops drawing turns hidden after HIDDEN_AFTER_MS, and a render lifts it', async () => {
  const { $, dispatch, run, marker, surface } = await boot({ columns: 162, bodyColumns: 72 });
  await run('');
  assert.deepEqual($.ui.statuses, []);

  // /diff takes the dock: from now on the engine asks for no tree. The next
  // change to the model invalidates, and the watch runs out.
  surface.hidden = true;
  $.clock.tick(1000);
  await dispatch('session.compact', { trigger: 'auto', messages: [] }, () => ({}));
  await settle();
  assert.deepEqual($.ui.statuses, [], 'not hidden before the watch runs out');
  $.clock.tick(HIDDEN_AFTER_MS);
  await settle();
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS]);
  assert.equal(marker()?.visibility, 'hidden');

  // Every further change re-arms the watch without repeating the status.
  await dispatch('session.compact', { trigger: 'auto', messages: [] }, () => ({}));
  $.clock.tick(HIDDEN_AFTER_MS + 1000);
  await settle();
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS], 'said once');

  // /diff hidden again: the dock mounts the pane and asks for its tree.
  surface.hidden = false;
  await dispatch('ui.render', paneRender('cctop', 162, { bodyColumns: 72 }));
  await settle();
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS, undefined], 'the status line is cleared');
  assert.equal(marker()?.visibility, 'visible');

  // An open while it is shown again answers docked at once.
  const again = await run('files');
  assert.match(again.text ?? '', /^cctop pane on Files docked beside the transcript \(72 columns\)/);
});

test('closing a hidden pane clears the status line and the watch', async () => {
  const { $, run, marker } = await boot({ columns: 162, hidden: true });
  const pending = run('');
  await settle();
  $.clock.tick(OPEN_SETTLE_MS);
  await pending;
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS]);
  await run('close');
  await settle();
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS, undefined]);
  assert.equal(marker()?.open, false);
  assert.equal(marker()?.visibility, 'unknown');
  $.clock.tick(HIDDEN_AFTER_MS * 2);
  await settle();
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS, undefined], 'no watch fires after the close');
  assert.equal($.clock.pending(), 0);
});

test('the marker says loaded before any open, with the session id', async () => {
  const { marker } = await boot({ columns: 162 });
  const m = marker();
  assert.equal(m?.loaded, true);
  assert.equal(m?.open, false);
  assert.equal(m?.sessionId, 'fake-session');
  assert.equal(m?.loadedAt, new Date(T0).toISOString());
});

test('the skill prompt relays the outcome, hidden included', async () => {
  const { $, dispatch } = await boot({ columns: 162, hidden: true });
  const pending = dispatch<SkillPromptResult>('skill.prompt', { skill: 'cctop', text: 'original skill text' });
  await settle();
  $.clock.tick(OPEN_SETTLE_MS);
  const result = await pending;
  assert.match(result.text ?? '', /its state is: "cctop pane is open but not shown: the \/diff panel holds the side dock\./);
  assert.deepEqual($.ui.statuses, [HIDDEN_STATUS]);
});
