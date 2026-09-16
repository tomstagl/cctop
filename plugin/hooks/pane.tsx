// cctop pane: the dashboard drawn inside Claude Code's own TUI through the
// function-hooks API (early access). It registers the /cctop-pane command,
// opens the pane, draws it, and keeps a Model of the session up to date from
// the engine's own events (model.ts) and, when the binary is installed, from
// `cctop query` through the poller (poller.ts). The views (views/*.tsx) draw
// the model; the Overview is the default.
import type { ElementTable, EngineInterface, Register, RenderElement, RenderInput, Timer, ToolCallResult } from 'claude-code';
import {
  HIDDEN_STATUS,
  initialModel,
  outcomeText,
  reduce,
  unsupportedVerbs,
  UNSUPPORTED,
  TESTED_WITH,
  type Action,
  type Binary,
  type Model,
  type View,
} from './model';
import { createPoller, writeMarker, type Poller, type PollerEngine } from './poller';
import { coachOf, statusLine, type CoachActions, type LightId } from './views/coach';
import { renderView } from './views/index';
import { badgesLine, header, type OverviewActions } from './views/overview';

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
  return $.clock.now().then((now) => invalidateAt($, now));
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
  model = reduce(model, {
    type: 'render',
    at: now,
    placement: e.props.placement,
    bodyColumns: e.props.bodyColumns,
    viewportColumns: e.viewport?.columns ?? null,
  });
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
    const now = await $.clock.now();
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
  if (model.open) requestRender($).catch((err: unknown) => $.ui.log(`cctop: redraw failed: ${String(err)}`));
  if (model.query.coach !== before.query.coach) coachChanged($);
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
// body in place; the header never moves.
function overviewActions($: EngineInterface): OverviewActions {
  return {
    open: (id) => apply($, { type: 'overview.body', id }),
    keys: (keys) => apply($, { type: 'overview.keys', keys }),
  };
}

// What the coach view's Buttons do: `fill` writes a prompt- or slash-class
// action into the prompt box (never submits it), `snooze` asks the binary
// (queued for the TUI while it runs) and re-polls, `why` and `light` are
// view state.
function coachActions($: EngineInterface): CoachActions {
  return {
    fill: (text) => {
      $.prompt
        .fill({ text })
        .then(({ isFilled }) => {
          if (!isFilled) $.ui.toast('cctop: the prompt box is busy — the action was not filled');
        })
        .catch((err: unknown) => $.ui.log(`cctop: prompt.fill failed: ${String(err)}`));
    },
    snooze: (rule) => {
      const id = model.sessionId;
      if (id === null) return;
      $.process
        .run(['cctop', 'query', 'coach', '--snooze', rule, '--session', id, '--surface', 'pane'], { timeoutMs: 5000 })
        .then((result) => {
          if (result.exitCode !== 0) throw new Error(`exit ${result.exitCode}`);
          const answer = JSON.parse(result.stdout) as { snooze?: unknown };
          if (typeof answer.snooze === 'string') $.ui.toast(`cctop: ${answer.snooze}`);
          apply($, { type: 'query', verb: 'coach', data: answer });
        })
        .catch((err: unknown) => $.ui.log(`cctop: snooze failed: ${String(err)}`));
    },
    why: () => apply($, { type: 'coach.why', why: !model.coachWhy }),
    light: (light: LightId) => apply($, { type: 'coach.light', light }),
  };
}

function apply($: EngineInterface, action: Action): void {
  replaceModel($, reduce(model, action));
}

// The slice of `$` the poller runs on: the validator follows `$` only into
// functions declared in this file, so the calls are spelled here.
function pollerEngine($: EngineInterface): PollerEngine {
  return {
    clock: { now: () => $.clock.now(), every: (ms, fn) => $.clock.every(ms, fn) },
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
function readVersion($: EngineInterface): void {
  $.fs
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
function noteSession($: EngineInterface): void {
  poller?.sync().catch((err: unknown) => $.ui.log(`cctop: session.id failed: ${String(err)}`));
}

// `/clear` gives the session a new id and fires no session.start (the d.ts:
// "Not `/clear`"), so every turn reads the id again. When it changed, the
// model has just dropped the old session (model.ts) and, while the pane is
// open, the new one is read at once instead of at the next timer. Never
// awaited by the hook: the turn owes the pane nothing.
function followSession($: EngineInterface): void {
  poller
    ?.sync()
    .then((rotated) => {
      if (!rotated || !model.open) return;
      readUsage($);
      void poller?.tick();
    })
    .catch((err: unknown) => $.ui.log(`cctop: session.id failed: ${String(err)}`));
}

function readUsage($: EngineInterface): void {
  Promise.all([$.session.usage(), $.clock.now()])
    .then(([usage, at]) => apply($, { type: 'usage', usage, at }))
    .catch((err: unknown) => $.ui.log(`cctop: session.usage failed: ${String(err)}`));
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
  const openedAt = opened ? model.openedAt : await $.clock.now();
  model = {
    ...model,
    open: true,
    view: view ?? model.view,
    openedAt,
    visibility: opened && model.visibility === 'visible' ? 'visible' : 'unknown',
  };
  persistPane($);
  if (!opened) {
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
  model = { ...model, view };
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
  if (e.props.placement === 'inline') {
    return (
      <Box flexDirection="column">
        {renderView(model, el, columns, 'inline', now)}
        {model.binary === 'missing' && <Text wrap="truncate">{INSTALL_HINT}</Text>}
      </Box>
    );
  }
  return (
    <Box flexDirection="column">
      {viewBar($, el, columns)}
      {renderView(model, el, columns, 'dock', now, { el, coach: coachActions($), overview: overviewActions($) })}
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
    const line = lines[lines.length - 1];
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
  // The `bin · shim · hooks 2.1.273` badges after the tabs, when they fit:
  // Console's header is the object's, identical on both surfaces.
  const badges = badgesLine(header(model, 0).badges);
  const badgesWidth = badges.reduce((n, b) => n + b.text.length, 0);
  if (used + 2 + badgesWidth <= columns) {
    lines[lines.length - 1].push(
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
  invalidatedAt = null;
  renderPending = false;
  poller?.stop();
  poller = null;

  // The command is declared once the session is ready; session.start is
  // awaited before the first prompt, so the command is listed from turn one.
  // Every hook below reads the clock through the host (one round trip, the
  // cost of the hook's own dispatch again) before its `next(e)`: the
  // timestamps are the engine's, so the pane and the marker agree with it.
  on('session.start', async ($, e, next) => {
    apply($, { type: 'session.start', at: await $.clock.now() });
    poller = makePoller($);
    await $.command.register({
      name: COMMAND,
      description: 'Open the cctop dashboard pane',
      argumentHint: '[view|close]',
      immediate: true,
    });
    const result = await next(e);
    readModelName($);
    readVersion($);
    noteSession($);
    void detectBinary($).then(() => restorePane($, e.isInteractive));
    return result;
  }).catch(($, e, next) => {
    $.ui.log(`cctop: session.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // While the pane is closed the turn and tool hooks keep the books and
  // follow the session id, nothing else: no timer, no poll, no usage read.
  on('turn.start', async ($, e, next) => {
    apply($, { type: 'turn.start', at: await $.clock.now() });
    if (model.open) startUsageTimer($);
    poller?.reschedule();
    followSession($);
    return next(e);
  }).catch(($, e, next) => {
    $.ui.log(`cctop: turn.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('turn.complete', async ($, e, next) => {
    apply($, { type: 'turn.complete', at: await $.clock.now(), durationMs: e.durationMs, reason: e.reason });
    stopUsageTimer();
    poller?.reschedule();
    const result = await next(e);
    if (model.open) readUsage($);
    return result;
  }).catch(($, e, next) => {
    $.ui.log(`cctop: turn.complete failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('session.compact', ($, e, next) => {
    apply($, { type: 'session.compact' });
    return next(e);
  }).catch(($, e, next) => {
    $.ui.log(`cctop: session.compact failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // Times every tool call around `next(e)`: a timestamp before, the result's
  // `isError` and text length after. The result itself passes through
  // untouched; a call that throws beneath us is recorded as an error.
  on('tool.call', async ($, e, next) => {
    const startedAt = await $.clock.now();
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
        at: await $.clock.now(),
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

  // A view that throws must not take the pane down: the error is drawn in
  // one line above the last tree that built, kept in the model for that.
  // The clock is read once per render, before the tree: the frame's time.
  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== PANE_ID) return next(e);
    const now = await $.clock.now();
    noteRender($, e, now);
    const { Box, Text } = $.ui.resolve(e);
    try {
      const tree = buildPane($, e, now);
      model = { ...model, lastTree: tree };
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
