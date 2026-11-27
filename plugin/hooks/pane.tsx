// cctop pane: the dashboard drawn inside Claude Code's own TUI through the
// function-hooks API (early access). It registers the /cctop-pane command,
// opens the pane, draws it, and keeps a Model of the session up to date from
// the engine's own events (model.ts) and, when the binary is installed, from
// `cctop query` through the poller (poller.ts). The views (views/*.tsx) draw
// the model; the Overview is the default.
import type { ElementTable, EngineInterface, Register, RenderElement, RenderInput, SessionUsage, Timer, ToolCallResult } from 'claude-code';
import {
  breakdownDue,
  HIDDEN_STATUS,
  initialModel,
  outcomeText,
  overviewShown,
  reduce,
  unsupportedVerbs,
  UNSUPPORTED,
  TESTED_WITH,
  type Action,
  type Binary,
  type Model,
  type SessionFacts,
  type View,
} from './model';
import { spoolText } from './breakdown';
import { chartsDecision, drawPicture, pictureHash, probeOf, startCharts, type ChartRenderer, type ChartsEnv, type ChartsMode, type Picture } from './charts';
import { createPoller, writeMarker, type Poller, type PollerEngine } from './poller';
import { initialNotifyState, nudgeActions, noteFailure, noteResult, resetActions, resetCancels, settledRules, type NotifyState } from './notify';
import { coachOf, copiedText, DRAFT_KEPT, openerFor, settingsPath, settingsScope, statusLine, USER_SETTINGS, type CoachActions, type LightId, type SettingsKind } from './views/coach';
import { missPlacements, nudgePlacement, placedTree, type PlacedRow } from './views/placed';
import { ACT_ID, ACT_ROWS, renderAct, type ActActions } from './views/act';
import { renderView } from './views/index';
import { badgesLine, header, type OverviewActions } from './views/overview';
import { maxTurnsFor, overviewOf, pictureJobs, type Pictures } from './views/overview-charts';

// Re-exported so the checked-in `$` contract's own version (US-009) has one
// source (`model.ts`, already imported by both this file and the views) and
// this file, which the header badge names, still carries the export.
export { TESTED_WITH };

const PANE_ID = 'cctop';
// The native command is /cctop-pane, not /cctop: the engine reserves /cctop
// for this plugin's own skill (listed as /cctop:cctop) and refuses a
// $.command.register of that name (PRD open question 1, resolved 2026-09-12).
// The skill stays the fallback path for builds without function hooks.
const COMMAND = 'cctop-pane';
const USAGE_POLL_MS = 1000;
const VERSION_TIMEOUT_MS = 3000;
// The least gap between two `$.ui.invalidate("ui.render")` calls: at most
// four a second, the engine folds further (ten a second).
const RENDER_MIN_MS = 250;
// How long an open waits for the engine's first `ui.render` before the pane
// is reported as not shown: the dock mounts and asks for the tree within a
// few frames; nothing arrives at all while the /diff panel holds the dock.
export const OPEN_SETTLE_MS = 800;
// After an invalidate, how long without a render before an open pane counts
// as hidden (the /diff panel took the dock after the open) and the status
// line under the prompt says so.
export const HIDDEN_AFTER_MS = 2000;
// How long after a render that drew the first Image its probe blit waits:
// the tree is mounted at the surface's next frame, after the hook returned.
export const PROBE_AFTER_MS = 200;
const INSTALL_HINT = 'needs the cctop binary: brew install tomstagl/tap/cctop';
// The views in view-bar order; the `view` is the /cctop-pane argument.
const VIEWS: { view: View; label: string }[] = [
  { view: 'coach', label: 'Coach' },
  { view: 'overview', label: 'Overview' },
  { view: 'tools', label: 'Tools' },
  { view: 'agents', label: 'Agents' },
  { view: 'files', label: 'Files' },
  { view: 'events', label: 'Events' },
  { view: 'advisor', label: 'Advisor' },
];
const USAGE = `usage: /${COMMAND} [${VIEWS.map((v) => v.view).join('|')}|close]`;
// The `$.store` key under which `{ open, view }` survives a reload: an
// interactive session.start reopens the pane on that view.
const STORE_KEY = 'pane';
const MANIFEST = '.claude-plugin/plugin.json';

// Module state: one Model per loaded module (a hot reload starts a fresh
// environment, and `register` resets it). The helpers that take `$` are
// top-level functions: the validator refuses `$` handed to a closure.
let model: Model = initialModel();
// `$.session.usage()` is read from this timer alone while a turn runs, and
// once after session.start and turn.complete: never inside a hook's own
// path before `next(e)`, so the pane costs the turn nothing.
let usageTimer: Timer | null = null;
// Whether this session's failed breakdown read or spool write is logged
// already: once per session, and never retried in a loop (the next turn
// simply tries again).
let breakdownFailureLogged = false;
// charts.wasm, compiled and instantiated once per session.start (null until
// then, and for good when it would not load: the query's cells are drawn).
let chartRenderer: ChartRenderer | null = null;
// The overview's pictures (US-007), rendered off the render hook: by key, the
// picture and the hash of what it was drawn from, so only a changed chart is
// drawn again. `mounted` is what the last render drew under each key (a
// blit needs the same kind and size mounted), `chartsTimer` the pending
// refresh, `chartsBusy`/`chartsAgain` one refresh at a time with one more
// after it, `probeTimer` the probe blit waiting for its Image to mount.
let pictures = new Map<string, { hash: string; picture: Picture }>();
let mounted = new Map<string, Picture>();
let chartsTimer: Timer | null = null;
let chartsBusy = false;
let chartsAgain = false;
let probeTimer: Timer | null = null;
// The overview's object is read again with the next refresh: its charts came
// into view, or the turns that fit the width changed.
let chartsQuery = false;
// Probe blits sent this session that came back saying nothing of the terminal.
let probeTries = 0;
const PROBE_TRIES = 5;
// The query poller, built at session.start and run while the pane is open
// and the binary is present.
let poller: Poller | null = null;
// The redraw throttle: when the last invalidate was, the timer holding the
// trailing one back while changes come faster than RENDER_MIN_MS, and
// whether a request is reading the clock right now (changes that land
// meanwhile fold into it).
let invalidatedAt: number | null = null;
let renderTimer: Timer | null = null;
let renderPending = false;
// Visibility watch: the timer that calls the pane hidden when an invalidate
// draws no render, and the resolvers of opens waiting for their first render.
let hiddenTimer: Timer | null = null;
let renderWaiters: (() => void)[] = [];
// Whether HIDDEN_STATUS is pinned under the prompt right now.
let statusPinned = false;
// Opt-in native notifications (CCTOP_PANE_NOTIFY=1, read at session.start)
// and what has been sent so far.
let notifyOn = false;
let notifyState: NotifyState = initialNotifyState();
// One pane-side timer per booked rate-limit reset (key `reset-5h` / `reset-7d`):
// a terminal ignores `at` and raises a notification at once, so the wait is
// ours and the notification is sent only when the reset arrives.
const resetTimers = new Map<string, Timer>();
// Rows placed in the transcript (`$.ui.place`), by id: what each draws from
// when the engine asks. Each id is placed once. `standing` holds what the
// first coach and summary answers after each pane open already had (the
// nudge in the slot, the located misses of a resumed session or of the time
// the pane was closed): history is not replayed into the transcript. The
// flags say which answers have been taken since the last open.
const placedRows = new Map<string, PlacedRow>();
const standing = new Set<string>();
let standingTaken = { coach: false, summary: false };

// Asks for a redraw as of `now` (the clock's answer, or the trailing timer's
// due time) and arms the hidden watch.
function invalidateAt($: EngineInterface, now: number): void {
  invalidatedAt = now;
  $.ui.invalidate('ui.render');
  watchForRender($);
}

// An unthrottled redraw (an open, a view switch). `$.clock.now()` is a host
// round trip since Claude Code 2.1.271 (issue #3), so this resolves once the
// request is made.
function invalidateNow($: EngineInterface): Promise<void> {
  return clockNow($).then((now) => invalidateAt($, now));
}

// Arms (or re-arms) the hidden watch: if the engine asks for no tree within
// HIDDEN_AFTER_MS of this invalidate, the open pane is not being drawn.
function watchForRender($: EngineInterface): void {
  if (!model.open) return;
  hiddenTimer?.cancel();
  hiddenTimer = $.clock.after(HIDDEN_AFTER_MS, () => {
    hiddenTimer = null;
    markHidden($);
  });
}

function stopHiddenTimer(): void {
  hiddenTimer?.cancel();
  hiddenTimer = null;
}

// The pane is open but the engine draws it nowhere: pin the status line
// (once per transition) and tell the marker.
function markHidden($: EngineInterface): void {
  if (!model.open || model.visibility === 'hidden') return;
  model = reduce(model, { type: 'visibility', visibility: 'hidden' });
  $.ui.status(HIDDEN_STATUS);
  statusPinned = true;
  updateMarker($);
}

function unpinStatus($: EngineInterface): void {
  if (!statusPinned) return;
  $.ui.status(undefined);
  statusPinned = false;
  model = reduce(model, { type: 'coach.status', status: null });
  coachChanged($);
}

// A render arrived: the pane is drawn, here and this wide. Settles every
// open waiting on it and lifts the hidden status if it was pinned.
function noteRender($: EngineInterface, e: RenderInput<'Pane'>, now: number): void {
  const wasHidden = model.visibility === 'hidden';
  const before = model;
  model = reduce(model, {
    type: 'render',
    at: now,
    placement: e.props.placement,
    bodyColumns: e.props.bodyColumns,
    viewportColumns: e.viewport?.columns ?? null,
  });
  // The view in force (a switch from the tasks list shows here too).
  model = reduce(model, { type: 'agents.viewed', id: e.props.view?.agentId ?? null });
  // A new width is a full redraw of the overview: its pictures at the new
  // sizes, and its object again when the turns that fit changed.
  if (overviewShown(model) && model.bodyColumns !== before.bodyColumns) {
    // The read, like the pictures, a timer away: nothing is spawned from the render hook.
    if (before.bodyColumns === null || maxTurnsFor(before.bodyColumns) !== maxTurnsFor(e.props.bodyColumns)) chartsQuery = true;
    scheduleCharts($);
  }
  stopHiddenTimer();
  unpinStatus($);
  // The width may have changed the status line's form.
  coachChanged($);
  const waiters = renderWaiters;
  renderWaiters = [];
  for (const resolve of waiters) resolve();
  if (wasHidden) updateMarker($);
}

// Resolves on the next render of the pane, or after OPEN_SETTLE_MS without
// one (then the pane is marked hidden), so an open can answer with where the
// pane actually went.
function awaitRender($: EngineInterface): Promise<void> {
  return new Promise<void>((resolve) => {
    let settled = false;
    const done = (): void => {
      if (settled) return;
      settled = true;
      timer.cancel();
      resolve();
    };
    const timer = $.clock.after(OPEN_SETTLE_MS, () => {
      markHidden($);
      done();
    });
    renderWaiters.push(done);
  });
}

// Asks for a redraw at most every RENDER_MIN_MS: a change inside the gap
// arms one trailing call for the end of it, later changes fold into that.
// Runs under every model change, and the gap is measured on the host's
// clock, a round trip: `renderPending` is set before the first await, so the
// changes that land while one request reads the clock fold into it instead
// of each arming a timer. The trailing timer fires at the gap's end by
// construction, so it needs no second reading.
async function requestRender($: EngineInterface): Promise<void> {
  if (renderTimer !== null || renderPending) return;
  renderPending = true;
  try {
    const now = await clockNow($);
    const waited = invalidatedAt === null ? RENDER_MIN_MS : now - invalidatedAt;
    if (waited >= RENDER_MIN_MS) {
      invalidateAt($, now);
      return;
    }
    const due = now + RENDER_MIN_MS - waited;
    renderTimer = $.clock.after(RENDER_MIN_MS - waited, () => {
      renderTimer = null;
      invalidateAt($, due);
    });
  } finally {
    renderPending = false;
  }
}

function stopRenderTimer(): void {
  renderTimer?.cancel();
  renderTimer = null;
}

function replaceModel($: EngineInterface, next: Model): void {
  const before = model;
  model = next;
  if (model.open || model.actOpen) requestRender($).catch((err: unknown) => $.ui.log(`cctop: redraw failed: ${String(err)}`));
  if (model.query.coach !== before.query.coach) coachChanged($);
  if (model.query.coach !== before.query.coach || model.query.summary !== before.query.summary) placeNotes($, model.query.coach !== before.query.coach, model.query.summary !== before.query.summary);
  if (model.queryAt !== before.queryAt) checkFilled($);
  overviewChanged($, before);
  if (model.sessionId !== before.sessionId) {
    agentsAsked.clear();
    placedRows.clear();
    standing.clear();
    standingTaken = { coach: false, summary: false };
    // The act dialog acts on the old session's nudges: it closes.
    if (model.actOpen) closeAct($).catch((err: unknown) => $.ui.log(`cctop: act dialog close failed: ${String(err)}`));
  }
  if (model.query.agents !== before.query.agents) agentsPolled($);
}

// Ids already asked of `$.agent.list()` because a poll showed them running
// and unmapped; each is asked once (a session change forgets them).
const agentsAsked = new Set<string>();

// An agent that starts mid-turn is not in `liveAgents` (read on turn.start /
// turn.complete / the agents view opening): a running row whose id is neither
// there nor already asked for reads the list once.
function agentsPolled($: EngineInterface): void {
  const agents = (model.query.agents as { agents?: unknown } | null | undefined)?.agents;
  if (!Array.isArray(agents)) return;
  const known = new Set(model.liveAgents.map((a) => a.id));
  let ask = false;
  for (const a of agents as { id?: unknown; state?: unknown }[]) {
    if (a === null || typeof a !== 'object' || a.state !== 'running' || typeof a.id !== 'string') continue;
    if (known.has(a.id) || agentsAsked.has(a.id)) continue;
    agentsAsked.add(a.id);
    ask = true;
  }
  if (ask) void readAgents($);
}

// The coach's own surfaces beyond the pane, kept from the last `cctop
// query coach`: the status line under the prompt (L0 / L1 / L2 by the
// pane's width, set again only when it changes) and the one toast the
// coach raises — a NOW-class nudge taking the slot, once per fire and at
// most once per turn. The hidden notice keeps the status line while the
// pane is not drawn.
function coachChanged($: EngineInterface): void {
  const c = coachOf(model.query.coach);
  if (c === null) return;
  // The width decides the form, so nothing is pinned before the first render.
  if (model.open && !statusPinned && model.bodyColumns !== null) {
    const status = statusLine(c, model.bodyColumns);
    if (status !== model.coachStatus) {
      $.ui.status(status);
      model = reduce(model, { type: 'coach.status', status });
    }
  }
  notifyNudge($, c);
  const n = c.nudge;
  if (n !== null && n.cls === 'NOW') {
    const key = `${n.id}:${n.firedAt ?? 0}`;
    if (key !== model.coachToasted && model.coachToastTurn !== model.turn.number) {
      $.ui.toast(n.line1);
      model = reduce(model, { type: 'coach.toasted', key, turn: model.turn.number });
    }
  }
}

// What Console's targets do: a cell, the act line or `0 home` opens its
// body in place; the header never moves. A turn number under one of the
// overview's per-turn charts shows that turn's readout.
function overviewActions($: EngineInterface): OverviewActions {
  return {
    open: (id) => apply($, { type: 'overview.body', id }),
    keys: (keys) => apply($, { type: 'overview.keys', keys }),
    readout: (chart, turn) => apply($, { type: 'overview.readout', chart, turn }),
  };
}

// The picture-or-cells mode the overview's pictures are drawn in now.
function modeOf(m: Model): ChartsMode {
  return m.charts === null ? 'cells' : chartsDecision(m.charts).mode;
}

// What changed for the overview's charts: body 1 just came into view (its
// verb is read at once, not at the next tick), or its object or what the
// pane knows of picture-or-cells moved while it is shown (the pictures are
// drawn again, off the render hook; a new width is noteRender's).
function overviewChanged($: EngineInterface, before: Model): void {
  if (!overviewShown(model)) return;
  const opened = !overviewShown(before);
  if (opened) chartsQuery = true;
  if (opened || model.query.overview !== before.query.overview || model.charts !== before.charts) scheduleCharts($);
}

// One refresh of the overview's pictures, a timer away: never inside a hook
// (the render hook among them), and the triggers of one moment fold into it.
// When asked, the overview's verb is read first, on its own.
function scheduleCharts($: EngineInterface): void {
  if (chartsTimer !== null) return;
  chartsTimer = $.clock.after(1, () => {
    chartsTimer = null;
    const ask = chartsQuery;
    chartsQuery = false;
    if (ask) void poller?.queryNow('overview');
    void refreshCharts($).catch((err: unknown) => $.ui.log(`cctop: charts failed: ${String(err)}`));
  });
}

// Draws the overview's pictures that changed (spec §5.4): one wasm call per
// picture whose spec, size or mode moved since it was last drawn; a picture
// mounted at the same kind and size is swapped in place with `$.ui.blit`,
// anything else (a first draw, a new size, a refused blit) asks for one
// redraw. Pictures no longer drawn are dropped.
async function refreshCharts($: EngineInterface): Promise<void> {
  if (chartsBusy) {
    chartsAgain = true;
    return;
  }
  chartsBusy = true;
  try {
    const o = overviewOf(model.query.overview);
    const columns = model.bodyColumns;
    if (!overviewShown(model) || o === null || columns === null) return;
    const mode = modeOf(model);
    const jobs = pictureJobs(o, columns);
    const keys = new Set(jobs.map((j) => j.key));
    for (const key of [...pictures.keys()]) if (!keys.has(key)) pictures.delete(key);
    let redraw = false;
    const blits: Promise<void>[] = [];
    // The query's cells are drawn only until the wasm is there: its arrival is a change too.
    const source = chartRenderer === null ? 'query' : 'wasm';
    for (const job of jobs) {
      const hash = `${pictureHash(job, mode)}:${source}`;
      if (pictures.get(job.key)?.hash === hash) continue;
      const picture = drawPicture(chartRenderer, job, mode);
      if (picture === null) {
        pictures.delete(job.key);
        redraw = true;
        continue;
      }
      pictures.set(job.key, { hash, picture });
      const shown = mounted.get(job.key);
      if (shown === undefined || shown.kind !== picture.kind || shown.cols !== picture.cols || shown.rows !== picture.rows) {
        redraw = true;
        continue;
      }
      blits.push(blitPicture($, job.key, picture).then((ok) => {
        if (ok) mounted.set(job.key, picture);
        else redraw = true;
      }));
    }
    await Promise.all(blits);
    if (redraw && model.open) await requestRender($);
  } finally {
    chartsBusy = false;
    if (chartsAgain) {
      chartsAgain = false;
      scheduleCharts($);
    }
  }
}

function stopChartTimers(): void {
  chartsTimer?.cancel();
  chartsTimer = null;
  probeTimer?.cancel();
  probeTimer = null;
}

// Swaps one mounted picture: an Image's source or a Raster's cells, at the
// size it is mounted. True when the surface took it.
function blitPicture($: EngineInterface, key: string, picture: Picture): Promise<boolean> {
  const args =
    picture.kind === 'image'
      ? { requestId: PANE_ID, key, source: { png: picture.png }, columns: picture.cols, rows: picture.rows }
      : { requestId: PANE_ID, key, cells: picture.cells, columns: picture.cols, rows: picture.rows };
  return $.ui.blit(args).then(
    (answer) => answer.deny === undefined,
    (err: unknown) => {
      $.ui.log(`cctop: chart blit failed: ${String(err)}`);
      return false;
    },
  );
}

// The pictures a render may draw, each one it draws noted as mounted.
function picturesForRender(): Pictures {
  mounted = new Map();
  return {
    get: (key) => {
      const p = pictures.get(key)?.picture;
      if (p !== undefined) mounted.set(key, p);
      return p;
    },
  };
}

// After a render that drew an Image while the terminal's answer is not known:
// the probe blit (§5.3) once the Image is mounted, a moment after the tree.
function probeAfterRender($: EngineInterface): void {
  if (probeTimer !== null || probeTries >= PROBE_TRIES || model.charts === null || model.charts.probe !== 'unknown') return;
  const first = [...mounted.entries()].find(([, p]) => p.kind === 'image');
  if (first === undefined) return;
  const [key, picture] = first;
  if (picture.kind !== 'image') return;
  probeTimer = $.clock.after(PROBE_AFTER_MS, () => {
    probeTimer = null;
    if (mounted.get(key) !== picture) return;
    void probeCharts($, PANE_ID, key, picture.png);
  });
}

// What the agents view's Buttons do: a run's row opens its detail, `back` the
// list (view state); an agent's row puts its transcript in the main view.
function agentsActions($: EngineInterface): { open(run: string | null): void; jump(agentId: string): void } {
  return { open: (run) => apply($, { type: 'agents.run', run }), jump: (agentId) => viewAgent($, agentId) };
}

// `$.ui.view`: an agent's transcript in Claude Code's main view, or `{}` back
// to the main conversation. Called only from a Button's onPress (the d.ts:
// only while the call answers the person's own input). A `{ deny }` says why
// the view did not move: one toast with the reason.
function viewAgent($: EngineInterface, agentId: string | null): void {
  $.ui
    .view(agentId === null ? {} : { agentId })
    .then((shown) => {
      if (shown.deny !== undefined) {
        $.ui.toast(`cctop: ${shown.deny}`);
        return;
      }
      apply($, { type: 'agents.viewed', id: agentId });
      invalidateNow($).catch((err: unknown) => $.ui.log(`cctop: redraw failed: ${String(err)}`));
    })
    .catch((err: unknown) => $.ui.log(`cctop: ui.view failed: ${String(err)}`));
}

// `$.agent.list()`: the engine's ids the agent rows are mapped to (agentmap.ts).
// Read on turn.start, turn.complete and when the agents view opens; a read
// that lands after `/clear` is dropped.
function readAgents($: EngineInterface): Promise<void> {
  const sessionId = model.sessionId;
  return $.agent
    .list()
    .then((list) => {
      if (model.sessionId !== sessionId) return;
      apply($, {
        type: 'agents.live',
        agents: list.map((a) => ({ id: a.id, type: a.type, description: a.description })),
      });
    })
    .catch((err: unknown) => $.ui.log(`cctop: agent.list failed: ${String(err)}`));
}

// `CCTOP_PANE_NOTIFY=1` turns the notifications on; anything else leaves
// them off. Read once per session.start.
function readNotifyEnv($: EngineInterface): Promise<void> {
  return $.env
    .get("CCTOP_PANE_NOTIFY")
    .then((value) => {
      notifyOn = value === '1';
    })
    .catch((err: unknown) => $.ui.log(`cctop: env read failed: ${String(err)}`));
}

// What the terminal says of itself, one literal `$.env.get` each (the
// validator reads the names off the source). Unset and empty are absent.
async function readChartsEnv($: EngineInterface, config: string | undefined): Promise<ChartsEnv> {
  const set = (v: string | undefined): string | undefined => (typeof v === 'string' && v !== '' ? v : undefined);
  const [override, termProgram, kitty, tmux, iterm] = await Promise.all([
    $.env.get("CCTOP_PANE_CHARTS"),
    $.env.get("TERM_PROGRAM"),
    $.env.get("KITTY_WINDOW_ID"),
    $.env.get("TMUX"),
    $.env.get("ITERM_SESSION_ID"),
  ]);
  const env: ChartsEnv = { kitty: set(kitty) !== undefined, tmux: set(tmux) !== undefined, iterm: set(iterm) !== undefined };
  if (set(override) !== undefined) env.override = set(override);
  if (config !== undefined) env.config = config;
  if (set(termProgram) !== undefined) env.termProgram = set(termProgram);
  return env;
}

// `[pane] charts` from `cctop query config`; undefined when the binary is
// missing, older than the verb, or answers something else.
function readChartsConfig($: EngineInterface): Promise<string | undefined> {
  return $.process
    .run(['cctop', 'query', 'config'], { timeoutMs: VERSION_TIMEOUT_MS })
    .then((result) => {
      if (result.exitCode !== 0) return undefined;
      const charts = (JSON.parse(result.stdout) as { pane?: { charts?: unknown } }).pane?.charts;
      return typeof charts === 'string' ? charts : undefined;
    })
    .catch(() => undefined);
}

// US-006: compile charts.wasm once (a literal path, here, for the validator),
// read the terminal's environment and the config, decide picture or cells,
// and tell the marker. A wasm that will not load is logged once and leaves
// the cells the query carries. Never awaited by a hook; `detected` settles
// when detectBinary has put its answer in the model.
async function initCharts($: EngineInterface, detected: Promise<void>): Promise<void> {
  const runtime = await startCharts(
    () => $.wasm.compile('./charts.wasm'),
    (text) => $.ui.log(text),
  );
  chartRenderer = runtime.renderer;
  // The config is the binary's to say: only asked once it is known to be there.
  await detected;
  const config = model.binary === 'present' ? await readChartsConfig($) : undefined;
  const env = await readChartsEnv($, config);
  apply($, { type: 'charts.info', info: { env, probe: 'unknown', wasm: runtime.wasm } });
  updateMarker($);
}

// How the pane draws charts now: `Image` or `Raster`.
export function chartsMode(): ChartsMode {
  return modeOf(model);
}

// The wasm renderer, or null when it would not load.
export function chartsRenderer(): ChartRenderer | null {
  return chartRenderer;
}

// The probe blit (§5.3): the first time a chart is drawn as an `Image`, the
// same source is blitted once. A `deny` naming the alt means the terminal
// draws the alt, so the session is cells; `fix` (tmux's passthrough) goes to
// the marker. A deny that says nothing of the terminal (not mounted yet, no
// surface) leaves it unknown: a later render tries again, PROBE_TRIES times.
export async function probeCharts($: EngineInterface, requestId: string, key: string, png: string): Promise<void> {
  if (model.charts === null || model.charts.probe !== 'unknown') return;
  try {
    probeTries += 1;
    const answer = await $.ui.blit({ requestId, key, source: { png } });
    const probe = probeOf(answer);
    if (probe === 'unknown') return;
    apply($, { type: 'charts.probe', probe, ...(answer.fix !== undefined ? { fix: answer.fix } : {}) });
    updateMarker($);
    invalidateNow($).catch((err: unknown) => $.ui.log(`cctop: redraw failed: ${String(err)}`));
  } catch (err) {
    $.ui.log(`cctop: chart probe failed: ${String(err)}`);
  }
}

function sendNotify($: EngineInterface, text: string, key: string, options: { at?: number; untilBack?: boolean } = {}): void {
  $.ui
    .notify(text, { title: 'cctop', key, ...options })
    .then((result) => {
      const line = noteResult(notifyState, key, result);
      if (line !== null) $.ui.log(line);
    })
    .catch((err: unknown) => {
      const line = noteFailure(notifyState, key, err);
      if (line !== null) $.ui.log(line);
    });
}

// A NOW nudge notifies once per fire; one that was snoozed or acted on is
// taken back.
function notifyNudge($: EngineInterface, c: NonNullable<ReturnType<typeof coachOf>>): void {
  if (!notifyOn) return;
  // Without a fire time there is no fire identity to notify once per: none.
  const n = c.nudge !== null && c.nudge.firedAt !== null ? c.nudge : null;
  const fire = n === null ? null : String(n.firedAt);
  const todo = nudgeActions(
    notifyState,
    n === null || fire === null ? null : { ruleId: n.id, fireId: fire, urgency: n.cls === 'NOW' || n.cls === 'NEXT' ? n.cls : 'LATER', headline: n.line1 },
    settledRules(model.query.coach, notifyState, n === null || fire === null ? null : { ruleId: n.id, fireId: fire }),
  );
  for (const key of todo.cancel) $.ui.cancelNotify(key).catch((err: unknown) => $.ui.log(`cctop: cancelNotify failed: ${String(err)}`));
  for (const item of todo.notify) sendNotify($, item.text, item.key);
}

// A window at 90 % or more books its reset: a timer here for the wait to its
// resets_at, and the notification (`untilBack`, no `at`) sent when it fires.
// A new resets_at replaces the timer; a window that fell below the threshold
// cancels it (and the key, in case a host queued one).
function notifyResets($: EngineInterface, usage: SessionUsage, now: number): void {
  if (!notifyOn) return;
  const windows: { key: 'reset-5h' | 'reset-7d'; usedPct: number; resetsAtMs: number }[] = [];
  for (const l of usage.rateLimits) {
    const key = l.kind === 'five_hour' ? 'reset-5h' : l.kind === 'seven_day' ? 'reset-7d' : null;
    const resetsAtMs = l.resetsAt === undefined ? NaN : Date.parse(l.resetsAt);
    if (key !== null && Number.isFinite(resetsAtMs)) windows.push({ key, usedPct: l.percentUsed, resetsAtMs });
  }
  for (const key of resetCancels(notifyState, windows)) {
    clearResetTimer(key);
    $.ui.cancelNotify(key).catch((err: unknown) => $.ui.log(`cctop: cancelNotify failed: ${String(err)}`));
  }
  for (const item of resetActions(notifyState, windows, now)) bookReset($, item.text, item.key, Math.max(1, item.at - now));
}

function bookReset($: EngineInterface, text: string, key: string, waitMs: number): void {
  clearResetTimer(key);
  resetTimers.set(
    key,
    $.clock.after(waitMs, () => {
      resetTimers.delete(key);
      sendNotify($, text, key, { untilBack: true });
    }),
  );
}

function clearResetTimer(key: string): void {
  resetTimers.get(key)?.cancel();
  resetTimers.delete(key);
}

function clearResetTimers(): void {
  for (const t of resetTimers.values()) t.cancel();
  resetTimers.clear();
}

// The person's Stop: the running turn ends now rather than at the next poll,
// and a round runs at once (turn.complete may not follow an interrupt).
function noteStop($: EngineInterface): void {
  apply($, { type: 'turn.stop', at: Date.now() });
  stopUsageTimer();
  poller?.reschedule();
  poller?.pollNow();
}

// What the coach view's Buttons do. None of it acts on the session: `fill`
// drafts in an empty prompt box (never submits, never replaces a draft; the
// card then says nothing was sent), `copy` puts a rule or a setting on the
// clipboard and says which settings file it belongs in, `openSettings`
// opens that file in the person's editor (cctop never writes it), `snooze`
// and `gotIt` ask the binary (queued for the TUI while it runs) and take its
// answer; `agents`, `light` and `next` are view state.
function coachActions($: EngineInterface): CoachActions {
  return {
    fill: (text) => fillPrompt($, text),
    copy: (text, kind) => copySetting($, text, kind),
    openSettings: (kind) => openSettings($, kind),
    snooze: (rule) => snoozeRule($, rule, '--snooze'),
    gotIt: (rule) => snoozeRule($, rule, '--snooze-session'),
    agents: () => selectView($, 'agents'),
    light: (light: LightId | 'none') => apply($, { type: 'coach.light', light }),
    next: (id) => apply($, { type: 'coach.next', id }),
    act: (id) => openAct($, id),
  };
}

// Notes where they happened (US-010): a NOW nudge (`nudge-<fire time>`) and
// a located cache miss that came after the first summary (`miss-<turn>`) are
// placed at the transcript's end once each, and only between turns, so they
// land at the turn end. Display only: nothing is written to the transcript.
function placeNotes($: EngineInterface, coachMoved: boolean, summaryMoved: boolean): void {
  const nudge = nudgePlacement(coachOf(model.query.coach));
  const misses = missPlacements(model.query.summary);
  // What was already there when each answer first arrived (since the load
  // or the last open) is taken as history, mid-turn or not. Only an answer
  // that just arrived counts: the other verb's may be from before a close.
  if (coachMoved && model.query.coach !== undefined && !standingTaken.coach) {
    standingTaken.coach = true;
    if (nudge !== null) standing.add(nudge.id);
  }
  if (summaryMoved && model.query.summary !== undefined && !standingTaken.summary) {
    standingTaken.summary = true;
    for (const m of misses) standing.add(m.id);
  }
  if (model.turn.state !== 'busy') {
    for (const { id, row } of [...(nudge === null ? [] : [nudge]), ...misses]) {
      if (placedRows.has(id) || standing.has(id)) continue;
      placedRows.set(id, row);
      $.ui.place({ id }).catch((err: unknown) => $.ui.log(`cctop: place ${id} failed: ${String(err)}`));
    }
  }
  // A card whose nudge left the slot is redrawn as its settled line, pane open or not
  // (only a coach change can settle one; an open pane already redraws on every change).
  if (coachMoved && !model.open && [...placedRows.values()].some((r) => r.kind === 'nudge')) {
    requestRender($).catch((err: unknown) => $.ui.log(`cctop: redraw failed: ${String(err)}`));
  }
}

// What the act dialog's buttons do: the coach card's own (one implementation
// for both). Two differences: a fill, or a switch to the agents view, hands
// the keyboard back first (the dialog holds it; a fill is refused under a
// dialog), and the outcome is drawn in the dialog (`notice`), since toasts
// are held while it is open.
function actActions($: EngineInterface): ActActions {
  const card = coachActions($);
  const then = (run: () => void): void => {
    closeAct($).then(run, run);
  };
  return {
    ...card,
    fill: (text) => then(() => card.fill(text)),
    agents: () => then(() => card.agents()),
    select: (id) => apply($, { type: 'act.select', id }),
    jump: (agentId) => then(() => viewAgent($, agentId)),
  };
}

// `a` or a press on the nudge: the dialog opens (an open id is retitled)
// with the keyboard, Esc to close it, and every other plugin's toasts held.
// Called from a press, so it is placed at any width; a refusal is a toast.
function openAct($: EngineInterface, id?: string): void {
  apply($, { type: 'act.open', id: id ?? null });
  $.ui
    .open({ id: ACT_ID, title: 'cctop · act', focus: true, closeOnEscape: true, holdToasts: true, rows: ACT_ROWS })
    .then((opened) => {
      if (opened.isPlaced === false) {
        apply($, { type: 'act.close' });
        $.ui.toast(`cctop: the act dialog was not shown (${opened.reason ?? 'no room'})`);
      }
    })
    .catch((err: unknown) => {
      apply($, { type: 'act.close' });
      $.ui.log(`cctop: act dialog failed: ${String(err)}`);
    });
}

function closeAct($: EngineInterface): Promise<void> {
  if (!model.actOpen) return Promise.resolve();
  return $.ui.close({ id: ACT_ID }).then(() => {
    apply($, { type: 'act.close' });
  });
}

// A toast; while the dialog is open the words are drawn in it instead (toasts
// are held while it is open and would reappear after it closes).
function notice($: EngineInterface, text: string): void {
  if (model.actOpen) apply($, { type: 'act.note', text });
  else $.ui.toast(text);
}

// A fill never replaces what the person is typing: the box is read first,
// and only an empty one is filled. The fill appends (`$.prompt.fill`
// replaces by default), which on an empty box is the same text, and keeps a
// draft typed between the read and the fill. A draft, or a box cctop cannot
// read, keeps the draft and says so; a box already holding this very text is
// left as it is and counts as filled.
function fillPrompt($: EngineInterface, text: string): void {
  readPromptBox($)
    .then((box) => {
      if (box === text) {
        apply($, { type: 'coach.filled', text });
        return;
      }
      if (box !== '') {
        notice($, DRAFT_KEPT);
        return;
      }
      return $.prompt.fill({ text, mode: 'append' }).then(({ isFilled }) => {
        if (!isFilled) {
          notice($, 'cctop: the prompt box is busy — the action was not filled');
          return;
        }
        apply($, { type: 'coach.filled', text });
      });
    })
    .catch((err: unknown) => $.ui.log(`cctop: prompt.fill failed: ${String(err)}`));
}

// The prompt box's draft; null when it cannot be read (a host without
// `$.prompt.read`, or a read that failed), which a fill treats as a draft.
function readPromptBox($: EngineInterface): Promise<string | null> {
  // Called inside the chain: a host without the noun throws there, not here.
  return Promise.resolve()
    .then(() => $.prompt.read())
    .then(
      (box) => (typeof box?.text === 'string' ? box.text : null),
      () => null,
    );
}

// While the card says a fill is in the box, each poll looks at the box once:
// emptied (sent, cleared, deleted), the line goes before the next turn does.
function checkFilled($: EngineInterface): void {
  if (model.coachFilled === null) return;
  const filled = model.coachFilled;
  readPromptBox($)
    .then((box) => {
      if (box === '' && model.coachFilled === filled) apply($, { type: 'coach.filled', text: null });
    })
    .catch((err: unknown) => $.ui.log(`cctop: prompt.read failed: ${String(err)}`));
}

// The settings file an action concerns, read when pressed (`/cd` moves the
// root): the project's `.claude/settings.local.json` for an allow rule, the
// person's `~/.claude/settings.json` for a setting; null when HOME is unset
// (no shell expands `~`, so there is nothing to hand `open`).
async function settingsFile($: EngineInterface, kind: SettingsKind): Promise<string | null> {
  if (settingsScope(kind) === 'project') return settingsPath('project', await $.session.root(), '');
  const home = await $.env.get('HOME');
  return typeof home === 'string' && home !== '' ? settingsPath('user', '', home) : null;
}

function copySetting($: EngineInterface, text: string, kind: SettingsKind): void {
  Promise.all([$.ui.copy({ text }), settingsFile($, kind)])
    .then(([copied, path]) => {
      notice($, copied.isCopied ? copiedText(text, kind, path ?? USER_SETTINGS) : `cctop: nothing was copied (${copied.reason})`);
    })
    .catch((err: unknown) => $.ui.log(`cctop: copy failed: ${String(err)}`));
}

// `open` on macOS, `xdg-open` on Linux: the person's editor shows the file,
// and what they change there is theirs. A file that is not there yet is not
// created: the toast says so.
function openSettings($: EngineInterface, kind: SettingsKind): void {
  settingsFile($, kind)
    .then(async (path) => {
      if (path === null) {
        notice($, `cctop: HOME is not set: open ${USER_SETTINGS} in your editor`);
        return;
      }
      const opener = openerFor(model.sessionFacts?.platform);
      if (opener === null) {
        notice($, `cctop: open ${path} in your editor`);
        return;
      }
      const result = await $.process.run([opener, path], { timeoutMs: 5000 });
      notice(
        $,
        result.exitCode === 0
          ? `cctop: opened ${path} in your editor. cctop never edits it`
          : `cctop: could not open ${path} (is it there?): open it in your editor`,
      );
    })
    .catch((err: unknown) => $.ui.log(`cctop: opening settings.json failed: ${String(err)}`));
}

function snoozeRule($: EngineInterface, rule: string, flag: '--snooze' | '--snooze-session'): void {
  const id = model.sessionId;
  if (id === null) return;
  $.process
    .run(['cctop', 'query', 'coach', flag, rule, '--session', id, '--surface', 'pane'], { timeoutMs: 5000 })
    .then((result) => {
      if (result.exitCode !== 0) throw new Error(`exit ${result.exitCode}`);
      // The session rotated (`/clear`) while the binary ran: the answer is
      // the old session's coach, not the new one's.
      if (model.sessionId !== id) return;
      const answer = JSON.parse(result.stdout) as { snooze?: unknown };
      if (typeof answer.snooze === 'string') notice($, `cctop: ${answer.snooze}`);
      apply($, { type: 'query', verb: 'coach', data: answer });
    })
    .catch((err: unknown) => $.ui.log(`cctop: snooze failed: ${String(err)}`));
}

function apply($: EngineInterface, action: Action): void {
  replaceModel($, reduce(model, action));
}

// Every clock reading of the module: the host's when it resolves a number,
// else the environment's own `Date.now()`. A contract change in the clock
// (2.1.271 made it resolve a Promise; issue #3) then costs the session its
// engine timestamps, not every hook: the self-check names it once.
function clockNow($: EngineInterface): Promise<number> {
  return $.clock.now().then(
    (at) => (typeof at === 'number' && Number.isFinite(at) ? at : Date.now()),
    () => Date.now(),
  );
}

// What session.start found of the `$` surfaces the module cannot do without:
// the clock resolves a number, HOME is set (the marker's path), the session
// has an id. Never throws; `now` is the clock's answer or the fallback.
type SelfCheck = { now: number; problems: string[] };

const describe = (value: unknown): string => (value === null ? 'null' : typeof value);
const message = (err: unknown): string => (err instanceof Error ? err.message : String(err));

async function selfCheck($: EngineInterface): Promise<SelfCheck> {
  const problems: string[] = [];
  let now = Date.now();
  try {
    const at = await $.clock.now();
    if (typeof at === 'number' && Number.isFinite(at)) now = at;
    else problems.push(`$.clock.now() resolved ${describe(at)}, not a number`);
  } catch (err) {
    problems.push(`$.clock.now() failed: ${message(err)}`);
  }
  try {
    const home = await $.env.get('HOME');
    if (typeof home !== 'string' || home === '') problems.push('$.env.get("HOME") is unset: no marker can be written');
  } catch (err) {
    problems.push(`$.env.get("HOME") failed: ${message(err)}`);
  }
  try {
    const id = await $.session.id();
    if (typeof id !== 'string' || id === '') problems.push(`$.session.id() answered ${describe(id)}, not a string`);
  } catch (err) {
    problems.push(`$.session.id() failed: ${message(err)}`);
  }
  return { now, problems };
}

// The one line the debug log gets at session.start: the module, the
// contract it was built against and what the self-check found, so a
// contract change reads as a cause (`cctop pane status` relays the marker's
// `selfCheck`), not as a failure per hook.
export function selfCheckLine(version: string | null, check: SelfCheck): string {
  const who = `cctop: plugin ${version ?? '?'} (hooks contract ${TESTED_WITH}) loaded`;
  if (check.problems.length === 0) return `${who}; self-check ok`;
  return (
    `${who}; self-check failed: ${check.problems.join('; ')} — ` +
    `the Claude Code running this is newer than ${TESTED_WITH} and changed the contract, or the environment is unusual; ` +
    'update the plugin (claude plugin marketplace update cctop && claude plugin update cctop@cctop) or report it with `claude --version`'
  );
}

// After session.start's `next(e)`: the manifest first, so the line and the
// marker name the version (a `-p` run used to write `version: null` — the
// read lost the race), then the line, then the marker through noteSession.
function announce($: EngineInterface, check: SelfCheck): Promise<void> {
  return readVersion($).then(() => {
    $.ui.log(selfCheckLine(model.version, check));
    return noteSession($);
  });
}

// The slice of `$` the poller runs on: the validator follows `$` only into
// functions declared in this file, so the calls are spelled here.
function pollerEngine($: EngineInterface): PollerEngine {
  return {
    clock: { now: () => clockNow($), every: (ms, fn) => $.clock.every(ms, fn) },
    process: { run: (argv, init) => $.process.run(argv, init) },
    session: { id: () => $.session.id() },
    fs: { write: (path, text) => $.fs.write(path, text) },
    ui: { log: (text) => $.ui.log(text) },
    home: () => $.env.get('HOME'),
  };
}

function makePoller($: EngineInterface): Poller {
  return createPoller(
    pollerEngine($),
    () => model,
    (next) => replaceModel($, next),
  );
}

// Whether the `cctop` binary answers `--version`: `present` on exit 0,
// `missing` when it cannot start, exits non-zero or takes over 3 s. Runs
// after session.start's `next(e)` and is never awaited by a hook; settles
// once the answer is in the model, so the restore can follow it.
function detectBinary($: EngineInterface): Promise<void> {
  return $.process
    .run(['cctop', '--version'], { timeoutMs: VERSION_TIMEOUT_MS })
    .then(
      (result): Binary => (result.exitCode === 0 ? 'present' : 'missing'),
      (): Binary => 'missing',
    )
    .then((binary) => {
      apply($, { type: 'binary', binary });
      if (binary === 'present' && model.open) poller?.start();
    })
    .catch((err: unknown) => $.ui.log(`cctop: binary detection failed: ${String(err)}`));
}

// The plugin's version for the marker file, from plugin.json under
// `$.plugin.root`; stays null when the manifest cannot be read.
function readVersion($: EngineInterface): Promise<void> {
  return $.fs
    .read(`${$.plugin.root}/${MANIFEST}`)
    .then((text) => {
      const version = (JSON.parse(text) as { version?: unknown }).version;
      if (typeof version === 'string') model = { ...model, version };
    })
    .catch((err: unknown) => $.ui.log(`cctop: plugin.json unreadable: ${String(err)}`));
}

// Reopens the pane a reload closed: `$.store` holds `{ open: true, view }`
// from the last persistPane. Only where a person is at the prompt, and only
// after the binary check, so the poller starts with the pane.
function restorePane($: EngineInterface, isInteractive: boolean): Promise<void> {
  if (!isInteractive) return Promise.resolve();
  return $.store
    .get(STORE_KEY)
    .then((saved) => {
      if (model.open || saved === null || typeof saved !== 'object') return;
      const { open, view } = saved as { open?: unknown; view?: unknown };
      if (open !== true) return;
      const known = VIEWS.find((v) => v.view === view);
      // A restore has nobody to answer; the outcome reaches the person
      // through the status line (hidden) or the pane itself.
      return openPane($, known?.view ?? 'overview').then(() => undefined);
    })
    .catch((err: unknown) => $.ui.log(`cctop: restore failed: ${String(err)}`));
}

function updateMarker($: EngineInterface): void {
  writeMarker(pollerEngine($), model).catch((err: unknown) => $.ui.log(`cctop: marker write failed: ${String(err)}`));
}

// Learns the session id after session.start; the poller's sync writes the
// marker with `open: false`, so from then on `cctop pane status` can tell
// that the hooks module runs in this session, before any pane was opened.
function noteSession($: EngineInterface): Promise<void> {
  return (poller?.sync() ?? Promise.resolve(false))
    .then(() => readSessionInfo($))
    .then(() => readAgentLimits($))
    .catch((err: unknown) => $.ui.log(`cctop: session.id failed: ${String(err)}`));
}

// `$.session.info()`: the transcript path the poller queries by and the
// data rules, platform and screen reader mode the marker carries. Read at
// session.start and on every turn (`dataRules` and `isScreenReader` can
// change: the d.ts says to read them at each use). A read that lands after
// `/clear` rotated the id is dropped: its path is the old transcript's. A
// change is written to the marker at once, so the header badge need not
// wait for a tick.
function readSessionInfo($: EngineInterface): Promise<void> {
  const sessionId = model.sessionId;
  return $.session
    .info()
    .then((info) => {
      if (model.sessionId !== sessionId) return;
      const facts: SessionFacts = {};
      if (typeof info.transcriptPath === 'string' && info.transcriptPath !== '') facts.transcriptPath = info.transcriptPath;
      if (info.platform !== undefined) facts.platform = info.platform;
      if (info.isScreenReader !== undefined) facts.isScreenReader = info.isScreenReader;
      if (info.dataRules !== undefined) facts.dataRules = [...info.dataRules];
      // Unchanged: no redraw, no marker write (every turn reads it again).
      if (JSON.stringify(model.sessionFacts) === JSON.stringify(facts)) return;
      apply($, { type: 'session.info', facts });
      updateMarker($);
    })
    .catch((err: unknown) => $.ui.log(`cctop: session.info failed: ${String(err)}`));
}

// `$.agent.limits()`: how many agents may run at once and how deep they
// nest, for the agents view's header. Read at session.start and at each
// turn.complete (the d.ts: read at each call), written to the marker when
// it changed. A read that lands after `/clear` is dropped.
function readAgentLimits($: EngineInterface): Promise<void> {
  const sessionId = model.sessionId;
  return $.agent
    .limits()
    .then((l) => {
      if (model.sessionId !== sessionId) return;
      const limits = { concurrent: l.concurrent, depth: l.depth, isBackgroundEnabled: l.isBackgroundEnabled };
      if (JSON.stringify(model.agentLimits) === JSON.stringify(limits)) return;
      apply($, { type: 'agent.limits', limits });
      updateMarker($);
    })
    .catch((err: unknown) => $.ui.log(`cctop: agent.limits failed: ${String(err)}`));
}

// `/clear` gives the session a new id and fires no session.start (the d.ts:
// "Not `/clear`"), so every turn reads the id again. When it changed, the
// model has just dropped the old session (model.ts) and, while the pane is
// open, the new one is read at once instead of at the next timer. Never
// awaited by the hook: the turn owes the pane nothing.
function followSession($: EngineInterface): void {
  poller
    ?.sync()
    .then(async (rotated) => {
      // A new session is a new spool: its first failure is worth one line too.
      if (rotated) {
        breakdownFailureLogged = false;
        // The old session's bookings: the next usage read books afresh.
        clearResetTimers();
        notifyState.scheduledReset.clear();
      }
      // Before the first query of a rotated session: its transcript path
      // and the limits the model dropped with the old session.
      await readSessionInfo($);
      if (rotated) await readAgentLimits($);
      if (!rotated || !model.open) return;
      readUsage($);
      void poller?.tick();
    })
    .catch((err: unknown) => $.ui.log(`cctop: session.id failed: ${String(err)}`));
}

function readUsage($: EngineInterface): void {
  Promise.all([$.session.usage(), clockNow($)])
    .then(([usage, at]) => {
      apply($, { type: 'usage', usage, at });
      notifyResets($, usage, at);
    })
    .catch((err: unknown) => $.ui.log(`cctop: session.usage failed: ${String(err)}`));
}

// Reads /context's breakdown (`summary`: an estimate, no token-count
// request, never `full`) and writes the spool cctop reads. Called after a
// turn.complete or session.compact has run `next(e)`, at most once per
// BREAKDOWN_DEBOUNCE_MS, and only with a session bound.
async function writeBreakdown($: EngineInterface): Promise<void> {
  const sessionId = model.sessionId;
  if (sessionId === null) return;
  const at = await clockNow($);
  if (!breakdownDue(model, at)) return;
  // Bookkeeping only (the debounce's clock): nothing on screen changes, so no redraw.
  model = reduce(model, { type: 'breakdown.read', at });
  try {
    const usage = await $.session.usage({ breakdown: 'summary' });
    const text = spoolText(usage, at);
    if (text === null) return;
    const home = await $.env.get('HOME');
    if (typeof home !== 'string' || home === '') return;
    await $.fs.write(`${home}/.cctop/context/${sessionId}.json`, text);
  } catch (err) {
    if (breakdownFailureLogged) return;
    breakdownFailureLogged = true;
    $.ui.log(`cctop: context breakdown spool failed: ${String(err)}`);
  }
}

// The model's name for the header, read once after session.start's `next(e)`.
function readModelName($: EngineInterface): void {
  $.session
    .model()
    .then((name) => apply($, { type: 'session.model', name }))
    .catch((err: unknown) => $.ui.log(`cctop: session.model failed: ${String(err)}`));
}

function stopUsageTimer(): void {
  usageTimer?.cancel();
  usageTimer = null;
}

function startUsageTimer($: EngineInterface): void {
  if (usageTimer !== null) return;
  usageTimer = $.clock.every(USAGE_POLL_MS, () => {
    if (model.turn.state !== 'busy') {
      stopUsageTimer();
      return;
    }
    readUsage($);
  });
}

// Remembers `{ open, view }` so the next session.start can reopen the pane
// on the same view.
function persistPane($: EngineInterface): void {
  $.store
    .set(STORE_KEY, { open: model.open, view: model.view })
    .catch((err: unknown) => $.ui.log(`cctop: store.set failed: ${String(err)}`));
}

// Opens the pane (an open id is merely retitled) on `view` when given. Never
// asks for `focus`: the keyboard stays the person's. The timers and the
// poller run only while the pane is open, so they start here. Resolves to
// the sentence for the person once the engine's first render (or its
// absence, OPEN_SETTLE_MS later) says where the pane went.
async function openPane($: EngineInterface, view?: View): Promise<string> {
  await $.ui.open({ id: PANE_ID, title: 'cctop' });
  const opened = model.open;
  const label = view === undefined ? undefined : VIEWS.find((v) => v.view === view)?.label;
  const openedAt = opened ? model.openedAt : await clockNow($);
  const before = model;
  model = {
    ...model,
    open: true,
    view: view ?? model.view,
    openedAt,
    visibility: opened && model.visibility === 'visible' ? 'visible' : 'unknown',
  };
  overviewChanged($, before);
  persistPane($);
  if (model.view === 'agents') void readAgents($);
  if (!opened) {
    // Whatever happened while the pane was closed is history: the first
    // coach and summary answers after this open are the placed rows'
    // baseline again, so a miss located meanwhile is not placed here.
    standingTaken = { coach: false, summary: false };
    // The id first, so a `/clear` since the last look does not wipe the
    // usage read next.
    await poller?.sync();
    if (model.binary === 'present') poller?.start();
    if (model.turn.state === 'busy') startUsageTimer($);
    readUsage($);
    updateMarker($);
  }
  const rendered = awaitRender($);
  await invalidateNow($);
  await rendered;
  updateMarker($);
  return outcomeText(model, label);
}

// What every close does, whoever closes: the timers and the poller stop,
// the choice is remembered, the marker says `open: false`. Runs from the
// ui.close hook (any origin) and after the command's own $.ui.close, so it
// is a no-op the second time round.
function paneClosed($: EngineInterface): void {
  if (!model.open) return;
  model = { ...model, open: false, visibility: 'unknown' };
  poller?.stop();
  stopUsageTimer();
  stopRenderTimer();
  stopHiddenTimer();
  stopChartTimers();
  unpinStatus($);
  if (model.coachStatus !== null) {
    $.ui.status(undefined);
    model = reduce(model, { type: 'coach.status', status: null });
  }
  persistPane($);
  updateMarker($);
}

async function closePane($: EngineInterface): Promise<void> {
  await $.ui.close({ id: PANE_ID });
  paneClosed($);
}

// A view-bar press: the view changes, the choice is remembered, the pane
// redraws.
function selectView($: EngineInterface, view: View): void {
  const before = model;
  model = { ...model, view };
  overviewChanged($, before);
  if (view === 'agents') void readAgents($);
  persistPane($);
  invalidateNow($).catch((err: unknown) => $.ui.log(`cctop: redraw failed: ${String(err)}`));
}

// The pane's tree for one render. Inline (the classic renderer's few rows
// above the prompt): the header and the Context and Limits lines only, no
// view bar. Docked: the view bar, the view, then the state of the binary and
// its query verbs beneath it. `now` is the render hook's one clock reading,
// for the elapsed times and countdowns.
function buildPane($: EngineInterface, e: RenderInput<'Pane'>, now: number): RenderElement {
  const el = $.ui.resolve(e);
  const { Box, Text } = el;
  const columns = e.props.bodyColumns;
  if (e.props.placement !== 'dock') {
    return (
      <Box flexDirection="column">
        {renderView(model, el, columns, 'inline', now)}
        {model.binary === 'missing' && <Text wrap="truncate">{INSTALL_HINT}</Text>}
      </Box>
    );
  }
  // Raster and Image are the terminal's: elsewhere body 1 draws blank boxes.
  const charts = e.surface === 'terminal' ? { el: $.ui.resolve(e), pictures: picturesForRender() } : undefined;
  return (
    <Box flexDirection="column">
      {viewBar($, el, columns)}
      {renderView(model, el, columns, 'dock', now, { el, coach: coachActions($), overview: overviewActions($), agents: agentsActions($), ...(charts ? { charts } : {}) })}
      {model.binary === 'missing' && <Text wrap="truncate">{INSTALL_HINT}</Text>}
      {model.stale && <Text wrap="truncate">cctop query stale</Text>}
      {unsupportedVerbs(model).map((verb) => (
        <Text key={verb} wrap="truncate">
          {verb}: {UNSUPPORTED}
        </Text>
      ))}
    </Box>
  );
}

// The act dialog's tree: the items and the selected one's detail, drawn by
// the dialog's own width. It is not the pane: nothing of the pane's render
// bookkeeping (visibility, the width the status line follows) is touched.
function buildAct($: EngineInterface, e: RenderInput<'Pane'>): RenderElement {
  return renderAct(model, $.ui.resolve(e), e.props.bodyColumns, actActions($), e.props.isFocused);
}

// The view bar, the pane's first row: `cctop  Overview  Tools  …`. The
// current view is an inverse Text; the others are plain Buttons (a click,
// or Enter after `ctrl+x tab` and `tab`, switches). No hotkeys: the docked
// pane never reads them (docs/claude-code-panels.md §5.5), and the band
// above the prompt that used to carry them cost the transcript a row, so
// the bar lives here alone. Tabs that do not fit on one line continue on
// the next, so the bar never overflows a narrow pane.
function viewBar(
  $: EngineInterface,
  el: Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button'>,
  columns: number,
): RenderElement {
  const { Box, Text, Button } = el;
  const lines: RenderElement[][] = [
    [
      <Text wrap="truncate" bold>
        {BAR_LEAD}
      </Text>,
    ],
  ];
  let used = BAR_LEAD.length;
  let prevActive = false;
  for (const { view, label } of VIEWS) {
    const active = view === model.view;
    // Two cells between tabs; the active tab's inverse padding is one of them.
    let gap = prevActive || active ? 1 : 2;
    // The label plus two: the active tab's padding, or the `[ ]` a surface
    // may draw around a Button (the harness does; the engine's plain form
    // is the bare label), so the row never overflows on either.
    const width = label.length + 2;
    if (used + gap + width > columns) {
      lines.push([]);
      used = 0;
      gap = 0;
    }
    const line = lines.at(-1);
    if (!line) break;
    if (gap > 0) line.push(<Text wrap="truncate">{' '.repeat(gap)}</Text>);
    line.push(
      active ? (
        <Text key={view} wrap="truncate" inverse>
          {` ${label} `}
        </Text>
      ) : (
        <Button key={view} label={label} plain onPress={() => selectView($, view)} />
      ),
    );
    used += gap + width;
    prevActive = active;
  }
  // While an agent's transcript is in the main view: the way back.
  if (model.viewedAgent !== null) {
    const line = lines.at(-1);
    const back = 'back to main';
    if (line !== undefined) {
      if (used + 2 + back.length + 2 > columns) {
        lines.push([]);
        used = 0;
      }
      lines.at(-1)?.push(
        <Text wrap="truncate">{used === 0 ? '' : '  '}</Text>,
        <Button key="back-to-main" label={back} plain onPress={() => viewAgent($, null)} />,
      );
      used += (used === 0 ? 0 : 2) + back.length + 2;
    }
  }
  // The `bin · shim · hooks 2.1.273` badges after the tabs, when they fit:
  // Console's header is the object's, identical on both surfaces.
  const badges = badgesLine(header(model, 0).badges);
  const badgesWidth = badges.reduce((n, b) => n + b.text.length, 0);
  if (used + 2 + badgesWidth <= columns) {
    lines.at(-1)?.push(
      <Text wrap="truncate">{'  '}</Text>,
      ...badges.map((b) => (
        <Text wrap="truncate" color={b.color} dimColor={b.dim}>
          {b.text}
        </Text>
      )),
    );
  }
  return (
    <Box flexDirection="column">
      {lines.map((line, i) => (
        <Box key={`view-bar-${i}`} flexDirection="row">
          {line}
        </Box>
      ))}
    </Box>
  );
}
// The label before the first tab: whose bar this is.
const BAR_LEAD = 'cctop';

export const register: Register = (on) => {
  model = initialModel();
  stopUsageTimer();
  stopRenderTimer();
  stopHiddenTimer();
  renderWaiters = [];
  statusPinned = false;
  notifyOn = false;
  notifyState = initialNotifyState();
  clearResetTimers();
  agentsAsked.clear();
  invalidatedAt = null;
  renderPending = false;
  poller?.stop();
  poller = null;
  chartRenderer = null;
  placedRows.clear();
  standing.clear();
  standingTaken = { coach: false, summary: false };
  pictures = new Map();
  mounted = new Map();
  chartsBusy = false;
  chartsAgain = false;
  stopChartTimers();
  probeTries = 0;
  chartsQuery = false;

  // The command is declared once the session is ready; session.start is
  // awaited before the first prompt, so the command is listed from turn one.
  // Every hook below reads the clock through the host (one round trip, the
  // cost of the hook's own dispatch again) before its `next(e)`: the
  // timestamps are the engine's, so the pane and the marker agree with it.
  // session.start checks the surfaces first and says once what it found.
  on('session.start', async ($, e, next) => {
    const check = await selfCheck($);
    breakdownFailureLogged = false;
    await readNotifyEnv($);
    apply($, { type: 'session.start', at: check.now, selfCheck: check.problems.length === 0 ? 'ok' : check.problems.join('; ') });
    poller = makePoller($);
    await $.command.register({
      name: COMMAND,
      description: 'Open the cctop dashboard pane',
      argumentHint: '[view|close]',
      immediate: true,
    });
    const result = await next(e);
    void announce($, check);
    readModelName($);
    const detected = detectBinary($);
    void initCharts($, detected).catch((err: unknown) => $.ui.log(`cctop: charts init failed: ${String(err)}`));
    void detected.then(() => restorePane($, e.isInteractive));
    return result;
  }).catch(($, e, next) => {
    $.ui.log(`cctop: session.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // While the pane is closed the turn and tool hooks keep the books and
  // follow the session id, nothing else: no timer, no poll, no usage read.
  on('turn.start', async ($, e, next) => {
    apply($, { type: 'turn.start', at: await clockNow($) });
    void readAgents($);
    if (model.open) startUsageTimer($);
    poller?.reschedule();
    followSession($);
    return next(e);
  }).catch(($, e, next) => {
    $.ui.log(`cctop: turn.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('turn.complete', async ($, e, next) => {
    apply($, { type: 'turn.complete', at: await clockNow($), durationMs: e.durationMs, reason: e.reason });
    placeNotes($, false, false);
    stopUsageTimer();
    poller?.reschedule();
    const result = await next(e);
    if (model.open) readUsage($);
    void writeBreakdown($);
    void readAgentLimits($);
    void readAgents($);
    return result;
  }).catch(($, e, next) => {
    $.ui.log(`cctop: turn.complete failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // Observe only: the Stop is noted before any await (the d.ts: it is raised
  // before the abort and nothing waits on it), and `next(e)` is returned as is.
  on('session.stop', ($, e, next) => {
    noteStop($);
    return next(e);
  });

  on('session.compact', async ($, e, next) => {
    apply($, { type: 'session.compact' });
    const result = await next(e);
    void writeBreakdown($);
    return result;
  }).catch(($, e, next) => {
    $.ui.log(`cctop: session.compact failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // Times every tool call around `next(e)`: a timestamp before, the result's
  // `isError` and text length after. The result itself passes through
  // untouched; a call that throws beneath us is recorded as an error.
  on('tool.call', async ($, e, next) => {
    const startedAt = await clockNow($);
    apply($, { type: 'tool.start', name: e.tool, at: startedAt });
    let result: ToolCallResult | undefined;
    try {
      result = await next(e);
      return result;
    } finally {
      apply($, {
        type: 'tool.end',
        name: e.tool,
        startedAt,
        at: await clockNow($),
        isError: result === undefined || result.isError === true,
        resultChars: result?.text?.length ?? 0,
      });
    }
  }).catch(($, e, next) => {
    $.ui.log(`cctop: tool.call failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // `/cctop-pane` toggles the pane, `/cctop-pane <view>` opens it on that
  // view, `/cctop-pane close` closes it; anything else prints the usage. An
  // open answers with where the pane went (outcomeText), never a bare
  // "opened": the person must be able to tell a docked pane from one the
  // /diff panel is hiding.
  on('command.run', { command: COMMAND }, async ($, e) => {
    const arg = e.args.trim();
    if (arg === 'close') {
      await closePane($);
      return { text: 'cctop pane closed' };
    }
    if (arg === '') {
      if (model.open) {
        await closePane($);
        return { text: 'cctop pane closed' };
      }
      return { text: await openPane($) };
    }
    const view = VIEWS.find((v) => v.view === arg);
    if (view === undefined) return { text: USAGE };
    return { text: await openPane($, view.view) };
  }).catch(($, _e, next) => {
    $.ui.log(`cctop: /${COMMAND} failed: ${next.error.message ?? next.error.kind}`);
    return { text: 'cctop pane could not be opened' };
  });

  // On a build with function hooks, `/cctop` (the skill, US-001) still
  // resolves first; this replaces its prompt so the model does the same
  // thing the native command does — open the pane — instead of running
  // `cctop split`, and says nothing back into the transcript.
  on('skill.prompt', { skill: 'cctop' }, async ($) => {
    const outcome = await openPane($);
    return {
      text: `The cctop pane was just opened by the plugin; its state is: "${outcome}" Reply with exactly that sentence and nothing else. Do not run any tool.`,
    };
  }).catch(($, e, next) => {
    $.ui.log(`cctop: skill.prompt failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // The dialog's close, by Esc or by a Button: the state follows.
  on('ui.close', { id: ACT_ID }, ($, e, next) => {
    return next(e).then((result) => {
      apply($, { type: 'act.close' });
      return result;
    });
  }).catch(($, e, next) => {
    $.ui.log(`cctop: ui.close failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // Every close of the pane, the person's and an unload's as much as the
  // command's, ends the timers and the poller; a close of another pane is
  // not ours.
  on('ui.close', { id: PANE_ID }, ($, e, next) => {
    return next(e).then((result) => {
      paneClosed($);
      return result;
    });
  }).catch(($, e, next) => {
    $.ui.log(`cctop: ui.close failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // A row this plugin placed in the transcript: drawn from what was kept
  // when it was placed; another plugin's row is not ours.
  on('ui.render', { component: 'Placed' }, ($, e, next) => {
    const row = e.props.plugin === $.plugin.name ? placedRows.get(e.props.id) : undefined;
    if (row === undefined || e.surface !== 'terminal') return next(e);
    return placedTree(row, coachOf(model.query.coach), e.viewport?.columns ?? 80, $.ui.resolve(e), coachActions($));
  }).catch(($, e, next) => {
    $.ui.log(`cctop: render failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // A view that throws must not take the pane down: the error is drawn in
  // one line above the last tree that built, kept in the model for that.
  // The clock is read once per render, before the tree: the frame's time.
  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId === ACT_ID) return buildAct($, e);
    if (e.requestId !== PANE_ID) return next(e);
    const now = await clockNow($);
    noteRender($, e, now);
    const { Box, Text } = $.ui.resolve(e);
    try {
      const tree = buildPane($, e, now);
      model = { ...model, lastTree: tree };
      probeAfterRender($);
      return tree;
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err);
      $.ui.log(`cctop: render error: ${message}`);
      return (
        <Box flexDirection="column">
          <Text color="red" wrap="truncate">
            cctop render error: {message}
          </Text>
          {model.lastTree}
        </Box>
      );
    }
  }).catch(($, e, next) => {
    $.ui.log(`cctop: render failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });
};
