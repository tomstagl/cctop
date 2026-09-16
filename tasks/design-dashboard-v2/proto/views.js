// Shared content for every view, used as a framed panel (1, 2) or as the body (3).
function bContext(cols) {
  var wide = cols >= 100, bw = wide ? 24 : 14;
  var sl = [['prefix', 44, 's0', 'system prompt · CLAUDE.md · tool schemas — fixed this session'],
            ['harness', 21, 's0', 'reminders and injected files Claude Code adds per turn'],
            ['thinking', 42, 's1', 'retained reasoning — dropped at the next boundary'],
            ['inputs', 49, 's2', 'what Claude wrote to call tools — Bash text dominates'],
            ['results', 82, 's2', 'what tools returned — the lever you actually control']];
  var rows = [[sg('  350k', 'fg', true), sg(' of ', 'dim'), sg('1.00M', 'fg'), sg('   autocompact ', 'dim'), sg('967k', 'fg'), sg('   ', 'dim'), sg('616k', 'ok', true), sg(' left', 'dim'), sg('   +0/turn', 'dim')], []];
  for (var i = 0; i < sl.length; i++) {
    var l = []; at(l, 2); l.push(sg(sl[i][0], 'fg')); rt(l, 16, sl[i][1] + 'k', 'fg', true);
    at(l, 18); var f = Math.round(sl[i][1] / 82 * bw);
    l.push(sg((i % 2 === 0 ? '▇' : '▆').repeat(f), sl[i][2])); l.push(sg('▁'.repeat(bw - f), 'border'));
    if (wide) { at(l, 20 + bw + 2); l.push(sg(sl[i][3], 'dim')); }
    rows.push(l);
  }
  rows.push([]);
  rows.push([sg('  compactions ', 'dim'), sg('0', 'fg'), sg('   re-reads ', 'dim'), sg('3', 'warn'), sg('   stale ', 'dim'), sg('8', 'warn'), sg('   velocity ', 'dim'), sg('—', 'dim'), sg(' (turn 1)', 'dim')]);
  return { title: 'context', keys: 'Enter turn ledger', rows: rows };
}
function bCost(cols) {
  var wide = cols >= 100, bw = wide ? 34 : 18;
  var r = [['cache read', '31.53M', 100, 's2'], ['cache write', '320k', 4, 's1'], ['output', '140k', 2, 's2'], ['└ thinking', '42k', 1, 's1'], ['fresh input', '318', 1, 's0']];
  var rows = [[sg('  ≈$22.50', 'fg', true), sg('   API-equivalent list price — a subscription is not billed per token', 'warn')], []];
  for (var k = 0; k < r.length; k++) {
    var l = []; at(l, 2); l.push(sg(r[k][0], 'fg')); at(l, 16);
    var f = Math.max(1, Math.round(r[k][2] / 100 * bw));
    l.push(sg('▇'.repeat(f), r[k][3])); l.push(sg('▁'.repeat(bw - f), 'border'));
    at(l, 16 + bw + 2); l.push(sg(r[k][1], 'fg', true));
    rows.push(l);
  }
  rows.push([]);
  rows.push([sg('  cache hit ', 'dim'), sg('99 %', 'ok', true), sg('   TTL ', 'dim'), sg('1h', 'fg'), sg('   misses ', 'dim'), sg('0', 'ok'), sg('   burn ', 'dim'), sg('≈$90/h', 'warn', true), sg('   next 30c ', 'dim'), sg('≈$5.6', 'fg')]);
  rows.push([sg('  last session ', 'dim'), sg('$1.19', 'fg'), sg(' over ', 'dim'), sg('1d 13h', 'fg')]);
  return { title: 'cost', keys: 'a agents · Enter ledger', rows: rows };
}
function bTools(cols) {
  var wide = cols >= 100;
  var tr = [['Bash·explore', '69', '0', '148ms', '18k', '0:03', 'grep -rn "fn apply" src/'],
            ['Bash·test', '39', '0', '1.6s', '12k', '1:12', 'cargo test harness_facts'],
            ['Bash·implement', '30', '1', '227ms', '9k', '3:40', 'cargo fmt --all && make…'],
            ['Write', '2', '0', '38ms', '—', '8:20', 'src/agent_ledger.rs'],
            ['Read', '1', '0', '52ms', '10k', '12:04', 'src/agents.rs']];
  var h = []; at(h, 2); h.push(sg('TOOL / CLASS', 'dim')); rt(h, 27, 'CALLS', 'dim'); rt(h, 34, 'ERR', 'dim'); rt(h, 44, 'p50', 'dim'); rt(h, 53, '→CTX', 'dim'); rt(h, 62, 'LAST', 'dim');
  if (wide) { at(h, 66); h.push(sg('LAST INPUT', 'dim')); }
  var rows = [h];
  for (var q = 0; q < tr.length; q++) {
    var l = []; at(l, 2); l.push(sg(tr[q][0], 'fg'));
    rt(l, 27, tr[q][1], 'fg', true); rt(l, 34, tr[q][2], tr[q][2] === '0' ? 'dim' : 'warn', tr[q][2] !== '0');
    rt(l, 44, tr[q][3], 'fg'); rt(l, 53, tr[q][4], (tr[q][4] === '10k' || tr[q][4] === '18k') ? 'warn' : 'fg'); rt(l, 62, tr[q][5], 'dim');
    if (wide) { at(l, 66); l.push(sg(tr[q][6], 'dim')); }
    rows.push(l);
  }
  rows.push([]);
  rows.push([sg('  159 calls', 'fg', true), sg('   1 error ', 'dim'), sg('Command Failed', 'warn'), sg('   0 denied   ', 'dim'), sg('▸ running ', 'accent'), sg('Bash cargo fmt --all', 'fg'), sg('  0:03', 'fg', true)]);
  return { title: 'tools', keys: 's sort · / filter', rows: rows };
}
function bFiles(cols) {
  var fr = [['src/agents.rs', 'R×3', '8k', 're-read ×3 — no edit between', 'warn'],
            ['src/tools.rs', 'R×3', '6k', 're-read ×3 — no edit between', 'warn'],
            ['src/agent_ledger.rs', 'W×1', '—', 'IDE edit after Claude wrote it', 'dim'],
            ['src/task_notification.rs', 'W×1', '—', 'IDE edit after Claude wrote it', 'dim'],
            ['docs/metrics.md', 'R×1', '4k', '', 'dim']];
  var h = []; at(h, 2); h.push(sg('FILE', 'dim')); rt(h, 38, 'TOUCH', 'dim'); rt(h, 46, '→CTX', 'dim');
  if (cols >= 100) { at(h, 50); h.push(sg('NOTE', 'dim')); }
  var rows = [h];
  for (var i = 0; i < fr.length; i++) {
    var l = []; at(l, 2); l.push(sg(fr[i][0], 'fg')); rt(l, 38, fr[i][1], 'fg', true); rt(l, 46, fr[i][2], 'fg');
    if (cols >= 100 && fr[i][3]) { at(l, 50); l.push(sg(fr[i][3], fr[i][4])); }
    rows.push(l);
  }
  rows.push([]);
  rows.push([sg('  27 touched', 'fg', true), sg('   8 stale reads', 'warn'), sg('   2 IDE edits', 'dim'), sg('   git +0/−0', 'dim')]);
  return { title: 'files', keys: 's sort', rows: rows };
}
function bEvents(cols) {
  var ev = [['05:53', 'tool', 'Bash', 'cargo fmt --all && make check > /tmp/check.log', 'fg'],
            ['05:52', 'hook', '', 'PostToolBatch', 'dim'],
            ['05:52', 'tool', 'Bash', '✓ 94 tokens to context', 'fg'],
            ['05:52', 'tool', 'Bash', 'cargo test harness_facts 2>&1 | grep -c ok', 'fg'],
            ['05:52', 'hook', '', 'PostToolBatch', 'dim'],
            ['05:51', 'api', '', 'cost-state  ≈$22.50 · 159 calls · retries 0:00', 'warn'],
            ['05:44', 'note', '', '/clear · continued in a new session', 'accent']];
  return { title: 'events', keys: '/ filter', rows: ev.map(function (e) {
    var l = []; at(l, 2); l.push(sg(e[0], 'dim')); at(l, 9); l.push(sg(e[1], 'accent')); at(l, 15); if (e[2]) l.push(sg(e[2], 'fg'));
    at(l, 21); l.push(sg(e[3], e[4])); return l; }) };
}
var BODIES = { context: bContext, cost: bCost, tools: bTools, files: bFiles, events: bEvents };
function frame(cols, b, onBack) {
  var out = [], t = [sg('╭─ ', 'border'), sg(b.title, 'fg', true), sg(' ', 'border')];
  var tail = [sg(' ', 'border'), sg(b.keys, 'dim'), sg('  ', 'border'), sg('Esc back', 'accent', false, onBack), sg(' ─╮', 'border')];
  var n = cols - lw(t) - lw(tail); t.push(sg('─'.repeat(n > 0 ? n : 0), 'border'));
  out.push(t.concat(tail));
  for (var i = 0; i < b.rows.length; i++) {
    var l = fit([sg('│', 'border')].concat(b.rows[i]), cols - 1); at(l, cols - 1); l.push(sg('│', 'border')); out.push(l);
  }
  out.push([sg('╰' + '─'.repeat(Math.max(0, cols - 2)) + '╯', 'border')]);
  return out;
}
