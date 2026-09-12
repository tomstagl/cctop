// The Events view: the last 50 of `cctop query events` as `HH:MM:SS kind
// text`, oldest first so the newest is at the bottom, the kind coloured as
// the TUI does (tool ok, hook/agent accent, api crit, the rest warn).
import type { RenderElement } from 'claude-code';
import type { Model } from '../model';
import { at, stringAt } from './format';
import { NEEDS_BINARY, type Color, type ViewElements } from './overview';
import { line, row, type Cell } from './table';

export const EVENT_ROWS = 50;
const W = { clock: 8, kind: 7 };

const KIND_COLOR: Record<string, Color> = {
  tool: 'green',
  hook: 'cyan',
  agent: 'cyan',
  perm: 'yellow',
  note: 'yellow',
  compact: 'yellow',
  away: 'yellow',
  api: 'red',
};

/** The UTC wall clock of a millisecond timestamp, as the TUI's events panel shows it. */
export function clock(atMs: number): string {
  const s = ((Math.floor(atMs / 1000) % 86_400) + 86_400) % 86_400;
  const two = (n: number) => String(n).padStart(2, '0');
  return `${two(Math.floor(s / 3600))}:${two(Math.floor((s % 3600) / 60))}:${two(s % 60)}`;
}

function eventCells(e: unknown): Cell[] {
  const atMs = at(e, 'at_ms');
  const kind = stringAt(e, 'kind') ?? '';
  return [
    { text: typeof atMs === 'number' ? clock(atMs) : '', width: W.clock, dim: true },
    { text: kind, width: W.kind, color: KIND_COLOR[kind] },
    { text: stringAt(e, 'text') ?? '' },
  ];
}

export function renderEvents(model: Model, el: ViewElements): RenderElement {
  const { Box } = el;
  if (model.binary === 'missing') return line(NEEDS_BINARY, el);
  const events = Array.isArray(model.query.events) ? model.query.events.slice(-EVENT_ROWS) : [];
  return (
    <Box flexDirection="column">
      {events.length === 0 && line('no events yet', el)}
      {events.map((e) => row(eventCells(e), el))}
    </Box>
  );
}
