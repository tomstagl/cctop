// The Files view: blast radius and wasted re-reads. One row per file from
// `cctop query files` (newest touch first): the path (its tail kept when
// cut), touches as `R×n E×n W×n`, lines added and removed when git knows
// them, and a `re-read ⚠` marker on a file read again without a change.
import type { RenderElement } from 'claude-code';
import type { Model } from '../model';
import { DASH, at, measured, stringAt } from './format';
import { NEEDS_BINARY, type ViewElements } from './overview';
import { MAX_ROWS, line, row, type Cell } from './table';

const W = { touches: 13, lines: 5, reread: 9 };
const REREAD = 're-read ⚠';

function count(obj: unknown, key: string): number {
  const v = at(obj, key);
  return typeof v === 'number' ? v : 0;
}

/** `R×1 E×2 W×1`: the touch counts by kind, as the TUI writes them. */
export function touches(f: unknown): string {
  const parts: string[] = [];
  for (const [key, mark] of [
    ['reads', 'R'],
    ['edits', 'E'],
    ['writes', 'W'],
  ] as const) {
    const n = count(f, key);
    if (n > 0) parts.push(`${mark}×${n}`);
  }
  return parts.join(' ');
}

function fileCells(f: unknown): Cell[] {
  const added = measured(f, 'lines_added');
  const removed = measured(f, 'lines_removed');
  const diff = added !== null && removed !== null;
  return [
    { text: stringAt(f, 'path') ?? DASH, tail: true },
    { text: touches(f), width: W.touches, key: 'file_touches' },
    { text: diff ? `+${added.value}` : DASH, width: W.lines, color: diff ? 'green' : undefined, dim: !diff, key: 'file_lines' },
    { text: diff ? `−${removed.value}` : '', width: W.lines, color: 'red', key: 'file_lines' },
    { text: at(f, 'reread_warning') === true ? REREAD : '', width: W.reread, color: 'yellow', key: 'file_rereads' },
  ];
}

const HEADER: Cell[] = [
  { text: 'FILE', dim: true },
  { text: 'TOUCHES', width: W.touches, dim: true },
  { text: '+', width: W.lines, dim: true },
  { text: '−', width: W.lines, dim: true },
  { text: '', width: W.reread },
];

export function renderFiles(model: Model, el: ViewElements): RenderElement {
  const { Box } = el;
  if (model.binary === 'missing') return line(NEEDS_BINARY, el, { key: 'file_touches' });
  const files = Array.isArray(model.query.files) ? model.query.files.slice(0, MAX_ROWS - 1) : [];
  return (
    <Box flexDirection="column">
      {row(HEADER, el)}
      {files.length === 0 && line('no files touched yet', el)}
      {files.map((f) => row(fileCells(f), el, 'file_touches'))}
    </Box>
  );
}
