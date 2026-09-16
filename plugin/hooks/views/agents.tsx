// The Agents & MCP view: subagents as the TUI's agents view lists them
// (state glyph, type, model family, elapsed, tokens, priced cost, what came
// back, waste with its reason), MCP servers (name, RSS, calls, restarts) and
// background tasks, one row each, from `cctop query agents`.
import type { RenderElement } from 'claude-code';
import type { Model } from '../model';
import { DASH, at, formatBytes, formatDuration, isMissing, measured, stringAt, tokensOf } from './format';
import { NEEDS_BINARY, type Color, type ViewElements } from './overview';
import { bodyWidth, line, panel, row, type Cell, type FrameRow } from './table';

const W = { glyph: 10, model: 6, prefix: 3, kind: 6, elapsed: 6, tokens: 6, cost: 5, ret: 5, rss: 7, calls: 9, restarts: 4, status: 16 };

const STATE_GLYPH: Record<string, [string, Color]> = {
  running: ['◐', 'cyan'],
  done: ['✓', 'green'],
  failed: ['✗', 'red'],
};

/** `claude-opus-5` → `opus`: the family, as the TUI's agents view. */
function family(model: string | null): string {
  return (model ?? '').replace(/^claude-/, '').split('-')[0] ?? '';
}

/** Dollars without the sign in five cells, as the TUI's agents view (`0.19`, `12.4`, `118`). */
function cents(usd: number): string {
  if (usd >= 100) return usd.toFixed(0);
  if (usd >= 10) return usd.toFixed(1);
  return usd.toFixed(2);
}

/** `idle 55m` / `idle 2h10` from the waste's age, else the reason word. */
function wasteText(a: unknown): string {
  const waste = at(a, 'waste');
  if (waste === null || waste === undefined) return '0.00';
  const usd = measured(waste, 'usd');
  const reason = stringAt(waste, 'reason') ?? '';
  const idle = at(waste, 'idle_ms');
  // `reason` is the query's enum value (`failed`, `killed`, `no_ret`, `idle`); the TUI prints it with a space.
  const label = reason === 'idle' && typeof idle === 'number' ? `idle ${shortDuration(idle)}` : reason.replace('_', ' ');
  const amount = usd === null || stringAt(waste, 'usd', 'source') === 'unpriced' ? DASH : cents(usd.value);
  return `${amount} ${label}`;
}

/** Minutes-first, as the TUI's coach::short_duration: `41m`, `2h10`, `4:12` under five minutes. */
function shortDuration(ms: number): string {
  const s = Math.floor(Math.max(0, ms) / 1000);
  if (s < 300) return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h${String(Math.floor((s % 3600) / 60)).padStart(2, '0')}`;
}

/** Below this many body columns the model and `ret` columns make way for the waste. */
const NARROW = 58;

function agentCells(a: unknown, inner: number): Cell[] {
  const state = stringAt(a, 'state') ?? '';
  const [glyph, color] = STATE_GLYPH[state] ?? ['·', undefined];
  const elapsed = measured(a, 'elapsed');
  const cost = measured(a, 'cost');
  const unpriced = stringAt(a, 'cost', 'source') === 'unpriced';
  const ret = measured(a, 'returned_tokens');
  const wide = inner >= NARROW;
  const cells: Cell[] = [{ text: `${glyph} ${stringAt(a, 'type') ?? DASH}`, width: W.glyph, color, key: 'agent_state' }];
  if (wide) cells.push({ text: family(stringAt(a, 'model')), width: W.model, dim: true });
  cells.push(
    { text: elapsed === null ? DASH : formatDuration(elapsed.value), width: W.elapsed, right: true },
    { text: tokensOf(measured(a, 'tokens')), width: W.tokens, right: true, key: 'agent_tokens' },
    { text: cost === null || unpriced ? DASH : cents(cost.value), width: W.cost, right: true, key: 'agent_cost' },
  );
  if (wide) cells.push({ text: ret === null ? (state === 'running' ? '...' : DASH) : tokensOf(ret).replace(/^≈/, ''), width: W.ret, right: true, key: 'agent_returned' });
  cells.push({ text: wasteText(a), key: 'agent_waste', color: at(a, 'waste') ? 'yellow' : undefined });
  return cells;
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

/** `0/1 agents` as the TUI's panel summary: running over listed. */
function agentsSummary(data: unknown): string | undefined {
  const agents = listAt(data, 'agents');
  if (agents.length === 0) return undefined;
  const running = agents.filter((a) => stringAt(a, 'state') === 'running').length;
  return `${running}/${agents.length} agents`;
}

export function renderAgents(model: Model, el: ViewElements, columns: number, now: number): RenderElement {
  const data = model.query.agents;
  const p = { hotkey: '6', title: 'Agents & MCP', summary: model.binary === 'missing' ? undefined : agentsSummary(data) };
  if (model.binary === 'missing') return panel(p, [line(NEEDS_BINARY, { key: 'agent_state' })], columns, el);
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = [];
  for (const a of listAt(data, 'agents')) rows.push(row(agentCells(a, inner), inner, 'agent_state'));
  if (isMissing(data, 'mcp')) rows.push(line(`mcp: ${stringAt(data, 'mcp', 'hint') ?? DASH}`, { key: 'mcp_rss' }));
  for (const m of listAt(data, 'mcp')) rows.push(row(mcpCells(m), inner, 'mcp_rss'));
  for (const t of listAt(data, 'tasks')) rows.push(row(taskCells(t, now), inner));
  if (rows.length === 0) rows.push(line('no subagents, MCP servers or background tasks'));
  return panel(p, rows, columns, el);
}
