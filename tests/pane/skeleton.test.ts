import { test } from 'node:test';
import assert from 'node:assert/strict';
import { register } from '../../plugin/hooks/pane';

// Placeholder until US-002 brings the headless render harness: the module
// compiles to CommonJS, loads under Node and exports `register`.
test('pane module exports register', () => {
  assert.equal(typeof register, 'function');
});
