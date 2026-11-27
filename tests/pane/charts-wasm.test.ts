// plugin/hooks/charts.wasm, rendered the way the pane will: compiled, given a
// spec through alloc, drawn through render, freed. Every chart kind as a PNG
// and as Raster cells (the base64 of u32 triplets), on the fixture specs the
// Rust golden tests draw.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';

type Exports = {
  memory: { buffer: ArrayBuffer };
  alloc(len: number): number;
  free(ptr: number, len: number): void;
  render(kind: number, ptr: number, len: number, cols: number, rows: number, scale: number, mode: number): number;
};
type Wasm = {
  Module: new (bytes: Uint8Array) => object;
  Instance: new (m: object, imports: object) => { exports: Exports };
};
const WA = (globalThis as unknown as { WebAssembly: Wasm }).WebAssembly;
const now = (globalThis as unknown as { performance: { now(): number } }).performance.now.bind(
  (globalThis as unknown as { performance: object }).performance,
);

const root = join(__dirname, '..', '..', '..');
const wasmPath = join(root, 'plugin', 'hooks', 'charts.wasm');
const bytes = readFileSync(wasmPath);
const ex = new WA.Instance(new WA.Module(bytes), {}).exports;

const KINDS = ['area_forecast', 'cumulative_forecast', 'bars', 'pills', 'heat', 'stacked_bar', 'progress'];
type Fixture = { name: string; cols: number; rows: number; spec: { kind: string } };
const fixtures: Fixture[] = JSON.parse(
  readFileSync(join(root, 'crates', 'cctop-charts', 'tests', 'fixtures', 'poc.json'), 'utf8'),
);
const enc = new (globalThis as unknown as { TextEncoder: new () => { encode(s: string): Uint8Array } }).TextEncoder();

/** One render call; returns the result bytes (empty on a refusal). */
function render(f: Fixture, scale: number, mode: number): Uint8Array {
  const json = enc.encode(JSON.stringify(f.spec));
  const sp = ex.alloc(json.length);
  new Uint8Array(ex.memory.buffer, sp, json.length).set(json);
  const out = ex.render(KINDS.indexOf(f.spec.kind), sp, json.length, f.cols, f.rows, scale, mode);
  ex.free(sp, json.length);
  const len = new DataView(ex.memory.buffer).getUint32(out, true);
  const res = new Uint8Array(ex.memory.buffer, out + 4, len).slice();
  ex.free(out, 4 + len);
  return res;
}

const ascii = (u: Uint8Array) => String.fromCharCode(...u);

test('the fixtures cover every chart kind', () => {
  assert.deepEqual([...new Set(fixtures.map((f) => f.spec.kind))].sort(), [...KINDS].sort());
});

test('every kind renders a PNG under 2 MiB, at 1x and 2x, of the size asked for', () => {
  for (const f of fixtures) {
    for (const scale of [1, 2]) {
      const png = render(f, scale, 0);
      assert.deepEqual([...png.slice(0, 8)], [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], `${f.name}: PNG magic`);
      assert.ok(png.length < 2 * 1024 * 1024, `${f.name} @${scale}x is ${png.length} bytes`);
      const ihdr = new DataView(png.buffer, png.byteOffset + 16, 8);
      assert.equal(ihdr.getUint32(0), f.cols * 8 * scale, `${f.name}: width`);
      assert.equal(ihdr.getUint32(4), f.rows * 16 * scale, `${f.name}: height`);
    }
  }
});

test('every kind renders cells the Raster takes: base64 of u32 triplets, painted somewhere', () => {
  for (const f of fixtures) {
    const text = ascii(render(f, 1, 1));
    assert.equal(text.length, f.cols * f.rows * 16, `${f.name}: base64 length for ${f.cols * f.rows} cells`);
    assert.match(text, /^[A-Za-z0-9+/]+={0,2}$/, `${f.name}: standard padded base64`);
    const raw = Uint8Array.from(Buffer.from(text, 'base64'));
    assert.equal(raw.length, f.cols * f.rows * 12);
    const w = new DataView(raw.buffer);
    let painted = 0;
    for (let i = 0; i < f.cols * f.rows; i++) {
      const [cp, fg, bg] = [w.getUint32(i * 12, true), w.getUint32(i * 12 + 4, true), w.getUint32(i * 12 + 8, true)];
      assert.ok(cp === 0x20 || cp === 0x2580 || cp === 0x2584, `${f.name}: cell ${i} glyph ${cp.toString(16)}`);
      for (const c of [fg, bg]) assert.ok(c === 0x01000000 || c <= 0xffffff, `${f.name}: cell ${i} colour ${c.toString(16)}`);
      if (fg !== 0x01000000 || bg !== 0x01000000) painted++;
    }
    assert.ok(painted > 0, `${f.name}: nothing painted`);
  }
});

// Row identity with the TUI: Rust draws every overview chart of fixture B with
// render_cells (the TUI's cells) and writes them in the Raster's encoding
// (scripts/pane-fixtures.sh); the wasm's mode 1 must give the same bytes for
// the same spec and size.
test('the wasm draws the overview cells of fixture B exactly as the Rust side wrote them', () => {
  const cells: { name: string; cols: number; rows: number; spec: { kind: string }; cells: string }[] = JSON.parse(
    readFileSync(join(root, 'tests', 'pane', 'fixtures', 'overview-cells-b.json'), 'utf8'),
  );
  assert.ok(cells.length >= 6, 'five charts and the context bar');
  assert.deepEqual([...new Set(cells.map((c) => c.name.split('.')[0]))].sort(), ['cache', 'context', 'cost_per_turn', 'spend', 'tool_calls']);
  for (const c of cells) {
    const text = ascii(render({ name: c.name, cols: c.cols, rows: c.rows, spec: c.spec }, 1, 1));
    assert.equal(text.length, c.cells.length, `${c.name}: length`);
    assert.equal(text, c.cells, `${c.name}: cells differ from the Rust render_cells`);
  }
});

test('a bad spec, kind or size is refused with an empty result', () => {
  const f = fixtures[0]!;
  assert.equal(render({ ...f, cols: 0 }, 1, 0).length, 0);
  assert.equal(render({ ...f, cols: 513 }, 1, 1).length, 0);
  assert.equal(render({ ...f, spec: { kind: 'nope' } }, 1, 0).length, 0);
  assert.equal(render(f, 1, 9).length, 0);
  const junk = enc.encode('{');
  const p = ex.alloc(junk.length);
  new Uint8Array(ex.memory.buffer, p, junk.length).set(junk);
  const out = ex.render(0, p, junk.length, 10, 2, 1, 0);
  assert.equal(new DataView(ex.memory.buffer).getUint32(out, true), 0);
});

test('the memory is capped at 64 MiB and the module imports nothing', () => {
  // Section 5 (memory): one entry, flags 1 (has a maximum), minimum, maximum, in pages of 64 KiB.
  let i = 8;
  const leb = () => {
    let v = 0, s = 0, b;
    do { b = bytes[i++]!; v += (b & 0x7f) * 2 ** s; s += 7; } while (b & 0x80);
    return v;
  };
  let max = -1;
  while (i < bytes.length) {
    const id = bytes[i++]!;
    const size = leb();
    const end = i + size;
    if (id === 5) { leb(); const flags = leb(); leb(); if (flags & 1) max = leb(); }
    if (id === 2) assert.fail('the module has an import section');
    i = end;
  }
  assert.equal(max, 1024, 'memory maximum in 64 KiB pages');
});

// The budget is 30 ms a chart. Wall-clock timing is at the mercy of the
// machine (a loaded laptop, a shared CI runner, the suite's parallel files),
// so by default the assertion is 100 ms: a regression of kind, not of
// jitter. `CCTOP_WASM_TIMING=1` asserts the 30 ms budget itself, for a run
// on a quiet machine. The fastest of 5 runs is asserted (a neighbour's work
// only ever slows a run); the medians are reported as the test's
// diagnostics either way.
const STRICT = process.env.CCTOP_WASM_TIMING === '1';
const BUDGET_MS = STRICT ? 30 : 100;

test(`each chart renders in under ${BUDGET_MS} ms (budget 30 ms${STRICT ? '' : ', asserted with CCTOP_WASM_TIMING=1'}; fastest of 5 after a warm-up, median reported)`, (t) => {
  const rows: string[] = [];
  for (const f of fixtures) {
    for (const [label, scale, mode] of [['png1x', 1, 0], ['png2x', 2, 0], ['cells', 1, 1]] as const) {
      render(f, scale, mode);
      const times: number[] = [];
      for (let n = 0; n < 5; n++) {
        const a = now();
        render(f, scale, mode);
        times.push(now() - a);
      }
      times.sort((x, y) => x - y);
      rows.push(`${f.name} ${f.cols}x${f.rows} ${label}: fastest ${times[0]!.toFixed(2)} ms, median ${times[2]!.toFixed(2)} ms`);
      assert.ok(times[0]! < BUDGET_MS, `${f.name} ${label} took ${times[0]!.toFixed(1)} ms (asserted ${BUDGET_MS}, budget 30; median ${times[2]!.toFixed(1)})`);
    }
  }
  for (const r of rows) t.diagnostic(r);
});
