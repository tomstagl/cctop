// The Advisor view: `cctop query advice` (schema 2) as it comes — the
// coach's slot occupant first (`▸ NOW A47 headline … saving`), then the
// ranked queue one row per item, the occupant expanded beneath its row with
// evidence, action, the exact text to fill, saving and the rule's
// explanation, each wrapped by word into one truncating Text per line.
// Snoozed rules and the engine's session mode close the frame.
import type { RenderElement } from 'claude-code';
import type { Model } from '../model';
import { DASH, at, stringAt } from './format';
import { NEEDS_BINARY, type Color, type ViewElements } from './overview';
import { bodyWidth, line, panel, row, wrapWords, type Cell, type FrameRow } from './table';

const W = { rank: 3, cls: 5, rule: 4, saving: 12, label: 10 };
const EMPTY = 'no recommendation right now — the session looks efficient';

export type Advice = {
  rule: string;
  cls: string;
  headline: string;
  saving: string;
  evidence: string;
  action: string;
  actionText: string;
  actionKind: string;
  retiresOn: string;
  explain: string;
};

export function adviceOf(item: unknown): Advice {
  return {
    rule: stringAt(item, 'rule') ?? '',
    cls: stringAt(item, 'class') ?? '',
    headline: stringAt(item, 'headline') ?? DASH,
    saving: stringAt(item, 'saving') ?? '',
    evidence: stringAt(item, 'evidence') ?? '',
    action: stringAt(item, 'action') ?? '',
    actionText: stringAt(item, 'action_text') ?? '',
    actionKind: stringAt(item, 'action_kind') ?? '',
    retiresOn: stringAt(item, 'retires_on') ?? '',
    explain: stringAt(item, 'explain') ?? '',
  };
}

/** The advice payload as the pane reads it: schema 2's object, or the pre-2 bare array. */
export type AdvicePayload = { items: Advice[]; primary: Advice | null; mode: string | null; snoozed: string[] };

export function advicePayload(query: unknown): AdvicePayload {
  if (Array.isArray(query)) return { items: query.map(adviceOf), primary: null, mode: null, snoozed: [] };
  const items = at(query, 'items');
  const primary = at(query, 'primary');
  const snoozed = at(query, 'snoozed');
  return {
    items: Array.isArray(items) ? items.map(adviceOf) : [],
    primary: primary !== null && typeof primary === 'object' ? adviceOf(primary) : null,
    mode: stringAt(query, 'session_mode'),
    snoozed: Array.isArray(snoozed) ? snoozed.map((s) => stringAt(s, 'rule') ?? '').filter((r) => r !== '') : [],
  };
}

/** The class colour: NOW red, NEXT yellow, LATER plain. */
export function classColor(cls: string): Color | undefined {
  return cls === 'NOW' ? 'red' : cls === 'NEXT' ? 'yellow' : undefined;
}

function itemCells(a: Advice, rank: number, slot: boolean): Cell[] {
  return [
    { text: rank === 1 && slot ? '▸' : `${rank}.`, width: W.rank, right: true, color: rank === 1 && slot ? 'yellow' : undefined, dim: !(rank === 1 && slot) },
    { text: a.cls, width: W.cls, color: classColor(a.cls), dim: a.cls === 'LATER' },
    { text: a.rule, width: W.rule, dim: true },
    { text: a.headline, bold: rank === 1 },
    { text: a.saving, width: W.saving, right: true, key: 'advice_saving' },
  ];
}

// A labelled paragraph: the label on its first line, the text wrapped to
// the columns left of it.
function detail(label: string, text: string, inner: number, color?: Color): FrameRow[] {
  const width = inner - W.rank - 1 - W.label - 1;
  return wrapWords(text, width).map((l, i) =>
    row(
      [
        { text: '', width: W.rank },
        { text: i === 0 ? label : '', width: W.label, dim: true },
        { text: l, color },
      ],
      inner,
    ),
  );
}

export function renderAdvisor(model: Model, el: ViewElements, columns: number): RenderElement {
  const { items, primary, mode, snoozed } = advicePayload(model.query.advice);
  const snoozedTail = snoozed.length > 0 ? ` · ${snoozed.length} snoozed` : '';
  const p = {
    hotkey: '9',
    title: 'Advisor',
    summary: model.binary === 'missing' ? undefined : items.length === 0 ? (snoozedTail === '' ? undefined : snoozedTail.slice(3)) : `1 of ${items.length}${snoozedTail}`,
  };
  if (model.binary === 'missing') return panel(p, [line(NEEDS_BINARY, { key: 'advice_saving' })], columns, el);
  if (items.length === 0) return panel(p, [line(EMPTY, { key: 'advice_saving' })], columns, el);
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = [];
  items.forEach((a, i) => {
    rows.push(row(itemCells(a, i + 1, primary !== null), inner, 'advice_saving'));
    if (i === 0) {
      rows.push(...detail('evidence', a.evidence, inner));
      rows.push(...detail('action', a.action, inner, 'cyan'));
      if (a.actionText !== '') rows.push(...detail(a.actionKind || 'text', a.actionText, inner, 'cyan'));
      rows.push(...detail('saving', a.saving || DASH, inner));
      if (a.retiresOn !== '') rows.push(...detail('retires', `on ${a.retiresOn}`, inner));
      rows.push(...detail('why', a.explain, inner));
    }
  });
  if (snoozed.length > 0 || mode !== null) {
    const parts = [...(snoozed.length > 0 ? [`snoozed: ${snoozed.join(', ')}`] : []), ...(mode !== null && mode !== 'interactive' ? [`session mode: ${mode}`] : [])];
    if (parts.length > 0) rows.push(row([{ text: '', width: W.rank }, { text: parts.join(' · '), dim: true }], inner));
  }
  return panel(p, rows, columns, el);
}
