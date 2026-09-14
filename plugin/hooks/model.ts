// The pane's state and the pure reducer that advances it. Nothing in here
// touches `$`: the hooks in pane.tsx turn engine events into actions, the
// views draw the model. Kept pure so it is testable from plain data.
import type { RenderElement, SessionUsage } from 'claude-code';

export type TurnState = 'idle' | 'busy' | 'waiting';
export type View = 'coach' | 'overview' | 'tools' | 'agents' | 'files' | 'events' | 'advisor';
export type Placement = 'dock' | 'inline';
export type Binary = 'unknown' | 'present' | 'missing';
/** Whether the engine is drawing the pane: `hidden` once an open pane got
 * no `ui.render` for a while (the /diff panel holds the dock), `unknown`
 * while closed or before the first render after an open. */
export type Visibility = 'unknown' | 'visible' | 'hidden';

/** The Claude Code version this hooks module's `$` contract was checked
 * against (`plugin/.claude/types/claude-code.d.ts`'s own first line);
 * `scripts/check-plugin-types.sh` catches drift, the header badge shows it. */
export const TESTED_WITH = '2.1.270';

/** The narrowest terminal (whole screen, columns) at which the fullscreen
 * renderer docks a pane beside the transcript; below it the pane is drawn
 * inline above the prompt (docs/claude-code-panels.md §4). */
export const MIN_DOCK_COLUMNS = 110;

/** The `cctop query` verbs the pane polls, in the order one tick runs them. */
export const QUERY_VERBS = ['summary', 'coach', 'tools', 'files', 'agents', 'advice', 'events'] as const;
/** Verbs a busy tick (every 2 s) skips: they change at turn boundaries, the idle tick reads them. */
export const IDLE_ONLY_VERBS: readonly QueryVerb[] = ['advice'];
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
  /** `$.session.id()`, read after session.start and again on every turn and
   * poller tick: `/clear` rotates it in place (a new transcript, the registry
   * entry rewritten) and no session.start says so. */
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
  /** When the pane was last opened (the marker's `openedAt`); null before the first open. */
  openedAt: number | null;
  /** True when the last successful `cctop query` tick is older than 30 s. */
  stale: boolean;
  placement: Placement;
  /** The `version` of plugin.json, read once after session.start; null until then. */
  version: string | null;
  /** The last tree `ui.render` built without throwing: drawn again beneath a render error. */
  lastTree: RenderElement | null;
  /** When session.start ran (the marker's `loadedAt`); null before it. */
  loadedAt: number | null;
  visibility: Visibility;
  /** When the engine last asked for the pane's tree; null before the first render. */
  renderedAt: number | null;
  /** `e.props.bodyColumns` of the last render: cells across the body. */
  bodyColumns: number | null;
  /** `e.viewport.columns` of the last render: the whole screen's width; null where unmeasured. */
  viewportColumns: number | null;
  /** The context size at the end of each turn (the last HISTORY_TURNS), for the Context sparkline. */
  contextHistory: number[];
  /** The coach view: which light's detail frame is open (null = the highest light) and whether the why frame is. */
  coachLight: 'context' | 'cache' | 'limits' | 'rework' | null;
  coachWhy: boolean;
  /** `id:fired_at_ms` of the nudge last toasted, and the turn it was toasted in (one toast per turn). */
  coachToasted: string | null;
  coachToastTurn: number | null;
  /** The last `$.ui.status` line the coach set; set again only when it changes. */
  coachStatus: string | null;
};

/** How many turn-end context sizes the model keeps for the sparkline. */
export const HISTORY_TURNS = 24;

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
  | { type: 'stale'; stale: boolean }
  // The engine asked for the pane's tree: it is being drawn, here and this wide.
  | { type: 'render'; at: number; placement: Placement; bodyColumns: number; viewportColumns: number | null }
  | { type: 'visibility'; visibility: Visibility }
  | { type: 'coach.light'; light: 'context' | 'cache' | 'limits' | 'rework' | null }
  | { type: 'coach.why'; why: boolean }
  | { type: 'coach.toasted'; key: string; turn: number }
  | { type: 'coach.status'; status: string | null };

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
    openedAt: null,
    stale: false,
    placement: 'dock',
    version: null,
    lastTree: null,
    loadedAt: null,
    visibility: 'unknown',
    renderedAt: null,
    bodyColumns: null,
    viewportColumns: null,
    contextHistory: [],
    coachLight: null,
    coachWhy: false,
    coachToasted: null,
    coachToastTurn: null,
    coachStatus: null,
  };
}

const EMPTY_TOOL: ToolStats = { calls: 0, errors: 0, durationsMs: [], tokensToCtx: 0 };

export function reduce(model: Model, action: Action): Model {
  switch (action.type) {
    case 'session.start':
      return {
        ...model,
        loadedAt: action.at,
        turn: { ...model.turn, state: 'idle', startedAt: null, runningTool: null },
      };
    case 'turn.start':
      return {
        ...model,
        turn: { ...model.turn, number: model.turn.number + 1, state: 'busy', startedAt: action.at, toolMs: 0 },
      };
    case 'turn.complete': {
      // The turn's closing context size, from the engine's last usage read.
      const size = model.usage?.context.tokens;
      const contextHistory = size === undefined ? model.contextHistory : [...model.contextHistory, size].slice(-HISTORY_TURNS);
      return {
        ...model,
        contextHistory,
        turn: {
          ...model.turn,
          state: 'idle',
          startedAt: null,
          lastDurationMs: action.durationMs,
          lastReason: action.reason,
          runningTool: null,
        },
      };
    }
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
      if (model.sessionId === action.id) return model;
      if (model.sessionId === null) return { ...model, sessionId: action.id };
      // A different id for a known session: `/clear` rotated it, and every
      // figure the pane held describes a session that is over. The engine's
      // own bookkeeping and the query JSON start again (a turn running now
      // is the new session's first); the pane's own state, the binary and
      // its verbs are the process's and stay.
      return {
        ...model,
        sessionId: action.id,
        usage: null,
        usageAt: null,
        turn: { ...model.turn, number: model.turn.state === 'idle' ? 0 : 1, lastDurationMs: null, lastReason: null },
        tools: {},
        compactions: 0,
        query: {},
        queryAt: null,
        stale: false,
        contextHistory: [],
        coachToasted: null,
        coachToastTurn: null,
        coachStatus: null,
      };
    case 'verbs':
      return { ...model, verbs: action.verbs };
    case 'query':
      return { ...model, query: { ...model.query, [action.verb]: action.data } };
    case 'tick':
      return action.ok ? { ...model, queryAt: action.at } : model;
    case 'stale':
      return model.stale === action.stale ? model : { ...model, stale: action.stale };
    case 'render':
      return {
        ...model,
        renderedAt: action.at,
        placement: action.placement,
        bodyColumns: action.bodyColumns,
        viewportColumns: action.viewportColumns,
        visibility: 'visible',
      };
    case 'visibility':
      return model.visibility === action.visibility ? model : { ...model, visibility: action.visibility };
    case 'coach.light':
      return { ...model, coachLight: action.light, coachWhy: false };
    case 'coach.why':
      return { ...model, coachWhy: action.why };
    case 'coach.toasted':
      return { ...model, coachToasted: action.key, coachToastTurn: action.turn };
    case 'coach.status':
      return model.coachStatus === action.status ? model : { ...model, coachStatus: action.status };
  }
}

/** The status line pinned under the prompt while an open pane is not drawn. */
export const HIDDEN_STATUS = 'cctop pane hidden behind the /diff panel: run /diff to show it';

// What the person is told after an open (the command's reply, the skill's
// one-liner): where the engine put the pane and, when it is not beside the
// transcript, the one thing that gets it there. The engine tells the module
// about the renderer and the width through the first render's props; it
// says nothing when the /diff panel holds the dock, so that case is read
// off a missing render (docs/claude-code-panels.md §5.4).
export function outcomeText(model: Model, viewLabel?: string): string {
  const subject = viewLabel === undefined ? 'cctop pane' : `cctop pane on ${viewLabel}`;
  if (model.visibility !== 'visible') {
    return `${subject} is open but not shown: the /diff panel holds the side dock. Run /diff to hide it and cctop takes its place (needs /tui fullscreen and 110+ columns).`;
  }
  if (model.placement === 'dock') {
    const width = model.bodyColumns === null ? '' : ` (${model.bodyColumns} columns)`;
    return `${subject} docked beside the transcript${width}: click a view in its bar to switch (or ctrl+x tab, then tab and enter), ctrl+x x closes it.`;
  }
  const columns = model.viewportColumns;
  if (columns !== null && columns < MIN_DOCK_COLUMNS) {
    return `${subject} drawn above the prompt: the terminal is ${columns} columns wide, ${MIN_DOCK_COLUMNS} or more dock it beside the transcript.`;
  }
  return `${subject} drawn above the prompt: /tui fullscreen docks it beside the transcript.`;
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
