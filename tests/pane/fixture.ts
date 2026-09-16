import { readFileSync } from 'node:fs';
import { join } from 'node:path';

// The compiled tests run from .test-build/tests/pane; the JSON stays in
// tests/pane/fixtures (tsc copies nothing but sources).
const dir = join(__dirname, '..', '..', '..', 'tests', 'pane', 'fixtures');

export const QUERY_FIXTURES = ['summary', 'dashboard', 'coach', 'tools', 'files', 'agents', 'advice', 'events', 'agents-c', 'dashboard-b', 'dashboard-c', 'agents-d', 'dashboard-d', 'summary-d'] as const;

export function fixture<T = unknown>(name: string): T {
  return JSON.parse(readFileSync(join(dir, `${name}.json`), 'utf8')) as T;
}
