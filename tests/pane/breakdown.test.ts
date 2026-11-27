import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { SessionUsage } from 'claude-code';
import { spoolText } from '../../plugin/hooks/breakdown';

const BREAKDOWN = {
  categories: [
    { name: 'System prompt', tokens: 4100, color: 'promptBorder', isDeferred: false, kind: 'used' },
    { name: 'Messages', tokens: 84600, color: 'permission', isDeferred: false, kind: 'used' },
    { name: 'Free space', tokens: 878000, color: 'inactive', isDeferred: false, kind: 'free' },
    { name: 'Autocompact buffer', tokens: 33000, color: 'inactive', isDeferred: false, kind: 'buffer' },
  ],
  totalTokens: 88700,
  maxTokens: 1000000,
  rawMaxTokens: 1000000,
  autocompactSource: 'model',
  percentage: 9,
  gridRows: [[{ color: 'x' }]],
  model: 'claude-opus-5-5',
  memoryFiles: [{ path: '/p/CLAUDE.md', type: 'Project', tokens: 2100 }],
  mcpTools: [{ name: 'mcp__x__y', serverName: 'x', tokens: 900, isLoaded: true }],
  agents: [{ agentType: 'Explore', source: 'plugin', tokens: 300 }],
  slashCommands: { totalCommands: 3, includedCommands: 3, tokens: 50 },
  skills: { totalSkills: 14, includedSkills: 12, tokens: 1400, skillFrontmatter: [] },
  autoCompactThreshold: 967000,
  isAutoCompactEnabled: true,
  apiUsage: null,
};

function usage(breakdown?: unknown): SessionUsage {
  return { startedAt: 0, context: { window: 1000000, breakdown }, rateLimits: [] } as unknown as SessionUsage;
}

test('spoolText writes the 3.1 object, without grid rows, colours or slash commands', () => {
  const expected = {
    v: 1,
    at: 1764170000000,
    tokens: 88700,
    window: 1000000,
    model: 'claude-opus-5-5',
    categories: [
      { name: 'System prompt', tokens: 4100, kind: 'used' },
      { name: 'Messages', tokens: 84600, kind: 'used' },
      { name: 'Free space', tokens: 878000, kind: 'free' },
      { name: 'Autocompact buffer', tokens: 33000, kind: 'buffer' },
    ],
    autocompact_threshold: 967000,
    is_autocompact_enabled: true,
    memory_files: [{ path: '/p/CLAUDE.md', tokens: 2100 }],
    mcp_tools: [{ name: 'mcp__x__y', tokens: 900 }],
    skills: { count: 12, tokens: 1400 },
    agents: [{ name: 'Explore', tokens: 300 }],
  };
  const text = spoolText(usage(BREAKDOWN), 1764170000000);
  assert.equal(text, `${JSON.stringify(expected)}\n`);
  assert.doesNotMatch(text ?? '', /gridRows|color|slashCommands/);
});

test('spoolText leaves out what the session lacks', () => {
  const text = spoolText(usage({ ...BREAKDOWN, skills: undefined, autoCompactThreshold: undefined }), 1);
  const parsed = JSON.parse(text ?? '{}') as Record<string, unknown>;
  assert.equal('skills' in parsed, false);
  assert.equal('autocompact_threshold' in parsed, false);
});

test('spoolText rounds every count and the time to whole numbers', () => {
  const fractional = {
    ...BREAKDOWN,
    categories: [{ name: 'Messages', tokens: 84600.6, color: 'x', isDeferred: false, kind: 'used' }],
    totalTokens: 88700.4,
    rawMaxTokens: 999999.5,
    autoCompactThreshold: 967000.2,
    memoryFiles: [{ path: '/p/CLAUDE.md', type: 'Project', tokens: 2100.5 }],
    mcpTools: [{ name: 'm', serverName: 'x', tokens: 900.4, isLoaded: true }],
    agents: [{ agentType: 'Explore', source: 'plugin', tokens: 299.5 }],
    skills: { totalSkills: 14, includedSkills: 11.6, tokens: 1400.4, skillFrontmatter: [] },
  };
  const s = JSON.parse(spoolText(usage(fractional), 1764170000000.4) ?? '{}') as Record<string, unknown>;
  assert.equal(s.at, 1764170000000);
  assert.equal(s.tokens, 88700);
  assert.equal(s.window, 1000000);
  assert.equal(s.autocompact_threshold, 967000);
  assert.deepEqual(s.categories, [{ name: 'Messages', tokens: 84601, kind: 'used' }]);
  assert.deepEqual(s.memory_files, [{ path: '/p/CLAUDE.md', tokens: 2101 }]);
  assert.deepEqual(s.mcp_tools, [{ name: 'm', tokens: 900 }]);
  assert.deepEqual(s.agents, [{ name: 'Explore', tokens: 300 }]);
  assert.deepEqual(s.skills, { count: 12, tokens: 1400 });
});

test('spoolText is null without a breakdown', () => {
  assert.equal(spoolText(usage(), 1), null);
});

test("the pane never asks for a 'full' breakdown", () => {
  const dir = join(__dirname, '../../../plugin/hooks');
  const files = [] as string[];
  for (const sub of ['', 'views']) {
    for (const f of readdirSync(join(dir, sub))) if (/\.tsx?$/.test(f)) files.push(join(sub, f));
  }
  assert.ok(files.length > 0);
  for (const f of files) {
    const src = readFileSync(join(dir, f), 'utf8');
    assert.doesNotMatch(src, /breakdown[^\n]{0,40}['"]full['"]|['"]full['"][^\n]{0,40}breakdown/i, f);
  }
});
