// The act dialog (mod-pane POC 5, spec §5.2): `a`, or a press on the nudge,
// opens the pane `cctop-act` (focus, closeOnEscape, holdToasts). The
// actionable items (the NOW nudge, the queued one) are a list on the left;
// the selected item's detail is on the right: what changes (an allow rule as
// a settings diff, `Code format="diff"`; a setting as the line to add), the
// exact text a fill would put in the box, and the buttons of its kinds.
//
// Everything a button does is the coach card's own (CoachActions, built once
// in pane.tsx): a fill drafts into an empty prompt box and never sends, a
// copy only copies, a settings file is only opened. "Press Esc to stop the
// turn" is a step the person takes, shown before anything else on a key-kind
// item; cctop has no button that stops a turn. Toasts are held while the
// dialog is open, so the last action's outcome is drawn in the dialog.
//
// Items are the binary's words (the coach object), so the dialog and the card
// never disagree; nothing here keeps what the person or the model wrote.
import type { ElementTable, RenderElement } from 'claude-code';
import type { Model } from '../model';
import { mapAgents } from '../agentmap';
import { agentRows } from './agents';
import { at } from './format';
import { ACCENT, BUTTON_GAP, OK, buttonCells, clip, dim, fit, seg, textRow, type FrameButton, type Line } from './frame';
import {
  actionRows,
  blank,
  coachOf,
  fillTexts,
  hanging,
  isKeyStep,
  keyStepRow,
  packButtons,
  wrapped,
  type Advice,
  type Coach,
  type CoachAction,
  type CoachActions,
  type Counter,
} from './coach';
import { NEEDS_BINARY } from './overview';
import { wrapWords, type FrameRow } from './table';

/** The dialog's pane id, `$.ui.open({ id })`. */
export const ACT_ID = 'cctop-act';

/** The body rows the dialog asks for (`rows` of `$.ui.open`): a request, the engine sizes it. */
export const ACT_ROWS = 22;

/** Body columns from which the list sits left of the detail; below, it stacks above. */
export const ACT_SIDE_COLUMNS = 72;
/** Columns of the list on the left. */
export const ACT_LIST_COLUMNS = 28;

/** One actionable item: the NOW nudge or the queued one, with its full words. */
export type ActItem = { id: string; cls: string; name: string; headline: string; advice: Advice };

/** The items the dialog lists: the nudge in the slot first, then the queued one. */
export function actItems(c: Coach | null): ActItem[] {
  if (c === null) return [];
  const out: ActItem[] = [];
  for (const a of [c.nudge, c.next]) if (a !== null) out.push({ id: a.id, cls: a.cls, name: a.name, headline: a.headline, advice: a });
  return out;
}

/** The selected item: the one pressed while it is still listed, else the first. */
export function selectedItem(items: readonly ActItem[], model: Model): ActItem | null {
  return items.find((i) => i.id === model.actSel) ?? items[0] ?? null;
}

/**
 * What adding an allow rule does to the project's `settings.local.json`, as a
 * unified diff. cctop does not read the file, so the hunk shows the shape
 * (the added line under `permissions.allow`), not the person's other rules.
 */
export function allowDiff(rule: string): string {
  return ['--- a/.claude/settings.local.json', '+++ b/.claude/settings.local.json', '@@ -1,3 +1,4 @@', ' {', '   "permissions": {', '     "allow": [', `+      "${rule}",`].join('\n');
}

/** What the dialog's buttons do: the coach card's, plus the dialog's own selection and the agent jump. */
export type ActActions = CoachActions & {
  /** Select an item of the list. */
  select(id: string): void;
  /** `$.ui.view({ agentId })`: an agent's transcript in the main view (PRD 6). */
  jump(agentId: string): void;
};

export type ActElements = Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button' | 'Code'>;

// A FrameRow drawn without a frame: its segments fitted to `inner`, a press as one plain Button, buttons as a row.
function plainRow(r: FrameRow, inner: number, el: ActElements): RenderElement {
  const { Box, Button } = el;
  if (r.buttons !== undefined && r.buttons.length > 0) {
    return <Box flexDirection="row">{buttonCells(r.buttons, inner, el, Button).parts}</Box>;
  }
  const cells = fit(r.line, inner);
  if (r.press === undefined) return textRow(cells, el, r.key);
  const press = r.press;
  return <Button key={press.key} label={cells.map((s) => s.text).join('')} plain dimColor={press.dimColor ?? true} onPress={press.onPress} />;
}

/** The running agents the pane can put in the main view, `Jump to …` as Buttons (PRD 6). */
function jumpButtons(model: Model, actions: ActActions): FrameButton[] {
  const agents = at(model.query.agents, 'agents');
  if (!Array.isArray(agents)) return [];
  const rows = agentRows(agents).filter((r) => r.running);
  const live = mapAgents(rows, model.liveAgents);
  const out: FrameButton[] = [];
  for (const r of rows) {
    const target = live.get(r.id);
    if (target === undefined || out.length >= 3) continue;
    out.push({ key: `act-jump-${target}`, label: `Jump to agent ${r.type ?? r.id}`, onPress: () => actions.jump(target) });
  }
  return out;
}

/** Whether an action is the TUI's agents key, the one whose item can jump to an agent. */
const isAgentsKey = (x: CoachAction): boolean => x.kind === 'key' && x.payload === '6 Enter';

// ---------------------------------------------------------------- detail

/** The exact texts a press would put in the box, each once: the fix prompt first. */
function exactTexts(a: Advice): string[] {
  const out: string[] = [];
  if (a.fixPrompt !== null) out.push(a.fixPrompt);
  for (const t of fillTexts(a)) if (!out.includes(t)) out.push(t);
  return out;
}

function detail(item: ActItem, model: Model, inner: number, actions: ActActions, el: ActElements): RenderElement[] {
  const a = item.advice;
  const { Code } = el;
  const out: RenderElement[] = [];
  const add = (rows: FrameRow[]): void => {
    for (const r of rows) out.push(plainRow(r, inner, el));
  };
  const counter: Counter = { n: 1, hotkeys: true, said: { filled: false } };
  // The steps the person takes come first: "press Esc to stop the turn".
  const steps = a.actions.filter(isKeyStep);
  for (const x of steps) add(keyStepRow(x, inner, counter));
  if (steps.some((x) => x.payload === 'Esc')) {
    add(wrapped('Esc closes this dialog first; Esc at the prompt then stops the turn. cctop never stops it for you.', inner, { dim: true }, 3));
  }
  if (steps.length > 0) add([blank()]);
  add(wrapped(a.headline, inner, { bold: true }));
  if (a.body !== '') add([blank(), ...wrapped(a.body, inner)]);
  if (a.evidence.length > 0) {
    add([blank()]);
    for (const e of a.evidence) add(hanging(dim('· '), e, inner));
  }
  // What changes: the settings diff, the line to add, the exact text for the box.
  for (const x of a.actions) {
    if (x.kind === 'allow_rule') {
      add([blank(), ...wrapped('What changes: one line is added under permissions.allow in .claude/settings.local.json (yours, not committed). cctop shows it and copies it; it never edits your settings.', inner, { dim: true })]);
      out.push(<Code key="act-diff" source={allowDiff(x.payload)} format="diff" path=".claude/settings.local.json" wrap="truncate-end" />);
      add(wrapped(`copy: ${x.payload}`, inner, { dim: true }));
    } else if (x.kind === 'setting') {
      add([blank(), ...wrapped('What changes: this goes in ~/.claude/settings.json. cctop copies it and opens the file; it never edits it.', inner, { dim: true })]);
      out.push(<Code key="act-setting" source={x.payload} language="json" wrap="truncate-end" />);
    }
  }
  const texts = exactTexts(a);
  if (texts.length > 0) {
    add([blank(), ...wrapped('The exact text · a click puts it in your prompt box, nothing is sent', inner, { dim: true, bold: true })]);
    for (const [k, t] of texts.entries()) {
      wrapWords(t, inner - 2).forEach((w, i) => {
        add([{ line: [seg(i === 0 ? '↵ ' : '  ', { color: ACCENT }), seg(w)], press: { key: `act-text-${k}-${i}`, onPress: () => actions.fill(t), dimColor: false } }]);
      });
    }
  }
  add([blank()]);
  const rows = actionRows(a, 'act', inner, actions, counter, model, false, true);
  // The jump to an agent is a Button of the agents-key item.
  const jumps = a.actions.some(isAgentsKey) ? jumpButtons(model, actions) : [];
  add([...rows, ...packButtons(jumps, inner)]);
  const note = model.actNote;
  if (note !== null) add(wrapped(note, inner, { color: OK }));
  if (a.foot !== '') add(wrapped(a.foot, inner, { dim: true }));
  return out;
}

// ------------------------------------------------------------------ list

function listRows(items: readonly ActItem[], selected: ActItem | null, width_: number, actions: ActActions, el: ActElements): RenderElement[] {
  const { Text, Button } = el;
  const out: RenderElement[] = [];
  for (const it of items) {
    const label = clip(`${it.cls.padEnd(4)} ${it.name}`, width_);
    out.push(
      it.id === selected?.id ? (
        <Text key={`act-pick-${it.id}`} wrap="truncate" inverse>
          {label.padEnd(width_)}
        </Text>
      ) : (
        <Button key={`act-pick-${it.id}`} label={label} plain onPress={() => actions.select(it.id)} />
      ),
      <Text key={`act-sub-${it.id}`} wrap="truncate" dimColor>
        {`  ${clip(it.headline, width_ - 2)}`}
      </Text>,
    );
  }
  return out;
}

/** The dialog's tree: the header line, the list, the selected item's detail. */
export function renderAct(model: Model, el: ActElements, columns: number, actions: ActActions): RenderElement {
  const { Box, Text } = el;
  const c = coachOf(model.query.coach);
  const items = actItems(c);
  const selected = selectedItem(items, model);
  const head: Line = [seg('cctop · act', { color: ACCENT, bold: true }), seg(`  ${items.length} open · Tab and ↑↓ walk · Esc closes · toasts wait`, { dim: true })];
  const header = textRow(fit(head, columns), el);
  if (model.binary === 'missing') return <Box flexDirection="column">{header}<Text wrap="truncate">{NEEDS_BINARY}</Text></Box>;
  if (selected === null) {
    const waiting = c === null ? 'waiting for cctop query coach…' : 'nothing to act on · the coach has no nudge';
    return <Box flexDirection="column">{header}<Text wrap="truncate" dimColor>{waiting}</Text></Box>;
  }
  const side = columns >= ACT_SIDE_COLUMNS;
  const listW = side ? ACT_LIST_COLUMNS : columns;
  const detailW = side ? columns - ACT_LIST_COLUMNS - BUTTON_GAP : columns;
  const list = listRows(items, selected, listW, actions, el);
  const body = detail(selected, model, detailW, actions, el);
  if (!side) {
    return (
      <Box flexDirection="column">
        {header}
        <Box flexDirection="column">{list}</Box>
        <Text wrap="truncate" dimColor>{'─'.repeat(Math.max(0, columns))}</Text>
        <Box flexDirection="column">{body}</Box>
      </Box>
    );
  }
  return (
    <Box flexDirection="column">
      {header}
      <Box flexDirection="row" columnGap={BUTTON_GAP}>
        <Box flexDirection="column" width={listW} flexShrink={0}>
          {list}
        </Box>
        <Box flexDirection="column" width={detailW} flexShrink={0}>
          {body}
        </Box>
      </Box>
    </Box>
  );
}
