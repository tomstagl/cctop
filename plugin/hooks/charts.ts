// Picture or cells (US-006 of the mod-pane PRD), and the wrapper over
// charts.wasm. Pure: nothing here touches `$`. pane.tsx compiles the wasm
// (the validator wants the literal there), reads the environment, and hands
// both in.

/** How the pane draws a chart: a PNG `Image`, or a `Raster` of cells. */
export type ChartsMode = 'image' | 'cells';

/** A probe blit of an `Image`: not tried yet, drawn, or refused because the terminal draws its alt. */
export type Probe = 'unknown' | 'ok' | 'denied';

/** What the decision reads of the environment; every field is absent when the variable is unset. */
export type ChartsEnv = {
  /** `CCTOP_PANE_CHARTS`: `image` | `cells` | `auto`. */
  override?: string;
  /** `[pane] charts` from `cctop query config`: the same words. */
  config?: string;
  /** `TERM_PROGRAM`. */
  termProgram?: string;
  /** `KITTY_WINDOW_ID` is set. */
  kitty?: boolean;
  /** `ITERM_SESSION_ID` is set (survives where `TERM_PROGRAM` is rewritten, as in tmux). */
  iterm?: boolean;
  /** `TMUX` is set: pictures pass through tmux only when it allows them, which the probe finds out. */
  tmux?: boolean;
};

/** Which step of the decision spoke. */
export type ChartsWhy = 'override' | 'config' | 'terminal' | 'unknown' | 'probe' | 'wasm';

export type ChartsChoice = { mode: ChartsMode; why: ChartsWhy };

const IMAGE_TERMINALS = ['ghostty', 'iTerm.app', 'WezTerm'];
const CELL_TERMINALS = ['Apple_Terminal', 'vscode'];

const forced = (v: string | undefined): ChartsMode | null => (v === 'image' || v === 'cells' ? v : null);

/**
 * The order: `CCTOP_PANE_CHARTS`, then `[pane] charts`, then the terminal's
 * own name, then the probe's answer. Anything but `image`/`cells` (`auto`, a
 * typo) passes to the next step. An unrecognised terminal tries pictures
 * (`why: 'unknown'`) and the probe has the last word; a recognised
 * picture terminal is overruled by a denied probe too (tmux without
 * passthrough draws a known terminal's alt).
 */
export function chartsChoice(env: ChartsEnv, probe: Probe): ChartsChoice {
  const byOverride = forced(env.override);
  if (byOverride) return { mode: byOverride, why: 'override' };
  const byConfig = forced(env.config);
  if (byConfig) return { mode: byConfig, why: 'config' };
  if (env.termProgram !== undefined && CELL_TERMINALS.includes(env.termProgram)) return { mode: 'cells', why: 'terminal' };
  if (probe === 'denied') return { mode: 'cells', why: 'probe' };
  const known = env.kitty === true || env.iterm === true || (env.termProgram !== undefined && IMAGE_TERMINALS.includes(env.termProgram));
  return { mode: 'image', why: known ? 'terminal' : 'unknown' };
}

export function decideCharts(env: ChartsEnv, probe: Probe): ChartsMode {
  return chartsChoice(env, probe).mode;
}

/** What `$.ui.blit` answered, as far as the probe reads it. */
export type BlitAnswer = { deny?: string; fix?: string };

/**
 * What a probe blit says of the terminal: a taken source, pictures; a deny
 * that names the alt (or tmux's passthrough code), the terminal draws the
 * alt there, so cells. Any other deny — not mounted yet, another size, a bad
 * source, `no-surface` — says nothing of the terminal: still unknown, and the
 * next draw tries again.
 */
export function probeOf(answer: BlitAnswer): Probe {
  if (answer.deny === undefined) return 'ok';
  if (answer.fix === 'tmux-passthrough' || /\balt\b|placeholder/i.test(answer.deny)) return 'denied';
  return 'unknown';
}

/** Whether the wasm is there: not tried, loaded, or failed to compile or instantiate. */
export type WasmState = 'pending' | 'ok' | 'failed';

/** Everything the pane keeps of the decision, and what the marker's `charts` says. */
export type ChartsInfo = {
  env: ChartsEnv;
  probe: Probe;
  wasm: WasmState;
  /** `tmux-passthrough` when the probe's denial said tmux holds pictures back. */
  fix?: string;
};

/** The decision for what the pane knows: a wasm that failed leaves only the query's cells. */
export function chartsDecision(info: ChartsInfo): ChartsChoice {
  if (info.wasm === 'failed') return { mode: 'cells', why: 'wasm' };
  return chartsChoice(info.env, info.probe);
}

/** The marker's `charts`: `cctop pane status` prints it. */
export function chartsMarker(info: ChartsInfo): Record<string, string> {
  const { mode, why } = chartsDecision(info);
  const out: Record<string, string> = { mode, why, wasm: info.wasm };
  if (info.fix !== undefined) out.fix = info.fix;
  return out;
}

// ---------------------------------------------------------------------------
// The wasm: `alloc(len) -> ptr`, `free(ptr, len)`, `render(kind, specPtr,
// specLen, cols, rows, scale, mode) -> ptr` to `[u32 LE len][bytes]`, the
// caller freeing `4 + len`. Mode 0 is a PNG, mode 1 the Raster `cells` prop.

/** Chart kinds in the index order `render` takes: the names `spec.kind` carries. */
export const CHART_KINDS = ['area_forecast', 'cumulative_forecast', 'bars', 'pills', 'heat', 'stacked_bar', 'progress'] as const;

type Exports = {
  memory: { buffer: ArrayBuffer };
  alloc(len: number): number;
  free(ptr: number, len: number): void;
  render(kind: number, ptr: number, len: number, cols: number, rows: number, scale: number, mode: number): number;
};

/** Standard base64 of `bytes` (the `Image` source's `png` and a Raster's cells are this). */
export function toBase64(bytes: Uint8Array): string {
  const abc = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  let out = '';
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i] ?? 0;
    const b = bytes[i + 1] ?? 0;
    const c = bytes[i + 2] ?? 0;
    const n = (a << 16) | (b << 8) | c;
    out += abc.charAt((n >> 18) & 63) + abc.charAt((n >> 12) & 63);
    out += i + 1 < bytes.length ? abc.charAt((n >> 6) & 63) : '=';
    out += i + 2 < bytes.length ? abc.charAt(n & 63) : '=';
  }
  return out;
}

/** The chart renderer over one instantiated charts.wasm. Both answer null when the module refuses the spec. */
export type ChartRenderer = {
  /** How many times `render` was called into the wasm: one per picture drawn. */
  readonly renders: number;
  /** A PNG as base64, `cols * 8 * scale` by `rows * 16 * scale` pixels. */
  png(spec: { kind: string }, cols: number, rows: number, scale: number): string | null;
  /** The Raster `cells` prop: base64 of u32 triplets. */
  cells(spec: { kind: string }, cols: number, rows: number): string | null;
};

const encoder = (): { encode(s: string): Uint8Array } =>
  new (globalThis as unknown as { TextEncoder: new () => { encode(s: string): Uint8Array } }).TextEncoder();

/** Instantiates a compiled charts.wasm; throws when it lacks an export the wrapper calls. */
export async function instantiateCharts(module: WebAssembly.Module): Promise<ChartRenderer> {
  const instance = await WebAssembly.instantiate(module, {});
  const ex = instance.exports as unknown as Exports;
  for (const name of ['memory', 'alloc', 'free', 'render'] as const) {
    if (ex[name] === undefined) throw new Error(`charts.wasm has no export \`${name}\``);
  }
  let renders = 0;
  const call = (spec: { kind: string }, cols: number, rows: number, scale: number, mode: number): Uint8Array | null => {
    const kind = (CHART_KINDS as readonly string[]).indexOf(spec.kind);
    if (kind < 0) return null;
    renders += 1;
    const json = encoder().encode(JSON.stringify(spec));
    const sp = ex.alloc(json.length);
    new Uint8Array(ex.memory.buffer, sp, json.length).set(json);
    let out = 0;
    try {
      out = ex.render(kind, sp, json.length, cols, rows, scale, mode);
    } finally {
      ex.free(sp, json.length);
    }
    const len = new DataView(ex.memory.buffer).getUint32(out, true);
    const bytes = new Uint8Array(ex.memory.buffer, out + 4, len).slice();
    ex.free(out, 4 + len);
    return bytes.length === 0 ? null : bytes;
  };
  return {
    get renders() {
      return renders;
    },
    png: (spec, cols, rows, scale) => {
      const bytes = call(spec, cols, rows, scale, 0);
      return bytes === null ? null : toBase64(bytes);
    },
    cells: (spec, cols, rows) => {
      const bytes = call(spec, cols, rows, 1, 1);
      if (bytes === null) return null;
      let text = '';
      for (const b of bytes) text += String.fromCharCode(b);
      return text;
    },
  };
}

/** What `startCharts` leaves the pane with: a renderer, or none and the reason. */
export type ChartsRuntime = { wasm: 'ok'; renderer: ChartRenderer } | { wasm: 'failed'; renderer: null; error: string };

/**
 * Compiles and instantiates once. A failure is logged once and answered as
 * `failed`, never thrown: the pane then draws the cells the query carries.
 */
export async function startCharts(compile: () => Promise<WebAssembly.Module>, log: (text: string) => void): Promise<ChartsRuntime> {
  try {
    return { wasm: 'ok', renderer: await instantiateCharts(await compile()) };
  } catch (err) {
    const error = err instanceof Error ? err.message : String(err);
    log(`cctop: charts.wasm unavailable, drawing the query's cells: ${error}`);
    return { wasm: 'failed', renderer: null, error };
  }
}

/** A chart's cells as a Raster takes them. */
export type RasterCells = { cols: number; rows: number; cells: string };

/**
 * The cells for a chart's Raster: the wasm's at the size asked for, else (no
 * renderer, or the wasm refused the spec) the query's `fallback_cells`, which
 * are drawn at the chart's default size.
 */
export function chartCells(
  renderer: ChartRenderer | null,
  chart: { spec: { kind: string }; fallback_cells: RasterCells },
  cols: number,
  rows: number,
): RasterCells {
  const cells = renderer?.cells(chart.spec, cols, rows) ?? null;
  return cells === null ? chart.fallback_cells : { cols, rows, cells };
}

/** A chart as the pane draws it: a PNG for an `Image`, or the cells of a `Raster`, and the box of cells it fills. */
export type Picture = { kind: 'image'; png: string; cols: number; rows: number } | { kind: 'raster'; cells: string; cols: number; rows: number };

/** One picture the overview draws: its key, its spec, the box it fills, and the query's cells for it. */
export type PictureJob = {
  /** The `Image` / `Raster` key, and the picture cache's. */
  key: string;
  spec: { kind: string };
  cols: number;
  rows: number;
  /** Cells only, whatever the mode (the sparkline form). */
  cells: boolean;
  /** The query's cells at the spec's default size: what is drawn when the wasm is not there. */
  fallback?: RasterCells;
};

/** PNGs are drawn at twice an assumed 8 × 16 px cell (spec §3). */
export const PNG_SCALE = 2;

/**
 * The picture for one job: a PNG in image mode (one wasm call), else cells
 * at the job's size (one wasm call), else the query's cells at their own
 * size; null when there is none of these.
 */
export function drawPicture(renderer: ChartRenderer | null, job: PictureJob, mode: ChartsMode): Picture | null {
  if (mode === 'image' && !job.cells && renderer !== null) {
    const png = renderer.png(job.spec, job.cols, job.rows, PNG_SCALE);
    if (png !== null) return { kind: 'image', png, cols: job.cols, rows: job.rows };
  }
  const cells = renderer?.cells(job.spec, job.cols, job.rows) ?? null;
  if (cells !== null) return { kind: 'raster', cells, cols: job.cols, rows: job.rows };
  if (job.fallback === undefined) return null;
  return { kind: 'raster', cells: job.fallback.cells, cols: job.fallback.cols, rows: job.fallback.rows };
}

/**
 * What a picture is drawn from: the spec, the box and the mode. Two jobs
 * with the same hash draw the same picture, so only a changed hash is
 * drawn again (FNV-1a over the spec's JSON).
 */
export function pictureHash(job: PictureJob, mode: ChartsMode): string {
  const text = `${JSON.stringify(job.spec)}|${job.cols}x${job.rows}|${job.cells ? 'cells' : mode}`;
  let h = 0x811c9dc5;
  for (let i = 0; i < text.length; i++) {
    h ^= text.charCodeAt(i);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return `${h.toString(16)}:${text.length}`;
}
