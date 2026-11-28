import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { AgentInfo, ProcessRunResult, RenderElement, RenderNode } from 'claude-code';
import { fakeEngine, fakeOn, paneRender, type ProcessScript } from './harness';
import { renderToText } from './render';
import { QUERY_FIXTURES, fixture } from './fixture';
import { register } from '../../plugin/hooks/pane';

// US-003 through the harness: an agent row's Button puts the agent's
// transcript in the main view (`$.ui.view`), a refusal is one toast with its
// reason, and `back to main` shows while an agent is in view and goes back.
const SESSION = 'fake-session';
const HELP = `Commands:\n${['summary', 'tools', 'files', 'agents', 'advice', 'events'].map((v) => `  ${v}  x`).join('\n')}\n\nOptions:\n`;
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));
const agentRow = (fixture<{ agents: Array<{ id: string; type: string; description: string }> }>('agents')).agents[0];

function live(id: string): AgentInfo {
  return { id, description: agentRow.description, type: agentRow.type, status: 'running', kind: 'local_agent' } as AgentInfo;
}

async function boot() {
  const process: Record<string, ProcessScript> = { 'cctop --version': ok('cctop 0.9.0\n'), 'cctop query --help': ok(HELP) };
  for (const verb of QUERY_FIXTURES) process[`cctop query ${verb} --session ${SESSION} --surface pane`] = ok(JSON.stringify(fixture(verb)));
  const $ = fakeEngine({ env: { HOME: '/home/user' }, process });
  const { on, dispatch } = fakeOn($, { surface: { columns: 160, bodyColumns: 72 } });
  register(on, {});
  await dispatch('session.start', { cwd: '/home/user/project', surface: 'terminal', isInteractive: true }, () => ({ cwd: '/home/user/project' }));
  await settle();
  await dispatch('command.run', { command: 'cctop-pane', args: 'agents', origin: { kind: 'composer' } });
  await settle();
  const render = (view?: { agentId?: string }) =>
    dispatch<RenderElement>('ui.render', paneRender('cctop', 80, { view })).then((tree) => renderToText(tree, 80));
  return { $, dispatch, render };
}

type Node = { type: string; props?: Record<string, unknown>; children?: RenderNode[] };
function labels(node: RenderNode | undefined, out: string[] = []): string[] {
  if (node === undefined || typeof node === 'string' || node.type === 'engine') return out;
  const n = node as Node;
  if (n.type === 'Button') out.push(String(n.props?.label));
  for (const c of n.children ?? []) labels(c, out);
  return out;
}

test('pressing an agent row views its transcript; back to main returns', async () => {
  const { $, dispatch, render } = await boot();
  $.agent.live.push(live(agentRow.id));
  await dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  await settle();
  await render();
  assert.ok(!(await render()).some((r) => r.includes('back to main')), 'main view: no way back drawn');
  $.ui.press(`agent-${agentRow.id}`);
  await settle();
  assert.deepEqual($.ui.views, [{ agentId: agentRow.id }]);
  assert.deepEqual($.ui.toasts, []);
  // The surface re-renders with the view in force.
  const tree = await dispatch<RenderElement>('ui.render', paneRender('cctop', 80, { view: { agentId: agentRow.id } }));
  assert.ok(labels(tree).includes('back to main'));
  assert.ok(renderToText(tree, 80).some((r) => r.includes('back to main')));
  $.ui.press('back-to-main');
  await settle();
  assert.deepEqual($.ui.views, [{ agentId: agentRow.id }, {}]);
  const back = await render({});
  assert.ok(!back.some((r) => r.includes('back to main')));
});

test('a refused view is one toast with the reason, and no way back is drawn', async () => {
  const { $, dispatch, render } = await boot();
  $.agent.live.push(live(agentRow.id));
  await dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  await settle();
  await render();
  $.ui.viewAnswer.deny = 'the agent runs in a pane of its own';
  $.ui.press(`agent-${agentRow.id}`);
  await settle();
  assert.deepEqual($.ui.toasts, ['cctop: the agent runs in a pane of its own']);
  assert.ok(!(await render()).some((r) => r.includes('back to main')));
});

test('a row the engine does not list is no Button', async () => {
  const { $, render } = await boot();
  await render();
  assert.ok(!$.ui.presses.has(`agent-${agentRow.id}`));
  assert.deepEqual($.ui.views, []);
});

test('a running row that appears mid-turn reads $.agent.list once; the same row on the next poll does not', async () => {
  const { $, dispatch, render } = await boot();
  let reads = 0;
  const list = $.agent.list;
  $.agent.list = async () => {
    reads++;
    return list();
  };
  await dispatch('turn.start', { text: 'hi', turnId: 't1' }, () => ({ turnId: 't1' }));
  await settle();
  const before = reads;
  // The agent starts after the turn did: the engine lists it, cctop's rows show it running.
  $.agent.live.push(live(agentRow.id));
  const agents = fixture<{ agents: Array<Record<string, unknown>> }>('agents');
  const running = { ...agents, agents: agents.agents.map((a, i) => (i === 0 ? { ...a, state: 'running' } : a)) };
  $.process.script[`cctop query agents --session ${SESSION} --surface pane`] = ok(JSON.stringify(running));
  $.clock.tick(10_000);
  await settle();
  await settle();
  assert.equal(reads - before, 1);
  await render();
  $.ui.press(`agent-${agentRow.id}`);
  await settle();
  assert.deepEqual($.ui.views, [{ agentId: agentRow.id }], 'the row became a Button');
  $.process.script[`cctop query agents --session ${SESSION} --surface pane`] = ok(JSON.stringify(JSON.parse(JSON.stringify(running))));
  $.clock.tick(10_000);
  await settle();
  await settle();
  assert.equal(reads - before, 1, 'the same row again: no second read');
});
