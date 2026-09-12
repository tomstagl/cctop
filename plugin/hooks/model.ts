// The pane's state and the pure reducer that advances it. Nothing in here
// touches `$`: the hooks in pane.tsx turn engine events into actions, the
// views draw the model. Kept pure so it is testable from plain data.
import type { SessionUsage } from 'claude-code';

export type TurnState = 'idle' | 'busy' | 'waiting';
export type View = 'overview' | 'tools' | 'agents' | 'files' | 'events' | 'advisor';
export type Placement = 'dock' | 'inline';
export type Binary = 'unknown' | 'present' | 'missing';

/** The `cctop query` verbs the pane polls, in the order one tick runs them. */
export const QUERY_VERBS = ['summary', 'tools', 'files', 'agents', 'advice', 'events'] as const;
export type QueryVerb = (typeof QUERY_VERBS)[number];
/** The parsed JSON of the last successful `cctop query <verb>`, per verb. */
export type QueryData = Partial<Record<QueryVerb, unknown>>;
/** What a section draws when its verb is not in this binary's `cctop query --help`. */
export const UNSUPPORTED = 'unsupported by this cctop version';

export type RunningTool = { name: string; startedAt: number };

export type ToolStats = {
  calls: number;
  errors: number;
  durationsMs: number[];
  /** Σ ceil(len(result text) / 4): the TUI's `tokens_to_ctx` heuristic. */
  tokensToCtx: number;
};

export type Turn = {
  /** Turns started this session (0 before the first `turn.start`). */
  number: number;
  state: TurnState;
  /** When the current turn started; null while idle. */
  startedAt: number | null;
  /** `durationMs` and `reason` of the last `turn.complete`. */
  lastDurationMs: number | null;
  lastReason: string | null;
  /** The most recently started tool call that has not ended. */
  runningTool: RunningTool | null;
  /** Σ durations of the tool calls that ended in this turn (the running one is added at render). */
  toolMs: number;
};

export type Model = {
  /** The last `$.session.usage()` answer and when it was read. */
  usage: SessionUsage | null;
  usageAt: number | null;
  /** `$.session.model()`, read once after session.start. */
  modelName: string | null;
  turn: Turn;
  tools: Record<string, ToolStats>;
  compactions: number;
  binary: Binary;
  /** `$.session.id()`, read by the poller on its first tick. */
  sessionId: string | null;
  // Precedence between the two sources: the Context and Limits rows come
  // from `usage` (engine-native, live) when it is present, else from
  // `query.summary`; every other figure (tokens & cost breakdown, burn rate,
  // tools, files, agents, advice, events) comes from the query JSON alone.
  query: QueryData;
  /** When the last tick in which every called verb parsed ended; null before one. */
  queryAt: number | null;
  /** The verbs `cctop query --help` lists; null until the poller has read it. */
  verbs: readonly QueryVerb[] | null;
  view: View;
  open: boolean;
  /** True when the last successful `cctop query` tick is older than 30 s. */
  stale: boolean;
  placement: Placement;
};

export type Action =
  | { type: 'session.start'; at: number }
  | { type: 'turn.start'; at: number }
  | { type: 'turn.complete'; at: number; durationMs: number; reason: string }
  | { type: 'tool.start'; name: string; at: number }
  // `startedAt` is the matching `tool.start`'s `at`: the hook keeps it in its
  // closure, so concurrent calls of the same tool time themselves apart.
  | { type: 'tool.end'; name: string; startedAt: number; at: number; isError: boolean; resultChars: number }
  | { type: 'session.compact' }
  | { type: 'usage'; usage: SessionUsage; at: number }
  | { type: 'session.model'; name: string }
  | { type: 'binary'; binary: Binary }
  | { type: 'session.id'; id: string }
  | { type: 'verbs'; verbs: readonly QueryVerb[] }
  | { type: 'query'; verb: QueryVerb; data: unknown }
  // One poller tick ended; `ok` when every verb it called parsed.
  | { type: 'tick'; at: number; ok: boolean }
  | { type: 'stale'; stale: boolean };

export function initialModel(): Model {
  return {
    usage: null,
    usageAt: null,
    modelName: null,
    turn: {
      number: 0,
      state: 'idle',
      startedAt: null,
      lastDurationMs: null,
      lastReason: null,
      runningTool: null,
      toolMs: 0,
    },
    tools: {},
    compactions: 0,
    binary: 'unknown',
    sessionId: null,
    query: {},
    queryAt: null,
    verbs: null,
    view: 'overview',
    open: false,
    stale: false,
    placement: 'dock',
  };
}

const EMPTY_TOOL: ToolStats = { calls: 0, errors: 0, durationsMs: [], tokensToCtx: 0 };

export function reduce(model: Model, action: Action): Model {
  switch (action.type) {
    case 'session.start':
      return { ...model, turn: { ...model.turn, state: 'idle', startedAt: null, runningTool: null } };
    case 'turn.start':
      return {
        ...model,
        turn: { ...model.turn, number: model.turn.number + 1, state: 'busy', startedAt: action.at, toolMs: 0 },
      };
    case 'turn.complete':
      return {
        ...model,
        turn: {
          ...model.turn,
          state: 'idle',
          startedAt: null,
          lastDurationMs: action.durationMs,
          lastReason: action.reason,
          runningTool: null,
        },
      };
    case 'tool.start':
      return { ...model, turn: { ...model.turn, runningTool: { name: action.name, startedAt: action.at } } };
    case 'tool.end': {
      const prev = model.tools[action.name] ?? EMPTY_TOOL;
      const durationMs = Math.max(0, action.at - action.startedAt);
      const stats: ToolStats = {
        calls: prev.calls + 1,
        errors: prev.errors + (action.isError ? 1 : 0),
        durationsMs: [...prev.durationsMs, durationMs],
        tokensToCtx: prev.tokensToCtx + Math.ceil(action.resultChars / 4),
      };
      const running = model.turn.runningTool;
      const stillRunning =
        running !== null && !(running.name === action.name && running.startedAt === action.startedAt);
      return {
        ...model,
        tools: { ...model.tools, [action.name]: stats },
        turn: { ...model.turn, runningTool: stillRunning ? running : null, toolMs: model.turn.toolMs + durationMs },
      };
    }
    case 'session.compact':
      return { ...model, compactions: model.compactions + 1 };
    case 'usage':
      return { ...model, usage: action.usage, usageAt: action.at };
    case 'session.model':
      return { ...model, modelName: action.name };
    case 'binary':
      return { ...model, binary: action.binary };
    case 'session.id':
      return { ...model, sessionId: action.id };
    case 'verbs':
      return { ...model, verbs: action.verbs };
    case 'query':
      return { ...model, query: { ...model.query, [action.verb]: action.data } };
    case 'tick':
      return action.ok ? { ...model, queryAt: action.at } : model;
    case 'stale':
      return model.stale === action.stale ? model : { ...model, stale: action.stale };
  }
}

/** Whether `verb` may be polled: unknown until `--help` is read, then as listed. */
export function isSupported(model: Model, verb: QueryVerb): boolean {
  return model.verbs === null || model.verbs.includes(verb);
}

/** The verbs this binary lacks, for the sections that read `UNSUPPORTED`. */
export function unsupportedVerbs(model: Model): QueryVerb[] {
  return model.verbs === null ? [] : QUERY_VERBS.filter((verb) => !isSupported(model, verb));
}

/** The p-th percentile (0..1, nearest rank) of `values`; 0 when empty. */
export function percentile(values: readonly number[], p: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.min(sorted.length - 1, Math.max(0, Math.ceil(p * sorted.length) - 1));
  return sorted[rank];
}

/** `2h 30m`, `45m`, `30s`; `now` once the instant has passed. */
export function formatCountdown(ms: number): string {
  if (ms <= 0) return 'now';
  const s = Math.round(ms / 1000);
  const d = Math.floor(s / 86400);
  const h = Math.floor((s % 86400) / 3600);
  const m = Math.floor((s % 3600) / 60);
  if (d > 0) return `${d}d ${h}h`;
  if (h > 0) return `${h}h ${m}m`;
  if (m > 0) return `${m}m`;
  return `${s}s`;
}

export function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1000) return `${Math.round(n / 1000)}k`;
  return String(n);
}

/** One line of the Overview built from the engine's own figures. */
export type UsageRow = {
  /** The metric id as docs/metrics.md names it (`context_size`, `limit_5h`). */
  key: string;
  label: string;
  value: string;
};

const LIMIT_KEYS: Record<string, { key: string; label: string }> = {
  five_hour: { key: 'limit_5h', label: '5h' },
  seven_day: { key: 'limit_7d', label: '7d' },
};

// The engine-native rows: Context as `tokens / window (percent %)`, one row
// per rate-limit window with its countdown, and the cost with two decimals.
// A figure the engine left out is drawn as `?`, never as 0.
export function usageRows(usage: SessionUsage, now: number): UsageRow[] {
  const rows: UsageRow[] = [];
  const { tokens, window } = usage.context;
  const percent =
    usage.context.percent ?? (tokens !== undefined && window > 0 ? Math.round((tokens / window) * 100) : undefined);
  const size = tokens === undefined ? '?' : formatTokens(tokens);
  const pct = percent === undefined ? '?' : String(percent);
  rows.push({ key: 'context_size', label: 'Context', value: `${size} / ${formatTokens(window)} (${pct} %)` });
  for (const limit of usage.rateLimits) {
    const named = LIMIT_KEYS[limit.kind] ?? { key: `limit_${limit.kind}`, label: limit.kind };
    let value = `${Math.round(limit.percentUsed)} %`;
    if (limit.resetsAt !== undefined) {
      const resetsAt = Date.parse(limit.resetsAt);
      if (!Number.isNaN(resetsAt)) value += `, resets in ${formatCountdown(resetsAt - now)}`;
    }
    rows.push({ key: named.key, label: named.label, value });
  }
  if (usage.cost !== undefined) rows.push({ key: 'cost', label: 'Cost', value: `$${usage.cost.usd.toFixed(2)}` });
  return rows;
}
