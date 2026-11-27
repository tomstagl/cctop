import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { ProcessRunResult, SessionUsage } from 'claude-code';
import {
  initialNotifyState,
  nudgeActions,
  noteResult,
  noteFailure,
  resetActions,
  settledRules,
  type NudgeInput,
} from '../../plugin/hooks/notify';
import { fixture } from './fixture';
import { fakeEngine, fakeOn } from './harness';
import { register } from '../../plugin/hooks/pane';

const nudge = (urgency: NudgeInput['urgency'], fireId = '1', ruleId = 'A47'): NudgeInput => ({ ruleId, fireId, urgency, headline: '▸ Rate limit' });

test('one notification per NOW nudge instance, none for NEXT or LATER', () => {
  const s = initialNotifyState();
  assert.deepEqual(nudgeActions(s, nudge('NOW'), []).notify, [{ text: '▸ Rate limit', key: 'nudge-A47' }]);
  assert.deepEqual(nudgeActions(s, nudge('NOW'), []).notify, []);
  assert.equal(nudgeActions(s, nudge('NOW', '2'), []).notify.length, 1, 'a new fire notifies again');
  assert.deepEqual(nudgeActions(s, nudge('NEXT', '3', 'B'), []).notify, []);
  assert.deepEqual(nudgeActions(s, nudge('LATER', '4', 'C'), []).notify, []);
  assert.deepEqual(nudgeActions(s, null, []), { notify: [], cancel: [] });
});

test('a snooze or acted cancels the notification once, and the snoozed nudge is not notified', () => {
  const s = initialNotifyState();
  nudgeActions(s, nudge('NOW'), []);
  assert.deepEqual(nudgeActions(s, null, ['A47']).cancel, ['nudge-A47']);
  assert.deepEqual(nudgeActions(s, null, ['A47']).cancel, [], 'only once');
  assert.deepEqual(nudgeActions(s, nudge('NOW', '9'), ['A47']).notify, []);
  assert.deepEqual(nudgeActions(s, null, ['Z']).cancel, [], 'nothing sent for Z');
});

test('the reset is booked at 90 %, replaced on a new resets_at, never duplicated', () => {
  const s = initialNotifyState();
  const t = 1_000_000;
  assert.deepEqual(resetActions(s, [{ key: 'reset-5h', usedPct: 89, resetsAtMs: t + 5000 }], t), []);
  const first = resetActions(s, [{ key: 'reset-5h', usedPct: 90, resetsAtMs: t + 5000 }], t);
  assert.deepEqual(first, [{ text: 'rate limit resets now', key: 'reset-5h', at: t + 5000 }]);
  assert.deepEqual(resetActions(s, [{ key: 'reset-5h', usedPct: 95, resetsAtMs: t + 5000 }], t), []);
  assert.deepEqual(resetActions(s, [{ key: 'reset-5h', usedPct: 95, resetsAtMs: t + 9000 }], t).map((r) => r.at), [t + 9000]);
  // a reset in the past, or more than 7 days away, is never booked
  assert.deepEqual(resetActions(s, [{ key: 'reset-7d', usedPct: 99, resetsAtMs: t - 1 }], t), []);
  assert.deepEqual(resetActions(s, [{ key: 'reset-7d', usedPct: 99, resetsAtMs: t + 8 * 86_400_000 }], t), []);
});

test('no-channel or disabled stops every further call; the result is logged once per key', () => {
  const s = initialNotifyState();
  assert.equal(noteResult(s, 'nudge-A47', { isSent: true, channel: 'kitty' }), 'cctop: notify nudge-A47 sent (kitty)');
  assert.equal(noteResult(s, 'nudge-A47', { isSent: true, channel: 'kitty' }), null);
  assert.equal(s.disabled, false);
  assert.match(noteResult(s, 'reset-5h', { isSent: false, reason: 'no-channel' }) ?? '', /not sent \(no-channel\)/);
  assert.equal(s.disabled, true);
  assert.deepEqual(nudgeActions(s, nudge('NOW', '5'), []), { notify: [], cancel: [] });
  assert.deepEqual(resetActions(s, [{ key: 'reset-5h', usedPct: 99, resetsAtMs: 9e12 }], 0), []);
  assert.match(noteFailure(s, 'x', new Error('boom')) ?? '', /failed: boom/);
  assert.equal(noteFailure(s, 'x', new Error('boom')), null);
});

test('settledRules: snoozed ids, and acted ones only since the notified fire', () => {
  const s = initialNotifyState();
  nudgeActions(s, nudge('NOW', '100'), []);
  const coach = { snoozed: [{ id: 'S1' }], recent: [{ what: 'acted', id: 'A47', at_ms: 50 }, { what: 'fired', id: 'A47', at_ms: 150 }] };
  assert.deepEqual(settledRules(coach, s), ['S1']);
  coach.recent.push({ what: 'acted', id: 'A47', at_ms: 150 });
  assert.deepEqual(settledRules(coach, s), ['S1', 'A47']);
  assert.deepEqual(settledRules(null, s), []);
});

// The pane end to end, with a fake `$.ui.notify`.
const SESSION = 'fake-session';
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const HELP = `Commands:\n${['summary', 'dashboard', 'coach', 'tools', 'files', 'agents', 'advice', 'events'].map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
const repo = join(__dirname, '..', '..', '..');
const idle = JSON.parse(readFileSync(join(repo, 'tests', 'pane', 'fixtures', 'coach-idle.json'), 'utf8')) as { nudge: Record<string, unknown> };
const nowCoach = (cls: string, fired: number) => ({ ...idle, nudge: { ...idle.nudge, class: cls, id: 'A47', line1: '▸ Rate limit (session)', fired_at_ms: fired } });

async function boot(coach: unknown, env: Record<string, string>, usage?: SessionUsage) {
  const script: Record<string, ProcessRunResult> = {
    'cctop --version': ok('cctop 0.2.0\n'),
    'cctop query --help': ok(HELP),
    [`cctop query coach --session ${SESSION} --surface pane`]: ok(JSON.stringify(coach)),
  };
  for (const verb of ['summary', 'dashboard', 'tools', 'files', 'agents', 'advice', 'events'] as const) {
    script[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(fixture(verb)));
  }
  const $ = fakeEngine({ process: script, env, ...(usage ? { session: { usage } } : {}) });
  const { on, dispatch } = fakeOn($, { surface: { columns: 160, bodyColumns: 72 } });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({ cwd: '/home/user/project' }));
  await settle();
  await dispatch('command.run', { command: 'cctop-pane', args: 'coach', origin: { kind: 'composer' } });
  await settle();
  await settle();
  return { $, dispatch };
}

test('off without CCTOP_PANE_NOTIFY=1: a NOW nudge notifies nothing', async () => {
  const { $ } = await boot(nowCoach('NOW', 1), {});
  assert.deepEqual($.ui.notifies, []);
});

test('on: a NOW nudge notifies once, a NEXT one not at all, and a snooze cancels it', async () => {
  const { $ } = await boot(nowCoach('NOW', 1), { CCTOP_PANE_NOTIFY: '1' });
  assert.deepEqual($.ui.notifies, [{ text: '▸ Rate limit (session)', options: { title: 'cctop', key: 'nudge-A47' } }]);
  $.clock.tick(10_000);
  await settle();
  assert.equal($.ui.notifies.length, 1, 'the same fire again: nothing');
  $.process.script[`cctop query coach --session ${SESSION} --surface pane`] = ok(JSON.stringify({ ...nowCoach('NOW', 1), nudge: null, snoozed: [{ id: 'A47', family: 'x', turns_left: 5 }] }));
  $.clock.tick(10_000);
  await settle();
  await settle();
  assert.deepEqual($.ui.cancels, ['nudge-A47']);
  const next = await boot(nowCoach('NEXT', 1), { CCTOP_PANE_NOTIFY: '1' });
  assert.deepEqual(next.$.ui.notifies, []);
});

test('on: a window at 90 % books "rate limit resets now" with untilBack, once', async () => {
  const at = Date.UTC(2026, 8, 12, 15, 0, 0);
  const usage = { startedAt: 0, context: { window: 200000 }, rateLimits: [{ kind: 'five_hour', percentUsed: 93, resetsAt: new Date(at).toISOString() }] } as unknown as SessionUsage;
  const { $, dispatch } = await boot(nowCoach('LATER', 1), { CCTOP_PANE_NOTIFY: '1' }, usage);
  await dispatch('turn.start', {}, () => ({}));
  await dispatch('turn.complete', { durationMs: 1, reason: 'end_turn' }, () => ({}));
  await settle();
  await settle();
  await dispatch('turn.complete', { durationMs: 1, reason: 'end_turn' }, () => ({}));
  await settle();
  await settle();
  assert.deepEqual($.ui.notifies, [{ text: 'rate limit resets now', options: { title: 'cctop', key: 'reset-5h', at, untilBack: true } }]);
});
