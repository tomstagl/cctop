import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { CommandRunResult, ProcessRunResult, RenderElement, RenderNode } from 'claude-code';
import { fakeElements, fakeEngine, fakeOn, paneRender, textOf, type FakeEngine } from './harness';
import { body, renderToText } from './render';
import { fixture } from './fixture';
import { initialModel, reduce, type Action, type Model } from '../../plugin/hooks/model';
import { register } from '../../plugin/hooks/pane';
import { coachOf, fillable, highestLight, lightText, renderCoach, statusLine, type CoachActions } from '../../plugin/hooks/views/coach';
import { clip } from '../../plugin/hooks/views/frame';

// The Coach view against the six moments of fixture B
// (tests/pane/fixtures/coach-<moment>.json, the objects `cctop query coach`
// computes and the TUI snapshots draw): the card's rows are the object's
// strings, the Buttons act through `$`, the status line and the toast follow
// the object. At 56 columns the frame is the TUI's 56-column card.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const MOMENTS = ['explore', 'edits', 'denials', 'waiting', 'cold', 'idle'] as const;
const repo = join(__dirname, '..', '..', '..');
const el = fakeElements(new Map());
const noActions: CoachActions = { fill: () => {}, snooze: () => {}, why: () => {}, light: () => {} };

function moment(name: (typeof MOMENTS)[number]): unknown {
  return JSON.parse(readFileSync(join(repo, 'tests', 'pane', 'fixtures', `coach-${name}.json`), 'utf8')) as unknown;
}

function build(coach: unknown, extra: Partial<Model> = {}): Model {
  const actions: Action[] = [
    { type: 'session.start', at: T0 - 1000 },
    { type: 'binary', binary: 'present' },
    { type: 'query', verb: 'coach', data: coach },
  ];
  return { ...actions.reduce(reduce, initialModel()), view: 'coach', ...extra };
}

function rows(model: Model, columns: number): string[] {
  return renderToText(renderCoach(model, el, columns, noActions), columns);
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };
function buttons(node: RenderNode | undefined, out: Node[] = []): Node[] {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return out;
  const n = node as Node;
  if (n.type === 'Button') out.push(n);
  for (const child of n.children ?? []) buttons(child, out);
  return out;
}

test('every moment parses and the 56-column card is the object, row for row', () => {
  for (const name of MOMENTS) {
    const raw = moment(name);
    const c = coachOf(raw);
    assert.ok(c !== null, name);
    const lines = rows(build(raw), 56);
    for (const row of lines) assert.ok([...row].length <= 56, `${name}: ${JSON.stringify(row)}`);
    const inner = body(lines);
    // The state line, the four light rows, then (after the rule) the slot.
    assert.equal(inner[0], c.stateLine, name);
    c.lights.forEach((l, i) => assert.equal(inner[2 + i], clip(`${l.glyph} ${l.id.padEnd(8)} ${l.text}`, 52), `${name} light ${l.id}`));
    if (c.nudge !== null) {
      assert.equal(inner[7], c.nudge.line1, name);
      assert.equal(inner[8], c.nudge.line2, name);
      assert.equal(inner[9], c.nudge.evidence, name);
    } else {
      assert.equal(inner[7], c.quietRow, name);
    }
    assert.ok(inner.includes(c.nextRow) && inner.includes(c.snoozedRow), `${name}: ${JSON.stringify(inner)}`);
  }
  // The rendered object matches what the Rust side asserts about it.
  const waiting = coachOf(moment('waiting'))!;
  assert.equal(waiting.stateLine, '◆ WAITING 4:00 · Claude asked a question');
  assert.equal(waiting.stateKind, '◆ WAITING');
  const cold = coachOf(moment('cold'))!;
  assert.equal(cold.lights[1].text, 'cold · 193k re-write');
  assert.equal(cold.lights[1].level, 'watch');
  assert.equal(highestLight(cold), 'context');
  assert.equal(highestLight(coachOf(moment('denials'))!), 'rework');
});

test('a light row keeps two figures below 50 columns, the detail frame follows the highest light, why replaces it', () => {
  const raw = moment('denials');
  const c = coachOf(raw)!;
  const context = c.lights[0];
  assert.equal(lightText(context, 72), context.text);
  assert.equal(lightText(context, 48), context.text.split(' · ')[0], 'context keeps the tokens and the bar');
  const rework = c.lights[3];
  assert.equal(lightText(rework, 48), rework.text.split(' · ').slice(0, 2).join(' · '));
  const wide = rows(build(raw), 72);
  assert.ok(wide.some((r) => r.includes('● rework')), JSON.stringify(wide));
  assert.ok(wide.some((r) => r.includes('last check')), 'the rework detail frame follows the act light');
  const narrow = rows(build(raw), 48);
  for (const row of narrow) assert.ok([...row].length <= 48, JSON.stringify(row));
  assert.ok(!narrow.some((r) => r.includes('≈$.37/call')), 'the third figure is dropped');
  const why = rows(build(raw, { coachWhy: true }), 72);
  assert.ok(why.some((r) => r.startsWith('╭why')), JSON.stringify(why));
  assert.ok(why.some((r) => r.includes('retires') && r.includes('turn end')), JSON.stringify(why));
  const cache = rows(build(raw, { coachLight: 'cache' }), 72);
  assert.ok(cache.some((r) => r.startsWith('╭○ cache')), JSON.stringify(cache));
});

test('the status line follows the width, the fill button exists for prompt-class actions only', () => {
  const c = coachOf(moment('idle'))!;
  assert.equal(statusLine(c, 80), c.lines.l0);
  assert.equal(statusLine(c, 60), c.lines.l1);
  assert.equal(statusLine(c, 30), c.lines.l2);
  assert.ok(c.lines.l0.startsWith('●333k'), c.lines.l0);
  assert.equal(c.lines.l2.length, 4);
  // A23 is a slash-class nudge (`/compact `): fillable. A17 at the cold
  // moment is settings-class: no fill.
  assert.equal(fillable(c.nudge), true);
  const cold = coachOf(moment('cold'))!;
  assert.equal(cold.nudge?.id, 'A17');
  assert.equal(fillable(cold.nudge), false);
  const tree = renderCoach(build(moment('cold')), el, 72, noActions);
  assert.deepEqual(
    buttons(tree).map((b) => b.props?.label),
    ['snooze', 'why', '◐ cache', '○ limits', '◐ rework'],
  );
  // A prompt-class nudge gets `[1 fill]` with the exact action text.
  const raw = moment('idle') as { nudge: Record<string, unknown> };
  const prompt = { ...raw, nudge: { ...raw.nudge, action_kind: 'prompt', action_text: 'Use an Explore subagent for the rest.' } };
  const fills: string[] = [];
  const tree2 = renderCoach(build(prompt), el, 72, { ...noActions, fill: (t) => fills.push(t) });
  const fill = buttons(tree2).find((b) => b.props?.label === 'fill');
  assert.ok(fill !== undefined && fill.props?.hotkey === '1', JSON.stringify(buttons(tree2).map((b) => b.props)));
});

// The pane end to end: the Coach tab, the status line, the toast, the presses.
const SESSION = 'fake-session';
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '' });
const HELP = `Commands:\n${['summary', 'dashboard', 'coach', 'tools', 'files', 'agents', 'advice', 'events'].map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function boot(coach: unknown) {
  const script: Record<string, ProcessRunResult> = {
    'cctop --version': ok('cctop 0.2.0\n'),
    'cctop query --help': ok(HELP),
    [`cctop query coach --session ${SESSION} --surface pane`]: ok(JSON.stringify(coach)),
  };
  for (const verb of ['summary', 'dashboard', 'tools', 'files', 'agents', 'advice', 'events'] as const) {
    script[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(fixture(verb)));
  }
  const $ = fakeEngine({ process: script });
  const { on, dispatch } = fakeOn($, { surface: { columns: 160, bodyColumns: 72 } });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({ cwd: '/home/user/project' }));
  await settle();
  const run = (args: string) => dispatch<CommandRunResult>('command.run', { command: 'cctop-pane', args, origin: { kind: 'composer' } });
  const render = (columns: number) =>
    dispatch<RenderElement>('ui.render', paneRender('cctop', columns, { placement: 'dock' })).then((tree) => ({ tree, rows: renderToText(tree, columns) }));
  return { $, dispatch, run, render };
}

test('the Coach tab draws the card, sets the status line once per change and fills the prompt on press', async () => {
  const raw = moment('idle') as { nudge: Record<string, unknown> };
  const prompt = { ...raw, nudge: { ...raw.nudge, action_kind: 'prompt', action_text: 'Use an Explore subagent for the rest.' } };
  const { $, run, render } = await boot(prompt);
  await run('coach');
  await settle();
  await settle();
  const { tree, rows: lines } = await render(72);
  assert.ok(lines[0].startsWith('cctop  Coach  [Overview]  [Tools]'), lines[0]);
  assert.ok(lines.some((r) => r.includes('IDLE 4m · python3 - lor… ok 8m ago')), JSON.stringify(lines));
  // The status line under the prompt is the L0 form at 72 body columns → L1 (< 80).
  const c = coachOf(prompt)!;
  const statuses = $.ui.statuses.filter((s): s is string => typeof s === 'string');
  assert.deepEqual(statuses, [statusLine(c, 72)]);
  // Another render with the same object sets nothing again.
  await render(72);
  assert.equal($.ui.statuses.filter((s) => typeof s === 'string').length, 1);
  // `[1 fill]` writes the action into the prompt box and never submits.
  assert.ok(buttons(tree).some((b) => b.props?.label === 'fill' && b.props?.hotkey === '1'));
  $.ui.press('coach-fill');
  await settle();
  assert.deepEqual($.prompt.fills, ['Use an Explore subagent for the rest.']);
  // `[2 snooze]` asks the binary and takes its answer as the new object.
  $.process.script[`cctop query coach --snooze A23 --session ${SESSION} --surface pane`] = ok(JSON.stringify({ ...prompt, nudge: null, snooze: 'A23 snoozed for 5 turns' }));
  $.ui.press('coach-snooze');
  await settle();
  await settle();
  assert.ok($.process.calls.some((argv) => argv.includes('--snooze') && argv.includes('A23')), JSON.stringify($.process.calls));
  assert.ok($.ui.toasts.includes('cctop: A23 snoozed for 5 turns'), JSON.stringify($.ui.toasts));
  const after = await render(72);
  assert.ok(after.rows.some((r) => r.includes('quiet · nothing to act on')), JSON.stringify(after.rows));
  // `[3 why]` needs a nudge; the light pickers switch the detail frame.
  $.ui.press('coach-light-cache');
  await settle();
  const cache = await render(72);
  assert.ok(cache.rows.some((r) => r.startsWith('╭○ cache')), JSON.stringify(cache.rows));
});

test('a NOW nudge taking the slot raises one toast per fire and at most one per turn', async () => {
  const raw = moment('idle') as { nudge: Record<string, unknown> };
  const now = { ...raw, nudge: { ...raw.nudge, class: 'NOW', id: 'A47', line1: '▸ Rate limit (session) · resets 22:20', fired_at_ms: 1 } };
  const { $, run, dispatch } = await boot(now);
  await run('coach');
  await settle();
  await settle();
  assert.deepEqual($.ui.toasts, ['▸ Rate limit (session) · resets 22:20']);
  // The same fire again: no second toast. A new fire in the same turn: none either.
  $.clock.tick(10_000);
  await settle();
  assert.equal($.ui.toasts.length, 1);
  $.process.script[`cctop query coach --session ${SESSION} --surface pane`] = ok(JSON.stringify({ ...now, nudge: { ...now.nudge, fired_at_ms: 2 } }));
  $.clock.tick(10_000);
  await settle();
  await settle();
  assert.equal($.ui.toasts.length, 1, 'one toast per turn');
  // The next turn: the new fire toasts.
  await dispatch('turn.start', {}, () => ({}));
  $.clock.tick(2_000);
  await settle();
  await settle();
  assert.equal($.ui.toasts.length, 2, JSON.stringify($.ui.toasts));
});
