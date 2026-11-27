import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mapAgents, type AgentRow, type LiveAgent } from '../../plugin/hooks/agentmap';

// US-003: which cctop agent row is which `$.agent.list()` agent. Never a guess.
const row = (id: string, description: string, running = true, type = 'Explore'): AgentRow => ({ id, type, description, running });
const live = (id: string, description: string, type = 'Explore'): LiveAgent => ({ id, type, description });

test('an exact id match maps the row to itself, whatever its state', () => {
  assert.deepEqual([...mapAgents([row('a1', 'look', false)], [live('a1', 'look')])], [['a1', 'a1']]);
});

test('no exact id: a running row with a unique description and type finds its live agent', () => {
  const m = mapAgents([row('cctop-1', 'look')], [live('live-0', 'look', 'Plan'), live('live-1', 'look')]);
  assert.deepEqual([...m], [['cctop-1', 'live-1']]);
});

test('two running rows with one pair: neither is mapped', () => {
  assert.equal(mapAgents([row('r0', 'same'), row('r1', 'same')], [live('l0', 'same'), live('l1', 'same')]).size, 0);
  assert.equal(mapAgents([row('r0', 'same'), row('r1', 'same')], [live('l0', 'same')]).size, 0);
});

test('one running row but two unmatched live agents alike: not mapped', () => {
  assert.equal(mapAgents([row('r0', 'same')], [live('l0', 'same'), live('l1', 'same')]).size, 0);
});

test('a finished row with the pair of a live agent is not mapped', () => {
  assert.equal(mapAgents([row('r0', 'same', false)], [live('l0', 'same')]).size, 0);
});

test('a live id an exact row holds is not offered to a fallback row', () => {
  const m = mapAgents([row('cctop-1', 'same'), row('l-1', 'other', false)], [live('l-1', 'same'), live('l-2', 'same')]);
  assert.deepEqual([...m].sort(), [['cctop-1', 'l-2'], ['l-1', 'l-1']]);
});

test('the order of the rows and of the live agents changes nothing', () => {
  const rows = [row('a', 'one'), row('b', 'two'), row('c', 'two'), row('d', 'three', false), row('e', 'four')];
  const agents = [live('x', 'one'), live('y', 'two'), live('z', 'two'), live('w', 'three'), live('v', 'four'), live('e', 'four')];
  const want = [...mapAgents(rows, agents)].sort();
  assert.deepEqual(want, [['a', 'x'], ['e', 'e']]);
  for (const [r, l] of [[[...rows].reverse(), agents], [rows, [...agents].reverse()], [[rows[4], rows[2], rows[0], rows[3], rows[1]], [...agents].reverse()]] as const) {
    assert.deepEqual([...mapAgents([...r], [...l])].sort(), want);
  }
});

test('no match, or a row that cannot be compared: no entry', () => {
  assert.equal(mapAgents([row('x', 'a')], [live('y', 'b')]).size, 0);
  assert.equal(mapAgents([{ id: 'x', running: true }], [live('y', 'b')]).size, 0);
  assert.equal(mapAgents([row('x', 'a')], []).size, 0);
});
