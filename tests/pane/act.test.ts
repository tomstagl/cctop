import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import type { AgentInfo, ProcessRunResult, RenderElement, RenderNode } from 'claude-code';
import { fakeElements, fakeEngine, fakeOn, paneRender } from './harness';
import { body, renderToText } from './render';
import { QUERY_FIXTURES, fixture } from './fixture';
import { initialModel, reduce, type Action, type Model } from '../../plugin/hooks/model';
import { register } from '../../plugin/hooks/pane';
import { ACT_ID, ACT_ROWS, actItems, allowDiff, renderAct, selectedItem, type ActActions } from '../../plugin/hooks/views/act';
import { coachOf } from '../../plugin/hooks/views/coach';

// US-009, the act dialog (mod-pane POC 5): the items on the left, the
// selected one's detail on the right; its buttons are the coach card's.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const repo = join(__dirname, '..', '..', '..');
const presses = new Map<string, () => void>();
const el = fakeElements(presses);

function coachFixture(name: string): Record<string, unknown> & { nudge: Record<string, unknown> | null; next: Record<string, unknown> | null } {
  return JSON.parse(readFileSync(join(repo, 'tests', 'pane', 'fixtures', `coach-${name}.json`), 'utf8'));
}

const A38 = {
  id: 'A38',
  class: 'NOW',
  name: 'Failing in a loop',
  headline: '3 fails in a row: gh pr view ×2, git push',
  body: 'Claude is retrying without a fact it cannot know. Stop the turn, then say what is missing.',
  evidence: ['gh pr view · there is no PR for this branch yet'],
  fix_prompt: 'Stop retrying. Run git pull --rebase, then push; then open the PR with gh pr create --fill.',
  actions: [
    { kind: 'key', label: 'Stop the turn (Esc)', payload: 'Esc' },
    { kind: 'prompt', label: 'Put the fix prompt in the box', payload: 'Stop retrying. Run git pull --rebase, then push; then open the PR with gh pr create --fill.' },
  ],
  foot: 'goes away when a call succeeds',
  fired_at_ms: T0 - 42_000,
};
const A09 = {
  id: 'A09',
  class: 'NEXT',
  name: 'Approving the same command',
  headline: "You've approved gh api 3 times (0:45 waiting)",
  body: 'Allow it once in your settings and it stops asking.',
  evidence: [],
  fix_prompt: null,
  actions: [{ kind: 'allow_rule', label: 'Copy the rule', payload: 'Bash(gh api:*)' }],
  foot: 'goes away when the rule appears in permissions.allow',
};
const A16 = {
  id: 'A16',
  class: 'LATER',
  name: 'A hook slows every turn',
  headline: 'A hook adds 2.1 s to every turn',
  body: 'A hook runs before every tool call and Claude waits for it.',
  evidence: [],
  fix_prompt: null,
  actions: [{ kind: 'setting', label: 'Copy the setting', payload: '"async": true' }],
  foot: 'goes away when the hook is async',
};
const AGENTS = {
  id: 'A22',
  class: 'NEXT',
  name: 'Agents are waiting',
  headline: 'An agent has been running 12 minutes',
  body: 'Look at what it is doing.',
  evidence: [],
  fix_prompt: null,
  actions: [{ kind: 'key', label: 'Open the agents view (6, Enter)', payload: '6 Enter' }],
  foot: '',
};

function coach(nudge: Record<string, unknown> | null, next: Record<string, unknown> | null = null) {
  return { ...coachFixture('waiting'), nudge, next };
}
function build(c: unknown, extra: Partial<Model> = {}): Model {
  const actions: Action[] = [
    { type: 'session.start', at: T0 - 1000 },
    { type: 'binary', binary: 'present' },
    { type: 'query', verb: 'coach', data: c },
  ];
  return { ...actions.reduce(reduce, initialModel()), ...extra };
}
type Call = [string, ...unknown[]];
function recorder(): { actions: ActActions; calls: Call[] } {
  const calls: Call[] = [];
  const actions: ActActions = {
    fill: (t) => calls.push(['fill', t]),
    copy: (t, k) => calls.push(['copy', t, k]),
    openSettings: (k) => calls.push(['open', k]),
    snooze: (r) => calls.push(['snooze', r]),
    gotIt: (r) => calls.push(['gotIt', r]),
    agents: () => calls.push(['agents']),
    light: (id) => calls.push(['light', id]),
    next: (id) => calls.push(['next', id]),
    act: (id) => calls.push(['act', id]),
    select: (id) => calls.push(['select', id]),
    jump: (id) => calls.push(['jump', id]),
  };
  return { actions, calls };
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };
function find(node: RenderNode | undefined, type: string, out: Node[] = []): Node[] {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return out;
  const n = node as Node;
  if (n.type === type) out.push(n);
  for (const c of n.children ?? []) find(c, type, out);
  return out;
}
const draw = (m: Model, columns: number, actions: ActActions = recorder().actions) => {
  const tree = renderAct(m, el, columns, actions);
  return { tree, rows: renderToText(tree, columns) };
};
const flat = (rows: string[]) => rows.join('\n');

test('nothing under plugin/ stops a turn: no $.turn.abort, no turn.abort at all', () => {
  const hits: string[] = [];
  const walk = (dir: string): void => {
    for (const name of readdirSync(dir)) {
      if (name === 'node_modules' || name === '.claude') continue;
      const path = join(dir, name);
      if (statSync(path).isDirectory()) walk(path);
      else if (!name.endsWith('.d.ts') && /\.(ts|tsx|js|json|md)$/.test(name) && /turn\s*\.\s*abort|\babort\s*\(/.test(readFileSync(path, 'utf8'))) hits.push(path);
    }
  };
  walk(join(repo, 'plugin'));
  assert.deepEqual(hits, []);
});

test('the items are the nudge then the queued one; the selection falls back to the first', () => {
  const c = coachOf(coach(A38, A09));
  const items = actItems(c);
  assert.deepEqual(items.map((i) => i.id), ['A38', 'A09']);
  assert.equal(selectedItem(items, build(coach(A38, A09), { actSel: 'A09' }))?.id, 'A09');
  assert.equal(selectedItem(items, build(coach(A38, A09), { actSel: 'gone' }))?.id, 'A38');
  assert.deepEqual(actItems(null), []);
  assert.equal(selectedItem([], initialModel()), null);
});

test('a key-kind item shows "Press Esc to stop the turn" first, before its words and never as a Button', () => {
  const { rows, tree } = draw(build(coach(A38, A09)), 100);
  const inner = rows.map((r) => r.trim());
  const step = inner.findIndex((r) => r.includes('1  Press Esc to stop the turn'));
  const headline = inner.findIndex((r) => r.includes(A38.headline));
  assert.ok(step >= 0 && headline > step, JSON.stringify(inner));
  assert.ok(flat(rows).includes('Esc closes this dialog first'), flat(rows));
  assert.ok(!find(tree, 'Button').some((b) => /stop|esc/i.test(String(b.props?.label)) && String(b.props?.key).startsWith('coach-act-')), 'no stop Button');
  // The fill Button keeps its digit after the step's 1.
  assert.ok(find(tree, 'Button').some((b) => b.props?.hotkey === '2' && b.props?.label === 'Put the fix prompt in the box'));
});

test('the list is on the left and the detail on the right at 100 columns; stacked below 72', () => {
  const side = draw(build(coach(A38, A09)), 100).rows;
  assert.match(side[0] ?? '', /cctop · act\s+2 open · Tab and ↑↓ walk · Esc closes · toasts wait/);
  const first = side.find((r) => r.includes('NOW  Failing in a loop'));
  assert.ok(first !== undefined && first.includes('Press Esc to stop the turn'), JSON.stringify(side));
  assert.ok(side.some((r) => /^NEXT Approving/.test(r.trim()) || r.includes('NEXT Approving')), JSON.stringify(side));
  const stacked = draw(build(coach(A38, A09)), 60).rows;
  const listAt = stacked.findIndex((r) => r.includes('NEXT Approving'));
  const detailAt = stacked.findIndex((r) => r.includes('Press Esc to stop the turn'));
  assert.ok(listAt >= 0 && detailAt > listAt, JSON.stringify(stacked));
  for (const r of stacked) assert.ok([...r].length <= 60, r);
});

test('an allow rule shows its settings diff as Code format="diff", the rule, and the copy/open/permissions Buttons', () => {
  const { tree, rows } = draw(build(coach(A09), { actSel: 'A09' }), 100);
  const code = find(tree, 'Code');
  assert.equal(code.length, 1);
  assert.equal(code[0]?.props?.format, 'diff');
  assert.equal(code[0]?.props?.source, allowDiff('Bash(gh api:*)'));
  assert.match(String(code[0]?.props?.source), /^\+ {6}"Bash\(gh api:\*\)",$/m);
  assert.ok(!/@@|^ /m.test(String(code[0]?.props?.source)), 'no invented hunk header or context');
  assert.equal(allowDiff('Bash(gh api:*)').split('\n').length, 3, 'two file headers and the one line');
  assert.ok(flat(rows).replace(/\s+/g, ' ').includes("the line to add under permissions.allow — cctop doesn't read your file"), flat(rows));
  assert.ok(flat(rows).replace(/\s+/g, ' ').includes('never edits your settings'), flat(rows));
  const labels = find(tree, 'Button').map((b) => `${b.props?.hotkey ?? '-'} ${b.props?.label}`);
  assert.ok(labels.includes('1 Copy the rule') && labels.includes('2 Open settings.local.json') && labels.includes('3 Put /permissions in the box'), JSON.stringify(labels));
  assert.ok(labels.some((l) => l.endsWith('Snooze')));
});

test('a setting shows the line to add; a fix prompt shows its exact text, a press that fills', () => {
  const s = draw(build(coach(A16), { actSel: 'A16' }), 100);
  const code = find(s.tree, 'Code');
  assert.equal(code[0]?.props?.source, '"async": true');
  assert.notEqual(code[0]?.props?.format, 'diff');
  const r = recorder();
  const f = draw(build(coach(A38)), 100, r.actions);
  assert.ok(flat(f.rows).includes('The exact text'), flat(f.rows));
  assert.ok(flat(f.rows).includes('↵ Stop retrying. Run git pull --rebase'), flat(f.rows));
  const keys = find(f.tree, 'Button').map((b) => String(b.props?.key)).filter((k) => k.startsWith('act-text-'));
  assert.ok(keys.length >= 1);
  presses.get(keys[0] as string)?.();
  assert.deepEqual(r.calls, [['fill', A38.fix_prompt]]);
  // The same text under the fix Button is shown once.
  assert.equal(flat(f.rows).split('↵ Stop retrying.').length, 2);
});

test('when the dialog does not hold the keyboard it claims no Esc or walking keys', () => {
  const rows = renderToText(renderAct(build(coach(A38)), el, 100, recorder().actions, false), 100);
  assert.ok(!flat(rows).includes('Esc closes'), flat(rows));
  assert.ok(!flat(rows).includes('Esc closes this dialog first'), flat(rows));
  assert.ok(flat(rows).includes('1  Press Esc to stop the turn'), 'the step stays');
});

test('the list presses select; the selected row is not a Button', () => {
  const r = recorder();
  const { tree } = draw(build(coach(A38, A09)), 100, r.actions);
  const keys = find(tree, 'Button').map((b) => String(b.props?.key));
  assert.ok(!keys.includes('act-pick-A38'));
  presses.get('act-pick-A09')?.();
  assert.deepEqual(r.calls, [['select', 'A09']]);
});

test('with no nudge, or no binary, the dialog says so', () => {
  assert.ok(flat(draw(build(coach(null)), 80).rows).includes('nothing to act on'));
  assert.ok(flat(draw(build(coach(A38), { binary: 'missing' }), 80).rows).includes('needs the cctop binary'));
});

// ------------------------------------------------------------ end to end
const SESSION = 'fake-session';
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const HELP = `Commands:\n${['summary', 'dashboard', 'coach', 'tools', 'files', 'agents', 'advice', 'events'].map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function boot(c: unknown, agents?: unknown) {
  const script: Record<string, ProcessRunResult> = {
    'cctop --version': ok('cctop 0.2.0\n'),
    'cctop query --help': ok(HELP),
    [`cctop query coach --session ${SESSION} --surface pane`]: ok(JSON.stringify(c)),
  };
  for (const verb of QUERY_FIXTURES) {
    if (verb === 'coach') continue;
    script[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(verb === 'agents' && agents !== undefined ? agents : fixture(verb)));
  }
  const $ = fakeEngine({ process: script, env: { HOME: '/home/user' }, session: { info: { transcriptPath: '', platform: 'macos' } } });
  const { on, dispatch } = fakeOn($, { surface: { columns: 160, bodyColumns: 72 } });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({ cwd: '/home/user/project' }));
  await settle();
  await dispatch('command.run', { command: 'cctop-pane', args: 'coach', origin: { kind: 'composer' } });
  await settle();
  await settle();
  const pane = () => dispatch<RenderElement>('ui.render', paneRender('cctop', 72, { placement: 'dock' }));
  const dialog = () => dispatch<RenderElement>('ui.render', paneRender(ACT_ID, 100, { placement: 'dock' })).then((tree) => ({ tree, rows: renderToText(tree, 100) }));
  await pane();
  return { $, dispatch, pane, dialog };
}

test('/clear closes an open act dialog: its nudges were the old session\'s', async () => {
  const { $, dispatch, pane } = await boot(coach(A38, A09));
  $.ui.press('coach-now-act');
  await settle();
  assert.deepEqual($.ui.closes, []);
  $.session.scripted.id = 'rotated-session';
  for (const verb of QUERY_FIXTURES) $.process.script[`cctop query ${verb} --session rotated-session --surface pane`] = ok(JSON.stringify(fixture(verb)));
  await dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  await settle();
  await settle();
  assert.deepEqual($.ui.closes, [{ id: ACT_ID }]);
  await pane();
});

test('`a` or a press on the nudge opens the dialog with focus, Esc, held toasts and its rows', async () => {
  const { $, pane } = await boot(coach(A38, A09));
  await pane();
  $.ui.press('coach-now-act');
  await settle();
  assert.deepEqual($.ui.opens.filter((o) => o.id === ACT_ID), [{ id: ACT_ID, title: 'cctop · act', focus: true, closeOnEscape: true, holdToasts: true, rows: ACT_ROWS }]);
  const act = find(await pane(), 'Button').find((b) => b.props?.key === 'coach-now-act');
  assert.equal(act?.props?.hotkey, 'a');
  $.ui.press('coach-now-headline-0');
  await settle();
  assert.equal($.ui.opens.filter((o) => o.id === ACT_ID).length, 2, 'a press on the headline opens it too');
});

test('the dialog renders through ui.render for its own id, the pane is untouched, and a press selects', async () => {
  const { $, dialog, pane } = await boot(coach(A38, A09));
  $.ui.press('coach-now-act');
  await settle();
  const { rows } = await dialog();
  assert.ok(flat(rows).includes('Press Esc to stop the turn'), flat(rows));
  assert.ok(body(await pane().then((t) => renderToText(t, 72))).length > 0);
  $.ui.press('act-pick-A09');
  await settle();
  const after = await dialog();
  assert.ok(flat(after.rows).includes('Allow it once in your settings'), flat(after.rows));
  assert.ok(find((await dialog()).tree, 'Code').length === 1);
});

test('copy stays in the dialog and says so there (toasts are held); open settings never writes', async () => {
  const { $, dialog } = await boot(coach(A09));
  $.ui.press('coach-now-act');
  await settle();
  await dialog();
  $.ui.press('coach-act-0-copy');
  await settle();
  assert.deepEqual($.ui.copies, ['Bash(gh api:*)']);
  const project = '/home/user/project/.claude/settings.local.json';
  const note = `cctop: copied Bash(gh api:*). Paste it under permissions.allow in ${project}`;
  const shown = flat((await dialog()).rows).replace(/\s+/g, ' ');
  assert.ok(shown.includes(note), shown);
  assert.deepEqual($.ui.closes, [], 'a copy does not close the dialog');
  assert.deepEqual($.ui.toasts, [], 'no toast while the dialog is open: held toasts would reappear after it closes');
  $.process.script[`open ${project}`] = ok('');
  $.ui.press('coach-act-0-open');
  await settle();
  await settle();
  assert.ok($.process.calls.some((a) => a.join(' ') === `open ${project}`));
  assert.ok(![...$.fs.files.keys()].some((p) => p.endsWith('settings.json') || p.endsWith('settings.local.json')));
});

test('a fill hands the keyboard back first, then drafts into an empty box and never sends; a draft is kept', async () => {
  const { $, dialog } = await boot(coach(A38, A09));
  $.ui.press('coach-now-act');
  await settle();
  await dialog();
  $.ui.press('act-text-0-0');
  await settle();
  await settle();
  assert.deepEqual($.ui.closes, [{ id: ACT_ID }]);
  assert.deepEqual($.prompt.fills, [A38.fix_prompt]);
  // With a draft, the dialog closes and the draft stays.
  $.prompt.fills.length = 0;
  $.prompt.draft = 'half a sentence';
  $.ui.press('coach-now-act');
  await settle();
  await dialog();
  $.ui.press('act-text-0-0');
  await settle();
  await settle();
  assert.deepEqual($.prompt.fills, []);
  assert.equal($.prompt.draft, 'half a sentence');
  assert.ok($.ui.toasts.some((t) => t.includes('Your draft is kept')), JSON.stringify($.ui.toasts));
});

test('Esc (the engine closing the dialog) ends it; opening again redraws', async () => {
  const { $, dispatch, dialog } = await boot(coach(A38));
  $.ui.press('coach-now-act');
  await settle();
  await dispatch('ui.close', { id: ACT_ID, origin: 'person' }, () => ({}));
  $.ui.press('coach-now-act');
  await settle();
  assert.equal($.ui.opens.filter((o) => o.id === ACT_ID).length, 2);
  assert.ok(flat((await dialog()).rows).includes('Press Esc to stop the turn'));
});

test('an item with the agents key can jump to a running agent from the dialog (PRD 6)', async () => {
  const agents = fixture<{ agents: Array<Record<string, unknown>> }>('agents');
  const row = { ...agents.agents[0], state: 'running' } as unknown as { id: string; type: string; description: string };
  const { $, dispatch, dialog } = await boot(coach(AGENTS), { ...agents, agents: [row, ...agents.agents.slice(1)] });
  $.agent.live.push({ id: row.id, description: row.description, type: row.type, status: 'running', kind: 'local_agent' } as AgentInfo);
  await dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  await settle();
  $.ui.press('coach-now-act');
  await settle();
  const { tree, rows } = await dialog();
  assert.ok(flat(rows).includes('Jump to agent'), flat(rows));
  assert.ok(find(tree, 'Button').some((b) => b.props?.label === 'Open the agents view'));
  $.ui.press(`act-jump-${row.id}`);
  await settle();
  await settle();
  assert.deepEqual($.ui.views, [{ agentId: row.id }]);
  assert.deepEqual($.ui.closes, [{ id: ACT_ID }], 'the dialog hands the keys back before the jump');
});
