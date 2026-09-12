// cctop pane: the dashboard drawn inside Claude Code's own TUI through the
// function-hooks API (early access). It registers the /cctop-pane command,
// opens the pane, draws it, and keeps a Model of the session up to date from
// the engine's own events (model.ts). The views and the binary-backed poller
// land in later stories.
import type { EngineInterface, Register, Timer, ToolCallResult } from 'claude-code';
import { initialModel, reduce, usageRows, type Action, type Model } from './model';

const PANE_ID = 'cctop';
// The native command is /cctop-pane, not /cctop: the engine reserves /cctop
// for this plugin's own skill (listed as /cctop:cctop) and refuses a
// $.command.register of that name (PRD open question 1, resolved 2026-09-12).
// The skill stays the fallback path for builds without function hooks.
const COMMAND = 'cctop-pane';
const USAGE_POLL_MS = 1000;

// Module state: one Model per loaded module (a hot reload starts a fresh
// environment, and `register` resets it). The helpers that take `$` are
// top-level functions: the validator refuses `$` handed to a closure.
let model: Model = initialModel();
// `$.session.usage()` is read from this timer alone while a turn runs, and
// once after session.start and turn.complete: never inside a hook's own
// path before `next(e)`, so the pane costs the turn nothing.
let usageTimer: Timer | null = null;

function apply($: EngineInterface, action: Action): void {
  model = reduce(model, action);
  if (model.open) $.ui.invalidate('ui.render');
}

function readUsage($: EngineInterface): void {
  $.session
    .usage()
    .then((usage) => apply($, { type: 'usage', usage, at: $.clock.now() }))
    .catch((err: unknown) => $.ui.log(`cctop: session.usage failed: ${String(err)}`));
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

  // The command is declared once the session is ready; session.start is
  // awaited before the first prompt, so the command is listed from turn one.
  on('session.start', ($, e, next) => {
    apply($, { type: 'session.start', at: $.clock.now() });
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
        return result;
      });
  }).catch(($, e, next) => {
    $.ui.log(`cctop: session.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('turn.start', ($, e, next) => {
    apply($, { type: 'turn.start', at: $.clock.now() });
    startUsageTimer($);
    return next(e);
  }).catch(($, e, next) => {
    $.ui.log(`cctop: turn.start failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('turn.complete', ($, e, next) => {
    apply($, { type: 'turn.complete', at: $.clock.now(), durationMs: e.durationMs, reason: e.reason });
    stopUsageTimer();
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
    return { text: 'cctop pane opened' };
  }).catch(($, _e, next) => {
    $.ui.log(`cctop: /${COMMAND} failed: ${next.error.message ?? next.error.kind}`);
    return { text: 'cctop pane could not be opened' };
  });

  on('ui.render', { component: 'Pane' }, ($, e, next) => {
    if (e.requestId !== PANE_ID) return next(e);
    if (model.placement !== e.props.placement) model = { ...model, placement: e.props.placement };
    const { Box, Text } = $.ui.resolve(e);
    const now = $.clock.now();
    // Until the Overview view lands, the pane draws what the engine alone
    // provides: the turn, the running tool, and the usage rows.
    const running = model.turn.runningTool;
    return (
      <Box flexDirection="column">
        <Text wrap="truncate">
          cctop · turn {model.turn.number} · {model.turn.state}
        </Text>
        {running !== null && (
          <Text wrap="truncate">
            running {running.name} {Math.round((now - running.startedAt) / 1000)}s
          </Text>
        )}
        {model.usage !== null &&
          usageRows(model.usage, now).map((row) => (
            <Text key={row.key} wrap="truncate">
              {row.label} {row.value}
            </Text>
          ))}
      </Box>
    );
  }).catch(($, e, next) => {
    $.ui.log(`cctop: render failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });
};
