// The Coach view: `cctop query coach` drawn as the TUI draws it — the state
// line, the four lights, the nudge slot, `next`, `snoozed` — in one frame,
// then the action Buttons (`[1 fill]` for prompt- and slash-class actions,
// `[2 snooze]`, `[3 why]`), then the detail frame of one light (the highest
// by default; four Buttons pick another) or the why frame. Every string is
// the binary's, cut at 52 cells there, so the two surfaces never disagree;
// below 50 body columns each light's row drops its third figure.
import type { ElementTable, RenderElement } from 'claude-code';
import type { Model } from '../model';
import { at, stringAt } from './format';
import { ACCENT, CRIT, WARN, seg, textRow, type Line } from './frame';
import { NEEDS_BINARY, type ViewElements } from './overview';
import { bodyWidth, line, panel, row, wrapWords, type FrameRow } from './table';

/** Body columns below which a light's row keeps only its first two figures. */
export const NARROW_COLUMNS = 50;

export type LightId = 'context' | 'cache' | 'limits' | 'rework';
export const LIGHT_IDS: readonly LightId[] = ['context', 'cache', 'limits', 'rework'];
export type Level = 'quiet' | 'watch' | 'act';

export type Light = { id: LightId; level: Level; glyph: string; number: string; text: string; lines: string[]; source: string; approx: boolean };
export type Nudge = {
  id: string;
  family: string;
  cls: string;
  line1: string;
  line2: string;
  evidence: string;
  actionText: string;
  actionKind: string;
  explain: string;
  saving: string;
  retiresOn: string;
  firedAt: number | null;
  acting: boolean;
};
export type Coach = {
  session: string;
  model: string;
  turn: number;
  stateLine: string;
  stateKind: string;
  lights: Light[];
  nudge: Nudge | null;
  nextRow: string;
  snoozedRow: string;
  quietRow: string;
  recent: string[];
  sessionMode: string;
  /** The one-line forms: L0 (≥ 80 columns), L1 (≥ 40), L2. */
  lines: { l0: string; l1: string; l2: string };
};

const LEVELS: Level[] = ['quiet', 'watch', 'act'];

function levelOf(v: unknown): Level {
  const s = stringAt(v, 'level');
  return s !== null && (LEVELS as string[]).includes(s) ? (s as Level) : 'quiet';
}

function lightOf(v: unknown): Light | null {
  const id = stringAt(v, 'id');
  if (id === null || !(LIGHT_IDS as string[]).includes(id)) return null;
  const lines = at(v, 'lines');
  return {
    id: id as LightId,
    level: levelOf(v),
    glyph: stringAt(v, 'glyph') ?? '○',
    number: stringAt(v, 'number') ?? '—',
    text: stringAt(v, 'text') ?? '',
    lines: Array.isArray(lines) ? lines.filter((l): l is string => typeof l === 'string') : [],
    source: stringAt(v, 'source') ?? '',
    approx: at(v, 'approx') === true,
  };
}

function nudgeOf(v: unknown): Nudge | null {
  if (v === null || typeof v !== 'object') return null;
  const id = stringAt(v, 'id');
  if (id === null) return null;
  const fired = at(v, 'fired_at_ms');
  return {
    id,
    family: stringAt(v, 'family') ?? '',
    cls: stringAt(v, 'class') ?? '',
    line1: stringAt(v, 'line1') ?? '',
    line2: stringAt(v, 'line2') ?? '',
    evidence: stringAt(v, 'evidence') ?? '',
    actionText: stringAt(v, 'action_text') ?? '',
    actionKind: stringAt(v, 'action_kind') ?? '',
    explain: stringAt(v, 'explain') ?? '',
    saving: stringAt(v, 'saving') ?? '',
    retiresOn: stringAt(v, 'retires_on') ?? '',
    firedAt: typeof fired === 'number' ? fired : null,
    acting: at(v, 'acting') === true,
  };
}

/** The coach object as the pane reads it, or null when the query has not answered (or is not this shape). */
export function coachOf(query: unknown): Coach | null {
  if (query === null || typeof query !== 'object') return null;
  const lights = at(query, 'lights');
  if (!Array.isArray(lights)) return null;
  const parsed = lights.map(lightOf).filter((l): l is Light => l !== null);
  if (parsed.length !== 4) return null;
  const state = at(query, 'state');
  const lines = at(query, 'lines');
  const recent = at(query, 'recent');
  return {
    session: stringAt(query, 'session') ?? '',
    model: stringAt(query, 'model') ?? '',
    turn: typeof at(query, 'turn') === 'number' ? (at(query, 'turn') as number) : 0,
    stateLine: stringAt(state, 'line') ?? '',
    stateKind: stringAt(state, 'kind') ?? '',
    lights: parsed,
    nudge: nudgeOf(at(query, 'nudge')),
    nextRow: stringAt(query, 'next_row') ?? 'next     —',
    snoozedRow: stringAt(query, 'snoozed_row') ?? 'snoozed  —',
    quietRow: stringAt(query, 'quiet_row') ?? '  quiet · nothing to act on',
    recent: Array.isArray(recent) ? recent.map((r) => stringAt(r, 'row') ?? '').filter((r) => r !== '') : [],
    sessionMode: stringAt(query, 'session_mode') ?? 'interactive',
    lines: {
      l0: stringAt(lines, 'l0') ?? '',
      l1: stringAt(lines, 'l1') ?? '',
      l2: stringAt(lines, 'l2') ?? '',
    },
  };
}

/** The status line for `columns` body cells: L0 at ≥ 80, L1 at ≥ 40, L2 below. */
export function statusLine(c: Coach, columns: number): string {
  return columns >= 80 ? c.lines.l0 : columns >= 40 ? c.lines.l1 : c.lines.l2;
}

/** The light whose level is highest (the first of equals): what the detail frame follows. */
export function highestLight(c: Coach): LightId {
  let best = c.lights[0];
  for (const l of c.lights) if (LEVELS.indexOf(l.level) > LEVELS.indexOf(best.level)) best = l;
  return best.id;
}

/** Prompt- and slash-class actions go into the prompt box; nothing else does. */
export function fillable(n: Nudge | null): boolean {
  return n !== null && n.actionText !== '' && (n.actionKind === 'prompt' || n.actionKind === 'slash');
}

function levelColor(level: Level): string | undefined {
  return level === 'act' ? CRIT : level === 'watch' ? WARN : undefined;
}

/** A light's row text, its third figure dropped below NARROW_COLUMNS body columns (the context light keeps its tokens and bar, and drops the price). */
export function lightText(l: Light, columns: number): string {
  if (columns >= NARROW_COLUMNS) return l.text;
  const parts = l.text.split(' · ');
  return parts.slice(0, l.id === 'context' ? 1 : 2).join(' · ');
}

/** The elements the coach draws with: the views' Box and Text, plus Button. */
export type CoachElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button'>;

/** What the pane may do on a press; built in pane.tsx over `$`. */
export type CoachActions = {
  fill(text: string): void;
  snooze(rule: string): void;
  why(): void;
  light(id: LightId): void;
};

function stateRow(c: Coach): FrameRow {
  const text = c.stateLine;
  const waiting = c.stateKind.startsWith('◆');
  const idle = c.stateKind === 'IDLE' || c.stateKind === 'LOOP';
  const sp = text.indexOf(' ', waiting ? 2 : 0);
  const word = sp < 0 ? text : text.slice(0, sp);
  const rest = sp < 0 ? '' : text.slice(sp);
  const style = waiting ? { color: WARN } : idle ? { dim: true } : { color: ACCENT };
  return { line: [seg(word, style), seg(rest)] };
}

function separatorRow(inner: number): FrameRow {
  return { line: [seg('─'.repeat(inner), { dim: true })] };
}

export function renderCoach(model: Model, el: CoachElements, columns: number, actions: CoachActions): RenderElement {
  const { Box } = el;
  const c = coachOf(model.query.coach);
  const p = { title: 'coach', summary: c === null ? undefined : `${c.model.replace(/^claude-/, '')} · turn ${c.turn}` };
  if (model.binary === 'missing') return panel(p, [line(NEEDS_BINARY, { key: 'advice_saving' })], columns, el);
  if (c === null) return panel(p, [line('waiting for cctop query coach…', { key: 'advice_saving' })], columns, el);
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = [stateRow(c), separatorRow(inner)];
  for (const l of c.lights) {
    const color = levelColor(l.level);
    const style = color === undefined ? {} : { color };
    rows.push({
      line: [seg(l.glyph, style), seg(' '), seg(l.id.padEnd(8), style), seg(' '), seg(lightText(l, columns), style)],
      key: `coach_${l.id}`,
    });
  }
  rows.push(separatorRow(inner));
  const n = c.nudge;
  if (n !== null) {
    rows.push({ line: [seg(n.line1, { bold: true })], key: 'advice_saving' });
    rows.push({ line: [seg(n.line2, { color: ACCENT })] });
    rows.push({ line: [seg(n.evidence, { dim: true })] });
  } else {
    rows.push({ line: [seg(c.quietRow, { dim: true })], key: 'advice_saving' });
    for (const r of c.recent.slice(0, 2)) rows.push({ line: [seg(`  ${r}`, { dim: true })] });
  }
  rows.push({ line: [seg('')] });
  rows.push({ line: [seg(c.nextRow, { dim: true })] });
  rows.push({ line: [seg(c.snoozedRow, { dim: true })] });
  const card = panel(p, rows, columns, el);
  return (
    <Box flexDirection="column">
      {card}
      {buttons(c, el, actions)}
      {model.coachWhy && n !== null ? whyFrame(n, columns, el) : detailFrame(c, model.coachLight ?? highestLight(c), columns, el, actions)}
    </Box>
  );
}

// The action row: fill (prompt-class only), snooze and why, hotkeys armed
// only while a Button of the row has the focus.
function buttons(c: Coach, el: CoachElements, actions: CoachActions): RenderElement {
  const { Box, Text, Button } = el;
  const n = c.nudge;
  const parts: RenderElement[] = [];
  if (fillable(n)) {
    parts.push(<Button key="coach-fill" label="fill" hotkey="1" onPress={() => actions.fill(n!.actionText)} />);
  }
  if (n !== null) {
    parts.push(<Text wrap="truncate"> </Text>);
    parts.push(<Button key="coach-snooze" label="snooze" hotkey="2" onPress={() => actions.snooze(n.id)} />);
    parts.push(<Text wrap="truncate"> </Text>);
    parts.push(<Button key="coach-why" label="why" hotkey="3" onPress={() => actions.why()} />);
  }
  if (parts.length === 0) return <Box />;
  return <Box flexDirection="row">{parts}</Box>;
}

// The detail frame of one light: its three lines, then the four Buttons
// that pick another light.
function detailFrame(c: Coach, id: LightId, columns: number, el: CoachElements, actions: CoachActions): RenderElement {
  const { Box, Text, Button } = el;
  const l = c.lights.find((x) => x.id === id) ?? c.lights[0];
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = l.lines.map((text) => row([{ text }], inner));
  if (rows.length === 0) rows.push(line('—'));
  const frame = panel({ title: `${l.glyph} ${l.id}`, summary: `${l.source}${l.approx ? ' ≈' : ''}` }, rows, columns, el);
  const picks: RenderElement[] = [];
  for (const x of c.lights) {
    if (picks.length > 0) picks.push(<Text wrap="truncate"> </Text>);
    picks.push(
      x.id === l.id ? (
        <Text key={`coach-light-${x.id}`} wrap="truncate" inverse>
          {` ${x.glyph} ${x.id} `}
        </Text>
      ) : (
        <Button key={`coach-light-${x.id}`} label={`${x.glyph} ${x.id}`} plain onPress={() => actions.light(x.id)} />
      ),
    );
  }
  return (
    <Box flexDirection="column">
      {frame}
      <Box flexDirection="row">{picks}</Box>
    </Box>
  );
}

// The why frame: what the nudge rests on and the rule's explanation.
function whyFrame(n: Nudge, columns: number, el: ViewElements): RenderElement {
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = [
    { line: [seg(`${n.cls} ${n.id} · ${n.family}`, { bold: true })] },
    { line: [seg('evidence  ', { dim: true }), seg(n.evidence.trim())] },
    { line: [seg('retires   ', { dim: true }), seg(`on ${n.retiresOn}`)] },
    { line: [seg('saving    ', { dim: true }), seg(`${n.saving} (an estimate)`)] },
  ];
  if (n.actionText !== '') rows.push({ line: [seg(`${n.actionKind.padEnd(10)}`, { dim: true }), seg(n.actionText, { color: ACCENT })] });
  rows.push({ line: [seg('')] });
  for (const w of wrapWords(n.explain, inner)) rows.push({ line: [seg(w, { dim: true })] });
  return panel({ title: 'why' }, rows, columns, el);
}

/** The inline (classic renderer) form: the L1 line as one row. */
export function coachInlineRow(model: Model, el: ViewElements, columns: number): RenderElement | null {
  const c = coachOf(model.query.coach);
  if (c === null) return null;
  const text = statusLine(c, columns);
  const line: Line = [seg(text)];
  return textRow(line, el, 'advice_saving');
}
