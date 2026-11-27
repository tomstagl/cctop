import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { CommandRunResult, ProcessRunResult, RenderElement, RenderNode } from 'claude-code';
import { fakeElements, fakeEngine, fakeOn, paneRender } from './harness';
import { body, renderToText } from './render';
import { fixture } from './fixture';
import { initialModel, reduce, type Action, type Model } from '../../plugin/hooks/model';
import { register } from '../../plugin/hooks/pane';
import {
  DRAFT_KEPT,
  FILLED_FIX,
  coachOf,
  compositionCells,
  copiedText,
  filledText,
  highestLight,
  keyStep,
  lightText,
  openerFor,
  renderCoach,
  settingsPath,
  settingsScope,
  statusLine,
  type CoachActions,
} from '../../plugin/hooks/views/coach';
import { clip } from '../../plugin/hooks/views/frame';

// The Coach view (mod-pane POC 1) against the six moments of fixture B
// (tests/pane/fixtures/coach-<moment>.json, the objects `cctop query coach`
// computes and the TUI snapshots draw) and nudges shaped from them: the NOW
// card carries the nudge's full words, a fix-prompt card a click puts in the
// box, steps and Buttons per action kind; "Up next" and the lights open in
// place. Every press goes through CoachActions; the end-to-end tests at the
// bottom check what those do through `$`.
const T0 = Date.UTC(2026, 8, 12, 12, 0, 0);
const MOMENTS = ['explore', 'edits', 'denials', 'waiting', 'cold', 'idle'] as const;
const repo = join(__dirname, '..', '..', '..');
const presses = new Map<string, () => void>();
const el = fakeElements(presses);

type Call = [string, ...unknown[]];
function recorder(): { actions: CoachActions; calls: Call[] } {
  const calls: Call[] = [];
  const actions: CoachActions = {
    fill: (t) => calls.push(['fill', t]),
    copy: (t, k) => calls.push(['copy', t, k]),
    openSettings: (k) => calls.push(['open', k]),
    snooze: (r) => calls.push(['snooze', r]),
    gotIt: (r) => calls.push(['gotIt', r]),
    agents: () => calls.push(['agents']),
    light: (id) => calls.push(['light', id]),
    next: (id) => calls.push(['next', id]),
    act: (id) => calls.push(["act", id]),
  };
  return { actions, calls };
}
const noActions = recorder().actions;

type Raw = Record<string, unknown> & { nudge: Record<string, unknown> | null; next: Record<string, unknown> | null };
function moment(name: (typeof MOMENTS)[number]): Raw {
  return JSON.parse(readFileSync(join(repo, 'tests', 'pane', 'fixtures', `coach-${name}.json`), 'utf8')) as Raw;
}

/** A moment with its nudge replaced by `nudge` (over the waiting moment's A41). */
function withNudge(nudge: Record<string, unknown>, name: (typeof MOMENTS)[number] = 'waiting'): Raw {
  const raw = moment(name);
  return { ...raw, nudge: { ...raw.nudge, ...nudge } };
}

function build(coach: unknown, extra: Partial<Model> = {}, dashboard?: unknown): Model {
  const actions: Action[] = [
    { type: 'session.start', at: T0 - 1000 },
    { type: 'binary', binary: 'present' },
    { type: 'query', verb: 'coach', data: coach },
  ];
  if (dashboard !== undefined) actions.push({ type: 'query', verb: 'dashboard', data: dashboard });
  return { ...actions.reduce(reduce, initialModel()), view: 'coach', ...extra };
}

function draw(model: Model, columns: number, actions: CoachActions = noActions, now = T0): { tree: RenderElement; rows: string[] } {
  const tree = renderCoach(model, el, columns, actions, now);
  return { tree, rows: renderToText(tree, columns) };
}

/** The rows' text inside the frames, joined into one line with single spaces: what a wrapped string must appear in whole. */
function flat(rows: string[]): string {
  return body(rows).join(' ').replace(/\s+/g, ' ');
}
const norm = (s: string) => s.replace(/\s+/g, ' ').trim();

/** The rows of the NOW card frame alone (the first frame). */
function card(rows: string[]): string[] {
  const end = rows.findIndex((r) => r.startsWith('╰'));
  return rows.slice(0, end + 1);
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };
function buttons(node: RenderNode | undefined, out: Node[] = []): Node[] {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return out;
  const n = node as Node;
  if (n.type === 'Button') out.push(n);
  for (const child of n.children ?? []) buttons(child, out);
  return out;
}
function press(tree: RenderElement, key: string): void {
  const b = buttons(tree).find((x) => x.props?.key === key);
  assert.ok(b !== undefined, `no Button ${key}: ${JSON.stringify(buttons(tree).map((x) => x.props?.key))}`);
  const fn = presses.get(key);
  assert.ok(fn !== undefined, key);
  fn();
}
const labels = (tree: RenderElement, prefix: string) =>
  buttons(tree)
    .filter((b) => String(b.props?.key).startsWith(prefix) && !/^coach-now-(act|headline-)/.test(String(b.props?.key)))
    .map((b) => `${b.props?.hotkey ?? '-'} ${b.props?.label}`);

// A38 as Task 1's test words it: a key, then the fix prompt.
const A38 = {
  id: 'A38',
  class: 'NOW',
  name: 'Claude is stuck retrying',
  headline: '3 fails in a row: gh pr view ×2, git push',
  body: "Claude keeps trying without a fact it can't find out by itself: the remote has commits you don't and there is no PR for this branch yet.",
  evidence: ['gh pr view · there is no PR for this branch yet', 'git push · the remote has commits you don\'t'],
  fix_prompt: "Stop retrying. The remote has commits you don't and there is no PR for this branch yet. Run git pull --rebase, then push; then open the PR with gh pr create --fill.",
  actions: [
    { kind: 'key', label: 'Stop the turn (Esc)', payload: 'Esc' },
    { kind: 'prompt', label: 'Put the fix prompt in the box', payload: "Stop retrying. The remote has commits you don't and there is no PR for this branch yet. Run git pull --rebase, then push; then open the PR with gh pr create --fill." },
  ],
  foot: 'goes away when a call succeeds',
  fired_at_ms: T0 - 42_000,
};

// A09 with traces (PRD 7): the split by who decided is an evidence row.
const A09 = {
  id: 'A09',
  class: 'NEXT',
  name: 'Approving the same command',
  headline: "You've approved gh api 3 times (0:45 waiting)",
  body: 'Claude stopped and waited for you 3 times on gh api, 45 seconds in all. Allow it once in your settings and it stops asking. The rule: Bash(gh api:*)',
  evidence: ['5 permission prompts this session; the model idles while it waits', 'who decided the 5 permission waits: you 3 · rule 1 · classifier 1'],
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

test('every moment parses; the state line and the light rows are the object\'s strings; the card fits 56 columns', () => {
  for (const name of MOMENTS) {
    const raw = moment(name);
    const c = coachOf(raw);
    assert.ok(c !== null, name);
    const { rows } = draw(build(raw), 56);
    for (const row of rows) assert.ok([...row].length <= 56, `${name}: ${JSON.stringify(row)}`);
    const inner = body(rows);
    assert.equal(inner[0], c.stateLine, name);
    for (const l of c.lights) {
      const text = clip(`${l.glyph} ${l.id.padEnd(8)} ${l.text}`, 52);
      // The 52 cells whole; the chevron where there is room for it.
      assert.ok(inner.some((r) => r === text || (r.startsWith(text) && /^ +[›▾]$/.test(r.slice([...text].length)))), `${name} light ${l.id}: ${JSON.stringify(inner)}`);
    }
    if (c.nudge === null) assert.ok(inner.includes(c.quietRow), `${name}: ${JSON.stringify(inner)}`);
    assert.ok(norm(rows.join(' ')).endsWith(c.footer), `${name}: the footer`);
  }
  const waiting = coachOf(moment('waiting'))!;
  assert.equal(waiting.stateLine, '◆ WAITING 4:00 · Claude asked a question');
  assert.ok(waiting.footer.startsWith('spent so far ') && waiting.footer.endsWith(' · what this session would cost at API prices'), waiting.footer);
  const cold = coachOf(moment('cold'))!;
  assert.equal(cold.lights[1].text, 'cold · 193k re-write');
  assert.equal(highestLight(cold), 'context');
  assert.equal(highestLight(coachOf(moment('denials'))!), 'rework');
});

test('the NOW card shows the name, the age, the rule id, the headline, the body, the evidence, the fix prompt and the foot', () => {
  const raw = withNudge(A38);
  const { rows } = draw(build(raw), 72);
  const now = card(rows);
  const inner = body(now);
  // The head row: the class badge, the plain name, the age and the rule id at the right.
  const head = inner.find((r) => r.includes('Claude is stuck retrying'));
  assert.ok(head !== undefined && head.startsWith(' NOW  Claude is stuck retrying') && head.endsWith('42s ago · rule A38'), JSON.stringify(inner));
  const text = flat(now);
  for (const s of [A38.headline, A38.body, ...A38.evidence, A38.fix_prompt, A38.foot]) assert.ok(text.includes(norm(s)), `${s}\n${text}`);
  assert.ok(text.includes('Fix it with this prompt · click to put it in your prompt box'), text);
  // The card is a press on every line: the first starts with ↵.
  assert.ok(inner.some((r) => r.startsWith('↵ Stop retrying.')), JSON.stringify(inner));
  // A nudge without a fire time still names its rule.
  const unfired = draw(build(withNudge({ ...A38, fired_at_ms: null })), 72).rows;
  assert.ok(body(unfired).some((r) => r.endsWith('rule A38')), JSON.stringify(unfired));
});

test('clicking the fix-prompt card fills it, and once it took a line says nothing was sent', () => {
  const { actions, calls } = recorder();
  const raw = withNudge(A38);
  const { tree, rows } = draw(build(raw), 72, actions);
  const fixKeys = buttons(tree)
    .map((b) => String(b.props?.key))
    .filter((k) => k.startsWith('coach-now-fix-'));
  assert.ok(fixKeys.length >= 2, `every line of the card presses: ${JSON.stringify(fixKeys)}`);
  for (const k of fixKeys) press(tree, k);
  assert.deepEqual(calls, fixKeys.map(() => ['fill', A38.fix_prompt]));
  assert.ok(!flat(rows).includes("It's in your prompt box"), 'no confirmation before a fill');
  const after = draw(build(raw, { coachFilled: A38.fix_prompt }), 72).rows;
  const text = flat(after);
  assert.ok(text.includes(norm(FILLED_FIX)), text);
  assert.equal(text.split("It's in your prompt box").length, 2, 'said once, under the card');
  assert.match(FILLED_FIX, /Nothing has been sent/);
});

test('a key action is the step "press Esc to stop the turn", shown first and never a Button; then the fill and snooze Buttons', () => {
  const { actions, calls } = recorder();
  const { tree, rows } = draw(build(withNudge(A38)), 72, actions);
  const inner = body(card(rows));
  const step = inner.findIndex((r) => r === '1  Press Esc to stop the turn');
  assert.ok(step > 0, JSON.stringify(inner));
  const buttonRow = inner.findIndex((r) => r.startsWith('2: Put the fix prompt in the box'));
  assert.ok(buttonRow > step, 'the step comes before the Buttons');
  assert.deepEqual(labels(tree, 'coach-now-'), [
    ...buttons(tree)
      .filter((b) => String(b.props?.key).startsWith('coach-now-fix-'))
      .map((b) => `- ${b.props?.label}`),
    '2 Put the fix prompt in the box',
    '3 Snooze',
  ]);
  assert.ok(!buttons(tree).some((b) => /^coach-now-\d/.test(String(b.props?.key)) && /stop|esc/i.test(String(b.props?.label))), 'no stop Button');
  press(tree, 'coach-now-1-fill');
  press(tree, 'coach-now-snooze');
  assert.deepEqual(calls, [
    ['fill', A38.fix_prompt],
    ['snooze', 'A38'],
  ]);
  assert.equal(keyStep({ kind: 'key', label: 'Rewind (Esc Esc)', payload: 'Esc Esc' }), 'Press Esc twice to rewind');
  assert.equal(keyStep({ kind: 'key', label: 'Compact now (ctrl+k)', payload: 'ctrl+k' }), 'Press ctrl+k to compact now');
});

test('a slash action fills the command; a tip is "Got it"; an agents key opens the Agents view', () => {
  const idle = moment('idle');
  const slash = (idle.nudge!.actions as { kind: string; payload: string; label: string }[]).find((a) => a.kind === 'slash')!;
  const { actions, calls } = recorder();
  const { tree } = draw(build(idle), 72, actions);
  const fill = buttons(tree).find((b) => b.props?.label === slash.label);
  assert.ok(fill !== undefined, JSON.stringify(labels(tree, 'coach-now-')));
  press(tree, String(fill.props?.key));
  assert.deepEqual(calls, [['fill', slash.payload]]);
  assert.equal(slash.payload, '/compact ', 'the payload as sent: a space for a focus');
  // The waiting moment's A41 is a tip: Got it (quiet for the session), Snooze.
  const r2 = recorder();
  const t2 = draw(build(moment('waiting')), 72, r2.actions).tree;
  assert.deepEqual(labels(t2, 'coach-now-'), ['1 Got it', '2 Snooze']);
  press(t2, 'coach-now-0-gotit');
  assert.deepEqual(r2.calls, [['gotIt', 'A41']]);
  const r3 = recorder();
  const t3 = draw(build(withNudge({ actions: [{ kind: 'key', label: 'Open the agents view (6, Enter)', payload: '6 Enter' }] })), 72, r3.actions).tree;
  press(t3, 'coach-now-0-agents');
  assert.deepEqual(r3.calls, [['agents']]);
});

test('an allow rule copies, opens settings.json and fills /permissions; the card shows who decided the waits (PRD 7)', () => {
  const { actions, calls } = recorder();
  const { tree, rows } = draw(build(withNudge(A09)), 72, actions);
  assert.deepEqual(labels(tree, 'coach-now-'), ['1 Copy the rule', '2 Open settings.local.json', '3 Put /permissions in the box', '4 Snooze']);
  press(tree, 'coach-now-0-copy');
  press(tree, 'coach-now-0-open');
  press(tree, 'coach-now-0-permissions');
  assert.deepEqual(calls, [
    ['copy', 'Bash(gh api:*)', 'allow_rule'],
    ['open', 'allow_rule'],
    ['fill', '/permissions'],
  ]);
  const text = flat(card(rows));
  assert.ok(text.includes('· who decided the 5 permission waits: you 3 · rule 1 · classifier 1'), text);
  assert.ok(body(card(rows)).some((r) => r.startsWith(' NEXT  Approving the same command')), 'the class badge');
  // The fill took: its own line, naming the command.
  const after = flat(draw(build(withNudge(A09), { coachFilled: '/permissions' }), 72).rows);
  assert.ok(after.includes('✓ /permissions is in your prompt box. Nothing has been sent: press Enter to run it.'), after);
  // A setting copies and opens, nothing more.
  const r2 = recorder();
  const t2 = draw(build(withNudge(A16)), 72, r2.actions).tree;
  assert.deepEqual(labels(t2, 'coach-now-'), ['1 Copy the setting', '2 Open settings.json', '3 Snooze']);
  press(t2, 'coach-now-0-copy');
  press(t2, 'coach-now-0-open');
  assert.deepEqual(r2.calls, [
    ['copy', '"async": true', 'setting'],
    ['open', 'setting'],
  ]);
});

test('an MCP elicitation is a NOW card naming the server (PRD 5)', () => {
  // A41's words for an open elicitation (src/advisor/rules/outcome.rs a41_names_the_mcp_server_that_is_asking).
  const raw = withNudge({
    headline: '◆ WAITING 0:45 · MCP linear is asking you',
    body: 'MCP linear asked you something 45 seconds ago, and nothing moves until you answer. A one-word reply is enough; /btw asks a side question first if you need a detail.',
    fired_at_ms: T0 - 45_000,
  });
  const inner = body(card(draw(build(raw), 59).rows));
  assert.ok(inner.some((r) => r.startsWith(' NOW  Claude is waiting for you') && r.endsWith('45s ago · rule A41')), JSON.stringify(inner));
  assert.ok(inner.includes('◆ WAITING 0:45 · MCP linear is asking you'), JSON.stringify(inner));
});

test('at 59 columns (Review Focus 4): nothing overflows and nothing in the card is cut mid-word', () => {
  const cases: [string, Raw][] = [
    ...MOMENTS.map((m): [string, Raw] => [m, moment(m)]),
    ['A38', withNudge(A38)],
    ['A09', withNudge(A09)],
    ['A16', withNudge(A16)],
  ];
  for (const [name, raw] of cases) {
    const c = coachOf(raw)!;
    for (const extra of [{}, { coachNext: c.next?.id ?? null, coachLight: 'context' as const }]) {
      const { rows } = draw(build(raw, extra, fixture('dashboard-b')), 59);
      for (const row of rows) assert.ok([...row].length <= 59, `${name}: ${JSON.stringify(row)}`);
      const n = c.nudge;
      if (n === null) continue;
      const text = flat(card(rows));
      for (const s of [n.name, n.headline, n.body, ...n.evidence, n.fixPrompt ?? '', n.foot]) assert.ok(text.includes(norm(s)), `${name}: ${s}\n${text}`);
      assert.ok(!body(card(rows)).slice(2).some((r) => r.includes('…') && !n.headline.includes('…')), `${name}: a cut in the card`);
    }
  }
});

test('"Up next" opens in place: its words, its fix prompt and its Buttons, which take no digit', () => {
  const raw = moment('waiting');
  const next = raw.next!;
  const { actions, calls } = recorder();
  const closed = draw(build(raw), 72, actions);
  const text = flat(closed.rows);
  assert.ok(closed.rows.some((r) => r.startsWith('╭Up next ─ click a row to open it')), JSON.stringify(closed.rows));
  assert.ok(body(closed.rows).some((r) => r.startsWith('NOW   Claude keeps needing correction') && r.endsWith('›')), JSON.stringify(body(closed.rows)));
  assert.ok(text.includes(norm(String(next.headline))), 'the headline under it');
  assert.ok(!text.includes(norm(String(next.body))), 'closed: no body');
  press(closed.tree, 'coach-next-A36-0');
  press(closed.tree, 'coach-next-A36-1');
  assert.deepEqual(calls, [
    ['next', 'A36'],
    ['next', 'A36'],
  ]);
  const r2 = recorder();
  const open = draw(build(raw, { coachNext: 'A36' }), 72, r2.actions);
  const otext = flat(open.rows);
  for (const s of [next.body, next.fix_prompt, next.foot, ...(next.evidence as string[])]) assert.ok(otext.includes(norm(String(s))), `${s}\n${otext}`);
  assert.ok(body(open.rows).some((r) => r.startsWith('NOW   Claude keeps needing correction') && r.endsWith('▾')));
  // Its own numbering, its step first; its Buttons take no digit, so the
  // card's 1 Got it and 2 Snooze stay where they are.
  assert.ok(body(open.rows).includes('1  Press Esc twice to rewind'), JSON.stringify(body(open.rows)));
  const nextButtons = buttons(open.tree).filter((b) => /^coach-next-A36-(\d+-|snooze)/.test(String(b.props?.key)));
  assert.deepEqual(
    nextButtons.map((b) => `${b.props?.hotkey ?? '-'} ${b.props?.label}`),
    ['- Put the fix prompt in the box', '- Snooze'],
  );
  assert.deepEqual(labels(open.tree, 'coach-now-'), ['1 Got it', '2 Snooze']);
  press(open.tree, 'coach-next-A36-fix-0');
  press(open.tree, 'coach-next-A36-snooze');
  assert.deepEqual(r2.calls, [
    ['fill', next.fix_prompt],
    ['snooze', 'A36'],
  ]);
  // Nothing queued: one dim row.
  assert.ok(flat(draw(build({ ...raw, next: null }), 72).rows).includes('nothing else queued'));
});

test('the lights open in place: the highest by default, a press opens another or closes the open one', () => {
  const raw = moment('denials');
  const c = coachOf(raw)!;
  const rework = c.lights.find((l) => l.id === 'rework')!;
  const { actions, calls } = recorder();
  const d = draw(build(raw), 72, actions);
  const text = flat(d.rows);
  for (const l of rework.lines) assert.ok(text.includes(norm(l)), `the act light is open: ${l}`);
  const cache = c.lights.find((l) => l.id === 'cache')!;
  for (const l of cache.lines) assert.ok(!text.includes(norm(l)), 'the others are closed');
  press(d.tree, 'coach-light-cache');
  press(d.tree, 'coach-light-rework');
  assert.deepEqual(calls, [
    ['light', 'cache'],
    ['light', 'none'],
  ]);
  const shut = flat(draw(build(raw, { coachLight: 'none' }), 72).rows);
  for (const l of rework.lines) assert.ok(!shut.includes(norm(l)), 'all closed');
  const other = flat(draw(build(raw, { coachLight: 'cache' }), 72).rows);
  for (const l of cache.lines) assert.ok(other.includes(norm(l)), l);
  assert.equal(lightText(c.lights[0], 48), c.lights[0].text.split(' · ')[0], 'below 50 columns the context light keeps the tokens and the bar');
});

test('the context light opens into the composition (PRD 3), labelled with its age, and what put it there', () => {
  const raw = moment('cold');
  const dashB = fixture('dashboard-b');
  const rows = draw(build(raw, { coachLight: 'context' }, dashB), 72).rows;
  const inner = body(rows);
  const text = flat(rows);
  assert.ok(text.includes('what fills the window · per /context, 2m ago'), text);
  // One bar row as wide as the detail, then the legend in the TUI's token format.
  const bar = inner.find((r) => /^ {2}[█▓░▒]+$/.test(r));
  assert.ok(bar !== undefined && [...bar].length === 72 - 4, JSON.stringify(inner));
  for (const entry of ['System prompt 10k', 'System tools 31k', 'Skills 4.8k', 'Messages 42k', 'free 877k', 'buffer 33k']) assert.ok(text.includes(entry), `${entry}\n${text}`);
  // The residency rows of the context body, as the TUI draws them.
  assert.ok(inner.includes('  since model switch (−58k) · 3 calls'), JSON.stringify(inner));
  assert.ok(inner.some((r) => /^ {2}other +≈92k +64%$/.test(r)), JSON.stringify(inner));
  // Without a capture (fixture A): no composition, the residency stays.
  const a = flat(draw(build(raw, { coachLight: 'context' }, fixture('dashboard')), 72).rows);
  assert.ok(!a.includes('what fills the window'), a);
  assert.ok(a.includes('since session start · 143 calls'), a);
  // No dashboard yet: the light's own lines only.
  const none = flat(draw(build(raw, { coachLight: 'context' }), 72).rows);
  for (const l of coachOf(raw)!.lights[0].lines) assert.ok(none.includes(norm(l)), l);
});

test('the pure parts: settings paths, the opener, the bar cells, the fill lines', () => {
  assert.equal(settingsScope('allow_rule'), 'project');
  assert.equal(settingsScope('setting'), 'user');
  assert.equal(settingsPath('project', '/home/user/project/', '/home/user'), '/home/user/project/.claude/settings.local.json');
  assert.equal(settingsPath('user', '/home/user/project', '/home/user'), '/home/user/.claude/settings.json');
  assert.equal(openerFor('macos'), 'open');
  assert.equal(openerFor('linux'), 'xdg-open');
  assert.equal(openerFor('wsl'), 'xdg-open');
  assert.equal(openerFor('windows'), null);
  assert.equal(openerFor(undefined), null);
  assert.equal(copiedText('Bash(gh api:*)', 'allow_rule', '/p/.claude/settings.json'), 'cctop: copied Bash(gh api:*). Paste it under permissions.allow in /p/.claude/settings.json');
  assert.equal(filledText('/compact '), '✓ /compact is in your prompt box. Nothing has been sent: press Enter to run it.');
  assert.equal(filledText('Do the thing.'), FILLED_FIX);
  const parts = [
    { name: 'System prompt', tokens: 10_200 },
    { name: 'Skills', tokens: 900 },
    { name: 'Messages', tokens: 42_900 },
    { name: 'free', tokens: 877_900 },
    { name: 'buffer', tokens: 0 },
  ];
  const cells = compositionCells(parts, 68);
  assert.equal(cells.reduce((a, b) => a + b, 0), 68);
  assert.equal(cells[1], 1, 'a small part keeps one cell');
  assert.equal(cells[4], 0, 'an empty part none');
  assert.deepEqual(compositionCells([], 10), []);
});

// The pane end to end: the Coach tab, the status line, the toast, the presses through `$`.
const SESSION = 'fake-session';
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const HELP = `Commands:\n${['summary', 'dashboard', 'coach', 'tools', 'files', 'agents', 'advice', 'events'].map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function boot(coach: unknown, platform: string | null = 'macos', env: Record<string, string> = { HOME: '/home/user' }) {
  const script: Record<string, ProcessRunResult> = {
    'cctop --version': ok('cctop 0.2.0\n'),
    'cctop query --help': ok(HELP),
    [`cctop query coach --session ${SESSION} --surface pane`]: ok(JSON.stringify(coach)),
  };
  for (const verb of ['summary', 'dashboard', 'tools', 'files', 'agents', 'advice', 'events'] as const) {
    script[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(fixture(verb)));
  }
  const $ = fakeEngine({
    process: script,
    env,
    session: { info: { transcriptPath: '', ...(platform === null ? {} : { platform: platform as 'macos' }) } },
  });
  const { on, dispatch } = fakeOn($, { surface: { columns: 160, bodyColumns: 72 } });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({ cwd: '/home/user/project' }));
  await settle();
  const run = (args: string) => dispatch<CommandRunResult>('command.run', { command: 'cctop-pane', args, origin: { kind: 'composer' } });
  const render = (columns: number) =>
    dispatch<RenderElement>('ui.render', paneRender('cctop', columns, { placement: 'dock' })).then((tree) => ({ tree, rows: renderToText(tree, columns) }));
  await run('coach');
  await settle();
  await settle();
  return { $, dispatch, render };
}

test('the Coach tab: the status line once per change, the fix card fills and never sends, the line says so', async () => {
  const raw = withNudge(A38);
  const { $, render, dispatch } = await boot(raw);
  const { rows } = await render(72);
  assert.ok(rows[0].startsWith('cctop  Coach  Overview  Tools'), rows[0]);
  const c = coachOf(raw)!;
  assert.deepEqual(
    $.ui.statuses.filter((s): s is string => typeof s === 'string'),
    [statusLine(c, 72)],
  );
  await render(72);
  assert.equal($.ui.statuses.filter((s) => typeof s === 'string').length, 1);
  $.ui.press('coach-now-fix-0');
  await settle();
  assert.deepEqual($.prompt.fills, [A38.fix_prompt]);
  const after = await render(72);
  assert.ok(flat(after.rows).includes(norm(FILLED_FIX)), JSON.stringify(after.rows));
  // The next turn (the person pressed Enter): the line goes.
  $.prompt.draft = '';
  await dispatch('turn.start', {}, () => ({}));
  assert.ok(!flat((await render(72)).rows).includes("It's in your prompt box"));
  // A busy box: a toast, and no line saying it is there.
  $.prompt.draft = '';
  $.prompt.filled = false;
  $.ui.press('coach-now-1-fill');
  await settle();
  assert.ok($.ui.toasts.includes('cctop: the prompt box is busy — the action was not filled'), JSON.stringify($.ui.toasts));
  assert.ok(!flat((await render(72)).rows).includes("It's in your prompt box"));
});

test('copy and open settings.json go through $.ui.copy and `open`/`xdg-open`; nothing writes the file', async () => {
  const { $ } = await boot(withNudge(A09));
  const project = '/home/user/project/.claude/settings.local.json';
  $.ui.press('coach-now-0-copy');
  await settle();
  assert.deepEqual($.ui.copies, ['Bash(gh api:*)']);
  assert.ok($.ui.toasts.includes(`cctop: copied Bash(gh api:*). Paste it under permissions.allow in ${project}`), JSON.stringify($.ui.toasts));
  $.process.script[`open ${project}`] = ok('');
  $.ui.press('coach-now-0-open');
  await settle();
  await settle();
  assert.ok($.process.calls.some((a) => a.join(' ') === `open ${project}`), JSON.stringify($.process.calls));
  assert.ok($.ui.toasts.includes(`cctop: opened ${project} in your editor. cctop never edits it`), JSON.stringify($.ui.toasts));
  $.ui.press('coach-now-0-permissions');
  await settle();
  assert.deepEqual($.prompt.fills, ['/permissions']);
  assert.ok(![...$.fs.files.keys()].some((p) => p.endsWith('settings.json')), 'no settings.json written');
  // A copy the surface refused says why.
  $.ui.copyAnswer.current = { isCopied: false, reason: 'no-clipboard' };
  $.ui.press('coach-now-0-copy');
  await settle();
  assert.ok($.ui.toasts.includes('cctop: nothing was copied (no-clipboard)'), JSON.stringify($.ui.toasts));
  // A setting: the person's own settings.json, xdg-open on Linux.
  const linux = await boot(withNudge(A16), 'linux');
  const user = '/home/user/.claude/settings.json';
  linux.$.process.script[`xdg-open ${user}`] = ok('');
  linux.$.ui.press('coach-now-0-open');
  await settle();
  await settle();
  assert.ok(linux.$.process.calls.some((a) => a.join(' ') === `xdg-open ${user}`), JSON.stringify(linux.$.process.calls));
  // A platform cctop has no opener for: it says where the file is.
  const other = await boot(withNudge(A16), null);
  other.$.ui.press('coach-now-0-open');
  await settle();
  await settle();
  assert.ok(other.$.ui.toasts.includes(`cctop: open ${user} in your editor`), JSON.stringify(other.$.ui.toasts));
  assert.ok(!other.$.process.calls.some((a) => a[0] === 'open' || a[0] === 'xdg-open'));
  // HOME unset: nothing is handed `~/…` (no shell expands it); the toast names the file.
  const homeless = await boot(withNudge(A16), 'macos', {});
  homeless.$.ui.press('coach-now-0-open');
  await settle();
  await settle();
  assert.ok(homeless.$.ui.toasts.includes('cctop: HOME is not set: open ~/.claude/settings.json in your editor'), JSON.stringify(homeless.$.ui.toasts));
  assert.ok(!homeless.$.process.calls.some((a) => a[0] === 'open' || a[0] === 'xdg-open'));
  homeless.$.ui.press('coach-now-0-copy');
  await settle();
  assert.ok(homeless.$.ui.toasts.includes('cctop: copied "async": true. Paste it into ~/.claude/settings.json'), JSON.stringify(homeless.$.ui.toasts));
});

test('a fill never replaces a draft: only an empty box is filled; a draft or an unreadable box keeps it', async () => {
  const raw = withNudge(A38);
  const { $, render } = await boot(raw);
  await render(72);
  // A draft in the box: no fill call, the toast, the card unchanged.
  $.prompt.draft = 'half a sentence I am typ';
  const before = (await render(72)).rows;
  $.ui.press('coach-now-fix-0');
  await settle();
  await settle();
  assert.deepEqual($.prompt.fills, [], 'no fill over a draft');
  assert.ok($.ui.toasts.includes(DRAFT_KEPT), JSON.stringify($.ui.toasts));
  assert.equal(DRAFT_KEPT, 'cctop: Your draft is kept — clear the prompt box, then press again');
  assert.equal($.prompt.draft, 'half a sentence I am typ');
  assert.deepEqual((await render(72)).rows, before, 'the card is unchanged');
  // A box that cannot be read: refused the same way.
  $.prompt.draft = '';
  $.prompt.readFails = true;
  $.ui.press('coach-now-1-fill');
  await settle();
  await settle();
  assert.deepEqual($.prompt.fills, []);
  assert.equal($.ui.toasts.filter((t) => t === DRAFT_KEPT).length, 2);
  // An empty box: filled, and the card says nothing has been sent.
  $.prompt.readFails = false;
  $.ui.press('coach-now-fix-0');
  await settle();
  await settle();
  assert.deepEqual($.prompt.fills, [A38.fix_prompt]);
  assert.ok(flat((await render(72)).rows).includes(norm(FILLED_FIX)));
  // The person empties the box: the next poll sees it and the line goes, before any turn.
  $.prompt.draft = '';
  $.clock.tick(10_000);
  await settle();
  await settle();
  assert.ok(!flat((await render(72)).rows).includes("It's in your prompt box"));
});

test('the "is in your prompt box" line is said once, even when two cards fill the same command', () => {
  const raw = withNudge(A09);
  const twin = { ...A09, id: 'A99', class: 'LATER', name: 'Another rule to allow' };
  const both = { ...raw, next: { ...twin, promotes: 'next prompt', row: 'next     A99' } };
  const text = flat(draw(build(both, { coachNext: 'A99', coachFilled: '/permissions' }), 72).rows);
  assert.equal(text.split('is in your prompt box').length, 2, text);
});

test('snooze and Got it ask the binary and take its answer; the lights and Up next switch in place', async () => {
  const raw = moment('waiting');
  const { $, render } = await boot(raw);
  $.process.script[`cctop query coach --snooze-session A41 --session ${SESSION} --surface pane`] = ok(JSON.stringify({ ...raw, nudge: null, snooze: 'A41 quiet for the session' }));
  $.ui.press('coach-now-0-gotit');
  await settle();
  await settle();
  assert.ok($.ui.toasts.includes('cctop: A41 quiet for the session'), JSON.stringify($.ui.toasts));
  const quiet = await render(72);
  assert.ok(quiet.rows.some((r) => r.includes('quiet · nothing to act on')), JSON.stringify(quiet.rows));
  $.ui.press('coach-next-A36-0');
  await settle();
  const open = await render(72);
  assert.ok(flat(open.rows).includes(norm(String(raw.next!.body))), 'Up next opened in place');
  $.ui.press('coach-light-cache');
  await settle();
  const cache = await render(72);
  const lines = coachOf(raw)!.lights.find((l) => l.id === 'cache')!.lines;
  for (const l of lines) assert.ok(flat(cache.rows).includes(norm(l)), l);
  $.process.script[`cctop query coach --snooze A36 --session ${SESSION} --surface pane`] = ok(JSON.stringify({ ...raw, nudge: null, next: null, snooze: 'A36 snoozed for 5 turns' }));
  $.ui.press('coach-next-A36-snooze');
  await settle();
  await settle();
  assert.ok($.process.calls.some((a) => a.includes('--snooze') && a.includes('A36')), JSON.stringify($.process.calls));
});

test('a NOW nudge taking the slot raises one toast per fire and at most one per turn', async () => {
  const raw = moment('idle');
  const now = { ...raw, nudge: { ...raw.nudge, class: 'NOW', id: 'A47', line1: '▸ Rate limit (session) · resets 22:20', fired_at_ms: 1 } };
  const { $, dispatch } = await boot(now);
  assert.deepEqual($.ui.toasts, ['▸ Rate limit (session) · resets 22:20']);
  $.clock.tick(10_000);
  await settle();
  assert.equal($.ui.toasts.length, 1);
  $.process.script[`cctop query coach --session ${SESSION} --surface pane`] = ok(JSON.stringify({ ...now, nudge: { ...now.nudge, fired_at_ms: 2 } }));
  $.clock.tick(10_000);
  await settle();
  await settle();
  assert.equal($.ui.toasts.length, 1, 'one toast per turn');
  await dispatch('turn.start', {}, () => ({}));
  $.clock.tick(2_000);
  await settle();
  await settle();
  assert.equal($.ui.toasts.length, 2, JSON.stringify($.ui.toasts));
});
