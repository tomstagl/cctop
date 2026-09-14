// Terminal-mockup toolkit for the coach design canvas: tagged text rows
// (`{ok b:● BUSY}`) measured in cells, frame helpers that mirror the TUI's
// panel.rs / the pane's frame.tsx, and an HTML emitter that paints a row as
// spans in the cctop default-dark theme (themes/default-dark.toml).

export const THEME = {
  bg: '#0E1318',
  fg: '#D6DEE8',
  dim: '#5D6B7D',
  acc: '#4CC2C2',
  ok: '#5FC77E',
  warn: '#E2B04A',
  crit: '#E5605E',
  bd: '#2A3644',
  bdf: '#4CC2C2',
};

/** Visible width of a tagged string: tags stripped, one cell per code point. */
export function vw(s) {
  return [...s.replace(/\{[a-z ]*:/g, '').replace(/\}/g, '')].length;
}

/** Pads a tagged string with spaces to `w` cells; throws when it is wider. */
export function pad(s, w, right = false) {
  const n = w - vw(s);
  if (n < 0) throw new Error(`row is ${-n} cells too wide for ${w}: ${strip(s)}`);
  return right ? ' '.repeat(n) + s : s + ' '.repeat(n);
}

export function strip(s) {
  return s.replace(/\{[a-z ]*:/g, '').replace(/\}/g, '');
}

/** Cuts a tagged string to `w` cells with an ellipsis (tags kept intact). */
export function clip(s, w) {
  if (vw(s) <= w) return s;
  let out = '';
  let used = 0;
  const re = /\{([a-z ]*):([^}]*)\}|([^{]+)/g;
  let m;
  while ((m = re.exec(s)) !== null) {
    const style = m[1];
    const text = m[2] ?? m[3];
    const chars = [...text];
    const room = w - 1 - used;
    if (room <= 0) break;
    const take = chars.slice(0, room).join('');
    out += style === undefined ? take : `{${style}:${take}}`;
    used += [...take].length;
    if (chars.length > room) break;
  }
  return out + '{dim:…}';
}

const H = '─';

/** `╭1 Title ─ summary ────╮` at `w` cells; `hot` is the accent digit. */
export function ftop(w, title, summary, opts = {}) {
  const b = opts.focused ? 'bdf' : 'bd';
  let s = `{${b}:╭}`;
  if (opts.hot !== undefined) s += `{acc b:${opts.hot}}{${b}: }`;
  s += `{b:${title}}`;
  if (summary) s += `{${b}: ${H} }${summary}`;
  s += `{${b}: }`;
  const fill = w - vw(s) - 1;
  if (fill < 0) throw new Error(`title too wide for ${w}: ${strip(s)}`);
  return s + `{${b}:${H.repeat(fill)}╮}`;
}

export function frow(w, content = '', opts = {}) {
  const b = opts.focused ? 'bdf' : 'bd';
  return `{${b}:│ }` + pad(content, w - 4) + `{${b}: │}`;
}

export function fsep(w, opts = {}) {
  const b = opts.focused ? 'bdf' : 'bd';
  return `{${b}:├${H.repeat(w - 2)}┤}`;
}

export function fbot(w, opts = {}) {
  const b = opts.focused ? 'bdf' : 'bd';
  return `{${b}:╰${H.repeat(w - 2)}╯}`;
}

/** A whole frame: top, rows (padded with empty rows to `height` content rows), bottom. */
export function frame(w, title, summary, rows, opts = {}) {
  const out = [ftop(w, title, summary, opts)];
  for (const r of rows) out.push(r === SEP ? fsep(w, opts) : frow(w, r, opts));
  const want = opts.height ?? rows.length;
  while (out.length - 1 < want) out.push(frow(w, '', opts));
  out.push(fbot(w, opts));
  return out;
}

export const SEP = Symbol('sep');

/** A gauge of `cells`, `ratio` filled, fill styled `style`, the rest dim. */
export function gauge(ratio, cells, style = 'ok') {
  const r = Math.max(0, Math.min(1, ratio));
  // A non-zero value always shows one cell, as the TUI draws it.
  const filled = r > 0 ? Math.max(1, Math.round(r * cells)) : 0;
  let s = '';
  if (filled > 0) s += `{${style}:${'▇'.repeat(filled)}}`;
  if (cells - filled > 0) s += `{dim:${'▁'.repeat(cells - filled)}}`;
  return s;
}

/** A stacked gauge: segments `[cells, style]`, the remainder dim ▁. */
export function stacked(segments, cells) {
  let s = '';
  let used = 0;
  for (const [n, style] of segments) {
    s += `{${style}:${'▇'.repeat(n)}}`;
    used += n;
  }
  if (cells - used > 0) s += `{dim:${'▁'.repeat(cells - used)}}`;
  return s;
}

/**
 * The TUI's wide body: a left column of `leftW` and a right column of
 * `rightW` sharing one border column (the right panel draws it), so each
 * row is leftW + rightW - 1 cells.
 */
export function columns(left, right, leftW, rightW) {
  const h = Math.max(left.length, right.length);
  const out = [];
  for (let i = 0; i < h; i++) {
    const l = dropLast(left[i] ?? pad('', leftW));
    const r = right[i] ?? pad('', rightW);
    out.push(l + r);
  }
  return out;
}

/** Removes the last visible cell of a tagged string. */
function dropLast(s) {
  const re = /\{([a-z ]*):([^}]*)\}|([^{]+)/g;
  const parts = [];
  let m;
  while ((m = re.exec(s)) !== null) parts.push({ style: m[1], text: m[2] ?? m[3] });
  for (let i = parts.length - 1; i >= 0; i--) {
    const chars = [...parts[i].text];
    if (chars.length === 0) continue;
    chars.pop();
    parts[i].text = chars.join('');
    break;
  }
  return parts.map((p) => (p.style === undefined ? p.text : p.text === '' ? '' : `{${p.style}:${p.text}}`)).join('');
}

/** Two pane frames side by side, each with its own borders (frame.tsx `split`). */
export function beside(a, b) {
  const h = Math.max(a.length, b.length);
  const out = [];
  for (let i = 0; i < h; i++) out.push((a[i] ?? '') + (b[i] ?? ''));
  return out;
}

// ---- HTML ---------------------------------------------------------------

const esc = (t) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

function spanStyle(style) {
  const parts = style.split(' ').filter(Boolean);
  const css = [];
  let color = null;
  let inverse = false;
  for (const p of parts) {
    if (p === 'b') css.push('font-weight:600');
    else if (p === 'inv') inverse = true;
    else if (p === 'u') css.push('text-decoration:underline;text-underline-offset:3px');
    else if (THEME[p] !== undefined) color = THEME[p];
  }
  if (inverse) {
    css.push(`background:${color ?? THEME.fg}`, `color:${THEME.bg}`);
  } else if (color !== null) css.push(`color:${color}`);
  return css.join(';');
}

/** One row of tagged text as a `<div>` of spans; `w` asserts the width. */
export function rowHtml(s, w) {
  if (w !== undefined && vw(s) !== w) {
    const padded = vw(s) < w ? pad(s, w) : null;
    if (padded === null) throw new Error(`row is ${vw(s)} cells, want ${w}: ${strip(s)}`);
    s = padded;
  }
  const re = /\{([a-z ]*):([^}]*)\}|([^{]+)/g;
  let html = '';
  let m;
  while ((m = re.exec(s)) !== null) {
    const style = m[1];
    const text = esc(m[2] ?? m[3]);
    if (style === undefined || style.trim() === '' || style === 'fg') html += `<span>${text}</span>`;
    else html += `<span style="${spanStyle(style)}">${text}</span>`;
  }
  return `<div style="white-space:pre;margin:0">${html}</div>`;
}

export const CELL = { w: 7.8, h: 17, font: 13 };

/**
 * A terminal screen: `rows` tagged strings of exactly `cols` cells, drawn in
 * a fixed-size dark box. `label` is a small caption above the screen.
 */
export function screen(rows, cols, opts = {}) {
  const cell = opts.cell ?? CELL;
  const padX = 12;
  const padY = 10;
  const width = Math.round(cols * cell.w + padX * 2);
  const height = Math.round(rows.length * cell.h + padY * 2);
  const body = rows.map((r) => rowHtml(r, cols)).join('\n');
  return {
    width,
    height,
    html: `<div style="background:${THEME.bg};color:${THEME.fg};font-family:'IBM Plex Mono',ui-monospace,Menlo,Consolas,monospace;font-size:${cell.font}px;line-height:${cell.h}px;width:${width}px;height:${height}px;box-sizing:border-box;padding:${padY}px ${padX}px;border-radius:6px;overflow:hidden;display:flex;flex-direction:column">\n${body}\n</div>`,
  };
}

/** Wraps a screen (or several stacked) into a `.dc.html` artboard document. */
export function artboard(title, blocks, opts = {}) {
  const gap = 18;
  const captionH = 22;
  const parts = [];
  let height = 0;
  let width = 0;
  for (const b of blocks) {
    const cap = b.caption
      ? `<div style="font-family:'IBM Plex Sans',system-ui,sans-serif;font-size:13px;line-height:${captionH}px;color:#8B95A7;margin:0">${esc(b.caption)}</div>`
      : '';
    parts.push(`<div style="display:flex;flex-direction:column;gap:0;margin:0">${cap}${b.html}</div>`);
    height += (b.caption ? captionH : 0) + b.height;
    width = Math.max(width, b.width);
  }
  height += gap * (blocks.length - 1);
  const outer = 24;
  const head = opts.head
    ? `<div style="font-family:'IBM Plex Sans Condensed','IBM Plex Sans',system-ui,sans-serif;font-weight:700;font-size:20px;line-height:26px;color:#E3E8EF;margin:0">${esc(opts.head)}</div>` +
      (opts.sub
        ? `<div style="font-family:'IBM Plex Sans',system-ui,sans-serif;font-size:13px;line-height:20px;color:#8B95A7;margin:0;max-width:${width}px;text-wrap:pretty">${esc(opts.sub)}</div>`
        : '')
    : '';
  const headH = opts.head ? 26 + (opts.sub ? 20 * (opts.subLines ?? 1) : 0) + 14 : 0;
  const W = width + outer * 2;
  const Hh = height + outer * 2 + headH;
  const doc = `<!doctype html>
<html>
<head>
  <meta charset="utf-8">
  <script src="./support.js"></script>
</head>
<body>
<x-dc>
<helmet>
  <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;600&amp;family=IBM+Plex+Sans:wght@400;600&amp;family=IBM+Plex+Sans+Condensed:wght@700&amp;display=swap">
  <style>
    body { margin: 0; background: #0F1419; }
    a { color: #4CC2C2; } a:hover { color: #7fd6d6; }
  </style>
</helmet>
<div style="background:#0F1419;width:${W}px;height:${Hh}px;box-sizing:border-box;padding:${outer}px;display:flex;flex-direction:column;gap:${gap}px">
${head ? `<div style="display:flex;flex-direction:column;gap:4px;margin:0 0 -4px 0">${head}</div>` : ''}
${parts.join('\n')}
</div>
</x-dc>
</body>
</html>
`;
  return { doc, width: W, height: Hh, title };
}
