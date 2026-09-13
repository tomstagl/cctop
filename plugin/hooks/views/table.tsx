// Shared by the detail views: a borderless table row of fixed-width and
// flexible cells, the dim single line a view draws when it has nothing to
// show (or lacks the binary), and the row cap every view keeps to. Each cell
// is one truncating Text; a cell that shows a metric carries its id as `key`.
import type { RenderElement } from 'claude-code';
import type { Color, ViewElements } from './overview';

/** The most rows a view's tree holds: longer lists are cut, the pane scrolls the rest. */
export const MAX_ROWS = 400;

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

function cellText(cell: Cell, el: ViewElements): RenderElement {
  const { Text } = el;
  const keyed = cell.key === undefined ? {} : { key: cell.key };
  return (
    <Text {...keyed} wrap={cell.tail ? 'truncate-start' : 'truncate'} color={cell.color} dimColor={cell.dim} bold={cell.bold}>
      {cell.text}
    </Text>
  );
}

/** One row: fixed cells keep their width, the flexible one shrinks and grows. */
export function row(cells: Cell[], el: ViewElements, key?: string): RenderElement {
  const { Box } = el;
  const keyed = key === undefined ? {} : { key };
  return (
    <Box {...keyed} flexDirection="row" columnGap={1}>
      {cells.map((cell) =>
        cell.width === undefined ? (
          <Box flexGrow={1}>{cellText(cell, el)}</Box>
        ) : (
          <Box width={cell.width} flexShrink={0} flexDirection="row" justifyContent={cell.right ? 'flex-end' : 'flex-start'}>
            {cellText(cell, el)}
          </Box>
        ),
      )}
    </Box>
  );
}

/** A single line of text, dim unless coloured. */
export function line(text: string, el: ViewElements, opts: { key?: string; color?: Color } = {}): RenderElement {
  const { Box, Text } = el;
  const keyed = opts.key === undefined ? {} : { key: opts.key };
  return (
    <Box {...keyed}>
      <Text wrap="truncate" color={opts.color} dimColor={opts.color === undefined}>
        {text}
      </Text>
    </Box>
  );
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
