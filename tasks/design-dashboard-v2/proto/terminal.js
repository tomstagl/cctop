var P = { bg:'#0E1318', fg:'#D6DEE8', dim:'#5D6B7D', accent:'#4CC2C2', ok:'#5FC77E', warn:'#E2B04A', crit:'#E5605E', border:'#2A3644',
          s0:'#516A69', s1:'#69AAAA', s2:'#7EF0F0' };
function sg(t, r, b, on) { return { t: String(t), r: r || 'fg', b: !!b, on: on }; }
function lw(l) { var n = 0; for (var i = 0; i < l.length; i++) n += Array.from(l[i].t).length; return n; }
function at(l, c, r) { var n = c - lw(l); if (n > 0) l.push(sg(' '.repeat(n), r || 'dim')); return l; }
function rt(l, c, txt, role, b) { at(l, c - Array.from(String(txt)).length); l.push(sg(txt, role, b)); return l; }
function sp(l, right, cols) { var n = cols - lw(l) - lw(right); l.push(sg(' '.repeat(n > 0 ? n : 1), 'dim')); for (var i = 0; i < right.length; i++) l.push(right[i]); return l; }
function fit(l, cols) { var o = [], u = 0; for (var i = 0; i < l.length; i++) { if (u >= cols) break; var a = Array.from(l[i].t);
  if (u + a.length <= cols) { o.push(l[i]); u += a.length; } else { o.push({ t: a.slice(0, Math.max(0, cols - u - 1)).join('') + '…', r: l[i].r, b: l[i].b }); u = cols; } } return o; }
function rule(cols, role) { return [sg('─'.repeat(cols), role || 'border')]; }
function bar(pct, w, role) { var f = Math.max(0, Math.min(w, Math.round(pct / 100 * w))); if (f === 0 && pct > 0) f = 1; if (f === w && pct < 100) f = w - 1;
  return [sg('▇'.repeat(f), role || 'ok'), sg('▁'.repeat(w - f), 'border')]; }
// the context bar, stacked by agency: fixed / transient / yours, alternating glyphs
function ctx(w) { var filled = Math.round(0.35 * w), a = Math.round(filled * 65 / 238), b = Math.round(filled * 42 / 238), c = filled - a - b;
  return [sg('▇'.repeat(a), 's0'), sg('▆'.repeat(b), 's1'), sg('▇'.repeat(c), 's2'), sg('▁'.repeat(w - filled), 'border')]; }
function paint(rows, cols) { return rows.map(function (l) { l = fit(l, cols);
  if (!l.length) return [{ t: ' '.repeat(cols), c: P.dim, w: '400', cur: 'default', ul: 'none' }];
  var out = l.map(function (s) { return { t: s.t, c: P[s.r] || P.fg, w: s.b ? '600' : '400', on: s.on, cur: s.on ? 'pointer' : 'default', ul: s.on ? 'underline dotted' : 'none' }; });
  var n = cols - l.reduce(function (x, s) { return x + Array.from(s.t).length; }, 0);
  if (n > 0) out.push({ t: ' '.repeat(n), c: P.dim, w: '400', cur: 'default', ul: 'none' });
  return out; }); }
