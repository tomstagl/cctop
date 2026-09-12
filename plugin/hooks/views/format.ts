// Shared by the views: the shape `cctop query` gives every number (a value
// with its unit, metric id and `approx` flag, or a `{ source: "missing" }`
// object), accessors that read one out of the untyped query JSON, and the
// number and time formats the TUI uses (src/ui/fmt.rs) so both draw the same
// text.
import { formatTokens } from '../model';

/** A measured number as `cctop query` reports it. */
export type Measured = { value: number; unit: string; metric_id: string; approx: boolean };

/** What a view draws for a figure it has no source for. */
export const DASH = '—';

function isRecord(x: unknown): x is Record<string, unknown> {
  return x !== null && typeof x === 'object' && !Array.isArray(x);
}

/** `obj.a.b.c` of an unknown JSON value; undefined at the first miss. */
export function at(obj: unknown, ...path: string[]): unknown {
  let cur = obj;
  for (const key of path) {
    if (!isRecord(cur)) return undefined;
    cur = cur[key];
  }
  return cur;
}

/** The measured number at `path`; null when absent, `missing` or malformed. */
export function measured(obj: unknown, ...path: string[]): Measured | null {
  const v = at(obj, ...path);
  if (!isRecord(v) || typeof v.value !== 'number' || !Number.isFinite(v.value)) return null;
  return {
    value: v.value,
    unit: typeof v.unit === 'string' ? v.unit : '',
    metric_id: typeof v.metric_id === 'string' ? v.metric_id : '',
    approx: v.approx === true,
  };
}

/** Whether the value at `path` is a `{ source: "missing" }` object. */
export function isMissing(obj: unknown, ...path: string[]): boolean {
  return at(obj, ...path, 'source') === 'missing';
}

/** The non-empty string at `path`, else null. */
export function stringAt(obj: unknown, ...path: string[]): string | null {
  const v = at(obj, ...path);
  return typeof v === 'string' && v !== '' ? v : null;
}

/** `≈` before an estimate, as the TUI marks `approx: true`. */
export function mark(text: string, approx: boolean): string {
  return approx ? `≈${text}` : text;
}

/** A measured token count, marked when estimated; `—` when absent. */
export function tokensOf(m: Measured | null): string {
  return m === null ? DASH : mark(formatTokens(Math.round(m.value)), m.approx);
}

/** Milliseconds as the TUI's fmt::duration_ms: `0:48`, `2:31`, `1h 12m`, `4d 03h`. */
export function formatDuration(ms: number): string {
  const s = Math.floor(Math.max(0, ms) / 1000);
  const two = (n: number) => String(n).padStart(2, '0');
  if (s < 3600) return `${Math.floor(s / 60)}:${two(s % 60)}`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ${two(Math.floor((s % 3600) / 60))}m`;
  return `${Math.floor(s / 86400)}d ${two(Math.floor((s % 86400) / 3600))}h`;
}

/** Dollars as the TUI's fmt::usd: `$4.37`, `$12.5`, `$137`, `$0.004`. */
export function formatUsd(v: number): string {
  if (v >= 100) return `$${v.toFixed(0)}`;
  if (v >= 10) return `$${v.toFixed(1)}`;
  if (v >= 0.01) return `$${v.toFixed(2)}`;
  return `$${v.toFixed(3)}`;
}
