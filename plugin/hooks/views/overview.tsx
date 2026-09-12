// The Overview view: the TUI's top half in one pane. A header row, then the
// Context, Tokens & Cost, Limits and Turn blocks (two columns from 60 body
// columns, stacked below) and, last, the top Advisor headline when it is
// severe. A metric row is `label … value` with the value right-aligned in a
// fixed-width Box and the row keyed by its metric id (docs/metrics.md); `≈`
// marks a value whose source says `approx: true`.
//
// Sources, by precedence: the engine's own figures (`model.usage`, the turn
// and tool counters) first, `cctop query summary` for everything else. A
// section only the binary can fill draws `needs the cctop binary` while it is
// missing. Colours are palette names only: the pane takes Claude Code's
// theme as it is.
import type { ElementTable, RenderElement } from 'claude-code';
import { formatCountdown, formatTokens, type Model, type Placement, type TurnState } from '../model';
import { DASH, at, formatDuration, formatUsd, isMissing, mark, measured, stringAt, tokensOf } from './format';

/** The elements a view draws with, as `$.ui.resolve(e)` answers them. */
export type ViewElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text'>;

export const NEEDS_BINARY = 'needs the cctop binary';
/** Two-column rows from this many body columns; a single column below. */
export const TWO_COLUMN_MIN = 60;
/** The narrowest label a value Box leaves room for. */
const MIN_LABEL = 8;

export type Color = 'green' | 'yellow' | 'red';

export type Row = {
  /** The metric id as docs/metrics.md names it. */
  key: string;
  label: string;
  value: string;
  color?: Color;
  /** One line of text (`needs the cctop binary`) rather than a label/value pair. */
  line?: boolean;
};

export type Block = { title: string; rows: Row[] };

export type Badge = { label: 'bin' | 'shim' | 'hooks'; on: boolean };

export type Header = {
  status: TurnState;
  turn: number;
  elapsed: string;
  model: string;
  effort: string;
  badges: Badge[];
};

const STATUS_MARK: Record<TurnState, string> = { busy: '●', idle: '○', waiting: '◆' };
const STATUS_COLOR: Record<TurnState, Color | undefined> = { busy: 'green', idle: undefined, waiting: 'yellow' };
const TOKEN_CLASSES: [string, string][] = [
  ['cache_read', 'cache read'],
  ['cache_write', 'cache write'],
  ['fresh_input', 'fresh input'],
  ['output', 'output'],
  ['thinking', '└ thinking'],
];
// `cache_ttl` is the Rust enum's Debug name.
const TTL_NAMES: Record<string, string> = { FiveMinutes: '5m', OneHour: '1h' };

function line(key: string, text: string): Row {
  return { key, label: text, value: '', line: true };
}

// The colour bands the TUI's gauges use (band_style): green below `warn`,
// yellow from it, red from `crit`.
function band(ratio: number, warn: number, crit: number): Color {
  return ratio >= crit ? 'red' : ratio >= warn ? 'yellow' : 'green';
}

/** Elapsed of the running turn, else the last turn's duration; null before any. */
function turnElapsedMs(model: Model, now: number): number | null {
  return model.turn.startedAt !== null ? now - model.turn.startedAt : model.turn.lastDurationMs;
}

export function header(model: Model, now: number): Header {
  const summary = model.query.summary;
  const elapsed = turnElapsedMs(model, now);
  return {
    status: model.turn.state,
    turn: model.turn.number,
    elapsed: elapsed === null ? DASH : formatDuration(elapsed),
    model: model.modelName ?? stringAt(summary, 'session', 'model') ?? DASH,
    effort: stringAt(summary, 'session', 'effort') ?? DASH,
    badges: [
      { label: 'bin', on: model.binary === 'present' },
      // Limits come from the status-line shim alone, so their presence is its.
      { label: 'shim', on: at(summary, 'limits') !== undefined && !isMissing(summary, 'limits') },
      { label: 'hooks', on: at(summary, 'hooks_installed') === true },
    ],
  };
}

export function contextBlock(model: Model): Block {
  const summary = model.query.summary;
  const rows: Row[] = [];
  const usage = model.usage;
  if (usage !== null) {
    const { tokens, window } = usage.context;
    const percent =
      usage.context.percent ?? (tokens !== undefined && window > 0 ? Math.round((tokens / window) * 100) : undefined);
    const size = tokens === undefined ? '?' : formatTokens(tokens);
    const pct = percent === undefined ? '?' : String(percent);
    rows.push({
      key: 'context_size',
      label: 'context',
      value: `${size} / ${formatTokens(window)} (${pct} %)`,
      color: percent === undefined ? undefined : band(percent / 100, 0.6, 0.8),
    });
  } else {
    const size = measured(summary, 'context', 'size');
    const window = measured(summary, 'context', 'window');
    if (size !== null && window !== null && window.value > 0) {
      const ratio = measured(summary, 'context', 'ratio')?.value ?? size.value / window.value;
      const pct = Math.round(ratio * 100);
      rows.push({
        key: 'context_size',
        label: 'context',
        value: mark(`${formatTokens(size.value)} / ${formatTokens(window.value)} (${pct} %)`, size.approx || window.approx),
        color: band(ratio, 0.6, 0.8),
      });
    } else {
      rows.push({ key: 'context_size', label: 'context', value: DASH });
    }
  }
  if (model.binary === 'missing') {
    rows.push(line('context_velocity', NEEDS_BINARY));
  } else {
    const velocity = measured(summary, 'context', 'velocity');
    const perTurn = velocity === null ? null : Math.round(velocity.value);
    rows.push({
      key: 'context_velocity',
      label: 'velocity',
      value: velocity === null || perTurn === null ? DASH : mark(`${perTurn > 0 ? '+' : ''}${formatTokens(perTurn)}/turn`, velocity.approx),
    });
    const until = measured(summary, 'context', 'turns_until_compaction');
    rows.push({
      key: 'turns_until_compaction',
      label: 'autocompact in',
      value: until === null ? DASH : mark(`${Math.ceil(until.value)} turns`, until.approx),
      color: until === null ? undefined : band(1 - Math.min(1, until.value / 10), 0.6, 0.8),
    });
  }
  // The larger of the two counts: the transcript's includes compactions from
  // before the module loaded, the event's those the poller has not read yet.
  const counted = measured(summary, 'context', 'compactions');
  rows.push({ key: 'compactions', label: 'compactions', value: String(Math.max(model.compactions, counted?.value ?? 0)) });
  return { title: 'Context', rows };
}

export function tokensBlock(model: Model): Block {
  const summary = model.query.summary;
  const rows: Row[] = [];
  if (model.binary === 'missing') {
    rows.push(line('cache_read', NEEDS_BINARY));
  } else {
    for (const [key, label] of TOKEN_CLASSES) rows.push({ key, label, value: tokensOf(measured(summary, 'tokens', key)) });
    const hit = measured(summary, 'tokens', 'cache_hit_ratio');
    rows.push({
      key: 'cache_hit_ratio',
      label: 'cache hit',
      value: hit === null ? DASH : mark(`${Math.round(hit.value * 100)} %`, hit.approx),
      color: hit === null ? undefined : hit.value >= 0.8 ? 'green' : hit.value >= 0.5 ? 'yellow' : 'red',
    });
    const ttl = stringAt(summary, 'tokens', 'cache_ttl');
    rows.push({ key: 'cache_ttl', label: 'cache TTL', value: ttl === null ? DASH : (TTL_NAMES[ttl] ?? ttl) });
  }
  const live = model.usage?.cost;
  const cost = measured(summary, 'cost');
  rows.push({
    key: 'cost',
    label: 'cost',
    value: live !== undefined ? formatUsd(live.usd) : cost === null ? DASH : mark(formatUsd(cost.value), cost.approx),
  });
  if (model.binary !== 'missing') {
    const burn = measured(summary, 'burn_rate');
    rows.push({ key: 'burn_rate', label: 'burn rate', value: burn === null ? DASH : mark(`${formatUsd(burn.value)}/h`, burn.approx) });
  }
  return { title: 'Tokens & Cost', rows };
}

const LIMIT_WINDOWS: { kind: string; key: string; label: string }[] = [
  { kind: 'five_hour', key: 'limit_5h', label: '5 h' },
  { kind: 'seven_day', key: 'limit_7d', label: '7 d' },
];

export function limitsBlock(model: Model, now: number): Block {
  const limits = at(model.query.summary, 'limits');
  const rows: Row[] = [];
  let resets: number | null = null;
  for (const { kind, key, label } of LIMIT_WINDOWS) {
    const live = model.usage?.rateLimits.find((l) => l.kind === kind);
    const polled = live === undefined ? measured(limits, kind) : null;
    const pct = live?.percentUsed ?? polled?.value;
    let value = DASH;
    if (live !== undefined) {
      value = `${Math.round(live.percentUsed)} %`;
      if (kind === 'five_hour' && live.resetsAt !== undefined) {
        const t = Date.parse(live.resetsAt);
        if (!Number.isNaN(t)) resets = t;
      }
    } else if (polled !== null) {
      value = mark(`${Math.round(polled.value)} %`, polled.approx);
    }
    rows.push({ key, label, value, color: pct === undefined ? undefined : band(pct / 100, 0.6, 0.85) });
  }
  if (resets === null) {
    const polled = at(limits, 'five_hour_resets_at_ms');
    if (typeof polled === 'number') resets = polled;
  }
  rows.push({ key: 'limit_reset', label: 'resets in', value: resets === null ? DASH : formatCountdown(resets - now) });
  const exhaustion = measured(limits, 'exhaustion_ms');
  rows.push({
    key: 'limit_exhaustion',
    label: 'exhausted in',
    value: exhaustion === null ? DASH : mark(formatCountdown(exhaustion.value - now), exhaustion.approx),
  });
  return { title: 'Limits', rows };
}

export function turnBlock(model: Model, now: number): Block {
  const summary = model.query.summary;
  const turn = model.turn;
  const elapsed = turnElapsedMs(model, now);
  const running = turn.runningTool;
  const toolMs = turn.toolMs + (running === null ? 0 : Math.max(0, now - running.startedAt));
  const rows: Row[] = [
    { key: 'session_status', label: 'state', value: turn.state, color: STATUS_COLOR[turn.state] },
    { key: 'turn_elapsed', label: 'elapsed', value: elapsed === null ? DASH : formatDuration(elapsed) },
    // API time is what the turn spent outside its tool calls: an estimate.
    {
      key: 'api_time',
      label: 'api / tools',
      value: elapsed === null ? DASH : mark(`${formatDuration(Math.max(0, elapsed - toolMs))} / ${formatDuration(toolMs)}`, true),
    },
    {
      key: 'tool_last_call',
      label: 'waiting on',
      value:
        turn.state === 'waiting' ? 'permission' : running === null ? DASH : `${running.name} ${formatDuration(now - running.startedAt)}`,
      color: turn.state === 'waiting' ? 'yellow' : undefined,
    },
  ];
  const waits = at(summary, 'permission_waits');
  const count = at(waits, 'count');
  const total = measured(waits, 'total');
  rows.push({
    key: 'permission_wait',
    label: 'permission waits',
    value: typeof count !== 'number' ? DASH : `${count} · ${total === null ? DASH : mark(formatDuration(total.value), total.approx)}`,
  });
  const queued = measured(summary, 'queued_prompts');
  rows.push({
    key: 'queued_prompts',
    label: 'queued',
    value: queued === null ? DASH : mark(String(queued.value), queued.approx),
    color: queued !== null && queued.value > 0 ? 'yellow' : undefined,
  });
  return { title: 'Turn', rows };
}

/** The top Advisor headline when its severity is high; the binary line while it is missing; else null. */
export function advisorLine(model: Model): Row | null {
  if (model.binary === 'missing') return line('advice_saving', NEEDS_BINARY);
  const advice = model.query.advice;
  if (!Array.isArray(advice) || advice.length === 0) return null;
  const first: unknown = advice[0];
  if (at(first, 'severity') !== 'high') return null;
  const headline = stringAt(first, 'headline');
  return headline === null ? null : { ...line('advice_saving', `▲ ${headline}`), color: 'red' };
}

function renderRow(row: Row, valueWidth: number, el: ViewElements): RenderElement {
  const { Box, Text } = el;
  if (row.line) {
    return (
      <Box key={row.key}>
        <Text wrap="truncate" dimColor>
          {row.label}
        </Text>
      </Box>
    );
  }
  return (
    <Box key={row.key} flexDirection="row" columnGap={1}>
      <Box flexGrow={1}>
        <Text wrap="truncate">{row.label}</Text>
      </Box>
      <Box width={valueWidth} flexShrink={0} flexDirection="row" justifyContent="flex-end">
        <Text wrap="truncate" color={row.color}>
          {row.value}
        </Text>
      </Box>
    </Box>
  );
}

// A block's values share one right-aligned column as wide as the widest of
// them, leaving the labels at least MIN_LABEL columns.
function renderBlock(block: Block, width: number, el: ViewElements): RenderElement {
  const { Box, Text } = el;
  const widest = Math.max(0, ...block.rows.filter((r) => !r.line).map((r) => r.value.length));
  const valueWidth = Math.max(1, Math.min(widest, width - 1 - MIN_LABEL));
  return (
    <Box flexDirection="column">
      <Text wrap="truncate" bold>
        {block.title}
      </Text>
      {block.rows.map((row) => renderRow(row, valueWidth, el))}
    </Box>
  );
}

function renderHeader(head: Header, el: ViewElements): RenderElement {
  const { Box, Text } = el;
  return (
    <Box key="session_status" flexDirection="row" columnGap={1}>
      <Box flexGrow={1}>
        <Text wrap="truncate">
          <Text wrap="truncate" color={STATUS_COLOR[head.status]}>
            {STATUS_MARK[head.status]} {head.status}
          </Text>
          {` · turn ${head.turn} · ${head.elapsed} · ${head.model} · ${head.effort}`}
        </Text>
      </Box>
      <Box width={14} flexShrink={0} flexDirection="row" columnGap={1} justifyContent="flex-end">
        {head.badges.map((b) => (
          <Text key={b.label} wrap="truncate" dimColor={!b.on} color={b.on ? 'green' : undefined}>
            {b.label}
          </Text>
        ))}
      </Box>
    </Box>
  );
}

/**
 * The Overview: header, the four blocks (two per row from TWO_COLUMN_MIN
 * columns), the Advisor line. Inline placement (the classic renderer's few
 * rows above the prompt) draws the header, Context and Limits only.
 */
export function renderOverview(model: Model, el: ViewElements, columns: number, placement: Placement, now: number): RenderElement {
  const { Box, Text } = el;
  const blocks =
    placement === 'inline'
      ? [contextBlock(model), limitsBlock(model, now)]
      : [contextBlock(model), tokensBlock(model), limitsBlock(model, now), turnBlock(model, now)];
  const advice = placement === 'inline' ? null : advisorLine(model);
  const left = Math.floor((columns - 1) / 2);
  const right = columns - 1 - left;
  const body: RenderElement[] = [];
  if (columns >= TWO_COLUMN_MIN) {
    for (let i = 0; i < blocks.length; i += 2) {
      const pair = blocks.slice(i, i + 2);
      body.push(
        <Box flexDirection="row" columnGap={1}>
          {pair.map((block, j) => (
            <Box width={j === 0 ? left : right} flexShrink={0} flexDirection="column">
              {renderBlock(block, j === 0 ? left : right, el)}
            </Box>
          ))}
        </Box>,
      );
    }
  } else {
    for (const block of blocks) body.push(renderBlock(block, columns, el));
  }
  return (
    <Box flexDirection="column">
      {renderHeader(header(model, now), el)}
      {body}
      {advice !== null && (
        <Box key={advice.key}>
          <Text wrap="truncate" color={advice.color} dimColor={advice.color === undefined}>
            {advice.label}
          </Text>
        </Box>
      )}
    </Box>
  );
}
