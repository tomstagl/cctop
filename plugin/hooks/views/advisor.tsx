// The Advisor view: `cctop query advice` ranked as it comes, one row per
// item (`rank. rule headline … saving`), the top item expanded beneath its
// row with evidence, action, saving and the rule's explanation, each wrapped
// by word into one truncating Text per line.
import type { RenderElement } from 'claude-code';
import type { Model } from '../model';
import { DASH, at, stringAt } from './format';
import { NEEDS_BINARY, type Color, type ViewElements } from './overview';
import { MAX_ROWS, line, row, wrapWords, type Cell } from './table';

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
function detail(label: string, text: string, columns: number, el: ViewElements, color?: Color): RenderElement[] {
  const width = columns - W.rank - 1 - W.label - 1;
  return wrapWords(text, width).map((l, i) =>
    row(
      [
        { text: '', width: W.rank },
        { text: i === 0 ? label : '', width: W.label, dim: true },
        { text: l, color },
      ],
      el,
    ),
  );
}

export function renderAdvisor(model: Model, el: ViewElements, columns: number): RenderElement {
  const { Box } = el;
  if (model.binary === 'missing') return line(NEEDS_BINARY, el, { key: 'advice_saving' });
  const items = Array.isArray(model.query.advice) ? model.query.advice.map(adviceOf) : [];
  if (items.length === 0) return line(EMPTY, el, { key: 'advice_saving' });
  const rows: RenderElement[] = [];
  items.forEach((a, i) => {
    rows.push(row(itemCells(a, i + 1), el, 'advice_saving'));
    if (i === 0) {
      rows.push(...detail('evidence', a.evidence, columns, el));
      rows.push(...detail('action', a.action, columns, el, 'cyan'));
      rows.push(...detail('saving', a.saving || DASH, columns, el));
      rows.push(...detail('why', a.explain, columns, el));
    }
  });
  return <Box flexDirection="column">{rows.slice(0, MAX_ROWS)}</Box>;
}
