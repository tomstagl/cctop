import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mapAgents } from '../../plugin/hooks/agentmap';

// US-003: which cctop agent row is which `$.agent.list()` agent.

test('an exact id match maps the row to itself', () => {
  const m = mapAgents([{ id: 'a1', type: 'Explore', description: 'look', order: 0 }], [{ id: 'a1', type: 'Explore', description: 'look' }]);
  assert.deepEqual([...m], [['a1', 'a1']]);
});

test('no exact id: the description and type find the live agent', () => {
  const m = mapAgents(
    [{ id: 'cctop-1', type: 'Explore', description: 'look', order: 0 }],
    [
      { id: 'live-0', type: 'Plan', description: 'look' },
      { id: 'live-1', type: 'Explore', description: 'look' },
    ],
  );
  assert.deepEqual([...m], [['cctop-1', 'live-1']]);
});

test('two rows that say the same take the live agents in spawn order, and never one live id twice', () => {
  const rows = [
    { id: 'r-late', type: 'Explore', description: 'same', order: 1 },
    { id: 'r-early', type: 'Explore', description: 'same', order: 0 },
  ];
  const live = [
    { id: 'l-0', type: 'Explore', description: 'same' },
    { id: 'l-1', type: 'Explore', description: 'same' },
  ];
  assert.deepEqual([...mapAgents(rows, live)].sort(), [['r-early', 'l-0'], ['r-late', 'l-1']]);
  // Three rows, two live agents: the third has none.
  const m = mapAgents([...rows, { id: 'r-x', type: 'Explore', description: 'same', order: 2 }], live);
  assert.equal(m.size, 2);
  assert.equal(new Set(m.values()).size, 2);
  assert.ok(!m.has('r-x'));
});

test('an exact match outranks a description match: a live id an exact row holds is not given to a fallback row', () => {
  const m = mapAgents(
    [
      { id: 'cctop-1', type: 'Explore', description: 'same', order: 0 },
      { id: 'l-1', type: 'Explore', description: 'same', order: 1 },
    ],
    [
      { id: 'l-1', type: 'Explore', description: 'same' },
      { id: 'l-2', type: 'Explore', description: 'same' },
    ],
  );
  assert.deepEqual([...m].sort(), [['cctop-1', 'l-2'], ['l-1', 'l-1']]);
});

test('no match, or a row that cannot be compared: no entry', () => {
  assert.equal(mapAgents([{ id: 'x', type: 'Explore', description: 'a', order: 0 }], [{ id: 'y', type: 'Explore', description: 'b' }]).size, 0);
  assert.equal(mapAgents([{ id: 'x', order: 0 }], [{ id: 'y', type: 'Explore', description: 'b' }]).size, 0);
  assert.equal(mapAgents([{ id: 'x', type: 'Explore', description: 'a', order: 0 }], []).size, 0);
});
