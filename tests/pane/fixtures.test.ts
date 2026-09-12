import { test } from 'node:test';
import assert from 'node:assert/strict';
import type { SessionUsage } from 'claude-code';
import { QUERY_FIXTURES, fixture } from './fixture';

test('every query fixture parses', () => {
  for (const name of QUERY_FIXTURES) assert.ok(fixture(name) !== null, name);
  const summary = fixture<{ context: { size: { value: number; approx: boolean } } }>('summary');
  assert.equal(summary.context.size.value, 396365);
  assert.equal(summary.context.size.approx, true);
  assert.ok(fixture<unknown[]>('events').length > 50);
});

test('usage.json has the SessionUsage shape', () => {
  const usage = fixture<SessionUsage>('usage');
  assert.equal(usage.context.window, 1000000);
  assert.equal(usage.context.tokens, 396365);
  assert.equal(usage.context.percent, 40);
  assert.deepEqual(
    usage.rateLimits.map((r) => r.kind),
    ['five_hour', 'seven_day'],
  );
  for (const r of usage.rateLimits) assert.ok(r.resetsAt && !Number.isNaN(Date.parse(r.resetsAt)));
  assert.equal(usage.cost?.usd, 9.9035);
});
