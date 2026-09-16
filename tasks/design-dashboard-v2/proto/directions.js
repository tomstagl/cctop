// ============ 1 · INSTRUMENT ============
function instrumentScreen(self, cols) {
  var st = self.state, wide = cols >= 100, out = [];
  if (st.view !== 'overview') return frame(cols, BODIES[st.view](cols), function () { self.setState({ view: 'overview' }); });
  var go = function (v) { return function () { self.setState({ view: v }); }; };
  var C = wide ? { l: 1, v: 30, b: 47, c: 64, n: 70 } : { l: 1, v: 22, b: 35, c: 48, n: 52 };
  var row = function (label, view, v, vr, b, br, c, cr, note, nr) {
    var li = []; at(li, C.l); li.push(sg(label, view ? 'accent' : 'dim', false, view ? go(view) : undefined));
    rt(li, C.v, v, vr || 'fg', true);
    if (b) rt(li, C.b, b, br || 'fg');
    if (c) rt(li, C.c, c, cr || 'fg');
    if (note && wide) { at(li, C.n); li.push(sg(note, nr || 'dim')); }
    out.push(li);
    if (note && !wide && st.w) out.push(at([sg('   ↳ ', 'border'), sg(note, nr || 'dim')], 0));
  };
  out.push(sp([sg(' cctop', 'accent', true)], [sg('opus-5 · turn 1 · 52:11 · ~/code/cctop ', 'dim')], cols));
  out.push(rule(cols));
  var s = []; at(s, 1); s.push(sg('WORKING', 'ok', true));
  rt(s, C.v, '52:11', 'fg', true); rt(s, C.b, '159 calls', 'fg'); rt(s, C.c, '1 err', 'warn');
  if (wide) { at(s, C.n); s.push(sg('▸ ', 'ok')); s.push(sg('steer window', 'fg')); s.push(sg('  ·  1:56 silent', 'dim')); }
  out.push(s); out.push(rule(cols));
  row('context', 'context', '350k', 'fg', 'of 1.00M', 'dim', '35 %', 'fg', '616k to autocompact');
  row('5h limit', null, '4 %', 'fg', 'of 100 %', 'dim', '↻ 4h06', 'dim', '7d 22 %  ·  long ctx 78 %', 'warn');
  row('cache', 'cost', '59m', 'fg', 'of 1h', 'dim', 'warm', 'ok', '0 misses  ·  re-writes 350k cold');
  row('spend', 'cost', '≈$22.50', 'fg', '≈$90/h', 'warn', '≈$29/turn', 'warn', 'API-equivalent, not a bill');
  out.push(rule(cols));
  row('running', 'tools', 'Bash', 'accent', '0:03', 'fg', '', null, 'cargo fmt --all && make check > /tmp/check.log');
  row('tools', 'tools', '159', 'fg', '1 err', 'warn', '', null, 'explore 69 · test 39 · implement 30');
  row('files', 'files', '27', 'fg', '8 stale', 'warn', '', null, 'agents.rs ×3 re-read · 2 IDE edits');
  row('checks', null, '✓', 'ok', 'rework 0', 'ok', '', null, 'cargo test 10s  ·  git +0/−0');
  out.push(rule(cols));
  var e = []; at(e, 1); e.push(sg('05:53', 'dim', false, go('events'))); at(e, 9); e.push(sg('Bash', 'fg')); at(e, 16); e.push(sg('cargo fmt --all && make check > /tmp/check.log', 'dim'));
  out.push(e);
  var e2 = []; at(e2, 1); e2.push(sg('05:52', 'dim', false, go('events'))); at(e2, 9); e2.push(sg('hook', 'fg')); at(e2, 16); e2.push(sg('PostToolBatch', 'dim'));
  out.push(e2); out.push([]);
  out.push([sg(' ?', 'accent', true), sg('help   ', 'dim'), sg('1-9', 'accent', true), sg(' panels   ', 'dim'),
            sg('w', 'accent', true, function () { self.setState({ w: !st.w }); }), sg(st.w ? ' wide ON   ' : ' wide   ', st.w ? 'ok' : 'dim'),
            sg('q', 'accent', true), sg(' quit', 'dim')]);
  return out;
}

// ============ 2 · LEDGER ============
var LROWS = [
  { n: 'context', v: '350k', of: '1.00M', p: 35, note: '616k to autocompact', view: 'context', g: 'budget', stacked: true },
  { n: '5h limit', v: '4 %', of: '100 %', p: 4, note: 'resets 4h06', g: 'budget' },
  { n: '7d limit', v: '22 %', of: '100 %', p: 22, note: 'resets 76h06 · long ctx 78 %', nr: 'warn', g: 'budget' },
  { n: 'cache', v: '59m', of: '1h', p: 98, note: '0 misses · re-writes 350k cold', view: 'cost', g: 'budget' },
  { n: 'spend', v: '≈$22.50', of: '—', p: null, note: '≈$90/h · ≈$29/turn · API-equivalent', view: 'cost', g: 'cost' },
  { n: 'rework', v: '0', of: '—', p: null, note: '✓ cargo test 10s · git +0/−0', nr: 'ok', g: 'work' },
  { n: 'tools', v: '159', of: '—', p: null, note: '1 err · explore 69 · test 39 · impl 30', view: 'tools', g: 'work' },
  { n: 'files', v: '27', of: '—', p: null, note: '8 stale · agents.rs ×3 re-read', nr: 'warn', view: 'files', g: 'work' },
  { n: 'running', v: 'Bash', of: '0:03', p: null, note: 'cargo fmt --all && make check', view: 'tools', g: 'work' }
];
function ledgerScreen(self, cols) {
  var st = self.state, wide = cols >= 100, out = [];
  if (st.view !== 'overview') return frame(cols, BODIES[st.view](cols), function () { self.setState({ view: 'overview' }); });
  var C = wide ? { r: 1, v: 26, l: 40, b: 44, bw: 20, p: 69, n: 73 } : { r: 1, v: 22, l: 33, b: 36, bw: 12, p: 52, n: 56 };
  var rows = LROWS.filter(function (x) { return st.filter === 'all' || x.g === st.filter; });
  var key = st.sort, asc = st.asc;
  rows = rows.slice().sort(function (a, b) {
    var x, y;
    if (key === 'reading') { x = a.n; y = b.n; return asc ? (x < y ? -1 : x > y ? 1 : 0) : (x > y ? -1 : x < y ? 1 : 0); }
    if (key === 'value') { x = a.v; y = b.v; return asc ? (x < y ? -1 : x > y ? 1 : 0) : (x > y ? -1 : x < y ? 1 : 0); }
    x = a.p === null ? -1 : a.p; y = b.p === null ? -1 : b.p;      // blanks last on descending
    return asc ? x - y : y - x;
  });
  var sortBy = function (k) { return function () { self.setState(st.sort === k ? { asc: !st.asc } : { sort: k, asc: k === 'fill' ? false : true }); }; };
  var mark = function (k) { return st.sort === k ? (st.asc ? ' ▲' : ' ▼') : ''; };
  out.push(sp([sg(' cctop', 'accent', true), sg('  opus-5 · turn 1 · 52:11 · ~/code/cctop', 'dim')],
              [sg('● ', 'ok'), sg('WORKING', 'ok', true), sg(' 52:11 · 159c · ', 'dim'), sg('1 err', 'warn')], cols));
  out.push(sp([sg(' Readings', 'fg', true), sg('(' + st.filter + ')', 'dim'), sg('[' + rows.length + ']', 'accent', true)],
              [sg('▸ ', 'ok'), sg('steer window — 4 calls, 1:56 silent', 'fg')], cols));
  out.push([]);
  var h = []; at(h, C.r); h.push(sg('READING' + mark('reading'), st.sort === 'reading' ? 'accent' : 'dim', false, sortBy('reading')));
  rt(h, C.v, 'VALUE' + mark('value'), st.sort === 'value' ? 'accent' : 'dim', false); h[h.length - 1].on = sortBy('value');
  rt(h, C.l, 'OF', 'dim'); at(h, C.b); h.push(sg('FILL' + mark('fill'), st.sort === 'fill' ? 'accent' : 'dim', false, sortBy('fill')));
  rt(h, C.p, '%', 'dim'); at(h, C.n); h.push(sg('NOTE', 'dim'));
  out.push(h);
  for (var i = 0; i < rows.length; i++) {
    var x = rows[i], l = [];
    at(l, C.r); l.push(sg(x.n, x.view ? 'accent' : 'fg', false, x.view ? (function (v) { return function () { self.setState({ view: v }); }; })(x.view) : undefined));
    rt(l, C.v, x.v, 'fg', true); rt(l, C.l, x.of, 'dim');
    at(l, C.b);
    if (x.p === null) l.push(sg('·'.repeat(C.bw), 'border'));
    else if (x.stacked) { var cb = ctx(C.bw); for (var z = 0; z < cb.length; z++) l.push(cb[z]); }
    else { var bb = bar(x.p, C.bw, 'ok'); for (var z2 = 0; z2 < bb.length; z2++) l.push(bb[z2]); }
    rt(l, C.p, x.p === null ? '—' : String(x.p), x.p === null ? 'dim' : 'fg', x.p !== null);
    at(l, C.n); l.push(sg(x.note, x.nr || 'dim'));
    out.push(l);
  }
  out.push([]);
  var f = [sg(' s', 'accent', true), sg(' sort   ', 'dim'), sg('/', 'accent', true), sg(' filter: ', 'dim')];
  ['all', 'budget', 'cost', 'work'].forEach(function (g) {
    f.push(sg(g, st.filter === g ? 'ok' : 'dim', st.filter === g, function () { self.setState({ filter: g }); }));
    f.push(sg('  ', 'dim'));
  });
  f.push(sg('  1-9', 'accent', true)); f.push(sg(' panels', 'dim'));
  out.push(f);
  return out;
}

// ============ 3 · CONSOLE ============
// Header cells are whole-area targets. Each renders as Claude Code draws a
// `plain` Button with a hotkey — the digit in the accent colour, a colon, the
// label (claude-code.d.ts, ButtonProps.plain) — so the affordance is native and
// the digits are contiguous and 1:1 with the cells.
var CELLS = [
  { k: '1', id: 'context', short: 'ctx 35% 616k left',      mid: 'ctx 35% 616k left',       wide: 'ctx 35% · 616k left' },
  { k: '2', id: 'limits',  short: '5h 4%',                   mid: '5h 4% ↻4h06',             wide: '5h 4% · ↻ 4h06' },
  { k: '3', id: 'cache',   short: 'cache 59m',               mid: 'cache 59m 1h TTL',        wide: 'cache 59m · 1h TTL' },
  { k: '4', id: 'cost',    short: 'spend ≈$22.50',           mid: 'spend ≈$22.50 ≈$90/h',    wide: 'spend ≈$22.50 · ≈$90/h' },
  { k: '5', id: 'work',    short: '✓ test 10s',              mid: '✓ test 10s · rework 0',   wide: '✓ test 10s · rework 0', tone: 'ok' },
  { k: '6', id: 'tools',   short: '159c 1 err',              mid: '159 calls · 1 err',       wide: '159 calls · 1 err' }
];
function consoleScreen(self, cols) {
  var st = self.state, out = [];
  var go = function (v) { return function () { self.setState({ body: v }); }; };
  var perRow = cols >= 80 ? 3 : 2, cw = Math.floor((cols - 2) / perRow);
  var field = cols >= 80 ? 'wide' : (cw >= 28 ? 'mid' : 'short');
  out.push(sp([sg(' cctop', 'accent', true), sg('  opus-5 · t1 · 52:11', 'dim')],
              [sg('● ', 'ok'), sg('WORKING', 'ok', true), sg(' 52:11 ', 'dim')], cols));
  for (var i = 0; i < CELLS.length; i += perRow) {
    var l = [sg(' ', 'dim')];
    for (var j = 0; j < perRow && i + j < CELLS.length; j++) {
      var c = CELLS[i + j], on = go(c.id), grp = 'cell-' + c.id, active = st.body === c.id;
      var text = c[field], pad = cw - 2 - Array.from(text).length;
      l.push(sg(c.k + ':', active ? 'ok' : 'accent', true, on, grp));
      l.push(sg(text, active ? 'ok' : (c.tone || 'fg'), active, on, grp));
      if (pad > 0) l.push(sg(' '.repeat(pad), 'dim', false, on, grp));
    }
    out.push(at(l, cols));
  }
  var act = [sg(' ▸ ', 'ok', true, go('advisor'), 'cell-advisor'),
             sg('steer window', 'fg', true, go('advisor'), 'cell-advisor'),
             sg(cols >= 72 ? ' — 4 calls, 1:56 silent' : ' · 4c 1:56', 'dim', false, go('advisor'), 'cell-advisor')];
  out.push(cols >= 66 ? sp(act, [sg('a:', 'accent', true, go('advisor'), 'cell-advisor'), sg('advisor', 'dim', false, go('advisor'), 'cell-advisor')], cols)
                      : at(act, cols));
  var b = BODIES[st.body] ? BODIES[st.body](cols) : bEvents(cols);
  var t = [sg('─── ', 'border'), sg(b.title, 'fg', true), sg(' ', 'border')];
  var tail = [sg(' ', 'border'), sg('0', 'accent', true, go('events'), 'cell-home'), sg(' home', 'dim', false, go('events'), 'cell-home'),
              sg('  ·  ', 'border'), sg('?', 'accent', true), sg(' keys', 'dim'), sg(' ───', 'border')];
  var n = cols - lw(t) - lw(tail); t.push(sg('─'.repeat(n > 0 ? n : 0), 'border'));
  out.push(t.concat(tail));
  for (var z = 0; z < b.rows.length; z++) out.push(b.rows[z]);
  return out;
}
function bWork(cols) {
  var rows = [[sg('  ✓ ', 'ok'), sg('cargo test', 'fg', true), sg('  passed 10s ago  ·  0 edits since', 'dim')], [],
              [sg('  rework   ', 'dim'), sg('0 open', 'ok', true), sg('   no failing streak, no correction, nothing blocked', 'dim')],
              [sg('  files    ', 'dim'), sg('27 touched', 'fg', true), sg('   2 IDE edits  ·  ', 'dim'), sg('8 stale reads', 'warn')],
              [sg('  re-reads ', 'dim'), sg('agents.rs ×3', 'warn'), sg('  ·  ', 'border'), sg('tools.rs ×3', 'warn')],
              [sg('  git      ', 'dim'), sg('+0/−0', 'fg'), sg('   nothing uncommitted', 'dim')]];
  return { title: 'work', keys: 's sort', rows: rows };
}
function bLimits(cols) {
  var rows = [], m = function (label, v, pct, note) {
    var l = []; at(l, 2); l.push(sg(label, 'fg')); rt(l, 18, v, 'fg', true);
    at(l, 20); var bw = Math.min(20, cols - 46); var bb = bar(pct, bw, 'ok');
    for (var i = 0; i < bb.length; i++) l.push(bb[i]);
    at(l, 22 + bw); l.push(sg(note, 'dim')); return l; };
  rows.push(m('5 hours', '4 %', 4, 'resets 4h06'));
  rows.push(m('7 days', '22 %', 22, 'resets 76h06'));
  rows.push([]);
  rows.push([sg('  weight   ', 'dim'), sg('×5', 'warn', true), sg(' opus — why the bar moves faster than the dollars', 'dim')]);
  rows.push([sg('  long ctx ', 'dim'), sg('78 %', 'warn', true), sg(' of usage was above 150k context', 'dim')]);
  rows.push([sg('  account-wide: other sessions draw from the same pool', 'dim')]);
  return { title: 'limits', keys: '', rows: rows };
}
function bCache(cols) {
  var rows = [[sg('  warm ', 'ok'), sg('59m', 'ok', true), sg(' left of a ', 'dim'), sg('1h', 'fg'), sg(' entry', 'dim')], [],
              [sg('  misses      ', 'dim'), sg('0', 'ok', true)],
              [sg('  hit ratio   ', 'dim'), sg('99 %', 'ok', true)],
              [sg('  re-write    ', 'dim'), sg('350k', 'warn', true), sg(' if it goes cold before the next call', 'dim')], [],
              [sg('  reply before the countdown ends, or the next call pays the re-write', 'dim')]];
  return { title: 'cache', keys: '', rows: rows };
}
function bAdvisor(cols) {
  var rows = [[sg('  ▸ ', 'ok', true), sg('steer window is open', 'fg', true)],
              [sg('    4 tool calls and 1:56 since Claude last wrote to you.', 'dim')],
              [sg('    Type now to redirect, or let it run.', 'dim')], [],
              [sg('  class    ', 'dim'), sg('OPEN', 'ok', true), sg('   retires at the turn\'s end', 'dim')],
              [sg('  next     ', 'dim'), sg('—', 'dim')],
              [sg('  snoozed  ', 'dim'), sg('—', 'dim')]];
  return { title: 'advisor', keys: 'x snooze · e why', rows: rows };
}
BODIES.work = bWork; BODIES.limits = bLimits; BODIES.cache = bCache; BODIES.advisor = bAdvisor;
