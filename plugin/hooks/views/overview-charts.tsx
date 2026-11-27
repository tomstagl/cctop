// The overview in the pane (mod-pane US-007): Console's body 1 drawn from
// `cctop query overview`, the way the TUI draws it (src/ui/overview_view.rs):
// context across the width, then spend beside cost per turn, then the cache
// beside the tool calls, stacked below PAIRS columns, the footer last. The
// sizes are the TUI's for the same width — `layout`, `dims`, `chartSize` and
// `maxTurnsFor` mirror its functions — so a chart the person sees in the pane
// is the chart the TUI draws, cell for cell (row identity).
//
// Every chart is a picture placed on whole cells: an `Image` (a PNG from
// charts.wasm) or a `Raster` (cells), as the session's picture-or-cells
// decision says. The view draws no picture itself: pane.tsx renders them off
// the render hook, keyed as `pictureJobs` names them, and hands them in
// (`Pictures`); one not drawn yet is a blank box of its size. Labels, axis
// values and verdicts are text beside the pictures, never inside them.
//
// Below SPARKLINE columns every chart is a Raster one row high. Over each
// per-turn chart a keyed Box per turn column reveals that turn's readout
// under the chart on hover (no code runs on hover); the turn numbers are
// Buttons that set the readout for a terminal that reports no pointer.
import type { ElementTable, RenderElement } from 'claude-code';
import type { Picture, PictureJob, RasterCells } from '../charts';
import { at, stringAt } from './format';
import { ACCENT, THEME, dim, seg, textRow, width, type Line, type Seg } from './frame';

/** Two columns from this many body columns; the pairs stack below (the TUI's `PAIRS`). */
export const PAIRS = 100;
/** Below this many body columns every chart is one row of cells (spec §5.2). */
export const SPARKLINE = 60;
/** The y-axis labels' least column beside a chart. */
const GUTTER = 6;
/** Cells a turn takes in a per-turn chart when the turns are few: two for the mark, one of gap. */
const TURN_CELLS = 3;
/** The cache countdown bar's widest. */
const COUNTDOWN = 30;
/** The widest tool name the heat map's labels keep. */
const NAME_MAX = 12;
/** The widest a Raster may be. */
const MAX_COLS = 512;

// ------------------------------------------------------------------ the object

export type Axis = { from: number; to: number; unit: string };
export type ChartSpec = { kind: string; x: Axis; y: Axis; series: { name: string }[] };
export type Readout = { x: number; text: string };
export type Legend = { role: string; text: string };
export type Prevent = { turn: number; cause: string; tokens: number; habit: string };
export type Level = 'good' | 'watch' | 'act';
export type Chart = {
  id: string;
  title: string;
  figure: string;
  /** `metric_id` names the verdict in docs/metrics.md (`verdict_context`): the title row's key. */
  verdict: { level: Level; text: string; todo: string | null; metricId: string | null };
  note: string | null;
  label: string | null;
  spec: ChartSpec;
  bar: ChartSpec | null;
  readouts: Readout[];
  legend: Legend[];
  barLegend: Legend[];
  prevent: Prevent[];
  fallback: RasterCells | undefined;
  barFallback: RasterCells | undefined;
};
export type Overview = { charts: Chart[]; footer: string };

const num = (v: unknown, fallback = 0): number => (typeof v === 'number' && Number.isFinite(v) ? v : fallback);

function axisOf(v: unknown): Axis {
  return { from: num(at(v, 'from')), to: num(at(v, 'to')), unit: stringAt(v, 'unit') ?? '' };
}

function specOf(v: unknown): ChartSpec | null {
  if (v === null || typeof v !== 'object') return null;
  const kind = stringAt(v, 'kind');
  if (kind === null) return null;
  const series = at(v, 'series');
  // The spec goes to the wasm as it came: only what the view reads is checked.
  const spec = v as ChartSpec;
  return { ...spec, kind, x: axisOf(at(v, 'x')), y: axisOf(at(v, 'y')), series: Array.isArray(series) ? (series as { name: string }[]) : [] };
}

function legendOf(v: unknown): Legend[] {
  return Array.isArray(v) ? v.map((l) => ({ role: stringAt(l, 'role') ?? 'fg', text: stringAt(l, 'text') ?? '' })) : [];
}

function cellsOf(v: unknown): RasterCells | undefined {
  const cells = stringAt(v, 'cells');
  if (cells === null) return undefined;
  return { cols: num(at(v, 'cols'), 1), rows: num(at(v, 'rows'), 1), cells };
}

/** The overview object as the pane reads it, or null before the binary answered. */
export function overviewOf(query: unknown): Overview | null {
  const charts = at(query, 'charts');
  if (!Array.isArray(charts)) return null;
  const out: Chart[] = [];
  for (const c of charts) {
    const spec = specOf(at(c, 'spec'));
    const id = stringAt(c, 'id');
    if (spec === null || id === null) continue;
    const level = stringAt(c, 'verdict', 'level');
    out.push({
      id,
      title: stringAt(c, 'title') ?? id,
      figure: stringAt(c, 'figure') ?? '',
      verdict: {
        level: level === 'watch' || level === 'act' ? level : 'good',
        text: stringAt(c, 'verdict', 'text') ?? '',
        todo: stringAt(c, 'verdict', 'todo'),
        metricId: stringAt(c, 'verdict', 'metric_id'),
      },
      note: stringAt(c, 'note'),
      label: stringAt(c, 'label'),
      spec,
      bar: specOf(at(c, 'bar')),
      readouts: Array.isArray(at(c, 'readouts'))
        ? (at(c, 'readouts') as unknown[]).map((r) => ({ x: num(at(r, 'x')), text: stringAt(r, 'text') ?? '' }))
        : [],
      legend: legendOf(at(c, 'legend')),
      barLegend: legendOf(at(c, 'bar_legend')),
      prevent: Array.isArray(at(c, 'prevent'))
        ? (at(c, 'prevent') as unknown[]).map((p) => ({
            turn: num(at(p, 'turn')),
            cause: stringAt(p, 'cause') ?? '',
            tokens: num(at(p, 'tokens')),
            habit: stringAt(p, 'habit') ?? '',
          }))
        : [],
      fallback: cellsOf(at(c, 'fallback_cells')),
      barFallback: cellsOf(at(c, 'bar_fallback_cells')),
    });
  }
  return { charts: out, footer: stringAt(query, 'footer') ?? '' };
}

// --------------------------------------------------------------------- sizes

/** The screen split: context across `inner` cells, the pairs in `left` and `right` from PAIRS columns, else stacked. The TUI's `layout`. */
export type Layout = { inner: number; two: boolean; left: number; right: number; sparkline: boolean };

export function layout(columns: number): Layout {
  const inner = Math.max(1, columns - 2);
  // The rule between the columns takes three cells.
  const left = Math.floor(Math.max(0, inner - 3) / 2);
  return { inner, two: columns >= PAIRS, left, right: Math.max(0, inner - 3) - left, sparkline: columns < SPARKLINE };
}

/** The cells a chart's section is drawn in (the TUI's `Layout::section_width`). */
export function sectionWidth(l: Layout, id: string): number {
  if (!l.two) return l.inner;
  if (id === 'spend' || id === 'cache') return l.left;
  if (id === 'cost_per_turn' || id === 'tool_calls') return l.right;
  return l.inner;
}

/** The turns a per-turn chart keeps at this width: two cells a turn beside the heat map's names (the TUI's `build`). */
export function maxTurnsFor(columns: number): number {
  const l = layout(columns);
  const narrowest = l.two ? l.right : l.inner;
  return Math.max(1, Math.floor(Math.max(0, narrowest - (NAME_MAX + 1)) / 2));
}

/** The turns a per-turn chart's x axis spans. */
export function turnsIn(spec: ChartSpec): number {
  return Math.max(1, Math.floor(Math.max(0, spec.x.to - spec.x.from)) + 1);
}

/** The size a spec is drawn at when nothing says otherwise (src/overview.rs `default_size`). */
export function defaultSize(spec: ChartSpec): [number, number] {
  switch (spec.kind) {
    case 'area_forecast':
      return [98, 8];
    case 'cumulative_forecast':
    case 'bars':
      return [46, 8];
    case 'pills':
      return [46, 2];
    case 'heat':
      return [46, Math.min(8, Math.max(1, spec.series.length))];
    case 'stacked_bar':
      return [98, 1];
    default:
      return [30, 1];
  }
}

/** The cells a chart takes beside its axis labels: the whole width, except a per-turn chart, three cells a turn when the turns are few. */
export function chartSize(spec: ChartSpec, avail: number): [number, number] {
  const a = Math.min(MAX_COLS, Math.max(1, avail));
  const [, rows] = defaultSize(spec);
  const n = turnsIn(spec);
  if (spec.kind === 'bars' || spec.kind === 'pills') return [Math.max(1, Math.min(a, n * TURN_CELLS)), rows];
  if (spec.kind === 'heat') return [Math.max(1, Math.min(a, n * 2)), rows];
  return [a, rows];
}

/** `v` to `d` decimals as Rust's `{:.d}` prints it: ties to even. */
function fixed(v: number, d: number): string {
  const scaled = v * 10 ** d;
  if (Number.isInteger(scaled * 2) && !Number.isInteger(scaled)) {
    const down = Math.floor(scaled);
    const even = down % 2 === 0 ? down : down + 1;
    return (even / 10 ** d).toFixed(d);
  }
  return v.toFixed(d);
}

/** Tokens as the TUI prints them (src/ui/fmt.rs `tokens`). */
export function tokens(n: number): string {
  const v = Math.max(0, Math.floor(n));
  if (v <= 999) return String(v);
  if (v <= 9_999) return `${fixed(v / 1e3, 1)}k`;
  if (v <= 999_999) return `${Math.floor(v / 1000)}k`;
  return `${fixed(v / 1e6, 2)}M`;
}

/** An axis label: tokens as `412k`, dollars compact past $100 (the TUI's `axis_label`). */
export function axisLabel(unit: string, v: number): string {
  if (unit === 'tokens') return tokens(v);
  if (unit === 'USD') {
    if (v === 0) return '$0';
    if (v >= 1000) return `$${fixed(v / 1000, 1)}k`;
    if (v >= 100) return `$${fixed(v, 0)}`;
    return `$${fixed(v, 2)}`;
  }
  return fixed(v, 0);
}

/** The top, middle and bottom labels of a chart's y axis. */
function yLabels(spec: ChartSpec, rows: number): [string, string, string] {
  const y = spec.y;
  return [axisLabel(y.unit, y.to), rows >= 6 ? axisLabel(y.unit, (y.from + y.to) / 2) : '', axisLabel(y.unit, y.from)];
}

/** `s` cut to `max` characters with `…`, as the TUI's `fmt::clip` (zero keeps the ellipsis). */
function clipChars(s: string, max: number): string {
  const chars = [...s];
  if (chars.length <= max) return s;
  return `${chars.slice(0, Math.max(0, max - 1)).join('')}…`;
}

function namesGutter(spec: ChartSpec): number {
  return Math.max(0, ...spec.series.map((s) => width(clipChars(s.name, NAME_MAX)))) + 1;
}

/** Where a chart sits in its section: the label column to its left and the cells it is drawn at. */
export type Dims = { gutter: number; cols: number; rows: number };

/** `c`'s Dims in a section `w` cells wide (the TUI's `dims`); one row and no labels in the sparkline form. */
export function dims(c: Chart, w: number, sparkline = false): Dims {
  let gutter: number;
  if (sparkline || c.id === 'cache') gutter = 0;
  else if (c.id === 'tool_calls') gutter = namesGutter(c.spec);
  else gutter = Math.max(GUTTER, ...yLabels(c.spec, defaultSize(c.spec)[1]).map((l) => width(l) + 1));
  const [cols, rows] = chartSize(c.spec, Math.max(0, w - gutter));
  return { gutter, cols, rows: sparkline ? 1 : rows };
}

const PER_TURN = new Set(['cost_per_turn', 'cache', 'tool_calls']);

/** The picture keys of a chart and of its bar. */
export const chartKey = (id: string): string => `chart-${id}`;
export const barKey = (id: string): string => `chart-${id}-bar`;

/** The cache countdown's width: what is left beside its note, at most COUNTDOWN. */
function countdownWidth(note: string, w: number): number {
  return Math.min(COUNTDOWN, Math.max(1, w - width(`${note} `)));
}

/** Every picture the overview draws at `columns`, at the size it is drawn: what pane.tsx renders off the render hook. */
export function pictureJobs(o: Overview, columns: number): PictureJob[] {
  const l = layout(columns);
  const jobs: PictureJob[] = [];
  for (const c of o.charts) {
    const w = sectionWidth(l, c.id);
    const d = dims(c, w, l.sparkline);
    jobs.push({ key: chartKey(c.id), spec: c.spec, cols: d.cols, rows: d.rows, cells: l.sparkline, ...(c.fallback ? { fallback: c.fallback } : {}) });
    if (c.bar === null) continue;
    if (c.id === 'context') {
      jobs.push({ key: barKey(c.id), spec: c.bar, cols: Math.min(MAX_COLS, Math.max(1, w)), rows: 1, cells: l.sparkline, ...(c.barFallback ? { fallback: c.barFallback } : {}) });
    } else if (c.id === 'cache' && c.note !== null) {
      jobs.push({ key: barKey(c.id), spec: c.bar, cols: countdownWidth(c.note, w), rows: 1, cells: l.sparkline, ...(c.barFallback ? { fallback: c.barFallback } : {}) });
    }
  }
  return jobs;
}

// --------------------------------------------------------------------- draw

/** The rendered pictures, by key; undefined for one not drawn yet. */
export type Pictures = { get(key: string): Picture | undefined };

export type ChartElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button' | 'Raster' | 'Image'>;

export type ChartsActions = {
  /** A turn number pressed under a per-turn chart: show that turn's readout. */
  readout(chart: string, turn: number): void;
};

/**
 * A legend swatch's style by the role it names: the pane draws in the
 * theme's four colours and no hex (views/frame.tsx), so the series steps
 * s0 / s1 / s2 go by weight — dim, plain, bold — as Console's bars do.
 */
const ROLE_STYLE: Record<string, Omit<Seg, 'text'>> = {
  accent: { color: ACCENT },
  ok: { color: THEME.green },
  warn: { color: THEME.yellow },
  crit: { color: THEME.red },
  dim: { dim: true },
  s0: { dim: true },
  s2: { bold: true },
};

const LEVEL: Record<Level, { label: string; color: string }> = {
  good: { label: 'Good', color: THEME.green },
  watch: { label: 'Watch', color: THEME.yellow },
  act: { label: 'Act', color: THEME.red },
};

/** Word-wrap segments into rows of at most `columns` cells at spaces (the TUI's `wrap`, FR-7). */
function wrap(segs: Seg[], columns: number, indent: number): Line[] {
  const rows: Line[] = [[]];
  let used = 0;
  for (const s of segs) {
    let pending = '';
    const flush = (): void => {
      if (pending !== '') rows.at(-1)?.push({ ...s, text: pending });
      pending = '';
    };
    for (const word of s.text.split(/(?<= )/)) {
      const w = width(word);
      if (used + w > columns && used > indent) {
        flush();
        rows.push([seg(' '.repeat(indent))]);
        used = indent;
        const trimmed = word.replace(/^\s+/, '');
        pending += trimmed;
        used += width(trimmed);
      } else {
        pending += word;
        used += w;
      }
    }
    flush();
  }
  return rows;
}

/** A block of rows: its element and how many rows it takes (a pair's rule needs the taller). */
type Block = { el: RenderElement; rows: number };

type Ctx = {
  el: ChartElements;
  pictures: Pictures | undefined;
  readouts: Record<string, number>;
  actions: ChartsActions | undefined;
  sparkline: boolean;
};

const textBlock = (line: Line, ctx: Ctx, key?: string): Block => ({ el: textRow(line.length === 0 ? [seg('')] : line, ctx.el, key), rows: 1 });

/** `Title  figure`, the note (or the "last N turns" label) right-aligned when it fits. */
function titleLine(c: Chart, w: number): Line {
  // The cache's note (the countdown) is drawn once, beside its bar.
  const right = (c.id === 'cache' ? null : c.note) ?? c.label ?? '';
  const figure = clipChars(c.figure, Math.max(0, w - (width(c.title) + 2)));
  const used = width(c.title) + 2 + width(figure);
  const shown = width(right) + 2 + used <= w ? right : '';
  const gap = Math.max(0, w - (used + width(shown)));
  return [seg(c.title, { bold: true }), seg('  '), seg(figure, { color: ACCENT }), seg(' '.repeat(gap)), dim(shown)];
}

/** `Good` / `Watch` / `Act` in its colour, the sentence, the `Do:` bold, wrapped under the word. */
function verdictLines(c: Chart, w: number): Line[] {
  const level = LEVEL[c.verdict.level];
  const segs: Seg[] = [seg(`${level.label} `, { color: level.color }), seg(c.verdict.text)];
  if (c.verdict.todo !== null) segs.push(seg(' '), seg(c.verdict.todo, { bold: true }));
  return wrap(segs, w, width(level.label) + 1);
}

/** `■ text` per entry, the swatch in the chart's colour, wrapped at `w`; an entry longer than the row is clipped. */
function legendLines(items: Legend[], w: number): Line[] {
  const rows: Line[] = [[]];
  let used = 0;
  for (const l of items) {
    const text = clipChars(l.text, Math.max(0, w - 2));
    const n = 2 + width(text);
    if (used > 0 && used + 2 + n > w) {
      rows.push([]);
      used = 0;
    }
    const row = rows.at(-1)!;
    if (used > 0) {
      row.push(seg('  '));
      used += 2;
    }
    row.push(seg('■', ROLE_STYLE[l.role] ?? {}), dim(` ${text}`));
    used += n;
  }
  return rows.filter((r) => r.length > 0);
}

/** The turn numbers' places under a per-turn chart: where each fits (the TUI's `turn_numbers`). */
export function turnNumbers(spec: ChartSpec, cols: number): { at: number; label: string; turn: number }[] {
  const n = turnsIn(spec);
  const first = Math.floor(spec.x.from);
  const out: { at: number; label: string; turn: number }[] = [];
  let used = 0;
  for (let i = 0; i < n; i++) {
    const at = Math.floor((i * cols) / n);
    const label = String(first + i);
    if (!(at > used || (at === 0 && used === 0))) continue;
    used = at;
    if (at + label.length <= cols) {
      out.push({ at, label, turn: first + i });
      used += label.length + 1;
    }
  }
  return out;
}

/** The turn numbers row: each a plain Button that sets the readout, the gaps text. */
function numbersRow(c: Chart, indent: number, cols: number, ctx: Ctx): Block {
  const { Box, Button } = ctx.el;
  const places = turnNumbers(c.spec, cols);
  if (ctx.actions === undefined) {
    let text = '';
    for (const p of places) text += ' '.repeat(p.at - text.length) + p.label;
    return textBlock([seg(' '.repeat(indent)), dim(text)], ctx);
  }
  const actions = ctx.actions;
  const parts: RenderElement[] = [];
  let used = -indent;
  for (const p of places) {
    const gap = p.at - used;
    if (gap > 0) parts.push(textRow([seg(' '.repeat(gap))], ctx.el));
    parts.push(<Button key={`turn-${c.id}-${p.turn}`} label={p.label} plain dimColor onPress={() => actions.readout(c.id, p.turn)} />);
    used = p.at + p.label.length;
  }
  return { el: <Box flexDirection="row">{parts}</Box>, rows: 1 };
}

/** A picture placed on whole cells, or a blank box its size until it is drawn. */
function pictureEl(key: string, cols: number, rows: number, alt: string, ctx: Ctx): { el: RenderElement; cols: number; rows: number } {
  const { Box, Raster, Image } = ctx.el;
  const p = ctx.pictures?.get(key);
  if (p === undefined) return { el: <Box width={cols} height={rows} flexShrink={0} />, cols, rows };
  if (p.kind === 'image') return { el: <Image key={key} source={{ png: p.png }} columns={p.cols} rows={p.rows} alt={alt} />, cols: p.cols, rows: p.rows };
  return { el: <Raster key={key} columns={p.cols} rows={p.rows} cells={p.cells} />, cols: p.cols, rows: p.rows };
}

/** The hover scope a turn column and its readout share. */
export const turnScope = (chart: string, turn: number): string => `cctop-${chart}-turn-${turn}`;

/**
 * The chart's cells beside its label column, with a keyed Box laid over
 * each turn column of a per-turn chart: the pointer over it lights the
 * turn's scope, which reveals its readout (`readoutBlock`).
 */
function chartBand(c: Chart, d: Dims, labels: string[], ctx: Ctx): { block: Block; cols: number } {
  const { Box, Text } = ctx.el;
  const alt = c.readouts.at(-1)?.text ?? `${c.title} ${c.figure}`;
  const pic = pictureEl(chartKey(c.id), d.cols, d.rows, alt, ctx);
  const overlays: RenderElement[] = [];
  if (PER_TURN.has(c.id)) {
    const n = turnsIn(c.spec);
    const first = Math.floor(c.spec.x.from);
    for (let i = 0; i < n; i++) {
      const x0 = Math.floor((i * pic.cols) / n);
      const x1 = Math.floor(((i + 1) * pic.cols) / n);
      if (x1 <= x0) continue;
      const turn = first + i;
      overlays.push(
        <Box key={`hover-${c.id}-${turn}`} position="absolute" top={0} left={x0} width={x1 - x0} height={pic.rows} hover={{ scope: turnScope(c.id, turn) }} />,
      );
    }
  }
  const gutter =
    d.gutter > 0 ? (
      <Box flexDirection="column" width={d.gutter} flexShrink={0}>
        {Array.from({ length: pic.rows }, (_, i) => (
          <Text wrap="truncate" dimColor>
            {(labels[i] ?? '').padEnd(d.gutter)}
          </Text>
        ))}
      </Box>
    ) : null;
  return {
    block: {
      el: (
        <Box flexDirection="row" height={pic.rows}>
          {gutter}
          <Box key={`plot-${c.id}`} width={pic.cols} height={pic.rows} flexShrink={0} position="relative">
            {pic.el}
            {overlays}
          </Box>
        </Box>
      ),
      rows: pic.rows,
    },
    cols: pic.cols,
  };
}

/** The y labels per row of a chart with an axis column: top, middle, bottom. */
function axisColumn(c: Chart, rows: number): string[] {
  const [top, mid, bottom] = yLabels(c.spec, rows);
  const last = rows - 1;
  return Array.from({ length: rows }, (_, i) => (i === 0 ? top : i === last ? bottom : i === Math.floor(last / 2) ? mid : ''));
}

/** The x axis' ends under a time chart: `turn 1 … 6`, `min 0 … 626`. */
function xAxisLine(c: Chart, gutter: number, cols: number): Line {
  const x = c.spec.x;
  const from = `${x.unit} ${axisLabel(x.unit, x.from)}`;
  const to = axisLabel(x.unit, x.to);
  const gap = Math.max(0, cols - (width(from) + width(to)));
  return [seg(' '.repeat(gutter)), dim(`${from}${' '.repeat(gap)}${to}`)];
}

/** The readout lines a text takes: wrapped dim under the chart, at most two rows. */
function readoutLines(text: string, w: number): Line[] {
  return wrap([dim(text)], w, 2).slice(0, 2);
}

/**
 * The readout under a chart: the turn the person picked (a turn number's
 * press) or the latest. Over a per-turn chart, one hidden Box per turn laid
 * over it, revealed while its turn column is hovered.
 */
function readoutBlock(c: Chart, w: number, ctx: Ctx, hover: boolean): Block | null {
  if (c.readouts.length === 0) return null;
  const { Box, Text } = ctx.el;
  const picked = ctx.readouts[c.id];
  const shown = c.readouts.find((r) => r.x === picked) ?? c.readouts.at(-1)!;
  const base = readoutLines(shown.text, w);
  if (!hover) return { el: <Box flexDirection="column">{base.map((l) => textRow(l, ctx.el))}</Box>, rows: base.length };
  const n = turnsIn(c.spec);
  const first = Math.floor(c.spec.x.from);
  const turns = c.readouts.filter((r) => r.x >= first && r.x < first + n && Number.isInteger(r.x));
  const rows = Math.max(base.length, ...turns.map((r) => readoutLines(r.text, w).length));
  const padded = (lines: Line[]): string[] =>
    Array.from({ length: rows }, (_, i) => (lines[i] ?? []).map((s) => s.text).join('').padEnd(w).slice(0, Math.max(w, 0)));
  return {
    el: (
      <Box key={`readout-${c.id}`} flexDirection="column" height={rows} position="relative">
        {Array.from({ length: rows }, (_, i) => textRow(base[i] ?? [seg('')], ctx.el))}
        {turns.map((r) => (
          <Box position="absolute" top={0} left={0} width={w} height={rows} flexDirection="column" display="none" hover={{ scope: turnScope(c.id, r.x), display: 'flex' }}>
            {padded(readoutLines(r.text, w)).map((line) => (
              <Text wrap="truncate">{line}</Text>
            ))}
          </Box>
        ))}
      </Box>
    ),
    rows,
  };
}

function column(blocks: Block[], ctx: Ctx, w?: number): Block {
  const { Box } = ctx.el;
  const sized = w === undefined ? {} : { width: w, flexShrink: 0 };
  return { el: <Box flexDirection="column" {...sized}>{blocks.map((b) => b.el)}</Box>, rows: blocks.reduce((n, b) => n + b.rows, 0) };
}

function lines(ls: Line[], ctx: Ctx): Block[] {
  return ls.map((l) => textBlock(l, ctx));
}

function head(c: Chart, w: number, ctx: Ctx): Block[] {
  return [textBlock(titleLine(c, w), ctx, c.verdict.metricId ?? undefined), ...lines(verdictLines(c, w), ctx)];
}

/** A chart with its y axis on the left, the x axis' ends (or the turn numbers) under it. */
function axisChart(c: Chart, w: number, ctx: Ctx, numbers: boolean): Block[] {
  const d = dims(c, w, ctx.sparkline);
  const band = chartBand(c, d, axisColumn(c, d.rows), ctx);
  const out = [band.block];
  out.push(numbers ? numbersRow(c, d.gutter, band.cols, ctx) : textBlock(xAxisLine(c, d.gutter, band.cols), ctx));
  return out;
}

function contextSection(c: Chart, w: number, ctx: Ctx): Block[] {
  const out = head(c, w, ctx);
  if (c.bar !== null) {
    const alt = c.barLegend.map((l) => l.text).join(' · ') || ' ';
    const bar = pictureEl(barKey(c.id), Math.min(MAX_COLS, Math.max(1, w)), 1, alt, ctx);
    out.push({ el: bar.el, rows: bar.rows });
    out.push(...lines(legendLines(c.barLegend, w), ctx));
  }
  out.push(...lines(legendLines(c.legend, w), ctx));
  out.push(...axisChart(c, w, ctx, false));
  const r = readoutBlock(c, w, ctx, false);
  if (r !== null) out.push(r);
  return out;
}

function spendSection(c: Chart, w: number, ctx: Ctx): Block[] {
  return [...head(c, w, ctx), ...lines(legendLines(c.legend, w), ctx), ...axisChart(c, w, ctx, false)];
}

function costSection(c: Chart, w: number, ctx: Ctx): Block[] {
  const out = [...head(c, w, ctx), ...lines(legendLines(c.legend, w), ctx), ...axisChart(c, w, ctx, true)];
  const r = readoutBlock(c, w, ctx, true);
  if (r !== null) out.push(r);
  return out;
}

function cacheSection(c: Chart, w: number, ctx: Ctx): Block[] {
  const out = [...head(c, w, ctx), ...lines(legendLines(c.legend, w), ctx)];
  const d = dims(c, w, ctx.sparkline);
  const band = chartBand(c, d, [], ctx);
  out.push(band.block, numbersRow(c, 0, band.cols, ctx));
  const r = readoutBlock(c, w, ctx, true);
  if (r !== null) out.push(r);
  if (c.note !== null && c.bar !== null) {
    const { Box } = ctx.el;
    const label = `${c.note} `;
    const bar = pictureEl(barKey(c.id), countdownWidth(c.note, w), 1, c.note, ctx);
    out.push({ el: <Box flexDirection="row">{textRow([dim(label)], ctx.el)}{bar.el}</Box>, rows: 1 });
  }
  if (c.prevent.length > 0) {
    out.push(textBlock([], ctx), textBlock([{ text: 'How to avoid each rebuild', dim: true, bold: true }], ctx));
    for (const p of c.prevent) {
      const headText = `turn ${p.turn}`;
      const indent = Math.max(9, width(headText) + 1);
      out.push(textBlock([seg(headText, { color: THEME.red }), seg(' '.repeat(indent - width(headText))), dim(clipChars(`${p.cause} · ${tokens(p.tokens)}`, Math.max(0, w - indent)))], ctx));
      out.push(textBlock([seg(' '.repeat(indent)), seg(clipChars(p.habit, Math.max(0, w - indent)))], ctx));
    }
  }
  return out;
}

function toolsSection(c: Chart, w: number, ctx: Ctx): Block[] {
  const out = [...head(c, w, ctx), ...lines(legendLines(c.legend, w), ctx)];
  const d = dims(c, w, ctx.sparkline);
  const names = c.spec.series.map((s) => clipChars(s.name, NAME_MAX));
  const labels = d.gutter === 0 ? [] : names.map((n) => `${n.padStart(d.gutter - 1)} `);
  const band = chartBand(c, d, labels, ctx);
  out.push(band.block, numbersRow(c, d.gutter, band.cols, ctx));
  const r = readoutBlock(c, w, ctx, true);
  if (r !== null) out.push(r);
  return out;
}

function section(c: Chart, w: number, ctx: Ctx): Block[] {
  switch (c.id) {
    case 'context':
      return contextSection(c, w, ctx);
    case 'spend':
      return spendSection(c, w, ctx);
    case 'cost_per_turn':
      return costSection(c, w, ctx);
    case 'cache':
      return cacheSection(c, w, ctx);
    default:
      return toolsSection(c, w, ctx);
  }
}

const rule = (w: number): Line => [dim('─'.repeat(w))];

/** Two sections side by side, ` │ ` between them as tall as the taller. */
function pair(a: Block[], b: Block[], l: Layout, ctx: Ctx): Block {
  const { Box } = ctx.el;
  const left = column(a, ctx, l.left);
  const right = column(b, ctx, l.right);
  const rows = Math.max(left.rows, right.rows);
  const sep = column(
    Array.from({ length: rows }, () => textBlock([dim(' │ ')], ctx)),
    ctx,
    3,
  );
  return {
    el: (
      <Box flexDirection="row">
        {left.el}
        {sep.el}
        {right.el}
      </Box>
    ),
    rows,
  };
}

/** What the overview body draws with: the elements, the pictures pane.tsx rendered, the picked readouts, the presses. */
export type OverviewChartsInput = {
  el: ChartElements;
  pictures?: Pictures;
  readouts?: Record<string, number>;
  actions?: ChartsActions;
};

/** The overview's rows at `columns`: no height limit, the pane's body scrolls. */
export function renderOverviewCharts(o: Overview, columns: number, input: OverviewChartsInput): RenderElement {
  const { Box } = input.el;
  const l = layout(columns);
  const ctx: Ctx = { el: input.el, pictures: input.pictures, readouts: input.readouts ?? {}, actions: input.actions, sparkline: l.sparkline };
  const by = (id: string): Chart | undefined => o.charts.find((c) => c.id === id);
  const out: Block[] = [];
  const context = by('context');
  if (context !== undefined) {
    out.push(...section(context, l.inner, ctx), textBlock(rule(l.inner), ctx));
  }
  for (const [ida, idb] of [
    ['spend', 'cost_per_turn'],
    ['cache', 'tool_calls'],
  ] as const) {
    const a = by(ida);
    const b = by(idb);
    if (a === undefined || b === undefined) continue;
    if (l.two) out.push(pair(section(a, l.left, ctx), section(b, l.right, ctx), l, ctx));
    else out.push(...section(a, l.inner, ctx), textBlock([], ctx), ...section(b, l.inner, ctx));
    out.push(textBlock(rule(l.inner), ctx));
  }
  // Wrapped, never cut: one row from 66 columns, as the TUI draws it.
  out.push(...lines(wrap([dim(o.footer)], l.inner, 0), ctx));
  return (
    <Box flexDirection="column" paddingLeft={1}>
      {out.map((b) => b.el)}
    </Box>
  );
}
