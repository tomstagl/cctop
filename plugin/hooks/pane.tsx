// cctop pane: the dashboard drawn inside Claude Code's own TUI through the
// function-hooks API (early access). It registers the /cctop-pane command,
// opens the pane, draws it, and keeps a Model of the session up to date from
// the engine's own events (model.ts) and, when the binary is installed, from
// `cctop query` through the poller (poller.ts). The views (views/*.tsx) draw
// the model; the Overview is the default.
import type { EngineInterface, Register, Timer, ToolCallResult } from 'claude-code';
import { initialModel, reduce, unsupportedVerbs, UNSUPPORTED, type Action, type Binary, type Model } from './model';
import { createPoller, type Poller, type PollerEngine } from './poller';
import { renderOverview } from './views/overview';

const PANE_ID = 'cctop';
// The native command is /cctop-pane, not /cctop: the engine reserves /cctop
// for this plugin's own skill (listed as /cctop:cctop) and refuses a
// $.command.register of that name (PRD open question 1, resolved 2026-09-12).
// The skill stays the fallback path for builds without function hooks.
const COMMAND = 'cctop-pane';
const USAGE_POLL_MS = 1000;
const VERSION_TIMEOUT_MS = 3000;
const INSTALL_HINT = 'needs the cctop binary: brew install tomstagl/tap/cctop';

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

function replaceModel($: EngineInterface, next: Model): void {
  model = next;
  if (model.open) $.ui.invalidate('ui.render');
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
// after session.start's `next(e)` and is never awaited by a hook.
function detectBinary($: EngineInterface): void {
  $.process
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

export const register: Register = (on) => {
  model = initialModel();
  stopUsageTimer();
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
        readUsage($);
        readModelName($);
        detectBinary($);
        return result;
      });
  }).catch(($, e, next) => {
    $.ui.log(`cctop: session.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('turn.start', ($, e, next) => {
    apply($, { type: 'turn.start', at: $.clock.now() });
    startUsageTimer($);
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
      readUsage($);
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

  on('command.run', { command: COMMAND }, async ($) => {
    await $.ui.open({ id: PANE_ID, title: 'cctop' });
    model = { ...model, open: true };
    if (model.binary === 'present') poller?.start();
    return { text: 'cctop pane opened' };
  }).catch(($, _e, next) => {
    $.ui.log(`cctop: /${COMMAND} failed: ${next.error.message ?? next.error.kind}`);
    return { text: 'cctop pane could not be opened' };
  });

  on('ui.render', { component: 'Pane' }, ($, e, next) => {
    if (e.requestId !== PANE_ID) return next(e);
    if (model.placement !== e.props.placement) model = { ...model, placement: e.props.placement };
    const el = $.ui.resolve(e);
    const { Box, Text } = el;
    const now = $.clock.now();
    // The view, then the state of the binary and its query verbs beneath it.
    return (
      <Box flexDirection="column">
        {renderOverview(model, el, e.props.bodyColumns, e.props.placement, now)}
        {model.binary === 'missing' && <Text wrap="truncate">{INSTALL_HINT}</Text>}
        {model.stale && <Text wrap="truncate">cctop query stale</Text>}
        {unsupportedVerbs(model).map((verb) => (
          <Text key={verb} wrap="truncate">
            {verb}: {UNSUPPORTED}
          </Text>
        ))}
      </Box>
    );
  }).catch(($, e, next) => {
    $.ui.log(`cctop: render failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });
};
