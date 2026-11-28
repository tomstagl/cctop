// The Coach view (mod-pane POC 1, spec §5.2): `cctop query coach` drawn as
// three frames and a footer.
//
// - The NOW card: the state line and the nudge in full — its class, plain
//   name, age and dim rule id, the headline, the body and the evidence
//   wrapped at the pane's width, the fix prompt as a card a click puts in
//   the prompt box (never sent; a line says so), then the steps and Buttons
//   of its actions by kind, and when it goes away.
// - "Up next": the queued nudge as a row that opens in place (its body, its
//   Buttons, its foot), and the snoozed rules.
// - "Session": the four lights, each a row that opens in place; the context
//   light opens into the window's composition as Claude Code counted it
//   (PRD 3, from the dashboard object) and what put it there (residency).
//
// The one-line strings (the state line, the light rows) are the binary's,
// cut at 52 cells there, so the TUI and the pane show the same characters;
// the full words are uncut and wrapped here. Nothing here acts on the
// session: a fill only drafts into an empty box, a copy only copies, a
// settings file is only opened, and "press Esc to stop the turn" is a step
// the person takes.
import type { ElementTable, RenderElement } from 'claude-code';
import type { Model } from '../model';
import { at, formatAgo, formatTokensTui, stringAt } from './format';
import { ACCENT, BUTTON_GAP, CRIT, OK, WARN, buttonWidth, clip, dim, lineWidth, pad, seg, textRow, width, type FrameButton, type Line, type Seg } from './frame';
import { NEEDS_BINARY, dashboardOf, type ViewElements } from './overview';
import { bodyWidth, line, panel, wrapWords, type FrameRow } from './table';

/** Body columns below which a light's row keeps only its first two figures. */
export const NARROW_COLUMNS = 50;

export type LightId = 'context' | 'cache' | 'limits' | 'rework';
export const LIGHT_IDS: readonly LightId[] = ['context', 'cache', 'limits', 'rework'];
export type Level = 'quiet' | 'watch' | 'act';

export type Light = { id: LightId; level: Level; glyph: string; number: string; text: string; lines: string[]; source: string; approx: boolean };

export type ActionKind = 'prompt' | 'slash' | 'allow_rule' | 'setting' | 'key' | 'read';
const ACTION_KINDS: readonly ActionKind[] = ['prompt', 'slash', 'allow_rule', 'setting', 'key', 'read'];
/** One of a nudge's actions as the binary words it: what the Buttons are made from. */
export type CoachAction = { kind: ActionKind; label: string; payload: string };

/** A nudge's full words (the coach object's `Full`), uncut. */
export type Advice = {
  id: string;
  cls: string;
  name: string;
  headline: string;
  body: string;
  evidence: string[];
  fixPrompt: string | null;
  actions: CoachAction[];
  foot: string;
};
export type Nudge = Advice & {
  family: string;
  line1: string;
  line2: string;
  line3: string;
  actionText: string;
  actionKind: string;
  explain: string;
  saving: string;
  retiresOn: string;
  firedAt: number | null;
  acting: boolean;
};
export type Next = Advice & { promotes: string; row: string };
export type Coach = {
  session: string;
  model: string;
  turn: number;
  stateLine: string;
  stateKind: string;
  lights: Light[];
  nudge: Nudge | null;
  next: Next | null;
  nextRow: string;
  snoozed: number;
  snoozedRow: string;
  quietRow: string;
  recent: string[];
  sessionMode: string;
  /** `spent so far $X · what this session would cost at API prices`; empty from a binary without it. */
  footer: string;
  /** The one-line forms: L0 (≥ 80 columns), L1 (≥ 40), L2. */
  lines: { l0: string; l1: string; l2: string };
};

const LEVELS: Level[] = ['quiet', 'watch', 'act'];

function levelOf(v: unknown): Level {
  const s = stringAt(v, 'level');
  return s !== null && (LEVELS as string[]).includes(s) ? (s as Level) : 'quiet';
}

function strings(v: unknown): string[] {
  return Array.isArray(v) ? v.filter((l): l is string => typeof l === 'string' && l !== '') : [];
}

function lightOf(v: unknown): Light | null {
  const id = stringAt(v, 'id');
  if (id === null || !(LIGHT_IDS as string[]).includes(id)) return null;
  return {
    id: id as LightId,
    level: levelOf(v),
    glyph: stringAt(v, 'glyph') ?? '○',
    number: stringAt(v, 'number') ?? '—',
    text: stringAt(v, 'text') ?? '',
    lines: strings(at(v, 'lines')),
    source: stringAt(v, 'source') ?? '',
    approx: at(v, 'approx') === true,
  };
}

function actionOf(v: unknown): CoachAction | null {
  const kind = stringAt(v, 'kind');
  if (kind === null || !(ACTION_KINDS as string[]).includes(kind)) return null;
  // The payload is kept as the binary sent it: `/compact ` keeps its space for a focus.
  const payload = at(v, 'payload');
  return { kind: kind as ActionKind, label: stringAt(v, 'label') ?? '', payload: typeof payload === 'string' ? payload : '' };
}

// A coach object before schema 2 (cctop < 0.10) has no `actions` and one
// `evidence` string: its fill-class `action_text` becomes the one action and
// the string the one evidence row, so the card keeps its fill Button.
function legacyActions(v: unknown): CoachAction[] {
  const text = stringAt(v, 'action_text');
  const kind = stringAt(v, 'action_kind');
  if (text === null || text === '' || (kind !== 'prompt' && kind !== 'slash')) return [];
  return [{ kind, label: 'fill', payload: text }];
}

function adviceOf(v: unknown, id: string): Advice {
  const actions = at(v, 'actions');
  const evidence = at(v, 'evidence');
  return {
    id,
    cls: stringAt(v, 'class') ?? '',
    name: stringAt(v, 'name') ?? '',
    headline: stringAt(v, 'headline') ?? '',
    body: stringAt(v, 'body') ?? '',
    evidence: typeof evidence === 'string' ? (evidence === '' ? [] : [evidence]) : strings(evidence),
    fixPrompt: stringAt(v, 'fix_prompt'),
    actions: Array.isArray(actions) ? actions.map(actionOf).filter((a): a is CoachAction => a !== null) : legacyActions(v),
    foot: stringAt(v, 'foot') ?? '',
  };
}

function nudgeOf(v: unknown): Nudge | null {
  if (v === null || typeof v !== 'object') return null;
  const id = stringAt(v, 'id');
  if (id === null) return null;
  const fired = at(v, 'fired_at_ms');
  return {
    ...adviceOf(v, id),
    family: stringAt(v, 'family') ?? '',
    line1: stringAt(v, 'line1') ?? '',
    line2: stringAt(v, 'line2') ?? '',
    line3: stringAt(v, 'line3') ?? '',
    actionText: stringAt(v, 'action_text') ?? '',
    actionKind: stringAt(v, 'action_kind') ?? '',
    explain: stringAt(v, 'explain') ?? '',
    saving: stringAt(v, 'saving') ?? '',
    retiresOn: stringAt(v, 'retires_on') ?? '',
    firedAt: typeof fired === 'number' ? fired : null,
    acting: at(v, 'acting') === true,
  };
}

function nextOf(v: unknown): Next | null {
  if (v === null || typeof v !== 'object') return null;
  const id = stringAt(v, 'id');
  if (id === null) return null;
  return { ...adviceOf(v, id), promotes: stringAt(v, 'promotes') ?? '', row: stringAt(v, 'row') ?? '' };
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
  const snoozed = at(query, 'snoozed');
  return {
    session: stringAt(query, 'session') ?? '',
    model: stringAt(query, 'model') ?? '',
    turn: typeof at(query, 'turn') === 'number' ? (at(query, 'turn') as number) : 0,
    stateLine: stringAt(state, 'line') ?? '',
    stateKind: stringAt(state, 'kind') ?? '',
    lights: parsed,
    nudge: nudgeOf(at(query, 'nudge')),
    next: nextOf(at(query, 'next')),
    nextRow: stringAt(query, 'next_row') ?? 'next     —',
    snoozed: Array.isArray(snoozed) ? snoozed.length : 0,
    snoozedRow: stringAt(query, 'snoozed_row') ?? 'snoozed  —',
    quietRow: stringAt(query, 'quiet_row') ?? '  quiet · nothing to act on',
    recent: Array.isArray(recent) ? recent.map((r) => stringAt(r, 'row') ?? '').filter((r) => r !== '') : [],
    sessionMode: stringAt(query, 'session_mode') ?? 'interactive',
    footer: stringAt(query, 'footer') ?? '',
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

/** The light whose level is highest (the first of equals): open by default. */
export function highestLight(c: Coach): LightId {
  let best = c.lights[0];
  if (!best) return 'context';
  for (const l of c.lights) if (LEVELS.indexOf(l.level) > LEVELS.indexOf(best.level)) best = l;
  return best.id;
}

/** The light open in place: the one pressed, the highest until one is, none once the open one is pressed again. */
export function openLight(model: Model, c: Coach): LightId | null {
  if (model.coachLight === 'none') return null;
  return model.coachLight ?? highestLight(c);
}

/** A light's row text, its third figure dropped below NARROW_COLUMNS body columns (the context light keeps its tokens and bar, and drops the price). */
export function lightText(l: Light, columns: number): string {
  if (columns >= NARROW_COLUMNS) return l.text;
  const parts = l.text.split(' · ');
  return parts.slice(0, l.id === 'context' ? 1 : 2).join(' · ');
}

// ------------------------------------------------------------ the actions

/** Which settings file an action concerns: an allow rule the project's local one, a setting the person's own. */
export type SettingsKind = 'allow_rule' | 'setting';
export type SettingsScope = 'project' | 'user';

export function settingsScope(kind: SettingsKind): SettingsScope {
  return kind === 'allow_rule' ? 'project' : 'user';
}

/**
 * The settings file an action points at: an allow rule the project's
 * personal, uncommitted `<project>/.claude/settings.local.json` (where Claude
 * Code's own "don't ask again" writes), a setting `~/.claude/settings.json`.
 */
export function settingsPath(scope: SettingsScope, root: string, home: string): string {
  return scope === 'project' ? `${root.replace(/\/+$/, '')}/.claude/settings.local.json` : `${home.replace(/\/+$/, '')}/.claude/settings.json`;
}

/** How the person's own settings file is named when HOME is unknown: for them to open by hand, never for a command (no shell expands `~`). */
export const USER_SETTINGS = '~/.claude/settings.json';

/** What a press that would fill says when the box holds a draft (or cctop cannot tell): the draft is never replaced. */
export const DRAFT_KEPT = 'cctop: Your draft is kept — clear the prompt box, then press again';

/** The command that opens a file in the person's editor on `platform` (`$.session.info()`), or null where cctop knows none. */
export function openerFor(platform: string | undefined): 'open' | 'xdg-open' | null {
  if (platform === 'macos') return 'open';
  if (platform === 'linux' || platform === 'wsl') return 'xdg-open';
  return null;
}

/** What a copy of `text` tells the person: where to paste it. */
export function copiedText(text: string, kind: SettingsKind, path: string): string {
  return kind === 'allow_rule' ? `cctop: copied ${text}. Paste it under permissions.allow in ${path}` : `cctop: copied ${text}. Paste it into ${path}`;
}

/** The line under the fix-prompt card once it is in the box. */
export const FILLED_FIX = "✓ It's in your prompt box. Nothing has been sent: read it, change it if you like, then press Enter.";

/** The line after any other fill: what is in the box, and that nothing was sent. */
export function filledText(text: string): string {
  const t = text.trim();
  return t.startsWith('/') ? `✓ ${t} is in your prompt box. Nothing has been sent: press Enter to run it.` : FILLED_FIX;
}

/** A key-kind action as the step the person takes (no Button: cctop never stops a turn itself). */
export function keyStep(a: CoachAction): string {
  if (a.payload === 'Esc') return 'Press Esc to stop the turn';
  if (a.payload === 'Esc Esc') return 'Press Esc twice to rewind';
  const m = /^(.*?)\s*\(([^)]*)\)$/.exec(a.label);
  if (m === null) return `Press ${a.payload}`;
  const what = m[1] ?? '';
  return `Press ${m[2]} to ${what.charAt(0).toLowerCase()}${what.slice(1)}`;
}

/** The elements the coach draws with: the views' Box and Text, plus Button. */
export type CoachElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button'>;

/** What the pane may do on a press; built in pane.tsx over `$`. None of it acts on the session. */
export type CoachActions = {
  /** Draft `text` in an empty prompt box (`$.prompt.fill`); never sends, never replaces a draft. */
  fill(text: string): void;
  /** Put `text` on the clipboard (`$.ui.copy`) and say which settings file it belongs in. */
  copy(text: string, kind: SettingsKind): void;
  /** Open that settings file in the person's editor; cctop never writes it. */
  openSettings(kind: SettingsKind): void;
  /** Five turns' quiet for the rule (`cctop query coach --snooze`). */
  snooze(rule: string): void;
  /** A tip read: the rule is quiet for the session (`--snooze-session`). */
  gotIt(rule: string): void;
  /** The pane's Agents view, for a nudge whose step is the TUI's agents key. */
  agents(): void;
  /** Open a light in place, or close them all (`none`). */
  light(id: LightId | 'none'): void;
  /** Open an "Up next" row in place, or close it when it is the open one. */
  next(id: string): void;
  /** Open the act dialog on this nudge (`a`, or a press on the nudge); the first item when none is named. */
  act(id?: string): void;
};

// ---------------------------------------------------------------- drawing

type Style = Omit<Seg, 'text'>;

function classColor(cls: string): string | undefined {
  return cls === 'NOW' ? CRIT : cls === 'NEXT' ? WARN : undefined;
}

/** `text` wrapped at `inner - indent`, each line after `indent` spaces. */
export function wrapped(text: string, inner: number, style: Style = {}, indent = 0): FrameRow[] {
  const lead = ' '.repeat(indent);
  return wrapWords(text, inner - indent).map((w) => ({ line: [seg(lead), seg(w, style)] }));
}

/** `text` after a `prefix`, the lines after the first hanging under the text. */
export function hanging(prefix: Seg, text: string, inner: number, style: Style = {}): FrameRow[] {
  const w = width(prefix.text);
  return wrapWords(text, inner - w).map((t, i) => ({ line: [i === 0 ? prefix : seg(' '.repeat(w)), seg(t, style)] }));
}

export const blank = (): FrameRow => ({ line: [seg('')] });

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

/** The card's first row: the class badge and the plain name, the age and the rule id dim at the right (on a row of their own when they don't fit). */
function headRows(a: Advice, right: string, inner: number): FrameRow[] {
  const color = classColor(a.cls);
  const badge = seg(` ${a.cls} `, color === undefined ? { inverse: true, bold: true } : { inverse: true, bold: true, color });
  const left: Line = [badge, seg(' '), seg(a.name, { bold: true })];
  const lw = lineWidth(left);
  if (lw + 2 + width(right) <= inner) return [{ line: [...left, seg(' '.repeat(inner - lw - width(right))), dim(right)] }];
  return [{ line: left }, { line: [dim(right)] }];
}

/**
 * One card's numbering: its steps and Buttons 1, 2, … in drawing order, and
 * the digit as the Button's hotkey on the NOW card only (Up next goes
 * without, so the card's digits stay put across polls). `said` is shared by
 * the whole view: the "is in your prompt box" line is drawn once.
 */
export type Counter = { n: number; hotkeys: boolean; said: { filled: boolean } };
function hotkey(counter: Counter): string | undefined {
  const n = counter.n++;
  return counter.hotkeys && n <= 9 ? String(n) : undefined;
}

/** The "is in your prompt box" line, the first time the view draws it. */
export function filledRows(counter: Counter, line: string, inner: number): FrameRow[] {
  if (counter.said.filled) return [];
  counter.said.filled = true;
  return wrapped(line, inner, { color: OK });
}

/** Buttons packed greedily into rows `inner` wide, BUTTON_GAP apart. */
export function packButtons(buttons: FrameButton[], inner: number): FrameRow[] {
  const rows: FrameRow[] = [];
  let cur: FrameButton[] = [];
  let used = 0;
  for (const b of buttons) {
    const w = buttonWidth(b);
    if (cur.length > 0 && used + BUTTON_GAP + w > inner) {
      rows.push({ line: [], buttons: cur });
      cur = [];
      used = 0;
    }
    used += (cur.length === 0 ? 0 : BUTTON_GAP) + w;
    cur.push(b);
  }
  if (cur.length > 0) rows.push({ line: [], buttons: cur });
  return rows;
}

/** The texts a press of this advice can put in the box. */
export function fillTexts(a: Advice): string[] {
  const out: string[] = [];
  for (const x of a.actions) {
    if (x.kind === 'prompt' || x.kind === 'slash') out.push(x.payload);
    if (x.kind === 'allow_rule') out.push('/permissions');
  }
  return out;
}

/** A key-kind action as a numbered step row (it takes the next number of the card). */
export function keyStepRow(x: CoachAction, inner: number, counter: Counter): FrameRow[] {
  const n = counter.n++;
  return hanging(seg(`${n}  `, { color: ACCENT, bold: true }), keyStep(x), inner, { bold: true });
}

/** Whether a key action is a step the person takes (the TUI's agents key is a Button instead). */
export function isKeyStep(x: CoachAction): boolean {
  return x.kind === 'key' && x.payload !== '6 Enter';
}

/** An advice's actions as the key steps (rows) and the Buttons they make, numbered by `counter`; the one place a card's Buttons are built, so the pane's card and the transcript's say and do the same. */
function actionItems(a: Advice, where: string, actions: CoachActions, counter: Counter, stepsShown: boolean, inner = 0): { steps: FrameRow[]; buttons: FrameButton[] } {
  const steps: FrameRow[] = [];
  const buttons: FrameButton[] = [];
  const button = (key: string, label: string, onPress: () => void): void => {
    const hk = hotkey(counter);
    buttons.push({ key: `coach-${where}-${key}`, label, onPress, ...(hk === undefined ? {} : { hotkey: hk }) });
  };
  a.actions.forEach((x, i) => {
    switch (x.kind) {
      case 'key':
        // The TUI's agents key: in the pane, the Agents view.
        if (x.payload === '6 Enter') {
          button(`${i}-agents`, 'Open the agents view', () => actions.agents());
        } else if (!stepsShown) {
          steps.push(...keyStepRow(x, inner, counter));
        }
        break;
      case 'prompt':
      case 'slash':
        button(`${i}-fill`, x.label, () => actions.fill(x.payload));
        break;
      case 'allow_rule':
        button(`${i}-copy`, x.label, () => actions.copy(x.payload, 'allow_rule'));
        button(`${i}-open`, 'Open settings.local.json', () => actions.openSettings('allow_rule'));
        button(`${i}-permissions`, 'Put /permissions in the box', () => actions.fill('/permissions'));
        break;
      case 'setting':
        button(`${i}-copy`, x.label, () => actions.copy(x.payload, 'setting'));
        button(`${i}-open`, 'Open settings.json', () => actions.openSettings('setting'));
        break;
      case 'read':
        button(`${i}-gotit`, x.label === '' ? 'Got it' : x.label, () => actions.gotIt(a.id));
        break;
    }
  });
  button('snooze', 'Snooze', () => actions.snooze(a.id));
  return { steps, buttons };
}

/** The two Buttons a transcript card keeps: the advice's first action and Snooze. A nudge whose only step is a key press has just Snooze. */
export function mainButtons(a: Advice, where: string, actions: CoachActions): FrameButton[] {
  const { buttons } = actionItems(a, where, actions, { n: 1, hotkeys: false, said: { filled: false } }, true);
  const first = buttons.length > 1 ? buttons[0] : undefined;
  const snooze = buttons[buttons.length - 1];
  return [...(first ? [first] : []), ...(snooze ? [snooze] : [])];
}

// An advice's actions, by kind and in the binary's order: a key is a step
// the person takes (shown first, numbered with the Buttons); prompt and
// slash fill the box; an allow rule copies, opens settings.local.json and fills
// /permissions; a setting copies and opens; a tip is "Got it". Snooze last.
// Then the line saying a fill took and nothing was sent.
export function actionRows(
  a: Advice,
  where: string,
  inner: number,
  actions: CoachActions,
  counter: Counter,
  model: Model,
  fixShown: boolean,
  stepsShown = false,
): FrameRow[] {
  const { steps, buttons } = actionItems(a, where, actions, counter, stepsShown, inner);
  const rows: FrameRow[] = [...steps, ...packButtons(buttons, inner)];
  const filled = model.coachFilled;
  if (filled !== null && fillTexts(a).includes(filled) && !(fixShown && filled === a.fixPrompt)) {
    rows.push(...filledRows(counter, filledText(filled), inner));
  }
  return rows;
}

// An advice's words below its headline: the body, the evidence, the fix
// prompt as a card a click puts in the box (and the line saying it is there
// and nothing was sent), the steps and Buttons of its actions, the foot.
function fullRows(a: Advice, where: string, inner: number, actions: CoachActions, counter: Counter, model: Model): FrameRow[] {
  const rows: FrameRow[] = [];
  if (a.body !== '') rows.push(blank(), ...wrapped(a.body, inner));
  if (a.evidence.length > 0) {
    rows.push(blank());
    for (const e of a.evidence) rows.push(...hanging(dim('· '), e, inner));
  }
  const fix = a.fixPrompt;
  if (fix !== null) {
    rows.push(blank(), ...wrapped('Fix it with this prompt · click to put it in your prompt box', inner, { dim: true, bold: true }));
    wrapWords(fix, inner - 2).forEach((t, i) => {
      rows.push({ line: [seg(i === 0 ? '↵ ' : '  ', { color: ACCENT }), seg(t)], press: { key: `coach-${where}-fix-${i}`, onPress: () => actions.fill(fix), dimColor: false } });
    });
    if (model.coachFilled === fix) rows.push(...filledRows(counter, FILLED_FIX, inner));
  }
  rows.push(blank(), ...actionRows(a, where, inner, actions, counter, model, fix !== null));
  if (a.foot !== '') rows.push(...wrapped(a.foot, inner, { dim: true }));
  return rows;
}

// The NOW card: the nudge in full, or the quiet row and the last lifecycle rows.
function nowCard(c: Coach, model: Model, columns: number, now: number, actions: CoachActions, counter: Counter, el: CoachElements): RenderElement {
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = [stateRow(c), { line: [dim('─'.repeat(inner))] }];
  const n = c.nudge;
  if (n === null) {
    rows.push({ line: [dim(c.quietRow)], key: 'advice_saving' });
    for (const r of c.recent.slice(0, 2)) rows.push({ line: [dim(`  ${r}`)] });
  } else {
    const age = n.firedAt === null ? '' : `${formatAgo(now - n.firedAt)} ago · `;
    rows.push(...headRows(n, `${age}rule ${n.id}`, inner));
    // The headline is a press (the act dialog), and `a` is its Button's key.
    wrapWords(n.headline, inner).forEach((t, i) => {
      rows.push({ line: [seg(t, { bold: true })], press: { key: `coach-now-headline-${i}`, onPress: () => actions.act(n.id), dimColor: false } });
    });
    rows.push(...fullRows(n, 'now', inner, actions, counter, model));
    rows.push({ line: [], buttons: [{ key: 'coach-now-act', label: 'Work through it in a dialog', hotkey: 'a', onPress: () => actions.act(n.id) }] });
  }
  return panel({ title: 'coach', summary: `${c.model.replace(/^claude-/, '')} · turn ${c.turn}` }, rows, columns, el);
}

/** A row that opens in place: its text, padded, and the chevron at the right edge; a text that needs the chevron's cells keeps them (a light's 52 cells at 56 columns). */
function toggleRow(text: string, open: boolean, inner: number, key: string, onPress: () => void): FrameRow {
  const press = { key, onPress, dimColor: false };
  if (width(text) + 2 > inner) return { line: [seg(text)], press };
  return { line: [seg(pad(text, inner - 2)), seg(' '), seg(open ? '▾' : '›')], press };
}

// "Up next": the queued nudge as rows that open in place, then the snoozed.
function upNext(c: Coach, model: Model, columns: number, actions: CoachActions, said: Counter['said'], el: CoachElements): RenderElement {
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = [];
  const x = c.next;
  if (x === null) {
    rows.push(line('nothing else queued'));
  } else {
    const open = model.coachNext === x.id;
    const press = () => actions.next(x.id);
    const head = `${x.cls.padEnd(5)} ${x.name}`;
    rows.push(toggleRow(clip(head, inner - 2), open, inner, `coach-next-${x.id}-0`, press));
    // The headline under it, the same press on every line.
    wrapWords(x.headline, inner - 6).forEach((t, i) => {
      rows.push({ line: [seg(`      ${t}`)], press: { key: `coach-next-${x.id}-${i + 1}`, onPress: press, dimColor: false } });
    });
    if (open) rows.push(...fullRows(x, `next-${x.id}`, inner, actions, { n: 1, hotkeys: false, said }, model));
  }
  if (c.snoozed > 0) rows.push(line(c.snoozedRow));
  return panel({ title: 'Up next', summary: 'click a row to open it' }, rows, columns, el);
}

// ------------------------------------------------------- the context light

type Part = { name: string; tokens: number };

/** A `/context` category's glyph and colour: what cannot change (█), what the person loads (▓), the conversation (█, accent), free (░), the reserve (▒). */
function partStyle(name: string): { glyph: string; style: Style } {
  switch (name.toLowerCase()) {
    case 'messages':
      return { glyph: '█', style: { color: ACCENT } };
    case 'mcp tools':
    case 'skills':
    case 'custom agents':
    case 'memory files':
      return { glyph: '▓', style: {} };
    case 'free':
      return { glyph: '░', style: { dim: true } };
    case 'buffer':
      return { glyph: '▒', style: { color: CRIT } };
    default:
      return { glyph: '█', style: {} };
  }
}

/** Cells per part of a bar `cells` wide, by tokens; a part with any tokens keeps at least one cell. */
export function compositionCells(parts: readonly Part[], cells: number): number[] {
  const total = parts.reduce((n, p) => n + Math.max(0, p.tokens), 0);
  if (total <= 0 || cells <= 0) return parts.map(() => 0);
  const out = parts.map((p) => (p.tokens > 0 ? Math.max(1, Math.round((p.tokens / total) * cells)) : 0));
  // The largest part gives or takes the rounding, so the bar is exactly `cells`.
  let diff = cells - out.reduce((a, b) => a + b, 0);
  while (diff !== 0) {
    let big = 0;
    for (let i = 1; i < out.length; i++) if ((out[i] ?? 0) > (out[big] ?? 0)) big = i;
    const step = diff > 0 ? 1 : -1;
    if ((out[big] ?? 0) + step < 1) break;
    out[big] = (out[big] ?? 0) + step;
    diff -= step;
  }
  return out;
}

function partsOf(dashboard: unknown): Part[] {
  const items = at(dashboard, 'composition');
  if (!Array.isArray(items)) return [];
  return items
    .map((i) => ({ name: stringAt(i, 'name') ?? '', tokens: typeof at(i, 'tokens') === 'number' ? (at(i, 'tokens') as number) : 0 }))
    .filter((p) => p.name !== '');
}

// What fills the window, as Claude Code counted it, labelled with where and
// how long ago: a one-row bar and its legend. Nothing without a capture.
function compositionRows(dashboard: unknown, inner: number): FrameRow[] {
  const parts = partsOf(dashboard);
  if (parts.length === 0) return [];
  const w = inner - 2;
  const note = stringAt(dashboard, 'composition_note') ?? stringAt(dashboard, 'composition_source') ?? '';
  const rows = wrapped(`what fills the window${note === '' ? '' : ` · ${note}`}`, inner, { dim: true }, 2);
  const cells = compositionCells(parts, w);
  const bar: Line = [seg('  ')];
  parts.forEach((p, i) => {
    const { glyph, style } = partStyle(p.name);
    if ((cells[i] ?? 0) > 0) bar.push(seg(glyph.repeat(cells[i] ?? 0), style));
  });
  rows.push({ line: bar });
  // The legend, whole entries per row.
  let cur: Line = [seg('  ')];
  let used = 0;
  for (const p of parts) {
    const { glyph, style } = partStyle(p.name);
    const text = `${p.name} ${formatTokensTui(p.tokens)}`;
    const ew = 2 + width(text);
    if (used > 0 && used + 2 + ew > w) {
      rows.push({ line: cur });
      cur = [seg('  ')];
      used = 0;
    }
    if (used > 0) cur.push(seg('  '));
    cur.push(seg(glyph, style), seg(` ${text}`, { dim: true }));
    used += (used > 0 ? 2 : 0) + ew;
  }
  if (used > 0) rows.push({ line: cur });
  return rows;
}

// What put it there: the context body's residency rows (the `m`
// inspector's, as the TUI draws them), from `since …` to the blank row.
function residencyRows(dashboard: unknown, inner: number): FrameRow[] {
  const d = dashboardOf(dashboard);
  const body = d?.bodies.find((b) => b.id === 'context');
  if (body === undefined) return [];
  const text = (r: Seg[]) => r.map((s) => s.text).join('');
  const start = body.rows.findIndex((r) => text(r).trim().startsWith('since '));
  if (start < 0) return [];
  const rows: FrameRow[] = [];
  for (const r of body.rows.slice(start)) {
    const t = text(r).trim();
    if (t === '') break;
    if (width(t) + 2 > inner) {
      rows.push(...wrapped(t, inner, { dim: true }, 2));
      continue;
    }
    // The row's own segments, its two-cell indent kept.
    rows.push({ line: r });
  }
  return rows;
}

function lightDetail(l: Light, model: Model, inner: number): FrameRow[] {
  const rows: FrameRow[] = [];
  if (l.id === 'context') rows.push(...compositionRows(model.query.dashboard, inner), ...residencyRows(model.query.dashboard, inner));
  for (const t of l.lines) rows.push(...wrapped(t, inner, { dim: true }, 2));
  if (l.source !== '') rows.push(...wrapped(`from the ${l.source}${l.approx ? ', an estimate' : ''}`, inner, { dim: true }, 2));
  return rows;
}

// "Session": the four lights, each a row that opens in place.
function lightsFrame(c: Coach, model: Model, columns: number, actions: CoachActions, el: CoachElements): RenderElement {
  const inner = bodyWidth(columns);
  const open = openLight(model, c);
  const rows: FrameRow[] = [];
  for (const l of c.lights) {
    const text = clip(`${l.glyph} ${l.id.padEnd(8)} ${lightText(l, columns)}`, 52);
    const isOpen = open === l.id;
    rows.push({ ...toggleRow(text, isOpen, inner, `coach-light-${l.id}`, () => actions.light(isOpen ? 'none' : l.id)), key: `coach_${l.id}` });
    if (isOpen) rows.push(...lightDetail(l, model, inner));
  }
  return panel({ title: 'Session', summary: 'click a row for the detail' }, rows, columns, el);
}

export function renderCoach(model: Model, el: CoachElements, columns: number, actions: CoachActions, now: number): RenderElement {
  const { Box } = el;
  const c = coachOf(model.query.coach);
  const p = { title: 'coach', summary: c === null ? undefined : `${c.model.replace(/^claude-/, '')} · turn ${c.turn}` };
  if (model.binary === 'missing') return panel(p, [line(NEEDS_BINARY, { key: 'advice_saving' })], columns, el);
  if (c === null) return panel(p, [line('waiting for cctop query coach…', { key: 'advice_saving' })], columns, el);
  const counter: Counter = { n: 1, hotkeys: true, said: { filled: false } };
  const footer = c.footer === '' ? [] : wrapWords(c.footer, columns).map((t) => textRow([dim(t)], el));
  return (
    <Box flexDirection="column">
      {nowCard(c, model, columns, now, actions, counter, el)}
      {upNext(c, model, columns, actions, counter.said, el)}
      {lightsFrame(c, model, columns, actions, el)}
      {footer}
    </Box>
  );
}

/** The inline (classic renderer) form: the L1 line as one row. */
export function coachInlineRow(model: Model, el: ViewElements, columns: number): RenderElement | null {
  const c = coachOf(model.query.coach);
  if (c === null) return null;
  const text = statusLine(c, columns);
  const line: Line = [seg(text)];
  return textRow(line, el, 'advice_saving');
}
