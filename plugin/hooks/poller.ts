// The query poller: pulls the deep metrics from the `cctop` binary with
// `cctop query <verb> --session <id>`, one verb after another through a single
// in-flight promise, every 2 s while a turn runs and every 10 s idle. A verb
// the binary lacks (per `cctop query --help`, read once) is never called; a
// failed or unparsable call keeps the verb's previous JSON, and the model
// turns `stale` once no tick has fully succeeded for 30 s. It also follows
// the session id (`sync`): `/clear` rotates it under the running pane.
//
// The poller never sees `$` itself: `claude plugin validate` follows `$` only
// into functions declared in pane.tsx, so it receives `PollerEngine`, the
// slice of `$` it uses, built there by a top-level function.
import type { EngineInterface, Timer } from 'claude-code';
import { IDLE_ONLY_VERBS, QUERY_VERBS, TESTED_WITH, isSupported, reduce, type Action, type Model, type QueryVerb } from './model';

export type PollerEngine = {
  /** `now` resolves a Promise: a host round trip since Claude Code 2.1.271 (issue #3). */
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
  /**
   * Reads `$.session.id()` again and follows it: learns the id the first
   * time, and when a known id changed (`/clear` rotates it in place, with no
   * session.start) drops the old session from the model, closes its marker
   * and writes the new one. Resolves to whether a known id changed. Runs
   * whether or not the binary is present or the poller started.
   */
  sync(): Promise<boolean>;
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

// The marker other cctop processes read to learn what the pane is doing in
// this session: `cctop split` refuses a second dashboard while it is open
// (US-009) and `cctop pane status` reports it. Written once the module knows
// its session id (so `loaded: true` alone proves the hooks module runs in
// this session), on open, on every tick, and with `open: false` on ui.close.
// `$.fs` cannot delete, so a reader treats `open: false` or a `heartbeatAt`
// older than 30 s as "not open".
export async function writeMarker($: PollerEngine, model: Model): Promise<void> {
  if (model.sessionId === null) return;
  const home = await $.home();
  if (home === undefined) return;
  const now = await $.clock.now();
  const marker = {
    version: model.version,
    // The contract the module was built against and what its session.start
    // self-check found, so `cctop pane status` pairs the running module with
    // `claude --version` and relays a cause when a surface moved (issue #4).
    testedWith: TESTED_WITH,
    selfCheck: model.selfCheck,
    sessionId: model.sessionId,
    loaded: true,
    loadedAt: model.loadedAt === null ? null : new Date(model.loadedAt).toISOString(),
    openedAt: model.openedAt === null ? null : new Date(model.openedAt).toISOString(),
    heartbeatAt: new Date(now).toISOString(),
    open: model.open,
    // Where the engine drew it last and whether it still does; `unknown`
    // before the first render after an open (docs/claude-code-panels.md §5.4).
    visibility: model.visibility,
    placement: model.placement,
    bodyColumns: model.bodyColumns,
    viewportColumns: model.viewportColumns,
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

  const updateStale = async (): Promise<void> => {
    const now = await $.clock.now();
    dispatch({ type: 'stale', stale: now - (getModel().queryAt ?? startedAt) > STALE_AFTER_MS });
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
      // `--surface pane`: a fire the binary promotes for this pane (no
      // dashboard running) is recorded as shown here.
      const argv = ['cctop', 'query', verb, '--session', sessionId, '--surface', 'pane'];
      const result = await $.process.run(argv, { timeoutMs: QUERY_TIMEOUT_MS });
      // The session rotated while the binary ran: whatever it answered is
      // the old session's, not the new one's.
      if (getModel().sessionId !== sessionId) return false;
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

  // After `/clear` the engine serves a new session id for the same process
  // and, the d.ts says, no session.start; the registry entry is rewritten
  // and every `cctop query --session <old id>` exits 2 with "no session
  // matches". So the id is read again here, at every round and turn. The
  // model is switched before the markers are written, so a failing write
  // costs one round, never the switch.
  const sync = async (): Promise<boolean> => {
    const id = await $.session.id();
    const before = getModel();
    if (before.sessionId === id) return false;
    const rotated = before.sessionId !== null;
    if (rotated) {
      // Each verb's next failure is the new session's own, logged once
      // again; stale is judged from here, the new session having had no
      // success yet.
      failing.clear();
      startedAt = await $.clock.now();
      $.ui.log(`cctop: session ${before.sessionId} rotated to ${id}: following it`);
    }
    dispatch({ type: 'session.id', id });
    // The old id's marker says the pane is not open there (`$.fs` cannot
    // delete): `cctop split` would otherwise refuse for 30 s more, and the
    // new id's marker says the module runs in this session.
    if (rotated) await writeMarker($, { ...before, open: false });
    await writeMarker($, getModel());
    return rotated;
  };

  const round = async (): Promise<void> => {
    await sync();
    const sessionId = getModel().sessionId;
    if (sessionId === null || getModel().binary !== 'present') return;
    if (getModel().verbs === null) await readVerbs();
    let ok = true;
    let called = 0;
    const busy = getModel().turn.state === 'busy';
    for (const verb of QUERY_VERBS) {
      if (!isSupported(getModel(), verb)) continue;
      // The busy tick is the coach's: the slow verbs wait for the idle one.
      if (busy && IDLE_ONLY_VERBS.includes(verb)) continue;
      called += 1;
      if (!(await query(verb, sessionId))) ok = false;
    }
    dispatch({ type: 'tick', at: await $.clock.now(), ok: ok && called > 0 });
    await updateStale();
    await writeMarker($, getModel());
  };

  const poller: Poller = {
    start() {
      if (active) return;
      active = true;
      poller.reschedule();
      // The first round follows the start time, which `stale` is judged
      // against until the first success; a stop meanwhile ends it there.
      void $.clock
        .now()
        .then((now) => {
          startedAt = now;
          return active ? poller.tick() : undefined;
        })
        .catch((err: unknown) => $.ui.log(`cctop: poll failed: ${err instanceof Error ? err.message : String(err)}`));
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
      // Stale is judged beside the tick so a hung query still flips it.
      timer = $.clock.every(ms, () => {
        void updateStale().catch((err: unknown) => $.ui.log(`cctop: clock failed: ${err instanceof Error ? err.message : String(err)}`));
        void poller.tick();
      });
    },
    running: () => active,
    sync,
  };
  return poller;
}
