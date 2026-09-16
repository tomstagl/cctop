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
function consoleScreen(self, cols) {
  var st = self.state, out = [], b = BODIES[st.body](cols);
  var go = function (v) { return function () { self.setState({ body: v }); }; };
  out.push(sp([sg(' cctop', 'accent', true), sg('  opus-5 · t1 · 52:11 · ~/code/cctop', 'dim')],
              [sg('● ', 'ok'), sg('WORKING', 'ok', true), sg(' 52:11 ', 'dim')], cols));
  var hb = [sg(' ', 'dim')];
  var cell = function (k, v, vr, extra, er, view) {
    hb.push(sg(k + ' ', 'dim', false, view ? go(view) : undefined));
    hb.push(sg(v, vr || 'fg', true, view ? go(view) : undefined));
    if (extra) hb.push(sg(' ' + extra, er || 'dim'));
    hb.push(sg('  │  ', 'border'));
  };
  cell('ctx', '35%', 'fg', '616k left', 'dim', 'context');
  cell('5h', '4%', 'fg', '↻4h06');
  cell('cache', '59m', 'fg', '1h', 'dim', 'cost');
  cell('spend', '≈$22.50', 'fg', '≈$90/h', 'warn', 'cost');
  hb.push(sg('✓ ', 'ok')); hb.push(sg('test 10s', 'ok', false, go('files'))); hb.push(sg('  │  ', 'border'));
  hb.push(sg('159c ', 'fg', false, go('tools'))); hb.push(sg('1 err', 'warn', false, go('tools')));
  out.push(hb);
  out.push(sp([sg(' ▸ ', 'ok', true), sg('steer window', 'fg', true), sg(' — 4 calls and 1:56 since Claude last spoke', 'dim')],
              [sg('9 advisor', 'accent')], cols));
  var t = [sg('─── ', 'border'), sg(b.title, 'fg', true), sg(' ', 'border')];
  var map = [sg(' ', 'border')];
  [['1', 'context'], ['2', 'cost'], ['5', 'tools'], ['7', 'files'], ['8', 'events']].forEach(function (d, i) {
    if (i) map.push(sg('  ', 'border'));
    map.push(sg(d[0], st.body === d[1] ? 'ok' : 'accent', st.body === d[1], go(d[1])));
    map.push(sg(' ' + d[1], st.body === d[1] ? 'ok' : 'dim', st.body === d[1], go(d[1])));
  });
  map.push(sg(' ───', 'border'));
  var n = cols - lw(t) - lw(map); t.push(sg('─'.repeat(n > 0 ? n : 0), 'border'));
  out.push(t.concat(map));
  for (var i = 0; i < b.rows.length; i++) out.push(b.rows[i]);
  var pad = 9 - b.rows.length; for (var z = 0; z < pad; z++) out.push([]);
  out.push([]);
  out.push([sg(' ?', 'accent', true), sg('help   ', 'dim'), sg('1-9', 'accent', true), sg(' change the body   ', 'dim'),
            sg('-', 'accent', true, go('events')), sg(' back   ', 'dim'), sg('/', 'accent', true), sg(' filter   ', 'dim'), sg('q', 'accent', true)]);
  return out;
}
