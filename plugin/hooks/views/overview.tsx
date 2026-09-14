// The Overview view: plan B's dashboard drawn from `cctop query dashboard`
// (src/dashboard.rs), the way the TUI draws it (src/ui/dashboard.rs): the
// header line with the phase cell, four tiles of block digits (the coach's
// lights; two per row, the coach's L1 line below 50 body columns, L2 below
// 40), the nudge on two rows, then the nine ledger rows as plain Buttons —
// rows 5–9 switch to that view, rows 1–4 unfold the framed block the panel
// drew before (contextBlock and friends) beneath the row and fold on the next
// press. Before the binary answers, the tiles read the engine's own figures
// (`model.usage` for the context tile) and the rest says so.
//
// Inline placement (the classic renderer's few rows above the prompt) keeps
// the flat form: the header line, the coach's L1 line, the Context and Limits
// rows. Colours are the TUI's roles as Claude Code theme keys (views/frame.tsx).
import type { ElementTable, RenderElement } from 'claude-code';
import { formatCountdown, formatTokens, TESTED_WITH, type Model, type Placement, type TurnState, type View } from '../model';
import { DASH, at, formatDuration, formatUsd, isMissing, mark, measured, stringAt, tokensOf } from './format';
import { ACCENT, THEME, bigDigits, clip, dim, fit, frame, gauge, innerWidth, join, pad, seg, sparkline, textRow, width, type Line, type Seg } from './frame';

/** The elements a view draws with, as `$.ui.resolve(e)` answers them. */
export type ViewElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text'>;

export const NEEDS_BINARY = 'needs the cctop binary';
/** Two-column rows from this many body columns; a single column below. */
export const TWO_COLUMN_MIN = 60;
/** The narrowest label a value column leaves room for. */
const MIN_LABEL = 8;

// The TUI's four colour roles: `cyan` stands for its accent (running, hooks,
// agents); frame.tsx maps each to a Claude Code theme key.
export type Color = 'green' | 'yellow' | 'red' | 'cyan';

export type Row = {
  /** The metric id as docs/metrics.md names it. */
  key: string;
  label: string;
  value: string;
  color?: Color;
  /** One line of text (`needs the cctop binary`) rather than a label/value pair. */
  line?: boolean;
  /** A 0–1 fill drawn as a gauge between the label and the value, in the row's colour (or the accent). */
  gauge?: number;
  /** The value alone, left-aligned, as the TUI's Context panel writes `396k / 1.00M est`; the gauge above it. */
  bare?: boolean;
  /** A sparkline drawn in the accent before the value, as the TUI's Context panel trends the size. */
  spark?: readonly number[];
};

/**
 * A framed block. `hotkey` is the TUI's id for the panel (1 Context … 4
 * Turn), drawn in the frame as the TUI numbers it; `summaryShort` replaces
 * `summary` in the title when the full one would not fit the frame's top.
 */
export type Block = { hotkey?: string; title: string; rows: Row[]; summary?: string; summaryShort?: string };

export type Badge = { label: 'bin' | 'shim' | 'hooks'; on: boolean; text?: string };

export type Header = {
  status: TurnState;
  turn: number;
  elapsed: string;
  model: string;
  /** The permission mode (`auto`, `plan`) as `cctop query summary` reports it; null without the binary. */
  mode: string | null;
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
    mode: stringAt(summary, 'session', 'permission_mode'),
    effort: stringAt(summary, 'session', 'effort') ?? DASH,
    badges: [
      { label: 'bin', on: model.binary === 'present' },
      // Limits come from the status-line shim alone, so their presence is its.
      { label: 'shim', on: at(summary, 'limits') !== undefined && !isMissing(summary, 'limits') },
      // TESTED_WITH names the Claude Code version the `$` contract was
      // checked against (pane.tsx re-exports it; scripts/check-plugin-types.sh
      // catches drift), shown here whether or not the hooks are installed.
      { label: 'hooks', on: at(summary, 'hooks_installed') === true, text: `hooks ${TESTED_WITH}` },
    ],
  };
}

export function contextBlock(model: Model): Block {
  const summary = model.query.summary;
  const rows: Row[] = [];
  let summaryText: string | undefined;
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
      gauge: percent === undefined ? undefined : percent / 100,
      bare: true,
    });
    summaryText = percent === undefined ? undefined : `${percent} %`;
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
        gauge: ratio,
        bare: true,
      });
      summaryText = `${pct} %${size.approx || window.approx ? ' est' : ''}`;
    } else {
      rows.push({ key: 'context_size', label: 'context', value: DASH, bare: true });
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
      spark: model.contextHistory,
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
  return { hotkey: '1', title: 'Context', rows, summary: summaryText };
}

export function tokensBlock(model: Model): Block {
  const summary = model.query.summary;
  const rows: Row[] = [];
  let summaryText: string | undefined;
  if (model.binary === 'missing') {
    rows.push(line('cache_read', NEEDS_BINARY));
  } else {
    // Bars relative to the largest class, as the TUI's tokens panel draws them.
    const classes = TOKEN_CLASSES.map(([key, label]) => ({ key, label, m: measured(summary, 'tokens', key) }));
    const largest = Math.max(0, ...classes.map((c) => c.m?.value ?? 0));
    let total = 0;
    for (const { key, label, m } of classes) {
      if (m !== null && key !== 'thinking') total += m.value;
      rows.push({ key, label, value: tokensOf(m), gauge: m === null || largest <= 0 ? undefined : m.value / largest, color: 'cyan' });
    }
    if (total > 0) summaryText = formatTokens(Math.round(total));
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
  return { hotkey: '2', title: 'Tokens & Cost', rows, summary: summaryText };
}

const LIMIT_WINDOWS: { kind: string; key: string; label: string }[] = [
  { kind: 'five_hour', key: 'limit_5h', label: '5 h' },
  { kind: 'seven_day', key: 'limit_7d', label: '7 d' },
];

export function limitsBlock(model: Model, now: number): Block {
  const limits = at(model.query.summary, 'limits');
  const rows: Row[] = [];
  const summaryParts: string[] = [];
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
    rows.push({
      key,
      label,
      value,
      color: pct === undefined ? undefined : band(pct / 100, 0.6, 0.85),
      gauge: pct === undefined ? undefined : pct / 100,
    });
    if (pct !== undefined) summaryParts.push(`${label.replace(' ', '')} ${Math.round(pct)} %`);
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
  return {
    hotkey: '3',
    title: 'Limits',
    rows,
    summary: summaryParts.length === 0 ? undefined : summaryParts.join(' · '),
    summaryShort: summaryParts[0],
  };
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
      color: turn.state === 'waiting' ? 'yellow' : running === null ? undefined : 'cyan',
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
  return { hotkey: '4', title: 'Turn', rows, summary: elapsed === null ? undefined : formatDuration(elapsed) };
}

/** The coach's slot occupant (`▸ NOW A47 headline`, red for NOW, yellow for NEXT); the binary line while it is missing; else null. */
export function advisorLine(model: Model): Row | null {
  if (model.binary === 'missing') return line('advice_saving', NEEDS_BINARY);
  const primary = at(model.query.advice, 'primary');
  if (primary === null || typeof primary !== 'object') return null;
  const headline = stringAt(primary, 'headline');
  const cls = stringAt(primary, 'class') ?? '';
  if (headline === null) return null;
  const color: Color | undefined = cls === 'NOW' ? 'red' : cls === 'NEXT' ? 'yellow' : undefined;
  return { ...line('advice_saving', `▸ ${cls} ${stringAt(primary, 'rule') ?? ''} ${headline}`), color };
}

const STATUS_PILL: Record<TurnState, string> = { busy: 'BUSY', idle: 'IDLE', waiting: 'WAITING' };

/** The row's colour as a theme key; undefined leaves the default text colour. */
function themed(color: Color | undefined): string | undefined {
  return color === undefined ? undefined : THEME[color];
}

// A metric row inside a frame: `label`, a gauge when the row carries one,
// the value right-aligned at the edge. Labels share the block's label column
// and values its value column so the numbers line up. A gauge that has no
// room beside the label goes on a row of its own above, as the TUI's Context
// panel draws it; a `bare` row is the value alone, left-aligned.
function metricLines(row: Row, inner: number, labelWidth: number, valueWidth: number): Line[] {
  if (row.line) return [[dim(row.label)]];
  const color = themed(row.color);
  const fullGauge = (): Line[] => (row.gauge === undefined ? [] : [gauge(row.gauge, inner, color ?? ACCENT)]);
  if (row.bare) return [...fullGauge(), [seg(row.value, { color, key: row.key })]];
  const value = seg(pad(row.value, valueWidth, true), { color, key: row.key });
  if (row.spark !== undefined && row.spark.length >= 2) {
    // label, the sparkline in the accent, the value: `velocity ▂▃▅█  +50k/turn`.
    const cells = Math.min(12, inner - labelWidth - 2 - valueWidth);
    const spark = sparkline(row.spark, cells);
    if (cells >= 4 && spark !== '') {
      const rest = inner - labelWidth - 1 - width(spark) - valueWidth;
      return [[seg(pad(row.label, labelWidth)), seg(' '), seg(spark, { color: ACCENT }), seg(' '.repeat(Math.max(1, rest))), value]];
    }
  }
  if (row.gauge === undefined) return [[seg(pad(row.label, inner - 1 - valueWidth)), seg(' '), value]];
  const cells = inner - labelWidth - 2 - valueWidth;
  if (cells < 4) return [...fullGauge(), [seg(pad(row.label, inner - 1 - valueWidth)), seg(' '), value]];
  return [[seg(pad(row.label, labelWidth)), seg(' '), ...gauge(row.gauge, cells, color ?? ACCENT), seg(' '), value]];
}

/** A block's rows as frame rows: the label column is the widest label (at most 16), the value column the widest value. */
function blockRows(block: Block, inner: number): { line: Line; key?: string }[] {
  const metric = block.rows.filter((r) => !r.line && !r.bare);
  const widestValue = Math.max(0, ...metric.map((r) => width(r.value)));
  const valueWidth = Math.max(1, Math.min(widestValue, inner - 1 - MIN_LABEL));
  const widestLabel = Math.max(0, ...metric.map((r) => width(r.label)));
  const labelWidth = Math.max(MIN_LABEL, Math.min(16, widestLabel, inner - 1 - valueWidth));
  const out: { line: Line; key?: string }[] = [];
  for (const row of block.rows) {
    const lines = metricLines(row, inner, labelWidth, valueWidth);
    // The metric id keys the row that shows the value (the last one).
    lines.forEach((line, i) => out.push(i === lines.length - 1 ? { key: row.key, line } : { line }));
  }
  return out;
}

// A framed block; `height` pads it with empty rows so two frames side by
// side close on the same line, as the TUI's columns do.
function blockFrame(block: Block, frameWidth: number, el: ViewElements, height?: number): RenderElement {
  const rows = blockRows(block, innerWidth(frameWidth));
  while (height !== undefined && rows.length < height) rows.push({ line: [] });
  return frame({ hotkey: block.hotkey, title: block.title, summary: blockSummary(block, frameWidth), width: frameWidth, rows }, el);
}

/** The block's summary, or its short form when the title row cannot hold the full one. */
function blockSummary(block: Block, frameWidth: number): string | undefined {
  if (block.summary === undefined) return undefined;
  // `╭`, the digit and its space, the title, ` ─ `, the summary, a space, `╮`.
  const head = 1 + (block.hotkey === undefined ? 0 : block.hotkey.length + 1) + width(block.title) + 3 + width(block.summary) + 2;
  return head <= frameWidth || block.summaryShort === undefined ? block.summary : block.summaryShort;
}

/** Rows a block's frame holds, for pairing frames at one height. */
function blockHeight(block: Block, frameWidth: number): number {
  return blockRows(block, innerWidth(frameWidth)).length;
}

/** The status pill as the TUI's header draws it: `● BUSY`, `○ IDLE`, `◆ WAITING`. */
function statusPill(status: TurnState): Line {
  const text = `${STATUS_MARK[status]} ${STATUS_PILL[status]}`;
  if (status === 'busy') return [seg(text, { color: THEME.green, bold: true })];
  if (status === 'waiting') return [seg(text, { color: THEME.yellow, bold: true })];
  return [dim(text)];
}

function badgesLine(badges: Badge[]): Line {
  return join(
    badges.map((b) => [b.on ? seg(b.text ?? b.label, { color: THEME.green }) : dim(b.text ?? b.label)]),
    seg(' '),
  );
}

// The header frame: `╭cctop ─ model ─╮`, the status pill with the turn and
// its elapsed time, then the effort, cost and badges, as the TUI's header.
function headerFrame(head: Header, cost: string | null, frameWidth: number, el: ViewElements): RenderElement {
  const inner = innerWidth(frameWidth);
  const line1: Line = [...statusPill(head.status), seg(`  turn ${head.turn}  ${head.elapsed}`)];
  const badges = badgesLine(head.badges);
  const parts: Line[] = [];
  if (head.mode !== null) parts.push([seg(head.mode)]);
  parts.push([seg(head.effort)], [seg(cost ?? DASH)]);
  const left: Line = join(parts, dim(' · '));
  const room = inner - width(badges.map((s) => s.text).join('')) - 2;
  const line2: Line = room >= 8 ? [...fit(left, room), seg('  '), ...badges] : left;
  return frame(
    {
      title: 'cctop',
      summary: head.model,
      width: frameWidth,
      rows: [
        { key: 'session_status', line: line1 },
        { line: line2 },
      ],
    },
    el,
  );
}

/** What a press on the Overview may do; built in pane.tsx over `$`. */
export type OverviewActions = {
  /** A ledger row's digit: rows 5–9 open that view, 1–4 unfold their block. */
  row(digit: number): void;
};

/** The elements the Overview draws with: the views' Box and Text, plus Button for the ledger rows. */
export type OverviewElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button'>;

/** Two tiles per row from this many body columns; the coach's L1 line below; L2 below `L2_MAX`. */
export const TILES_MIN = 50;
export const L2_MAX = 40;

export type Tile = { id: string; level: string; glyph: string; figure: string; unit: string; sub1: string; sub2: string };
export type LedgerRow = { digit: number; name: string; values: Seg[]; detail: Seg[] };
export type Dashboard = {
  headerLine: string;
  phase: { glyph: string; word: string; tokens: string[] };
  tiles: Tile[];
  nudge: { line: string; tag: string; cls: string } | null;
  rows: LedgerRow[];
  lines: { l1: string; l2: string };
};

/** A tone as the binary tags it, to the pane's segment style. */
function toned(text: string, tone: unknown): Seg {
  switch (tone) {
    case 'dim':
      return dim(text);
    case 'accent':
      return seg(text, { color: ACCENT });
    case 'ok':
      return seg(text, { color: THEME.green });
    case 'warn':
      return seg(text, { color: THEME.yellow });
    case 'crit':
      return seg(text, { color: THEME.red });
    case 'bold':
      return seg(text, { bold: true });
    default:
      return seg(text);
  }
}

function segsOf(v: unknown): Seg[] {
  if (!Array.isArray(v)) return [];
  return v.map((s) => toned(stringAt(s, 'text') ?? '', at(s, 'tone'))).filter((s) => s.text !== '');
}

function tileOf(v: unknown): Tile | null {
  const id = stringAt(v, 'id');
  if (id === null) return null;
  return {
    id,
    level: stringAt(v, 'level') ?? 'quiet',
    glyph: stringAt(v, 'glyph') ?? '○',
    figure: stringAt(v, 'figure') ?? DASH,
    unit: stringAt(v, 'unit') ?? '',
    sub1: stringAt(v, 'sub1') ?? '',
    sub2: stringAt(v, 'sub2') ?? '',
  };
}

/** The dashboard object as the pane reads it, or null before the binary answered. */
export function dashboardOf(query: unknown): Dashboard | null {
  if (query === null || typeof query !== 'object') return null;
  const tiles = at(query, 'tiles');
  const rows = at(query, 'rows');
  if (!Array.isArray(tiles) || !Array.isArray(rows)) return null;
  const phase = at(query, 'header', 'phase');
  const tokens = at(phase, 'tokens');
  const nudge = at(query, 'nudge');
  return {
    headerLine: stringAt(query, 'header', 'line') ?? '',
    phase: {
      glyph: stringAt(phase, 'glyph') ?? '○',
      word: stringAt(phase, 'word') ?? '',
      tokens: Array.isArray(tokens) ? tokens.filter((t): t is string => typeof t === 'string') : [],
    },
    tiles: tiles.map(tileOf).filter((t): t is Tile => t !== null),
    nudge:
      nudge === null || typeof nudge !== 'object'
        ? null
        : { line: stringAt(nudge, 'line') ?? '', tag: stringAt(nudge, 'tag') ?? '', cls: stringAt(nudge, 'class') ?? '' },
    rows: rows.map((r) => ({
      digit: typeof at(r, 'digit') === 'number' ? (at(r, 'digit') as number) : 0,
      name: stringAt(r, 'name') ?? '',
      values: segsOf(at(r, 'values')),
      detail: segsOf(at(r, 'detail')),
    })),
    lines: { l1: stringAt(query, 'lines', 'l1') ?? '', l2: stringAt(query, 'lines', 'l2') ?? '' },
  };
}

/** The tiles before the binary answers: the context tile from the engine's own usage read, the rest `—`. */
export function engineTiles(model: Model): Tile[] {
  const usage = model.usage;
  let context: Tile = { id: 'context', level: 'quiet', glyph: '○', figure: DASH, unit: '%', sub1: 'waiting for a usage read', sub2: '' };
  if (usage !== null) {
    const { tokens, window } = usage.context;
    const percent = usage.context.percent ?? (tokens !== undefined && window > 0 ? Math.round((tokens / window) * 100) : undefined);
    context = {
      id: 'context',
      level: percent === undefined ? 'quiet' : percent >= 80 ? 'act' : percent >= 15 ? 'watch' : 'quiet',
      glyph: percent === undefined ? '○' : percent >= 80 ? '●' : percent >= 15 ? '◐' : '○',
      figure: percent === undefined ? DASH : String(percent),
      unit: '%',
      sub1: `${tokens === undefined ? '?' : formatTokens(tokens)} of ${formatTokens(window)}`,
      sub2: 'engine read',
    };
  }
  const blank = (id: string, unit: string): Tile => ({ id, level: 'quiet', glyph: '○', figure: DASH, unit, sub1: model.binary === 'missing' ? NEEDS_BINARY : 'waiting for cctop', sub2: '' });
  return [context, blank('cache', 'm'), blank('limits', '%'), blank('rework', '')];
}

function levelColor(level: string): string | undefined {
  return level === 'act' ? THEME.red : level === 'watch' ? THEME.yellow : undefined;
}

// One tile's three rows at `cells` wide: the digits in the level colour,
// the unit dim on the baseline, the glyph + name bold beside them, then the
// two sub-lines.
function tileLines(t: Tile, cells: number): [Line, Line, Line] {
  const color = levelColor(t.level);
  const digits = bigDigits(t.figure, color);
  const dw = Math.max(...digits.map((l) => width(l.map((s) => s.text).join(''))));
  const unitWidth = Math.max(1, width(t.unit));
  const textWidth = Math.max(1, cells - (dw + 2 + unitWidth + 2));
  const row = (i: number, text: string, style: Omit<Seg, 'text'>): Line => [
    seg(' '),
    ...fit(digits[i], dw),
    dim(` ${pad(i === 2 ? t.unit : '', unitWidth)} `),
    { text: pad(clip(text, textWidth), textWidth), ...style },
  ];
  const nameStyle: Omit<Seg, 'text'> = color === undefined ? { bold: true } : { bold: true, color };
  return [row(0, `${t.glyph} ${t.id}`, nameStyle), row(1, t.sub1, {}), row(2, t.sub2, {})];
}

/** The tiles two per row, each half the width. */
function tileRows(tiles: Tile[], columns: number, el: ViewElements): RenderElement[] {
  const half = Math.floor(columns / 2);
  const out: RenderElement[] = [];
  for (let i = 0; i < tiles.length; i += 2) {
    const pair = tiles.slice(i, i + 2).map((t) => tileLines(t, half));
    for (let r = 0; r < 3; r++) {
      const line: Line = [];
      for (const p of pair) line.push(...fit(p[r], half));
      out.push(textRow(line, el, r === 0 ? `coach_${tiles[i].id}` : undefined));
    }
    if (i + 2 < tiles.length) out.push(textRow([seg('')], el));
  }
  return out;
}

/** The header line: `cctop` bold, the session facts, the phase cell right-aligned. */
function headerLine(d: Dashboard, columns: number): Line {
  const facts = d.headerLine.replace(/^cctop\s+/, '');
  let phase = `${d.phase.glyph} ${d.phase.word}`;
  if (d.phase.tokens.length > 0) phase += ` · ${d.phase.tokens.join(' · ')}`;
  const phaseText = clip(phase, Math.min(48, Math.max(0, columns - 10)));
  const factsText = clip(facts, Math.max(0, columns - 8 - width(phaseText) - 2));
  const padCells = Math.max(1, columns - 8 - width(factsText) - width(phaseText));
  const phaseStyle: Omit<Seg, 'text'> =
    d.phase.glyph === '◆' ? { color: THEME.yellow } : d.phase.glyph === '○' ? { dim: true } : { color: ACCENT };
  return [seg(' cctop  ', { bold: true }), seg(factsText), seg(' '.repeat(padCells)), seg(phaseText, phaseStyle)];
}

/** The ledger's left column: the digit and the name. */
const GUTTER = 13;

// A ledger row: a plain Button `1: Context` … whose press opens the view or
// unfolds the block, then the values on the same line, the detail dim on
// the next (from 50 columns).
function ledgerRows(d: Dashboard, model: Model, columns: number, now: number, el: OverviewElements, actions: OverviewActions | undefined): RenderElement[] {
  const { Box, Button } = el;
  const out: RenderElement[] = [];
  for (const r of d.rows) {
    const values = fit(r.values, Math.max(1, columns - GUTTER));
    const label = `${r.digit} ${r.name}`;
    const gutter = pad(label, GUTTER - 1);
    const rowLine: RenderElement =
      actions === undefined ? (
        textRow([seg(gutter, { color: ACCENT, bold: true }), seg(' '), ...values], el, `ledger_${r.digit}`)
      ) : (
        <Box key={`ledger_${r.digit}`} flexDirection="row">
          <Button key={`ledger-${r.digit}`} label={pad(r.name, GUTTER - 4)} hotkey={String(r.digit)} plain onPress={() => actions.row(r.digit)} />
          {textRow([seg(' '), ...values], el)}
        </Box>
      );
    out.push(rowLine);
    if (columns >= TILES_MIN && r.detail.length > 0) {
      out.push(textRow([seg(' '.repeat(GUTTER)), ...fit(r.detail.map((s) => (s.color === undefined && !s.bold ? dim(s.text) : s)), Math.max(1, columns - GUTTER))], el));
    }
    if (r.digit >= 1 && r.digit <= 4 && model.unfolded.includes(r.digit)) {
      const block = [contextBlock, tokensBlock, (m: Model) => limitsBlock(m, now), (m: Model) => turnBlock(m, now)][r.digit - 1](model);
      out.push(blockFrame(block, columns, el));
    }
  }
  return out;
}

// The inline form (the classic renderer's few rows above the prompt): no
// frames, the header on one line, the coach's L1 line, then the Context and
// Limits rows.
function renderInline(model: Model, el: ViewElements, columns: number, now: number): RenderElement {
  const { Box } = el;
  const head = header(model, now);
  const blocks = [contextBlock(model), limitsBlock(model, now)];
  const rows: RenderElement[] = [
    textRow(
      [...statusPill(head.status), seg(` · turn ${head.turn} · ${head.elapsed} · ${head.model} · ${head.effort}  `), ...badgesLine(head.badges)],
      el,
      'session_status',
    ),
  ];
  const d = dashboardOf(model.query.dashboard);
  if (d !== null && d.lines.l1 !== '') rows.push(textRow([seg(d.lines.l1)], el, 'coach_context'));
  for (const block of blocks) {
    rows.push(textRow([seg(block.title, { bold: true })], el));
    for (const r of blockRows(block, columns)) rows.push(textRow(r.line, el, r.key));
  }
  return <Box flexDirection="column">{rows}</Box>;
}

/**
 * The Overview: the header line, the tiles (two per row; the L1 line below
 * TILES_MIN columns, L2 below L2_MAX), the nudge on two rows, the nine ledger
 * rows. Inline placement draws the flat short form (renderInline).
 */
export function renderOverview(
  model: Model,
  el: ViewElements,
  columns: number,
  placement: Placement,
  now: number,
  buttons?: { el: OverviewElements; actions: OverviewActions },
): RenderElement {
  const { Box } = el;
  if (placement === 'inline') return renderInline(model, el, columns, now);
  const d = dashboardOf(model.query.dashboard);
  const body: RenderElement[] = [];
  if (d === null) {
    // Before the binary answers: the engine's own header and tiles.
    const head = header(model, now);
    body.push(
      textRow(
        [seg(' cctop  ', { bold: true }), seg(`${head.model} · turn ${head.turn} · ${head.elapsed}  `), ...statusPill(head.status), seg('  '), ...badgesLine(head.badges)],
        el,
        'session_status',
      ),
    );
    if (columns >= TILES_MIN) body.push(...tileRows(engineTiles(model), columns, el));
    body.push(textRow([dim(model.binary === 'missing' ? NEEDS_BINARY : 'waiting for cctop query dashboard…')], el, 'advice_saving'));
    return <Box flexDirection="column">{body}</Box>;
  }
  body.push(textRow(headerLine(d, columns), el, 'session_status'));
  if (columns >= TILES_MIN) body.push(...tileRows(d.tiles, columns, el));
  else body.push(textRow([seg(` ${columns >= L2_MAX ? d.lines.l1 : d.lines.l2}`)], el, 'coach_context'));
  if (d.nudge !== null) {
    const [headline, action] = d.nudge.line.split(' — ');
    const tagStyle: Omit<Seg, 'text'> = { dim: true };
    body.push(textRow([seg(' ▸ ', { color: THEME.yellow }), seg(headline ?? d.nudge.line, { bold: true }), seg('  '), seg(d.nudge.tag, tagStyle)], el, 'advice_saving'));
    if (action !== undefined) body.push(textRow([seg('   '), seg(action, { color: ACCENT })], el));
  } else {
    body.push(textRow([dim(' quiet · nothing to act on')], el, 'advice_saving'));
  }
  body.push(...ledgerRows(d, model, columns, now, buttons?.el ?? { ...el, Button: el.Box as OverviewElements['Button'] }, buttons?.actions));
  return <Box flexDirection="column">{body}</Box>;
}

/** The view a ledger digit opens (5–9); rows 1–4 unfold instead. */
export function viewOfDigit(digit: number): View | null {
  return ({ 5: 'tools', 6: 'agents', 7: 'files', 8: 'events', 9: 'advisor' } as Record<number, View>)[digit] ?? null;
}
