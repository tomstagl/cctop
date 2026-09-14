// The pane's drawing primitives, shaped after the TUI (src/ui/panel.rs,
// src/ui/widgets.rs): a framed panel whose top border carries the hotkey,
// title and summary (`╭1 Context ─ 40 % est ──╮`), gauges of ▇ and ▁, a
// sparkline, and the colour bands. Every row is one truncating Text of inline
// segments, padded to the frame's width in TypeScript, so a row is three or
// four nodes and a whole view stays far below the engine's 2000-node cap.
//
// Colours are Claude Code theme keys, not palette names, so the pane follows
// the person's theme: `suggestion` is the accent (what the engine colours a
// Button's hotkey with), `success` / `warning` / `error` the three bands,
// and the borders are dimmed default text like the engine's own frames.
import type { RenderElement } from 'claude-code';
import type { Color, ViewElements } from './overview';

export const ACCENT = 'suggestion';
export const OK = 'success';
export const WARN = 'warning';
export const CRIT = 'error';

/** The TUI's palette roles, as the views name them, to the theme keys drawn. */
export const THEME: Record<Color, string> = { green: OK, yellow: WARN, red: CRIT, cyan: ACCENT };

/** One styled run of text inside a row; `key` names the metric it shows (docs/metrics.md). */
export type Seg = { text: string; color?: string; dim?: boolean; bold?: boolean; inverse?: boolean; key?: string };
export type Line = Seg[];

export const GAUGE_FILL = '▇';
export const GAUGE_EMPTY = '▁';
const SPARK = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
const H = '─';

export const seg = (text: string, style: Omit<Seg, 'text'> = {}): Seg => ({ text, ...style });
export const dim = (text: string): Seg => ({ text, dim: true });

/** Display columns of a string: every glyph the pane draws is one cell. */
export function width(text: string): number {
  return [...text].length;
}

export function lineWidth(line: Line): number {
  return line.reduce((n, s) => n + width(s.text), 0);
}

/** The colour band of a 0–1 fill, as the TUI's `band_style`. */
export function band(ratio: number, warn: number, crit: number): string {
  return ratio >= crit ? CRIT : ratio >= warn ? WARN : OK;
}

/** A gauge of `cells` columns filled to `ratio`, the fill in `color`, the rest dim. */
export function gauge(ratio: number, cells: number, color: string): Line {
  const n = Math.max(0, cells);
  const filled = Math.min(n, Math.round(Math.max(0, Math.min(1, ratio)) * n));
  const out: Line = [];
  if (filled > 0) out.push(seg(GAUGE_FILL.repeat(filled), { color }));
  if (n - filled > 0) out.push(dim(GAUGE_EMPTY.repeat(n - filled)));
  return out;
}

/** The last `cells` values scaled to their max, one ▁–█ glyph each; empty when all are zero. */
export function sparkline(values: readonly number[], cells: number): string {
  const tail = values.slice(Math.max(0, values.length - cells));
  const max = Math.max(0, ...tail);
  if (max <= 0) return '';
  return tail.map((v) => SPARK[Math.round((Math.max(0, v) / max) * 7)]).join('');
}

/** `text` cut to `cells` with `…`, or as it is when it fits. */
export function clip(text: string, cells: number): string {
  const chars = [...text];
  if (chars.length <= cells) return text;
  if (cells <= 0) return '';
  if (cells === 1) return '…';
  return chars.slice(0, cells - 1).join('') + '…';
}

/** `text` in a field of `cells`: left-aligned and space-padded, or right-aligned. */
export function pad(text: string, cells: number, right = false): string {
  const cut = clip(text, cells);
  const fill = ' '.repeat(Math.max(0, cells - width(cut)));
  return right ? fill + cut : cut + fill;
}

// Cuts a line at `cells` (an ellipsis on the segment that crosses the edge)
// and pads it with spaces to exactly `cells`, merging same-styled neighbours
// so a row stays a handful of nodes.
export function fit(line: Line, cells: number): Line {
  const out: Line = [];
  let used = 0;
  const push = (s: Seg): void => {
    if (s.text === '') return;
    const last = out[out.length - 1];
    if (last !== undefined && sameStyle(last, s)) last.text += s.text;
    else out.push({ ...s });
  };
  for (const s of line) {
    const room = cells - used;
    if (room <= 0) break;
    const w = width(s.text);
    if (w <= room) {
      push(s);
      used += w;
    } else {
      push({ ...s, text: clip(s.text, room) });
      used = cells;
    }
  }
  if (used < cells) push({ text: ' '.repeat(cells - used) });
  return out;
}

function sameStyle(a: Seg, b: Seg): boolean {
  return (
    a.color === b.color && !!a.dim === !!b.dim && !!a.bold === !!b.bold && !!a.inverse === !!b.inverse && a.key === b.key
  );
}

/** Joins lines with a separator segment. */
export function join(parts: Line[], separator: Seg): Line {
  const out: Line = [];
  parts.forEach((p, i) => {
    if (i > 0) out.push(separator);
    out.push(...p);
  });
  return out;
}

/** One row of the pane: a truncating Text of inline segments. `key` names the metric it shows. */
export function textRow(line: Line, el: ViewElements, key?: string): RenderElement {
  const { Box, Text } = el;
  const keyed = key === undefined ? {} : { key };
  return (
    <Box {...keyed}>
      <Text wrap="truncate">
        {line.map((s) => (
          <Text {...(s.key === undefined ? {} : { key: s.key })} color={s.color} dimColor={s.dim} bold={s.bold} inverse={s.inverse}>
            {s.text}
          </Text>
        ))}
      </Text>
    </Box>
  );
}

export type Frame = {
  /** The digit drawn in the accent colour before the title, as the TUI numbers its panels. */
  hotkey?: string;
  title: string;
  /** Drawn after the title, `─ summary`, as the TUI's panel summaries. */
  summary?: string;
  /** Rows inside the frame; each is fitted to the inner width. A `key` names its metric. */
  rows: { line: Line; key?: string }[];
  /** The whole frame's width in columns, borders included. */
  width: number;
  /** Accent-coloured border, as the TUI draws the focused panel. */
  focused?: boolean;
};

/** Columns a frame leaves for its rows: the two borders and one space each side. */
export function innerWidth(frameWidth: number): number {
  return Math.max(0, frameWidth - 4);
}

/**
 * A framed panel like the TUI's: `╭1 Title ─ summary ────╮`, the rows between
 * `│ ` and ` │`, `╰────╯`. Every line is exactly `width` columns.
 */
export function frame(f: Frame, el: ViewElements): RenderElement {
  const { Box } = el;
  const w = Math.max(4, f.width);
  const border: Omit<Seg, 'text'> = f.focused ? { color: ACCENT } : { dim: true };
  const inner = innerWidth(w);
  // The title run is trimmed before the fill so the corner always lands.
  const head: Line = [seg('╭', border)];
  if (f.hotkey !== undefined) head.push(seg(f.hotkey, { color: ACCENT, bold: true }), seg(' ', border));
  head.push(seg(f.title, { bold: true }));
  if (f.summary !== undefined && f.summary !== '') head.push(seg(` ${H} `, border), seg(f.summary));
  head.push(seg(' ', border));
  const used = lineWidth(head);
  const top: Line =
    used + 1 <= w
      ? [...head, seg(H.repeat(w - used - 1), border), seg('╮', border)]
      : [...fit(head, w - 1), seg('╮', border)];
  const bottom: Line = [seg('╰', border), seg(H.repeat(w - 2), border), seg('╯', border)];
  const rows = f.rows.map((r) => textRow([seg('│ ', border), ...fit(r.line, inner), seg(' │', border)], el, r.key));
  return (
    <Box flexDirection="column" width={w} flexShrink={0}>
      {textRow(top, el)}
      {rows}
      {textRow(bottom, el)}
    </Box>
  );
}

/** Frames side by side on one row; each keeps its own width. */
export function sideBySide(frames: RenderElement[], el: ViewElements): RenderElement {
  const { Box } = el;
  return <Box flexDirection="row">{frames}</Box>;
}

/** Widths of the left and right frames that together fill `columns`. */
export function split(columns: number): [number, number] {
  const left = Math.floor(columns / 2);
  return [left, columns - left];
}
