// The Tools view: the TUI's process table. One row per tool from `cctop
// query tools` (calls, errors, p50, p95, tokens → context), the running tool
// marked `▶` with its elapsed from the engine's own timing (and drawn from
// the engine's stats when the query does not list it yet), then the five
// calls that put the most into the context (`top_ctx`).
import type { RenderElement } from 'claude-code';
import { formatTokens, percentile, type Model } from '../model';
import { DASH, at, formatDuration, formatShortMs, mark, measured, stringAt, tokensOf, type Measured } from './format';
import { NEEDS_BINARY, type Color, type ViewElements } from './overview';
import { MAX_ROWS, line, row, type Cell } from './table';

const TOP_CTX = 5;
const W = { count: 4, ms: 7, tokens: 6, tool: 12, turn: 3 };

export type ToolRow = {
  name: string;
  calls: number;
  errors: number;
  p50: string;
  p95: string;
  tokens: string;
  /** The elapsed of the running call, null when the tool is not running. */
  running: string | null;
};

export type TopCtx = { tool: string; input: string; turn: string; tokens: string };

// The TUI's error bands: warn at one or two, crit from three.
function errorColor(n: number): Color | undefined {
  return n === 0 ? undefined : n <= 2 ? 'yellow' : 'red';
}

function msOf(m: Measured | null): string {
  return m === null ? DASH : mark(formatShortMs(m.value), m.approx);
}

/** The table rows: the query's tools in its order, the running one marked; prepended from the engine's stats when the query lacks it. */
export function toolRows(model: Model, now: number): ToolRow[] {
  const listed = at(model.query.tools, 'tools');
  const running = model.turn.runningTool;
  const rows: ToolRow[] = [];
  for (const t of Array.isArray(listed) ? listed : []) {
    const name = stringAt(t, 'tool');
    if (name === null) continue;
    rows.push({
      name,
      calls: measured(t, 'calls')?.value ?? 0,
      errors: measured(t, 'errors')?.value ?? 0,
      p50: msOf(measured(t, 'p50')),
      p95: msOf(measured(t, 'p95')),
      tokens: tokensOf(measured(t, 'tokens_to_ctx')),
      running: running !== null && running.name === name ? formatDuration(now - running.startedAt) : null,
    });
  }
  if (running !== null && !rows.some((r) => r.name === running.name)) {
    const stats = model.tools[running.name];
    const timed = stats !== undefined && stats.durationsMs.length > 0;
    rows.unshift({
      name: running.name,
      calls: stats?.calls ?? 0,
      errors: stats?.errors ?? 0,
      p50: timed ? formatShortMs(percentile(stats.durationsMs, 0.5)) : DASH,
      p95: timed ? formatShortMs(percentile(stats.durationsMs, 0.95)) : DASH,
      tokens: stats === undefined ? DASH : mark(formatTokens(stats.tokensToCtx), true),
      running: formatDuration(now - running.startedAt),
    });
  }
  return rows;
}

/** The five top context consumers as `cctop query tools` ranks them. */
export function topCtx(model: Model): TopCtx[] {
  const listed = at(model.query.tools, 'top_ctx');
  const out: TopCtx[] = [];
  for (const c of Array.isArray(listed) ? listed.slice(0, TOP_CTX) : []) {
    const turn = at(c, 'turn');
    out.push({
      tool: stringAt(c, 'tool') ?? DASH,
      input: stringAt(c, 'input') ?? '',
      turn: typeof turn === 'number' ? `t${turn}` : '',
      tokens: tokensOf(measured(c, 'tokens')),
    });
  }
  return out;
}

const HEADER: Cell[] = [
  { text: 'TOOL', dim: true },
  { text: 'N', width: W.count, right: true, dim: true },
  { text: 'ERR', width: W.count, right: true, dim: true },
  { text: 'p50', width: W.ms, right: true, dim: true },
  { text: 'p95', width: W.ms, right: true, dim: true },
  { text: '→CTX', width: W.tokens, right: true, dim: true },
];

function toolCells(r: ToolRow): Cell[] {
  return [
    { text: r.running === null ? r.name : `${r.name} ▶${r.running}`, color: r.running === null ? undefined : 'cyan' },
    { text: String(r.calls), width: W.count, right: true, key: 'tool_calls' },
    { text: String(r.errors), width: W.count, right: true, color: errorColor(r.errors), key: 'tool_errors' },
    { text: r.p50, width: W.ms, right: true, key: 'tool_p50' },
    { text: r.p95, width: W.ms, right: true, key: 'tool_p95' },
    { text: r.tokens, width: W.tokens, right: true, key: 'tokens_to_ctx' },
  ];
}

function topCells(c: TopCtx): Cell[] {
  return [
    { text: c.tool, width: W.tool },
    { text: c.input, dim: true },
    { text: c.turn, width: W.turn, right: true, dim: true },
    { text: c.tokens, width: W.tokens, right: true, key: 'top_ctx' },
  ];
}

export function renderTools(model: Model, el: ViewElements, now: number): RenderElement {
  const { Box, Text } = el;
  if (model.binary === 'missing') return line(NEEDS_BINARY, el, { key: 'tool_calls' });
  // The header, the `top ctx` title and its rows come out of the cap first.
  const rows = toolRows(model, now).slice(0, MAX_ROWS - 2 - TOP_CTX);
  const top = topCtx(model);
  return (
    <Box flexDirection="column">
      {row(HEADER, el)}
      {rows.length === 0 && line('no tool calls yet', el)}
      {rows.map((r) => row(toolCells(r), el, 'tool_calls'))}
      {top.length > 0 && (
        <Text wrap="truncate" dimColor>
          top ctx
        </Text>
      )}
      {top.map((c) => row(topCells(c), el, 'top_ctx'))}
    </Box>
  );
}
