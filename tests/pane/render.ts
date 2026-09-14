// Lays a `ui.render` element tree out as plain text, the way Ink would draw it
// on a terminal `columns` wide: enough of the flexbox model for the pane's
// views (row/column Boxes, fixed widths, flexGrow, padding, Text wrapping and
// truncation) and nothing more. Colours and styles are ignored. Widths are
// counted in UTF-16 code units, so wide glyphs (CJK, emoji) count as one
// column; the pane draws none.
import type { RenderElement, RenderNode } from 'claude-code';

type Props = Record<string, unknown>;
type AnyElement = { type: string; props?: Props; children?: RenderNode[] };

export function renderToText(tree: RenderNode | null | undefined, columns: number, rows?: number): string[] {
  const lines = layout(tree, Math.max(0, columns)).map((l) => l.replace(/\s+$/, ''));
  return rows === undefined ? lines : lines.slice(0, rows);
}

/** Whether a rendered line is a frame's top or bottom border (`╭…╮`, `╰…╯`), possibly two side by side. */
export function isBorder(line: string): boolean {
  return /^[╭╰]/.test(line) && /[╮╯]\s*$/.test(line);
}

/** The title runs of the frame tops in a rendered view: `5 Tools ─ 257 calls`. */
export function frameTitles(lines: string[]): string[] {
  const out: string[] = [];
  for (const line of lines) {
    if (!/^╭/.test(line)) continue;
    for (const m of line.matchAll(/╭([^╮]*?) ─*╮/g)) out.push(m[1].replace(/ ─+$/, '').trim());
  }
  return out;
}

// The body rows of a framed view: the borders dropped and each `│ … │` row
// stripped to what is between the bars, side-by-side frames joined with one
// space. What a flat view drew before the frames, so the shape assertions
// keep applying to the content.
export function body(lines: string[]): string[] {
  const out: string[] = [];
  for (const line of lines) {
    if (isBorder(line)) continue;
    if (!line.startsWith('│')) {
      out.push(line);
      continue;
    }
    const cells = line
      .split('│')
      .slice(1, -1)
      .map((c) => c.replace(/^ /, '').replace(/ $/, ''));
    out.push(cells.join(' ').replace(/\s+$/, ''));
  }
  return out;
}

function num(v: unknown, fallback = 0): number {
  return typeof v === 'number' && Number.isFinite(v) ? v : fallback;
}

function propsOf(node: RenderNode | null | undefined): Props {
  return node !== null && typeof node === 'object' && node.type !== 'engine' ? ((node as AnyElement).props ?? {}) : {};
}

// The text of a Text-like subtree: strings and nested inline elements joined.
function textOf(nodes: RenderNode[] | undefined): string {
  if (!nodes) return '';
  return nodes
    .map((n) => (typeof n === 'string' ? n : n.type === 'engine' ? '' : textOf((n as AnyElement).children)))
    .join('');
}

function pad(line: string, width: number): string {
  return line.length >= width ? line : line + ' '.repeat(width - line.length);
}

// Greedy word wrap; a word longer than the width is cut across lines.
export function softWrap(text: string, width: number): string[] {
  if (width <= 0) return [];
  const lines: string[] = [];
  let line = '';
  for (const word of text.split(' ')) {
    let w = word;
    while (w.length > width) {
      if (line !== '') {
        lines.push(line);
        line = '';
      }
      lines.push(w.slice(0, width));
      w = w.slice(width);
    }
    if (line === '') line = w;
    else if (line.length + 1 + w.length <= width) line += ' ' + w;
    else {
      lines.push(line);
      line = w;
    }
  }
  lines.push(line);
  return lines;
}

export function truncate(text: string, width: number, mode: 'start' | 'middle' | 'end'): string {
  if (text.length <= width) return text;
  if (width <= 0) return '';
  if (width === 1) return '…';
  const keep = width - 1;
  if (mode === 'end') return text.slice(0, keep) + '…';
  if (mode === 'start') return '…' + text.slice(text.length - keep);
  const head = Math.ceil(keep / 2);
  return text.slice(0, head) + '…' + text.slice(text.length - (keep - head));
}

// Text with a `wrap` prop: `wrap` (the default) soft-wraps, anything else cuts
// one line with `…` at the end, start or middle.
function layoutText(text: string, wrap: unknown, width: number): string[] {
  const paragraphs = text.split('\n');
  if (wrap === undefined || wrap === 'wrap') return paragraphs.flatMap((p) => softWrap(p, width));
  const mode =
    wrap === 'truncate-start' ? 'start' : wrap === 'truncate-middle' || wrap === 'middle' ? 'middle' : 'end';
  return paragraphs.map((p) => truncate(p, width, mode));
}

function layout(node: RenderNode | null | undefined, width: number): string[] {
  if (node === null || node === undefined) return [];
  if (typeof node === 'string') return layoutText(node, 'wrap', width);
  if (node.type === 'engine') return [];
  const el = node as AnyElement;
  const p = el.props ?? {};
  switch (el.type) {
    case 'Box':
      return layoutBox(el, Math.min(width, fixedWidth(p, width) ?? width));
    case 'Text':
      return layoutText(textOf(el.children), p.wrap, width);
    case 'Button': {
      const label = typeof p.label === 'string' ? p.label : textOf(el.children);
      const hotkey = typeof p.hotkey === 'string' ? `${p.hotkey} ` : '';
      return [truncate(`[${hotkey}${label}]`, width, 'end')];
    }
    case 'Input': {
      const label = typeof p.label === 'string' ? `${p.label}: ` : '';
      const value = typeof p.value === 'string' ? p.value : typeof p.placeholder === 'string' ? p.placeholder : '';
      return [truncate(`${label}[${value}]`, width, 'end')];
    }
    case 'Select': {
      const label = typeof p.label === 'string' ? `${p.label}: ` : '';
      return [truncate(`${label}[${typeof p.value === 'string' ? p.value : ''} ▾]`, width, 'end')];
    }
    case 'Link': {
      const text = textOf(el.children) || (typeof p.label === 'string' ? p.label : String(p.href ?? ''));
      return layoutText(text, 'wrap', width);
    }
    case 'Code':
      return layoutText(typeof p.source === 'string' ? p.source : '', p.wrap, width);
    default:
      return layoutColumn(el.children ?? [], width, {});
  }
}

function layoutBox(el: AnyElement, width: number): string[] {
  const p = el.props ?? {};
  if (p.display === 'none') return [];
  const margin = num(p.margin);
  const mL = num(p.marginLeft, num(p.marginX, margin));
  const mR = num(p.marginRight, num(p.marginX, margin));
  const mT = num(p.marginTop, num(p.marginY, margin));
  const mB = num(p.marginBottom, num(p.marginY, margin));
  const padding = num(p.padding);
  const padL = num(p.paddingLeft, num(p.paddingX, padding));
  const padR = num(p.paddingRight, num(p.paddingX, padding));
  const padT = num(p.paddingTop, num(p.paddingY, padding));
  const padB = num(p.paddingBottom, num(p.paddingY, padding));
  const border = typeof p.borderStyle === 'string' ? 1 : 0;
  const boxWidth = Math.max(0, width - mL - mR);
  const inner = Math.max(0, boxWidth - 2 * border - padL - padR);
  const children = el.children ?? [];
  const dir = p.flexDirection;
  let body = dir === 'row' || dir === 'row-reverse' ? layoutRow(dir === 'row-reverse' ? [...children].reverse() : children, inner, p) : layoutColumn(dir === 'column-reverse' ? [...children].reverse() : children, inner, p);
  body = [...blank(padT), ...body, ...blank(padB)];
  if (typeof p.height === 'number') body = fit(body, p.height - 2 * border);
  if (typeof p.minHeight === 'number' && body.length < p.minHeight - 2 * border) body = fit(body, p.minHeight - 2 * border);
  if (border) {
    const w = inner + padL + padR;
    body = [
      '┌' + '─'.repeat(w) + '┐',
      ...body.map((l) => '│' + pad(' '.repeat(padL) + l, w) + '│'),
      '└' + '─'.repeat(w) + '┘',
    ];
  } else if (padL > 0) {
    body = body.map((l) => (l === '' ? '' : ' '.repeat(padL) + l));
  }
  if (mL > 0) body = body.map((l) => (l === '' ? '' : ' '.repeat(mL) + l));
  return [...blank(mT), ...body, ...blank(mB)];
}

function blank(n: number): string[] {
  return n > 0 ? Array.from({ length: n }, () => '') : [];
}

function fit(lines: string[], height: number): string[] {
  const h = Math.max(0, height);
  return lines.length >= h ? lines.slice(0, h) : [...lines, ...blank(h - lines.length)];
}

// A child's own width inside `inner`: a number, a percentage, or none.
function fixedWidth(p: Props, inner: number): number | undefined {
  if (typeof p.width === 'number') return Math.min(inner, Math.max(0, p.width));
  if (typeof p.width === 'string' && p.width.endsWith('%')) {
    const pct = Number.parseFloat(p.width);
    if (Number.isFinite(pct)) return Math.min(inner, Math.round((inner * pct) / 100));
  }
  return undefined;
}

function shift(lines: string[], inner: number, align: unknown): string[] {
  if (align !== 'center' && align !== 'flex-end') return lines;
  return lines.map((l) => {
    const room = inner - l.length;
    if (room <= 0) return l;
    return ' '.repeat(align === 'center' ? Math.floor(room / 2) : room) + l;
  });
}

function layoutColumn(children: RenderNode[], inner: number, p: Props): string[] {
  const gap = num(p.rowGap, num(p.gap));
  const out: string[] = [];
  children.forEach((child, i) => {
    if (i > 0) out.push(...blank(gap));
    const cp = propsOf(child);
    const w = fixedWidth(cp, inner) ?? inner;
    const align = cp.alignSelf !== undefined && cp.alignSelf !== 'auto' ? cp.alignSelf : p.alignItems;
    out.push(...shift(layout(child, w), inner, align));
  });
  return out;
}

type RowItem = { node: RenderNode; basis: number; grow: number; shrink: number; fixed: boolean; width: number };

// Takes `overflow` columns away from `items` in proportion to basis x
// flexShrink; returns what could not be taken.
function shrinkItems(items: RowItem[], overflow: number): number {
  const weight = items.reduce((s, it) => s + it.width * it.shrink, 0);
  if (weight <= 0) return overflow;
  for (const it of items) it.width -= Math.min(it.width, Math.floor((overflow * it.width * it.shrink) / weight));
  let left = overflow - items.reduce((s, it) => s + it.basis - it.width, 0);
  for (const it of items) {
    if (left <= 0) break;
    if (it.shrink > 0 && it.width > 0) {
      const cut = Math.min(it.width, left);
      it.width -= cut;
      left -= cut;
    }
  }
  return Math.max(0, left);
}

// Flex row: fixed widths as given, the rest measured by content. When the row
// overflows, content-sized children shrink first (in proportion to their size
// and flexShrink), then fixed-width ones; flexShrink 0 keeps a child at its
// size, so an overflowing row shows up as an over-wide line. Leftover space
// is shared by flexGrow.
function layoutRow(children: RenderNode[], inner: number, p: Props): string[] {
  if (children.length === 0) return [];
  const gap = num(p.columnGap, num(p.gap));
  const avail = Math.max(0, inner - gap * (children.length - 1));
  const items: RowItem[] = children.map((node) => {
    const cp = propsOf(node);
    const fixed = fixedWidth(cp, avail);
    const basis = fixed ?? Math.max(0, ...layout(node, avail).map((l) => l.length));
    return { node, basis, grow: num(cp.flexGrow), shrink: num(cp.flexShrink, 1), fixed: fixed !== undefined, width: basis };
  });
  const total = items.reduce((s, it) => s + it.basis, 0);
  if (total > avail) {
    const left = shrinkItems(items.filter((it) => !it.fixed), total - avail);
    if (left > 0) shrinkItems(items.filter((it) => it.fixed), left);
  } else if (total < avail) {
    const grows = items.reduce((s, it) => s + it.grow, 0);
    if (grows > 0) {
      let free = avail - total;
      items.forEach((it, i) => {
        const share = i === items.length - 1 ? free : Math.floor(((avail - total) * it.grow) / grows);
        if (it.grow > 0) {
          it.width += share;
          free -= share;
        }
      });
    }
  }
  const cells = items.map((it) => layout(it.node, it.width).map((l) => pad(l, it.width)));
  const height = Math.max(0, ...cells.map((c) => c.length));
  const used = items.reduce((s, it) => s + it.width, 0) + gap * (items.length - 1);
  const room = Math.max(0, inner - used);
  const between =
    p.justifyContent === 'space-between' && items.length > 1 ? Math.floor(room / (items.length - 1)) : 0;
  const lead = p.justifyContent === 'flex-end' ? room : p.justifyContent === 'center' ? Math.floor(room / 2) : 0;
  const lines: string[] = [];
  for (let r = 0; r < height; r++) {
    const parts = cells.map((c, i) => c[r] ?? ' '.repeat(items[i].width));
    lines.push(' '.repeat(lead) + parts.join(' '.repeat(gap + between)));
  }
  return lines;
}
