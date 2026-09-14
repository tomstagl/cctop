// Shared by the detail views: a table row of fixed-width and flexible cells
// laid out into one frame row, the dim single line a view draws when it has
// nothing to show (or lacks the binary), the framed panel every detail view
// is, and the row cap they keep to. A cell that shows a metric carries its
// id as the row's `key` (docs/metrics.md).
import type { RenderElement } from 'claude-code';
import { THEME, clip, frame, innerWidth, pad, width, type Line, type Seg } from './frame';
import type { Color, ViewElements } from './overview';

/** The most rows a view's frame holds: longer lists are cut, the pane scrolls the rest. */
export const MAX_ROWS = 120;

export type Cell = {
  text: string;
  /** Columns the cell takes; the one cell without it takes the rest of the row. */
  width?: number;
  /** Right-aligned inside its width (numbers); left by default. */
  right?: boolean;
  /** Keeps the end of an overlong text (a path) rather than its start. */
  tail?: boolean;
  color?: Color;
  dim?: boolean;
  bold?: boolean;
  /** The metric id the cell shows, as docs/metrics.md names it. */
  key?: string;
};

/** One row of a framed panel: its segments and the metric id it shows. */
export type FrameRow = { line: Line; key?: string };

/** `text` kept from its end when it overflows `cells` (a path), else cut at the end. */
function clipTail(text: string, cells: number): string {
  const chars = [...text];
  if (chars.length <= cells) return text;
  if (cells <= 1) return clip(text, cells);
  return '…' + chars.slice(chars.length - (cells - 1)).join('');
}

function cellSeg(cell: Cell, cells: number): Seg {
  const text = cell.tail ? clipTail(cell.text, cells) : clip(cell.text, cells);
  const padded = pad(text, cells, cell.right);
  const style: Omit<Seg, 'text'> = {};
  if (cell.color !== undefined) style.color = THEME[cell.color];
  if (cell.dim) style.dim = true;
  if (cell.bold) style.bold = true;
  if (cell.key !== undefined) style.key = cell.key;
  return { text: padded, ...style };
}

/**
 * One row `inner` columns wide: fixed cells keep their width, the flexible
 * one takes what is left (at least one column), one space between cells.
 * The row's key is the first keyed cell's metric id unless `key` is given.
 */
export function row(cells: Cell[], inner: number, key?: string): FrameRow {
  const gaps = Math.max(0, cells.length - 1);
  const fixed = cells.reduce((n, c) => n + (c.width ?? 0), 0);
  const flexible = Math.max(1, inner - gaps - fixed);
  const line: Line = [];
  cells.forEach((cell, i) => {
    if (i > 0) line.push({ text: ' ' });
    line.push(cellSeg(cell, cell.width ?? flexible));
  });
  return { line, key: key ?? cells.find((c) => c.key !== undefined)?.key };
}

/** A single line of text, dim unless coloured. */
export function line(text: string, opts: { key?: string; color?: Color; bold?: boolean } = {}): FrameRow {
  const style: Omit<Seg, 'text'> = opts.color === undefined ? { dim: true } : { color: THEME[opts.color] };
  if (opts.bold) style.bold = true;
  return { line: [{ text, ...style }], key: opts.key };
}

/** Greedy word wrap into lines of at most `width` columns, for a long text drawn as one truncating Text per line. */
export function wrapWords(text: string, width: number): string[] {
  const w = Math.max(1, width);
  const lines: string[] = [];
  let cur = '';
  for (const word of text.split(/\s+/).filter((x) => x !== '')) {
    if (cur === '') cur = word;
    else if (cur.length + 1 + word.length <= w) cur += ` ${word}`;
    else {
      lines.push(cur);
      cur = word;
    }
  }
  if (cur !== '') lines.push(cur);
  return lines;
}

export type Panel = {
  /** The view's hotkey, drawn in the frame as the TUI numbers its panels. */
  hotkey: string;
  title: string;
  summary?: string;
};

/** A detail view: one framed panel `columns` wide holding at most MAX_ROWS rows. */
export function panel(p: Panel, rows: FrameRow[], columns: number, el: ViewElements): RenderElement {
  return frame({ hotkey: p.hotkey, title: p.title, summary: p.summary, width: columns, rows: rows.slice(0, MAX_ROWS) }, el);
}

/** Columns a detail view's rows may use inside its frame. */
export function bodyWidth(columns: number): number {
  return innerWidth(columns);
}

/** Re-exported for the views that size a cell by its text. */
export { width };
