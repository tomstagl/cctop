// The Overview view: Console (PRD dashboard-v2 §4) drawn from `cctop query
// dashboard` (src/dashboard.rs, schema 2) the way the TUI draws it
// (src/ui/dashboard.rs): the header line with the phase cell right-aligned,
// six cells — each a keyed Box of plain Buttons sharing one scope and one
// press, the digit the engine's own chrome — the act line as a target of its
// own with `a: advisor` at the right, the rule line naming the open body with
// `0: home`, then the open body's rows. The pane has four colours and no hex
// (views/frame.tsx), so a slice's series step is carried by weight and by the
// alternating fill glyph alone.
//
// Before the binary answers, the engine's own header and a waiting line; on a
// binary whose object is not schema 2, one line naming the cctop this pane
// needs (FR-16). Inline placement (the classic renderer's few rows above the
// prompt) is the strip: the status line, the coach's L1 line, the act line.
import type { ElementTable, RenderElement } from 'claude-code';
import { TESTED_WITH, usageRows, type Model, type Placement, type TurnState } from '../model';
import { DASH, at, formatDuration, isMissing, stringAt } from './format';
import { ACCENT, THEME, clip, dim, fit, join, seg, textRow, width, lineWidth, type Line, type Seg } from './frame';

/** The elements a view draws with, as `$.ui.resolve(e)` answers them. */
export type ViewElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text'>;

export const NEEDS_BINARY = 'needs the cctop binary';

/** The dashboard object's schema this pane draws; an older binary sends 1. */
export const SCHEMA = 2;

/** Three cells per row from this many body columns; two below (the prototype's ladder). */
export const THREE_CELLS = 80;
/** A cell's middle form from this many cells wide; the short one below. */
export const MID_CELL = 28;
/** The act line's right-aligned `a: advisor` from this width. */
export const ACT_TAIL = 66;
/** The act line's full copy from this width; the short one below. */
export const ACT_FULL = 72;

/** The four palette colours a view may name. */
export type Color = 'green' | 'yellow' | 'red' | 'cyan';

export type Badge = { label: 'bin' | 'shim' | 'hooks'; on: boolean; text?: string };

export type Header = {
  status: TurnState;
  turn: number;
  elapsed: string;
  model: string;
  mode: string | null;
  effort: string;
  badges: Badge[];
};

const STATUS_MARK: Record<TurnState, string> = { busy: '●', idle: '○', waiting: '◆' };
const STATUS_PILL: Record<TurnState, string> = { busy: 'BUSY', idle: 'IDLE', waiting: 'WAITING' };

function turnElapsedMs(model: Model, now: number): number | null {
  return model.turn.startedAt === null ? null : Math.max(0, now - model.turn.startedAt);
}

/** The engine's own header: what the pane knows before the binary answers. */
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

function statusPill(status: TurnState): Line {
  const text = `${STATUS_MARK[status]} ${STATUS_PILL[status]}`;
  if (status === 'busy') return [seg(text, { color: THEME.green, bold: true })];
  if (status === 'waiting') return [seg(text, { color: THEME.yellow, bold: true })];
  return [dim(text)];
}

/** The `bin · shim · hooks 2.1.273` badges, the on ones green. */
export function badgesLine(badges: Badge[]): Line {
  return join(
    badges.map((b) => [b.on ? seg(b.text ?? b.label, { color: THEME.green }) : dim(b.text ?? b.label)]),
    seg(' '),
  );
}

// ---------------------------------------------------------------- the object

export type Cell = { key: string; id: string; opens: string; label: Seg[]; mid: Seg[]; short: Seg[] };
export type Act = { key: string; opens: string; line: Seg[]; short: Seg[]; tag: string; nudge: string | null; blocked: boolean; acting: boolean };
export type Slice = { label: string; tokens: number; step: number };
export type Body = { key: string; id: string; title: string; keys: string; rows: Seg[][]; slices: Slice[] };
export type Dashboard = {
  schema: number;
  headerLine: string;
  phase: { glyph: string; word: string; elapsed: string; tokens: string[] };
  cells: Cell[];
  act: Act;
  bodies: Body[];
  /** Nudges are shown this session; false on the control arm of the coach's own measurement (`cctop run --coach off|auto`). */
  exposed: boolean;
  lines: { l1: string; l2: string };
};

/** A tone as the binary tags it, to the pane's segment style: the series
 * steps by weight (the pane has no ramp — dim, plain, bold). */
function toned(text: string, tone: unknown): Seg {
  switch (tone) {
    case 'dim':
    case 's0':
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
    case 's2':
      return seg(text, { bold: true });
    default:
      return seg(text);
  }
}

function segsOf(v: unknown): Seg[] {
  if (!Array.isArray(v)) return [];
  return v.map((s) => toned(stringAt(s, 'text') ?? '', at(s, 'tone'))).filter((s) => s.text !== '');
}

function strings(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((t): t is string => typeof t === 'string') : [];
}

/** The `schema` of a dashboard object, or null when there is none yet. */
export function schemaOf(query: unknown): number | null {
  if (query === null || typeof query !== 'object') return null;
  const s = at(query, 'schema');
  return typeof s === 'number' ? s : null;
}

/** The dashboard object as the pane reads it, or null before the binary answered or on another schema. */
export function dashboardOf(query: unknown): Dashboard | null {
  if (schemaOf(query) !== SCHEMA) return null;
  const cells = at(query, 'cells');
  const bodies = at(query, 'bodies');
  const act = at(query, 'act');
  if (!Array.isArray(cells) || !Array.isArray(bodies) || act === null || typeof act !== 'object') return null;
  const phase = at(query, 'header', 'phase');
  return {
    schema: SCHEMA,
    headerLine: stringAt(query, 'header', 'line') ?? '',
    phase: {
      glyph: stringAt(phase, 'glyph') ?? '○',
      word: stringAt(phase, 'word') ?? '',
      elapsed: stringAt(phase, 'elapsed') ?? DASH,
      tokens: strings(at(phase, 'tokens')),
    },
    cells: cells.map((c) => ({
      key: stringAt(c, 'key') ?? '',
      id: stringAt(c, 'id') ?? '',
      opens: stringAt(c, 'opens') ?? '',
      label: segsOf(at(c, 'label')),
      mid: segsOf(at(c, 'mid')),
      short: segsOf(at(c, 'short')),
    })),
    act: {
      key: stringAt(act, 'key') ?? 'a',
      opens: stringAt(act, 'opens') ?? 'advisor',
      line: segsOf(at(act, 'line')),
      short: segsOf(at(act, 'short')),
      tag: stringAt(act, 'tag') ?? '',
      nudge: stringAt(act, 'nudge'),
      blocked: at(act, 'blocked') === true,
      acting: at(act, 'acting') === true,
    },
    bodies: bodies.map((b) => ({
      key: stringAt(b, 'key') ?? '',
      id: stringAt(b, 'id') ?? '',
      title: stringAt(b, 'title') ?? '',
      keys: stringAt(b, 'keys') ?? '',
      rows: Array.isArray(at(b, 'rows')) ? (at(b, 'rows') as unknown[]).map(segsOf) : [],
      slices: Array.isArray(at(b, 'slices'))
        ? (at(b, 'slices') as unknown[]).map((s) => ({
            label: stringAt(s, 'label') ?? '',
            tokens: typeof at(s, 'tokens') === 'number' ? (at(s, 'tokens') as number) : 0,
            step: typeof at(s, 'step') === 'number' ? (at(s, 'step') as number) : 0,
          }))
        : [],
    })),
    exposed: at(query, 'exposed') !== false,
    lines: { l1: stringAt(query, 'lines', 'l1') ?? '', l2: stringAt(query, 'lines', 'l2') ?? '' },
  };
}

/** The line the pane draws on a binary whose object is another schema (FR-16). */
export function schemaMismatch(schema: number): string {
  return `cctop ${schema < SCHEMA ? '0.6.0 or newer' : 'newer than this pane'} needed: the binary sends dashboard schema ${schema}, this pane draws ${SCHEMA} — brew upgrade cctop && claude plugin update cctop@cctop`;
}

// -------------------------------------------------------------- the surface

export type OverviewActions = {
  /** A cell's, the act line's or `0 home`'s press: open that body in place. */
  open(id: string): void;
};

/** The elements the Overview draws with: the views' Box and Text, plus Button for the targets. */
export type OverviewElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button'>;

/** Row 1: ` cctop`, the session facts dim, the phase cell right-aligned in its colour. */
function headerLine(d: Dashboard, columns: number): Line {
  const facts = d.headerLine.replace(/^cctop\s+/, '');
  const phaseText = clip(`${d.phase.glyph} ${d.phase.word} ${d.phase.elapsed}`, Math.min(40, Math.max(0, columns - 10)));
  const factsText = clip(facts, Math.max(0, columns - 8 - width(phaseText) - 2));
  const padCells = Math.max(0, columns - 8 - width(factsText) - width(phaseText));
  const phaseStyle: Omit<Seg, 'text'> =
    d.phase.glyph === '◆' ? { color: THEME.yellow, bold: true } : d.phase.glyph === '○' ? { dim: true, bold: true } : { color: THEME.green, bold: true };
  return [seg(' cctop  ', { color: ACCENT, bold: true }), dim(factsText), seg(' '.repeat(padCells)), seg(phaseText, phaseStyle)];
}

/** The cell text form for the width: wide at ≥ 80 columns, mid at cells ≥ 28, short below. */
export function cellForm(columns: number, cellWidth: number): (c: Cell) => Seg[] {
  if (columns >= THREE_CELLS) return (c) => c.label;
  if (cellWidth >= MID_CELL) return (c) => c.mid;
  return (c) => c.short;
}

// A cell: a keyed Box of plain Buttons sharing one scope and one press —
// the first carries the hotkey (the engine draws `1: ctx 35%`), the rest of
// the label and the padding are Buttons too, so the whole area presses and
// lights (FR-13). The open body's cell is Text in `ok`, bold: the one state
// that is not a target. Without Buttons (a test's bare elements, the inline
// strip) every part is Text and the digit is drawn as the engine would.
function cellBox(c: Cell, text: Seg[], cw: number, active: boolean, el: OverviewElements, actions: OverviewActions | undefined): RenderElement {
  const { Box, Button } = el;
  // `1: ` is the engine's; the text has the rest of the cell, cut a cell
  // short so the gap to the next cell survives (the TUI's `cw - 4`), the
  // gap a Button of one space so the whole area presses.
  const body = [...fit(text, Math.max(1, cw - 4)), seg(' ')];
  const first = body[0] ?? seg('');
  const rest = body.slice(1);
  if (actions === undefined || active) {
    const style: Omit<Seg, 'text'> = active ? { color: THEME.green, bold: true } : { color: ACCENT, bold: true };
    const line: Line = [seg(`${c.key}: `, style), ...(active ? body.map((s) => ({ text: s.text, color: THEME.green, bold: true })) : body)];
    return textRow(line, el, `cell_${c.id}`);
  }
  const scope = `cctop-cell-${c.id}`;
  const press = () => actions.open(c.opens);
  return (
    <Box key={`cell_${c.id}`} flexDirection="row" hover={{ scope }}>
      <Button key={`cell-${c.id}`} label={first.text} hotkey={c.key} plain dimColor={first.dim === true} onPress={press} />
      {rest.map((s, i) => (
        <Button key={`cell-${c.id}-${i}`} label={s.text} plain dimColor={s.dim === true} onPress={press} />
      ))}
    </Box>
  );
}

/** Rows 2–4: the cells, three per row at ≥ 80 columns, two below. */
function cellRows(d: Dashboard, open: string, columns: number, el: OverviewElements, actions: OverviewActions | undefined): RenderElement[] {
  const { Box } = el;
  const perRow = columns >= THREE_CELLS ? 3 : 2;
  const cw = Math.floor(Math.max(0, columns - 2) / perRow);
  const form = cellForm(columns, cw);
  const out: RenderElement[] = [];
  for (let i = 0; i < d.cells.length; i += perRow) {
    const cells = d.cells.slice(i, i + perRow).map((c) => (
      <Box width={cw} flexShrink={0}>
        {cellBox(c, form(c), cw, c.opens === open, el, actions)}
      </Box>
    ));
    out.push(
      <Box key={`cells_${i}`} flexDirection="row">
        {textRow([seg(' ')], el)}
        {cells}
      </Box>,
    );
  }
  return out;
}

/** Word-wrap segments into rows of at most `columns` cells, breaking at spaces (FR-7). */
export function wrap(segs: Seg[], columns: number, indent: number): Line[] {
  const rows: Line[] = [[]];
  let used = 0;
  for (const s of segs) {
    let pending = '';
    const flush = (): void => {
      if (pending !== '') rows[rows.length - 1].push({ ...s, text: pending });
      pending = '';
    };
    for (const word of s.text.split(/(?<= )/)) {
      const w = width(word);
      if (used + w > columns && used > indent) {
        flush();
        rows.push([seg(' '.repeat(indent))]);
        used = indent;
        const trimmed = word.replace(/^ +/, '');
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

// Row 5: the act line, the whole of it a target (a keyed Box of plain
// Buttons in the advisor's scope) with `a: advisor` right-aligned at
// ACT_TAIL columns; wrapped onto a second row rather than cut.
function actRows(d: Dashboard, open: string, columns: number, el: OverviewElements, actions: OverviewActions | undefined): RenderElement[] {
  const { Box, Button } = el;
  const text = columns >= ACT_FULL ? d.act.line : d.act.short;
  const segs: Seg[] = [dim(' '), ...text];
  if (d.act.tag !== '' && columns >= ACT_FULL) segs.push(dim(`  ${d.act.tag}`));
  if (d.act.acting) segs.push(dim(' · acting…'));
  const tailWidth = columns >= ACT_TAIL ? 'a: advisor '.length : 0;
  const rows = wrap(segs, Math.max(1, columns - tailWidth - 1), 3).slice(0, 2);
  const active = open === d.act.opens;
  const out: RenderElement[] = [];
  rows.forEach((row, i) => {
    const line = fit(row, Math.max(1, columns - (i === 0 ? tailWidth : 0)));
    if (actions === undefined || active) {
      const tail: Line = i === 0 && tailWidth > 0 ? [seg('a: ', { color: active ? THEME.green : ACCENT, bold: true }), dim('advisor ')] : [];
      out.push(textRow([...line, ...tail], el, i === 0 ? 'advice_saving' : undefined));
      return;
    }
    const scope = 'cctop-cell-advisor';
    const press = () => actions.open(d.act.opens);
    out.push(
      <Box key={i === 0 ? 'advice_saving' : `act_${i}`} flexDirection="row" hover={{ scope }}>
        {line.map((s, j) => (
          <Button key={`act-${i}-${j}`} label={s.text} plain dimColor={s.dim === true} onPress={press} />
        ))}
        {i === 0 && tailWidth > 0 ? <Button key="act-advisor" label="advisor " hotkey="a" plain dimColor onPress={press} /> : null}
      </Box>,
    );
  });
  return out;
}

/** Row 6: `─── title ─────── 0: home  ·  keys ───`, the home a plain Button with hotkey 0. */
function ruleRow(body: Body, columns: number, el: OverviewElements, actions: OverviewActions | undefined): RenderElement {
  const { Box, Button } = el;
  const home = body.id === 'events';
  const head: Line = [dim('─── '), seg(body.title, { bold: true }), dim(' ')];
  const homeText = '0: home';
  let keys = body.keys !== '' && lineWidth(head) + 12 + width(body.keys) + 8 <= columns ? body.keys : '';
  let tailWidth = 1 + width(homeText) + (keys === '' ? 0 : 5 + width(keys)) + 4;
  if (lineWidth(head) + tailWidth > columns) {
    keys = '';
    tailWidth = 1 + 1 + 4;
  }
  const rule = dim('─'.repeat(Math.max(0, columns - lineWidth(head) - tailWidth)));
  const keysLine: Line = keys === '' ? [] : [dim('  ·  '), dim(keys)];
  if (actions === undefined || home || lineWidth(head) + tailWidth > columns) {
    const homeLine: Line = tailWidth === 6 ? [seg('0', { color: home ? THEME.green : ACCENT, bold: true })] : [seg('0: ', { color: home ? THEME.green : ACCENT, bold: true }), dim('home'), ...keysLine];
    return textRow([...head, rule, dim(' '), ...homeLine, dim(' ───')], el, `rule_${body.id}`);
  }
  return (
    <Box key={`rule_${body.id}`} flexDirection="row">
      {textRow([...head, rule, dim(' ')], el)}
      <Button key="cell-home" label="home" hotkey="0" plain dimColor onPress={() => actions.open('events')} />
      {textRow([...keysLine, dim(' ───')], el)}
    </Box>
  );
}

/** The engine's own reading (`$.session.usage`): the context and the rate
 * limits on one line — the pane's figures with no binary behind them. */
export function engineUsageLine(model: Model, now: number): Line | null {
  if (model.usage === null) return null;
  const rows = usageRows(model.usage, now).filter((r) => r.key !== 'cost');
  if (rows.length === 0) return null;
  return [seg(' '), ...join(rows.map((r) => [dim(`${r.label.toLowerCase()} `), seg(r.value)]), dim(' · '))];
}

/** The inline form (the classic renderer's few rows above the prompt): the status line, the engine's usage line, the strip, the act line. */
function renderInline(model: Model, el: ViewElements, columns: number, now: number): RenderElement {
  const { Box } = el;
  const head = header(model, now);
  const rows: RenderElement[] = [
    textRow(
      [...statusPill(head.status), seg(` · turn ${head.turn} · ${head.elapsed} · ${head.model} · ${head.effort}  `), ...badgesLine(head.badges)],
      el,
      'session_status',
    ),
  ];
  const usage = engineUsageLine(model, now);
  if (usage !== null) rows.push(textRow(fit(usage, columns), el, 'context_size'));
  const d = dashboardOf(model.query.dashboard);
  if (d !== null) {
    if (d.lines.l1 !== '') rows.push(textRow([seg(` ${d.lines.l1}`)], el, 'coach_context'));
    rows.push(textRow(fit([dim(' '), ...d.act.short], columns), el, 'advice_saving'));
  }
  return <Box flexDirection="column">{rows}</Box>;
}

/**
 * Console: the header line, the six cells, the act line, the rule line and
 * the open body's rows. Inline placement draws the strip (renderInline).
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
    // Before the binary answers, or on another schema: the engine's own
    // header and one line saying what is missing.
    const head = header(model, now);
    body.push(
      textRow(
        [seg(' cctop  ', { bold: true }), seg(`${head.model} · turn ${head.turn} · ${head.elapsed}  `), ...statusPill(head.status), seg('  '), ...badgesLine(head.badges)],
        el,
        'session_status',
      ),
    );
    const usage = engineUsageLine(model, now);
    if (usage !== null) body.push(textRow(fit(usage, columns), el, 'context_size'));
    const schema = schemaOf(model.query.dashboard);
    const text = schema !== null && schema !== SCHEMA ? schemaMismatch(schema) : model.binary === 'missing' ? NEEDS_BINARY : 'waiting for cctop query dashboard…';
    body.push(textRow([dim(` ${text}`)], el, 'advice_saving'));
    return <Box flexDirection="column">{body}</Box>;
  }
  const bel: OverviewElements = buttons?.el ?? { ...el, Button: el.Box as OverviewElements['Button'] };
  const actions = buttons?.actions;
  const open = model.body;
  body.push(textRow(headerLine(d, columns), el, 'session_status'));
  body.push(...cellRows(d, open, columns, bel, actions));
  body.push(...actRows(d, open, columns, bel, actions));
  const shown = d.bodies.find((b) => b.id === open) ?? d.bodies.find((b) => b.id === 'events') ?? d.bodies[0];
  if (shown !== undefined) {
    body.push(ruleRow(shown, columns, bel, actions));
    shown.rows.forEach((row, i) => body.push(textRow(row.length === 0 ? [seg('')] : fit(row, columns), el, i === 0 ? `body_${shown.id}` : undefined)));
  }
  return <Box flexDirection="column">{body}</Box>;
}
