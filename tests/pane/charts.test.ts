import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { ProcessRunResult } from 'claude-code';
import { fakeEngine, fakeOn, type ProcessScript } from './harness';
import { probeCharts, register } from '../../plugin/hooks/pane';
import {
  chartCells,
  chartsChoice,
  chartsMarker,
  decideCharts,
  instantiateCharts,
  probeOf,
  startCharts,
  toBase64,
  type ChartsEnv,
  type ChartsMode,
  type Probe,
} from '../../plugin/hooks/charts';

// US-006: the decision (a pure table), the wasm wrapper on the real
// charts.wasm, and the pane's wiring: one compile, the marker's `charts`,
// the failure path that draws the query's cells.

const root = join(__dirname, '..', '..', '..');

test('decideCharts: override, then config, then the terminal, then the probe', () => {
  const table: [string, ChartsEnv, Probe, ChartsMode][] = [
    // 1. CCTOP_PANE_CHARTS beats everything, the probe included.
    ['override image', { override: 'image', config: 'cells', termProgram: 'Apple_Terminal' }, 'denied', 'image'],
    ['override cells', { override: 'cells', termProgram: 'ghostty' }, 'ok', 'cells'],
    // 2. `[pane] charts`; `auto` and a typo pass through.
    ['config cells', { config: 'cells', termProgram: 'ghostty' }, 'unknown', 'cells'],
    ['config image', { config: 'image', termProgram: 'vscode' }, 'unknown', 'image'],
    ['override auto falls to config', { override: 'auto', config: 'cells' }, 'unknown', 'cells'],
    ['config typo falls to the terminal', { config: 'pictures', termProgram: 'WezTerm' }, 'unknown', 'image'],
    // 3. The terminal.
    ['ghostty', { termProgram: 'ghostty' }, 'unknown', 'image'],
    ['iTerm.app', { termProgram: 'iTerm.app' }, 'unknown', 'image'],
    ['WezTerm', { termProgram: 'WezTerm' }, 'unknown', 'image'],
    ['kitty', { kitty: true }, 'unknown', 'image'],
    ['iTerm session in tmux', { termProgram: 'tmux', iterm: true, tmux: true }, 'unknown', 'image'],
    ['Apple_Terminal', { termProgram: 'Apple_Terminal' }, 'unknown', 'cells'],
    ['vscode', { termProgram: 'vscode' }, 'unknown', 'cells'],
    ['a cell terminal stays cells whatever the probe', { termProgram: 'vscode' }, 'ok', 'cells'],
    ['unknown terminal tries pictures', {}, 'unknown', 'image'],
    ['unknown terminal in tmux tries pictures', { tmux: true, termProgram: 'tmux' }, 'unknown', 'image'],
    // 4. The probe.
    ['unknown terminal, probe drew it', {}, 'ok', 'image'],
    ['unknown terminal, probe drew the alt', {}, 'denied', 'cells'],
    ['ghostty in tmux without passthrough', { termProgram: 'ghostty', tmux: true }, 'denied', 'cells'],
  ];
  for (const [name, env, probe, want] of table) assert.equal(decideCharts(env, probe), want, name);
});

test('chartsChoice says which step decided; a failed wasm leaves cells', () => {
  assert.equal(chartsChoice({ override: 'cells' }, 'unknown').why, 'override');
  assert.equal(chartsChoice({ config: 'image' }, 'unknown').why, 'config');
  assert.equal(chartsChoice({ termProgram: 'ghostty' }, 'unknown').why, 'terminal');
  assert.equal(chartsChoice({}, 'unknown').why, 'unknown');
  assert.equal(chartsChoice({}, 'denied').why, 'probe');
  assert.deepEqual(chartsMarker({ env: { termProgram: 'ghostty' }, probe: 'unknown', wasm: 'failed' }), { mode: 'cells', why: 'wasm', wasm: 'failed' });
  assert.deepEqual(chartsMarker({ env: { termProgram: 'ghostty' }, probe: 'denied', wasm: 'ok', fix: 'tmux-passthrough' }), {
    mode: 'cells',
    why: 'probe',
    wasm: 'ok',
    fix: 'tmux-passthrough',
  });
});

test('a blit that names a deny is the probe failing', () => {
  assert.equal(probeOf({}), 'ok');
  assert.equal(probeOf({ deny: 'the Image is drawing its alt' }), 'denied');
});

test('toBase64 is standard padded base64', () => {
  for (const n of [0, 1, 2, 3, 4, 5, 255]) {
    const bytes = Uint8Array.from({ length: n }, (_, i) => (i * 37 + 11) & 255);
    const text = toBase64(bytes);
    assert.match(text, /^[A-Za-z0-9+/]*={0,2}$/);
    assert.equal(text.length, Math.ceil(n / 3) * 4, `${n} bytes: padded`);
    assert.deepEqual([...Buffer.from(text, 'base64')], [...bytes], `${n} bytes round trip`);
  }
});

type PocFixture = { name: string; cols: number; rows: number; spec: { kind: string } };
const poc: PocFixture[] = JSON.parse(readFileSync(join(root, 'crates', 'cctop-charts', 'tests', 'fixtures', 'poc.json'), 'utf8'));

async function realModule(): Promise<WebAssembly.Module> {
  const $ = fakeEngine();
  return $.wasm.compile('./charts.wasm');
}

test('the wrapper draws every chart kind as a PNG and as Raster cells through the real wasm', async () => {
  const renderer = await instantiateCharts(await realModule());
  for (const f of poc) {
    const png = renderer.png(f.spec, f.cols, f.rows, 1);
    assert.ok(png !== null, `${f.name}: png`);
    assert.deepEqual([...Buffer.from(png, 'base64').slice(0, 4)], [0x89, 0x50, 0x4e, 0x47], f.name);
    const cells = renderer.cells(f.spec, f.cols, f.rows);
    assert.equal(cells?.length, f.cols * f.rows * 16, `${f.name}: cells`);
  }
  assert.equal(renderer.png({ kind: 'not-a-chart' }, 10, 2, 1), null, 'an unknown kind is refused, not thrown');
});

const FALLBACK = { cols: 3, rows: 1, cells: 'AAAA' };

test('chartCells: the wasm at the size asked for, the query\'s cells when it cannot', async () => {
  const renderer = await instantiateCharts(await realModule());
  const f = poc[0]!;
  const drawn = chartCells(renderer, { spec: f.spec, fallback_cells: FALLBACK }, f.cols, f.rows);
  assert.deepEqual([drawn.cols, drawn.rows], [f.cols, f.rows]);
  assert.notEqual(drawn.cells, FALLBACK.cells);
  assert.deepEqual(chartCells(null, { spec: f.spec, fallback_cells: FALLBACK }, f.cols, f.rows), FALLBACK);
  assert.deepEqual(chartCells(renderer, { spec: { kind: 'nope' }, fallback_cells: FALLBACK }, 9, 9), FALLBACK);
});

test('startCharts: a module that will not compile is logged once and answered, not thrown', async () => {
  const logs: string[] = [];
  const out = await startCharts(() => Promise.reject(new Error('CompileError: bad magic')), (t) => logs.push(t));
  assert.equal(out.wasm, 'failed');
  assert.equal(out.renderer, null);
  assert.equal(logs.length, 1);
  assert.match(logs[0] ?? '', /bad magic/);
});

// -- the pane's wiring ---------------------------------------------------

const SESSION = 'fake-session';
const MARKER = `/home/user/.cctop/pane/${SESSION}.json`;
const ok = (stdout: string): ProcessRunResult => ({ exitCode: 0, stdout, stderr: '', isStdoutTruncated: false, isStderrTruncated: false });
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

async function boot(opts: { env?: Record<string, string>; config?: string; wasm?: 'real' | 'fail' }) {
  const process: Record<string, ProcessScript> = { 'cctop --version': opts.config === undefined ? new Error('ENOENT') : ok('cctop 0.9.0\n') };
  if (opts.config !== undefined) process['cctop query config'] = ok(JSON.stringify({ schema: 1, pane: { charts: opts.config } }));
  const $ = fakeEngine({ env: { HOME: '/home/user', ...opts.env }, process, wasm: opts.wasm });
  const { on, dispatch } = fakeOn($, {});
  register(on, {});
  await dispatch('session.start', { cwd: '/p', surface: 'terminal', isInteractive: false }, () => ({ cwd: '/p' }));
  await settle();
  await settle();
  const marker = () => (JSON.parse($.fs.files.get(MARKER) ?? 'null') as { charts?: Record<string, string> } | null)?.charts;
  return { $, marker };
}

test('session.start compiles ./charts.wasm once and the marker says what was decided', async () => {
  const { $, marker } = await boot({ env: { TERM_PROGRAM: 'ghostty' } });
  assert.deepEqual($.wasm.compiles, ['./charts.wasm']);
  assert.deepEqual(marker(), { mode: 'image', why: 'terminal', wasm: 'ok' });
});

test('the marker follows the order: CCTOP_PANE_CHARTS, config, terminal', async () => {
  assert.deepEqual((await boot({ env: { TERM_PROGRAM: 'ghostty', CCTOP_PANE_CHARTS: 'cells' }, config: 'image' })).marker(), { mode: 'cells', why: 'override', wasm: 'ok' });
  assert.deepEqual((await boot({ env: { TERM_PROGRAM: 'ghostty' }, config: 'cells' })).marker(), { mode: 'cells', why: 'config', wasm: 'ok' });
  assert.deepEqual((await boot({ env: { TERM_PROGRAM: 'Apple_Terminal' } })).marker(), { mode: 'cells', why: 'terminal', wasm: 'ok' });
  assert.deepEqual((await boot({ env: { KITTY_WINDOW_ID: '3' } })).marker(), { mode: 'image', why: 'terminal', wasm: 'ok' });
  assert.deepEqual((await boot({})).marker(), { mode: 'image', why: 'unknown', wasm: 'ok' });
});

test('a wasm that will not compile: one log line, cells, the pane keeps going', async () => {
  const { $, marker } = await boot({ env: { TERM_PROGRAM: 'ghostty' }, wasm: 'fail' });
  assert.equal($.wasm.compiles.length, 1);
  assert.equal($.ui.logs.filter((l) => l.includes('charts.wasm')).length, 1);
  assert.deepEqual(marker(), { mode: 'cells', why: 'wasm', wasm: 'failed' });
  assert.deepEqual(
    $.ui.logs.filter((l) => l.includes('failed')),
    [],
    'nothing else failed: no session.start failure, no init failure',
  );
});

test('the probe blit: a deny switches the session to cells and the marker keeps tmux\'s fix', async () => {
  const { $, marker } = await boot({ env: { TERM_PROGRAM: 'ghostty', TMUX: '/tmp/tmux-1/default,1,0' } });
  assert.deepEqual(marker(), { mode: 'image', why: 'terminal', wasm: 'ok' });
  Object.assign($.ui.blitAnswer, { deny: 'the Image draws its alt', fix: 'tmux-passthrough' });
  await probeCharts($, 'cctop', 'ctx', 'iVBORw0KGgo=');
  await settle();
  assert.equal($.ui.blits.length, 1);
  assert.deepEqual($.ui.blits[0], { requestId: 'cctop', key: 'ctx', source: { png: 'iVBORw0KGgo=' } });
  assert.deepEqual(marker(), { mode: 'cells', why: 'probe', wasm: 'ok', fix: 'tmux-passthrough' });
  await probeCharts($, 'cctop', 'ctx', 'iVBORw0KGgo=');
  assert.equal($.ui.blits.length, 1, 'probed once');
});

test('the probe blit: a taken source keeps pictures', async () => {
  const { $, marker } = await boot({});
  await probeCharts($, 'cctop', 'ctx', 'iVBORw0KGgo=');
  assert.deepEqual(marker(), { mode: 'image', why: 'unknown', wasm: 'ok' });
});
