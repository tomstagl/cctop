// Notes placed in the transcript where they happened (mod-pane POC 4,
// US-010): a NOW nudge as a bordered card at the turn end — the headline,
// one line of body and its two main Buttons — and a located cache miss as
// one dim row naming the cause and what it rebuilt. Display only: a row is
// drawn by `ui.render` for `Placed`, nothing is written to the transcript,
// and `--resume` brings none back. The Buttons are the coach card's own
// (`mainButtons`), so a press does what the pane's card does.
//
// A placed row cannot be taken back, so a card is drawn live only while its
// nudge is still the one in the slot; once it is snoozed, acted on or
// replaced, the same id answers with one dim settled line.
import type { RenderElement } from 'claude-code';
import { at, formatTokensTui, formatUsd, stringAt } from './format';
import { CRIT, clip, frame, innerWidth, seg, type FrameElements, type Line } from './frame';
import { mainButtons, type Advice, type Coach, type CoachActions } from './coach';
import { wrapWords, type FrameRow } from './table';

/** The widest a card is drawn, however wide the transcript. */
export const CARD_COLUMNS = 100;

/** What a placed row keeps to draw itself later: fixed at the time it was placed. */
export type PlacedRow =
  | { kind: 'nudge'; fire: number; advice: Advice }
  | { kind: 'miss'; turn: number; text: string };

/** The id of a NOW nudge's row, from the fire time the notify code keys on; null for any other nudge or one with no fire time. */
export function nudgePlacement(c: Coach | null): { id: string; row: PlacedRow } | null {
  const n = c?.nudge ?? null;
  if (n === null || n.cls !== 'NOW' || n.firedAt === null) return null;
  return { id: `nudge-${n.firedAt}`, row: { kind: 'nudge', fire: n.firedAt, advice: n } };
}

/** The one line a located miss is: what changed, and how much it re-wrote. */
export function missText(cause: string, tokens: number | null, usd: number | null, after: string | null): string {
  const what = cause === '' ? 'cause not named' : cause;
  const cost = usd === null ? '' : ` ≈ ${formatUsd(usd)}`;
  const written = tokens === null ? '' : `, ${formatTokensTui(tokens)} tokens${cost} re-written`;
  return `cache miss here: ${what}${written}${after === null ? '' : ', after a settings change'}`;
}

/** Every located miss of `cache.located_misses` with a turn, as `miss-<turn>` rows (an unlocated one has no cause to name). */
export function missPlacements(summary: unknown): { id: string; row: PlacedRow }[] {
  const all = at(summary, 'cache', 'located_misses');
  if (!Array.isArray(all)) return [];
  const out: { id: string; row: PlacedRow }[] = [];
  for (const m of all) {
    const turn = at(m, 'turn');
    const kind = stringAt(at(m, 'located'), 'kind');
    if (typeof turn !== 'number' || !Number.isInteger(turn) || kind === null || kind === 'unlocated') continue;
    const tokens = at(m, 'tokens', 'value');
    const usd = at(m, 'cost', 'value');
    const after = stringAt(m, 'after_config_change');
    out.push({
      id: `miss-${turn}`,
      row: { kind: 'miss', turn, text: missText(stringAt(at(m, 'located'), 'detail') ?? '', typeof tokens === 'number' ? tokens : null, typeof usd === 'number' ? usd : null, after) },
    });
  }
  return out;
}

const LEAD = 'cctop';

function missRow(text: string, columns: number, el: FrameElements): RenderElement {
  const { Text } = el;
  const room = Math.max(1, columns - LEAD.length - 2);
  return (
    <Text wrap="truncate">
      <Text color="suggestion">{LEAD}</Text>
      {'  '}
      <Text dimColor>{clip(text, room)}</Text>
    </Text>
  );
}

function settledRow(a: Advice, columns: number, el: FrameElements): RenderElement {
  return missRow(`${a.id} ${a.headline} · no longer current`, columns, el);
}

function cardRows(a: Advice, fire: number, inner: number, actions: CoachActions): FrameRow[] {
  const badge = seg(' NOW ', { inverse: true, bold: true, color: CRIT });
  const head: Line = [badge, seg(' '), seg(clip(a.headline, inner - 6), { bold: true })];
  const rows: FrameRow[] = [{ line: head }];
  const words = wrapWords(a.body, inner);
  const first = words[0];
  // One line of body: the rest of a longer one is cut with an ellipsis.
  if (first !== undefined) rows.push({ line: [seg(words.length > 1 ? `${[...first].slice(0, inner - 1).join('')}…` : first)] });
  const buttons = mainButtons(a, `placed-${fire}`, actions);
  if (buttons.length > 0) rows.push({ line: [], buttons });
  return rows;
}

/**
 * What a placed row draws. `live` is the NOW nudge now in the slot: a card
 * whose fire is not it is a settled line instead. `columns` is the
 * transcript's width.
 */
export function placedTree(row: PlacedRow, live: Coach | null, columns: number, el: FrameElements, actions: CoachActions): RenderElement {
  if (row.kind === 'miss') return missRow(row.text, columns, el);
  const now = nudgePlacement(live);
  if (now === null || now.row.kind !== 'nudge' || now.row.fire !== row.fire) return settledRow(row.advice, columns, el);
  const width = Math.min(columns, CARD_COLUMNS);
  return frame({ title: 'cctop', summary: `rule ${row.advice.id}`, width, rows: cardRows(row.advice, row.fire, innerWidth(width), actions) }, el);
}
