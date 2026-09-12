// The Agents & MCP view: subagents (state glyph, type, description, elapsed,
// tokens), MCP servers (name, RSS, calls, restarts) and background tasks, one
// row each as the TUI's panel draws them, from `cctop query agents`.
import type { RenderElement } from 'claude-code';
import type { Model } from '../model';
import { DASH, at, formatBytes, formatDuration, isMissing, measured, stringAt, tokensOf } from './format';
import { NEEDS_BINARY, type Color, type ViewElements } from './overview';
import { MAX_ROWS, line, row, type Cell } from './table';

const W = { glyph: 10, prefix: 3, kind: 6, elapsed: 6, tokens: 6, rss: 7, calls: 9, restarts: 4, status: 16 };

const STATE_GLYPH: Record<string, [string, Color]> = {
  running: ['◐', 'cyan'],
  done: ['✓', 'green'],
  failed: ['✗', 'red'],
};

function agentCells(a: unknown): Cell[] {
  const state = stringAt(a, 'state') ?? '';
  const [glyph, color] = STATE_GLYPH[state] ?? ['·', undefined];
  const elapsed = measured(a, 'elapsed');
  return [
    { text: `${glyph} ${stringAt(a, 'type') ?? DASH}`, width: W.glyph, color, key: 'agent_state' },
    { text: stringAt(a, 'description') ?? '', dim: true },
    { text: elapsed === null ? DASH : formatDuration(elapsed.value), width: W.elapsed, right: true },
    { text: tokensOf(measured(a, 'tokens')), width: W.tokens, right: true, key: 'agent_tokens' },
  ];
}

function mcpCells(m: unknown): Cell[] {
  const rss = measured(m, 'rss');
  const calls = measured(m, 'calls');
  const restarts = at(m, 'restarts');
  return [
    { text: 'mcp', width: W.prefix, dim: true },
    { text: stringAt(m, 'name') ?? DASH },
    { text: rss === null ? DASH : formatBytes(rss.value), width: W.rss, right: true, key: 'mcp_rss' },
    { text: calls === null ? DASH : `${calls.value} calls`, width: W.calls, right: true, key: 'mcp_calls' },
    { text: typeof restarts === 'number' && restarts > 0 ? `↻${restarts}` : '', width: W.restarts, color: 'yellow' },
  ];
}

function taskCells(t: unknown, now: number): Cell[] {
  const started = at(t, 'started_at_ms');
  return [
    { text: 'bg', width: W.prefix, dim: true },
    { text: stringAt(t, 'kind') ?? DASH, width: W.kind },
    { text: stringAt(t, 'description') ?? '', dim: true },
    { text: typeof started === 'number' ? formatDuration(now - started) : DASH, width: W.elapsed, right: true },
    { text: [stringAt(t, 'id')?.slice(0, 8), stringAt(t, 'status')].filter((x) => x !== undefined && x !== null).join(' '), width: W.status, dim: true },
  ];
}

function listAt(obj: unknown, key: string): unknown[] {
  const v = at(obj, key);
  return Array.isArray(v) ? v : [];
}

export function renderAgents(model: Model, el: ViewElements, now: number): RenderElement {
  const { Box } = el;
  if (model.binary === 'missing') return line(NEEDS_BINARY, el, { key: 'agent_state' });
  const data = model.query.agents;
  const rows: RenderElement[] = [];
  for (const a of listAt(data, 'agents')) rows.push(row(agentCells(a), el, 'agent_state'));
  // MCP servers need a live process: the query says so when it has none.
  if (isMissing(data, 'mcp')) rows.push(line(`mcp: ${stringAt(data, 'mcp', 'hint') ?? DASH}`, el, { key: 'mcp_rss' }));
  for (const m of listAt(data, 'mcp')) rows.push(row(mcpCells(m), el, 'mcp_rss'));
  for (const t of listAt(data, 'tasks')) rows.push(row(taskCells(t, now), el));
  if (rows.length === 0) rows.push(line('no subagents, MCP servers or background tasks', el));
  return <Box flexDirection="column">{rows.slice(0, MAX_ROWS)}</Box>;
}
