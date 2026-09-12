// cctop pane: the dashboard drawn inside Claude Code's own TUI through the
// function-hooks API (early access). It registers the /cctop-pane command,
// opens the pane, draws it, and keeps a Model of the session up to date from
// the engine's own events (model.ts) and, when the binary is installed, from
// `cctop query` through the poller (poller.ts). The views (views/*.tsx) draw
// the model; the Overview is the default.
import type { ElementTable, EngineInterface, Register, RenderElement, RenderInput, Timer, ToolCallResult } from 'claude-code';
import { initialModel, reduce, unsupportedVerbs, UNSUPPORTED, TESTED_WITH, type Action, type Binary, type Model, type View } from './model';
import { createPoller, writeMarker, type Poller, type PollerEngine } from './poller';
import { renderView } from './views/index';

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
const INSTALL_HINT = 'needs the cctop binary: brew install tomstagl/tap/cctop';
// The views in view-bar order; the hotkey is the 1-based position.
const VIEWS: { view: View; label: string }[] = [
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
// The redraw throttle: when the last invalidate was, and the timer holding
// the trailing one back while changes come faster than RENDER_MIN_MS.
let invalidatedAt: number | null = null;
let renderTimer: Timer | null = null;

function invalidateNow($: EngineInterface): void {
  invalidatedAt = $.clock.now();
  $.ui.invalidate('ui.render');
}

// Asks for a redraw at most every RENDER_MIN_MS: a change inside the gap
// arms one trailing call for the end of it, later changes fold into that.
function requestRender($: EngineInterface): void {
  if (renderTimer !== null) return;
  const waited = invalidatedAt === null ? RENDER_MIN_MS : $.clock.now() - invalidatedAt;
  if (waited >= RENDER_MIN_MS) {
    invalidateNow($);
    return;
  }
  renderTimer = $.clock.after(RENDER_MIN_MS - waited, () => {
    renderTimer = null;
    invalidateNow($);
  });
}

function stopRenderTimer(): void {
  renderTimer?.cancel();
  renderTimer = null;
}

function replaceModel($: EngineInterface, next: Model): void {
  model = next;
  if (model.open) requestRender($);
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
      return openPane($, known?.view ?? 'overview');
    })
    .catch((err: unknown) => $.ui.log(`cctop: restore failed: ${String(err)}`));
}

function updateMarker($: EngineInterface): void {
  writeMarker(pollerEngine($), model).catch((err: unknown) => $.ui.log(`cctop: marker write failed: ${String(err)}`));
}

function readUsage($: EngineInterface): void {
  $.session
    .usage()
    .then((usage) => apply($, { type: 'usage', usage, at: $.clock.now() }))
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
// poller run only while the pane is open, so they start here.
async function openPane($: EngineInterface, view?: View): Promise<void> {
  await $.ui.open({ id: PANE_ID, title: 'cctop' });
  const opened = model.open;
  model = { ...model, open: true, view: view ?? model.view, openedAt: opened ? model.openedAt : $.clock.now() };
  persistPane($);
  if (opened) {
    invalidateNow($);
    return;
  }
  if (model.binary === 'present') poller?.start();
  if (model.turn.state === 'busy') startUsageTimer($);
  readUsage($);
  if (model.sessionId === null) apply($, { type: 'session.id', id: await $.session.id() });
  updateMarker($);
}

// What every close does, whoever closes: the timers and the poller stop,
// the choice is remembered, the marker says `open: false`. Runs from the
// ui.close hook (any origin) and after the command's own $.ui.close, so it
// is a no-op the second time round.
function paneClosed($: EngineInterface): void {
  if (!model.open) return;
  model = { ...model, open: false };
  poller?.stop();
  stopUsageTimer();
  stopRenderTimer();
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
  invalidateNow($);
}

// The pane's tree for one render. Inline (the classic renderer's few rows
// above the prompt): the header and the Context and Limits lines only, no
// view bar. Docked: the view bar, the view, then the state of the binary and
// its query verbs beneath it.
function buildPane($: EngineInterface, e: RenderInput<'Pane'>): RenderElement {
  const el = $.ui.resolve(e);
  const { Box, Text } = el;
  const now = $.clock.now();
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
      {renderView(model, el, columns, 'dock', now)}
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

// The view bar: `1 Overview · 2 Tools · …` as plain Buttons whose hotkeys
// act while the pane is focused. Buttons that do not fit on one line
// continue on the next, so the bar never overflows a narrow pane.
function viewBar($: EngineInterface, el: Pick<ElementTable<'terminal'>, 'Box' | 'Text' | 'Button'>, columns: number): RenderElement {
  const { Box, Text, Button } = el;
  const lines: RenderElement[][] = [[]];
  let used = 0;
  VIEWS.forEach(({ view, label }, i) => {
    const hotkey = String(i + 1);
    // `1: Overview` on the engine; one more for the harness's `[1 Overview]`.
    const width = hotkey.length + 3 + label.length;
    const line = lines[lines.length - 1];
    if (line.length > 0) {
      if (used + 3 + width > columns) {
        lines.push([]);
        used = 0;
      } else {
        line.push(<Text wrap="truncate"> · </Text>);
        used += 3;
      }
    }
    lines[lines.length - 1].push(
      <Button key={view} label={label} hotkey={hotkey} plain onPress={() => selectView($, view)} />,
    );
    used += width;
  });
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

export const register: Register = (on) => {
  model = initialModel();
  stopUsageTimer();
  stopRenderTimer();
  invalidatedAt = null;
  poller?.stop();
  poller = null;

  // The command is declared once the session is ready; session.start is
  // awaited before the first prompt, so the command is listed from turn one.
  on('session.start', ($, e, next) => {
    apply($, { type: 'session.start', at: $.clock.now() });
    poller = makePoller($);
    return $.command
      .register({
        name: COMMAND,
        description: 'Open the cctop dashboard pane',
        argumentHint: '[view|close]',
        immediate: true,
      })
      .then(() => next(e))
      .then((result) => {
        readModelName($);
        readVersion($);
        void detectBinary($).then(() => restorePane($, e.isInteractive));
        return result;
      });
  }).catch(($, e, next) => {
    $.ui.log(`cctop: session.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // While the pane is closed the turn and tool hooks keep the books and
  // nothing else: no timer, no poll, no usage read.
  on('turn.start', ($, e, next) => {
    apply($, { type: 'turn.start', at: $.clock.now() });
    if (model.open) startUsageTimer($);
    poller?.reschedule();
    return next(e);
  }).catch(($, e, next) => {
    $.ui.log(`cctop: turn.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('turn.complete', ($, e, next) => {
    apply($, { type: 'turn.complete', at: $.clock.now(), durationMs: e.durationMs, reason: e.reason });
    stopUsageTimer();
    poller?.reschedule();
    return next(e).then((result) => {
      if (model.open) readUsage($);
      return result;
    });
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
    const startedAt = $.clock.now();
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
        at: $.clock.now(),
        isError: result === undefined || result.isError === true,
        resultChars: result?.text?.length ?? 0,
      });
    }
  }).catch(($, e, next) => {
    $.ui.log(`cctop: tool.call failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  // `/cctop-pane` toggles the pane, `/cctop-pane <view>` opens it on that
  // view, `/cctop-pane close` closes it; anything else prints the usage.
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
      await openPane($);
      return { text: 'cctop pane opened' };
    }
    const view = VIEWS.find((v) => v.view === arg);
    if (view === undefined) return { text: USAGE };
    await openPane($, view.view);
    return { text: `cctop pane opened on ${view.label}` };
  }).catch(($, _e, next) => {
    $.ui.log(`cctop: /${COMMAND} failed: ${next.error.message ?? next.error.kind}`);
    return { text: 'cctop pane could not be opened' };
  });

  // On a build with function hooks, `/cctop` (the skill, US-001) still
  // resolves first; this replaces its prompt so the model does the same
  // thing the native command does — open the pane — instead of running
  // `cctop split`, and says nothing back into the transcript.
  on('skill.prompt', { skill: 'cctop' }, async ($) => {
    await openPane($);
    return {
      text: 'The cctop pane is already open beside the transcript. Reply with exactly one line: "cctop is open in the side pane." Do not run any tool.',
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
  on('ui.render', { component: 'Pane' }, ($, e, next) => {
    if (e.requestId !== PANE_ID) return next(e);
    if (model.placement !== e.props.placement) model = { ...model, placement: e.props.placement };
    const { Box, Text } = $.ui.resolve(e);
    try {
      const tree = buildPane($, e);
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
