import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { ProcessRunResult, RenderElement } from 'claude-code';
import { coachOf, type CoachActions } from '../../plugin/hooks/views/coach';
import { missPlacements, nudgePlacement, placedTree } from '../../plugin/hooks/views/placed';
import { fixture } from './fixture';
import { fakeElements, fakeEngine, fakeOn, placedRender } from './harness';
import { renderToText } from './render';
import { register } from '../../plugin/hooks/pane';

// US-010: notes placed in the transcript where they happened.
const repo = join(__dirname, '..', '..', '..');
const SESSION = 'fake-session';
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const HELP = `Commands:\n${['summary', 'dashboard', 'coach', 'tools', 'files', 'agents', 'advice', 'events'].map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
const idle = JSON.parse(readFileSync(join(repo, 'tests', 'pane', 'fixtures', 'coach-idle.json'), 'utf8')) as { nudge: Record<string, unknown> };
const nowCoach = (cls: string, fired: number, extra: Record<string, unknown> = {}) => ({ ...idle, nudge: { ...idle.nudge, class: cls, id: 'A47', headline: 'Rate limit', fired_at_ms: fired, ...extra } });
const miss = (turn: number, kind = 'tools_changed', detail = 'a new MCP tool loaded', tokens = 112_000) => ({ turn, at: 't', message_id: 'm', tokens: { value: tokens, unit: 'tokens' }, api_reason: null, located: { kind, detail } });
const summaryWith = (misses: unknown[]) => {
  const s = JSON.parse(JSON.stringify(fixture('summary'))) as { cache: Record<string, unknown> };
  s.cache.located_misses = misses;
  return s;
};

const calls: string[][] = [];
const actions: CoachActions = {
  fill: (t) => calls.push(['fill', t]),
  copy: (t, k) => calls.push(['copy', t, k]),
  openSettings: (k) => calls.push(['open', k]),
  snooze: (r) => calls.push(['snooze', r]),
  gotIt: (r) => calls.push(['gotIt', r]),
  agents: () => calls.push(['agents']),
  light: () => {},
  next: () => {},
  act: () => {},
};
const presses = new Map<string, () => void>();
const el = fakeElements(presses);

test('a NOW nudge places nudge-<fire>; NEXT and fireless ones place nothing', () => {
  assert.equal(nudgePlacement(coachOf(nowCoach('NOW', 77)))?.id, 'nudge-77');
  assert.equal(nudgePlacement(coachOf(nowCoach('NEXT', 77))), null);
  assert.equal(nudgePlacement(coachOf({ ...nowCoach('NOW', 1), nudge: { ...nowCoach('NOW', 1).nudge, fired_at_ms: undefined } })), null);
  assert.equal(nudgePlacement(coachOf({ ...idle, nudge: null })), null);
});

test('the card is bordered: the headline, one body line and the two main buttons', () => {
  const coach = coachOf(nowCoach('NOW', 77, { body: 'word '.repeat(60), actions: [{ kind: 'prompt', label: 'Draft the fix', payload: 'fix it' }] }));
  const placed = nudgePlacement(coach)!;
  presses.clear();
  const lines = renderToText(placedTree(placed.row, coach, 80, el, actions), 80);
  assert.equal(lines.length, 5, lines.join('\n'));
  assert.ok(/^╭/.test(lines[0] ?? '') && /^╰/.test(lines[4] ?? ''));
  assert.ok((lines[1] ?? '').includes('NOW') && (lines[1] ?? '').includes('Rate limit'));
  assert.ok((lines[2] ?? '').startsWith('│ word word'));
  assert.ok((lines[3] ?? '').includes('Draft the fix') && (lines[3] ?? '').includes('Snooze'));
  assert.deepEqual([...presses.keys()], ['coach-placed-77-0-fill', 'coach-placed-77-snooze']);
  calls.length = 0;
  presses.get('coach-placed-77-0-fill')?.();
  presses.get('coach-placed-77-snooze')?.();
  assert.deepEqual(calls, [['fill', 'fix it'], ['snooze', 'A47']]);
});

test('a card whose nudge left the slot is one settled dim line with no buttons', () => {
  const coach = coachOf(nowCoach('NOW', 77))!;
  const placed = nudgePlacement(coach)!;
  presses.clear();
  for (const now of [coachOf({ ...idle, nudge: null }), coachOf(nowCoach('NOW', 78))]) {
    const lines = renderToText(placedTree(placed.row, now, 80, el, actions), 80);
    assert.equal(lines.length, 1);
    assert.ok((lines[0] ?? '').includes('no longer current'), lines[0]);
  }
  assert.equal(presses.size, 0);
});

test('a located miss is one dim row with the cause and what it re-wrote; an unlocated one is skipped', () => {
  const rows = missPlacements(summaryWith([miss(4), miss(5, 'unlocated', 'x'), miss(6, 'expired', 'cache expired', 900)]));
  assert.deepEqual(rows.map((r) => r.id), ['miss-4', 'miss-6']);
  const lines = renderToText(placedTree(rows[0]!.row, null, 100, el, actions), 100);
  assert.deepEqual(lines, ['cctop  cache miss here: a new MCP tool loaded, 112k tokens re-written']);
  assert.deepEqual(missPlacements({}), []);
});

async function boot(coach: unknown, summary: unknown) {
  const script: Record<string, ProcessRunResult> = {
    'cctop --version': ok('cctop 0.2.0\n'),
    'cctop query --help': ok(HELP),
    [`cctop query coach --session ${SESSION} --surface pane`]: ok(JSON.stringify(coach)),
    [`cctop query summary --session ${SESSION} --surface pane`]: ok(JSON.stringify(summary)),
  };
  for (const verb of ['dashboard', 'tools', 'files', 'agents', 'advice', 'events'] as const) script[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(fixture(verb)));
  const $ = fakeEngine({ process: script, env: { HOME: '/home/user' } });
  const { on, dispatch } = fakeOn($, { surface: { columns: 160, bodyColumns: 72 } });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({ cwd: '/home/user/project' }));
  await settle();
  await dispatch('command.run', { command: 'cctop-pane', args: 'coach', origin: { kind: 'composer' } });
  await settle();
  await settle();
  const set = (verb: string, data: unknown) => {
    $.process.script[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(data));
  };
  const poll = async () => {
    $.clock.tick(15_000);
    await settle();
    await settle();
  };
  const draw = (id: string) => dispatch<RenderElement>('ui.render', placedRender(id, 100));
  return { $, dispatch, set, poll, draw };
}

test('the pane places a NOW nudge once, a miss after the first summary once, and not the history', async () => {
  const { $, set, poll } = await boot(nowCoach('NOW', 5), summaryWith([miss(3)]));
  assert.deepEqual($.ui.places, ['nudge-5'], 'the history miss is not replayed');
  await poll();
  assert.deepEqual($.ui.places, ['nudge-5'], 'once');
  set('summary', summaryWith([miss(3), miss(4)]));
  await poll();
  assert.deepEqual($.ui.places, ['nudge-5', 'miss-4']);
  await poll();
  assert.deepEqual($.ui.places, ['nudge-5', 'miss-4']);
});

test('a note that arrives mid-turn is placed at the turn end', async () => {
  const { $, dispatch, set, poll } = await boot({ ...idle, nudge: null }, summaryWith([]));
  await dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  set('summary', summaryWith([miss(7)]));
  set('coach', nowCoach('NOW', 9));
  await poll();
  assert.deepEqual($.ui.places, []);
  await dispatch('turn.complete', { answer: 'ok', durationMs: 1, isAborted: false, turnId: 't1', reason: 'answer' }, () => ({ text: 'ok' }));
  assert.deepEqual($.ui.places, ['nudge-9', 'miss-7']);
});

test('the placed row draws through ui.render, and settles once the nudge is gone; another plugin\'s row is left alone', async () => {
  const { $, set, poll, draw, dispatch } = await boot(nowCoach('NOW', 5, { body: 'one line' }), summaryWith([]));
  const card = renderToText(await draw('nudge-5'), 100);
  assert.ok(card.some((l) => l.includes('Rate limit')) && /^╭/.test(card[0] ?? ''), card.join('\n'));
  set('coach', { ...idle, nudge: null });
  await poll();
  assert.ok(renderToText(await draw('nudge-5'), 100).join('').includes('no longer current'));
  const other = await dispatch<RenderElement | undefined>('ui.render', placedRender('nudge-5', 100, 'someone-else'));
  assert.equal(other, undefined);
  assert.deepEqual($.ui.places, ['nudge-5'], 'nothing placed again');
});

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    return statSync(p).isDirectory() ? sources(p) : /\.tsx?$/.test(f) ? [p] : [];
  });
}

test('display only: nothing in the plugin writes to the transcript', () => {
  for (const f of sources(join(repo, 'plugin', 'hooks'))) {
    const text = readFileSync(f, 'utf8');
    assert.ok(!/session\.append|\.append\(/.test(text), f);
  }
});
