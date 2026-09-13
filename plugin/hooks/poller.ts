// The query poller: pulls the deep metrics from the `cctop` binary with
// `cctop query <verb> --session <id>`, one verb after another through a single
// in-flight promise, every 2 s while a turn runs and every 10 s idle. A verb
// the binary lacks (per `cctop query --help`, read once) is never called; a
// failed or unparsable call keeps the verb's previous JSON, and the model
// turns `stale` once no tick has fully succeeded for 30 s.
//
// The poller never sees `$` itself: `claude plugin validate` follows `$` only
// into functions declared in pane.tsx, so it receives `PollerEngine`, the
// slice of `$` it uses, built there by a top-level function.
import type { EngineInterface, Timer } from 'claude-code';
import { QUERY_VERBS, isSupported, reduce, type Action, type Model, type QueryVerb } from './model';

export type PollerEngine = {
  clock: Pick<EngineInterface['clock'], 'now' | 'every'>;
  process: EngineInterface['process'];
  session: Pick<EngineInterface['session'], 'id'>;
  fs: Pick<EngineInterface['fs'], 'write'>;
  ui: Pick<EngineInterface['ui'], 'log'>;
  /** `$.env.get("HOME")`: spelled in pane.tsx, the validator reads the name off its source. */
  home: () => Promise<string | undefined>;
};

export type Poller = {
  /** Arms the timer for the current cadence and runs a first tick; a no-op while running. */
  start(): void;
  stop(): void;
  /** One round of queries; resolves at once when a round is already in flight. */
  tick(): Promise<void>;
  /** Re-arms the timer when the cadence changed with the turn state; a no-op while stopped. */
  reschedule(): void;
  running(): boolean;
};

export const BUSY_POLL_MS = 2000;
export const IDLE_POLL_MS = 10000;
export const STALE_AFTER_MS = 30000;
export const QUERY_TIMEOUT_MS = 5000;

/** How often to poll: every 2 s while a turn runs, every 10 s otherwise. */
export function pollInterval(model: Model): number {
  return model.turn.state === 'busy' ? BUSY_POLL_MS : IDLE_POLL_MS;
}

// The `Commands:` block of `cctop query --help`: one verb per line up to the
// blank line before `Options:`; only the verbs the pane knows are kept.
export function parseQueryVerbs(help: string): QueryVerb[] {
  const lines = help.split('\n');
  const start = lines.findIndex((line) => line.trim() === 'Commands:');
  if (start < 0) return [];
  const found = new Set<string>();
  for (const line of lines.slice(start + 1)) {
    if (line.trim() === '') break;
    const m = /^\s+([a-z][\w-]*)/.exec(line);
    if (m) found.add(m[1]);
  }
  return QUERY_VERBS.filter((verb) => found.has(verb));
}

// The marker other cctop processes read to learn that a pane is open for this
// session (`cctop split` refuses a second one, US-009): written on open, on
// every tick, and with `open: false` on ui.close. `$.fs` cannot delete, so a
// reader treats `open: false` or a `heartbeatAt` older than 30 s as absent.
export async function writeMarker($: PollerEngine, model: Model): Promise<void> {
  if (model.sessionId === null) return;
  const home = await $.home();
  if (home === undefined) return;
  const marker = {
    version: model.version,
    sessionId: model.sessionId,
    openedAt: model.openedAt === null ? null : new Date(model.openedAt).toISOString(),
    heartbeatAt: new Date($.clock.now()).toISOString(),
    open: model.open,
  };
  await $.fs.write(`${home}/.cctop/pane/${model.sessionId}.json`, JSON.stringify(marker));
}

export function createPoller($: PollerEngine, getModel: () => Model, setModel: (model: Model) => void): Poller {
  let active = false;
  let timer: Timer | null = null;
  let timerMs = 0;
  let inflight: Promise<void> | null = null;
  // When the poller started; stands in for `queryAt` until the first success.
  let startedAt = 0;
  // Verbs that failed since their last success, so each failure is logged once.
  const failing = new Set<string>();

  const dispatch = (action: Action): void => setModel(reduce(getModel(), action));

  const updateStale = (): void => {
    const model = getModel();
    dispatch({ type: 'stale', stale: $.clock.now() - (model.queryAt ?? startedAt) > STALE_AFTER_MS });
  };

  const readVerbs = async (): Promise<readonly QueryVerb[] | null> => {
    try {
      const result = await $.process.run(['cctop', 'query', '--help'], { timeoutMs: QUERY_TIMEOUT_MS });
      if (result.exitCode !== 0) throw new Error(`exit ${result.exitCode}`);
      const verbs = parseQueryVerbs(result.stdout);
      dispatch({ type: 'verbs', verbs });
      return verbs;
    } catch (err) {
      // Unknown until the next tick: every verb is tried meanwhile.
      $.ui.log(`cctop: query --help failed: ${err instanceof Error ? err.message : String(err)}`);
      return null;
    }
  };

  const query = async (verb: QueryVerb, sessionId: string): Promise<boolean> => {
    try {
      const argv = ['cctop', 'query', verb, '--session', sessionId];
      const result = await $.process.run(argv, { timeoutMs: QUERY_TIMEOUT_MS });
      if (result.exitCode !== 0) throw new Error(`exit ${result.exitCode}: ${result.stderr.trim()}`);
      dispatch({ type: 'query', verb, data: JSON.parse(result.stdout) as unknown });
      failing.delete(verb);
      return true;
    } catch (err) {
      if (!failing.has(verb)) {
        failing.add(verb);
        $.ui.log(`cctop: query ${verb} failed: ${err instanceof Error ? err.message : String(err)}`);
      }
      return false;
    }
  };

  const round = async (): Promise<void> => {
    if (getModel().binary !== 'present') return;
    let sessionId = getModel().sessionId;
    if (sessionId === null) {
      sessionId = await $.session.id();
      dispatch({ type: 'session.id', id: sessionId });
    }
    if (getModel().verbs === null) await readVerbs();
    let ok = true;
    let called = 0;
    for (const verb of QUERY_VERBS) {
      if (!isSupported(getModel(), verb)) continue;
      called += 1;
      if (!(await query(verb, sessionId))) ok = false;
    }
    dispatch({ type: 'tick', at: $.clock.now(), ok: ok && called > 0 });
    updateStale();
    await writeMarker($, getModel());
  };

  const poller: Poller = {
    start() {
      if (active) return;
      active = true;
      startedAt = $.clock.now();
      poller.reschedule();
      void poller.tick();
    },
    stop() {
      active = false;
      timer?.cancel();
      timer = null;
    },
    tick() {
      if (inflight !== null) return Promise.resolve();
      inflight = round()
        .catch((err: unknown) => $.ui.log(`cctop: poll failed: ${err instanceof Error ? err.message : String(err)}`))
        .finally(() => {
          inflight = null;
        });
      return inflight;
    },
    reschedule() {
      if (!active) return;
      const ms = pollInterval(getModel());
      if (timer !== null && timerMs === ms) return;
      timer?.cancel();
      timerMs = ms;
      // Stale is judged before the tick so a hung query still flips it.
      timer = $.clock.every(ms, () => {
        updateStale();
        void poller.tick();
      });
    },
    running: () => active,
  };
  return poller;
}
