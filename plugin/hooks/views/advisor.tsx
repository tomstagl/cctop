// The Advisor view: `cctop query advice` ranked as it comes, one row per
// item (`rank. rule headline … saving`), the top item expanded beneath its
// row with evidence, action, saving and the rule's explanation, each wrapped
// by word into one truncating Text per line.
import type { RenderElement } from 'claude-code';
import type { Model } from '../model';
import { DASH, at, stringAt } from './format';
import { NEEDS_BINARY, type Color, type ViewElements } from './overview';
import { bodyWidth, line, panel, row, wrapWords, type Cell, type FrameRow } from './table';

const W = { rank: 3, rule: 4, saving: 12, label: 10 };
const EMPTY = 'no recommendation right now — the session looks efficient';

export type Advice = { rule: string; headline: string; saving: string; evidence: string; action: string; explain: string };

export function adviceOf(item: unknown): Advice {
  return {
    rule: stringAt(item, 'rule') ?? '',
    headline: stringAt(item, 'headline') ?? DASH,
    saving: stringAt(item, 'saving') ?? '',
    evidence: stringAt(item, 'evidence') ?? '',
    action: stringAt(item, 'action') ?? '',
    explain: stringAt(item, 'explain') ?? '',
  };
}

function itemCells(a: Advice, rank: number): Cell[] {
  return [
    { text: rank === 1 ? '▸' : `${rank}.`, width: W.rank, right: true, color: rank === 1 ? 'yellow' : undefined, dim: rank !== 1 },
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
  const items = Array.isArray(model.query.advice) ? model.query.advice.map(adviceOf) : [];
  const p = { hotkey: '9', title: 'Advisor', summary: model.binary === 'missing' || items.length === 0 ? undefined : `1 of ${items.length}` };
  if (model.binary === 'missing') return panel(p, [line(NEEDS_BINARY, { key: 'advice_saving' })], columns, el);
  if (items.length === 0) return panel(p, [line(EMPTY, { key: 'advice_saving' })], columns, el);
  const inner = bodyWidth(columns);
  const rows: FrameRow[] = [];
  items.forEach((a, i) => {
    rows.push(row(itemCells(a, i + 1), inner, 'advice_saving'));
    if (i === 0) {
      rows.push(...detail('evidence', a.evidence, inner));
      rows.push(...detail('action', a.action, inner, 'cyan'));
      rows.push(...detail('saving', a.saving || DASH, inner));
      rows.push(...detail('why', a.explain, inner));
    }
  });
  return panel(p, rows, columns, el);
}
