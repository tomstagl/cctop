// cctop pane: the dashboard drawn inside Claude Code's own TUI through the
// function-hooks API (early access). This is the skeleton: it registers the
// /cctop-pane command, opens the pane and draws one line. The views, poller and
// engine-native metrics land in later stories.
import type { Register } from 'claude-code';

const PANE_ID = 'cctop';
// The native command is /cctop-pane, not /cctop: the engine reserves /cctop
// for this plugin's own skill (listed as /cctop:cctop) and refuses a
// $.command.register of that name (PRD open question 1, resolved 2026-09-12).
// The skill stays the fallback path for builds without function hooks.
const COMMAND = 'cctop-pane';

export const register: Register = (on) => {
  // The command is declared once the session is ready; session.start is
  // awaited before the first prompt, so the command is listed from turn one.
  on('session.start', ($, e, next) =>
    $.command
      .register({
        name: COMMAND,
        description: 'Open the cctop dashboard pane',
        argumentHint: '[view|close]',
        immediate: true,
      })
      .then(() => next(e)),
  ).catch(($, e, next) => {
    $.ui.log(`cctop: command registration failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });

  on('command.run', { command: COMMAND }, async ($) => {
    await $.ui.open({ id: PANE_ID, title: 'cctop' });
    return { text: 'cctop pane opened' };
  }).catch(($, _e, next) => {
    $.ui.log(`cctop: /${COMMAND} failed: ${next.error.message ?? next.error.kind}`);
    return { text: 'cctop pane could not be opened' };
  });

  on('ui.render', { component: 'Pane' }, ($, e, next) => {
    if (e.requestId !== PANE_ID) return next(e);
    const { Text } = $.ui.resolve(e);
    return <Text>cctop</Text>;
  }).catch(($, e, next) => {
    $.ui.log(`cctop: render failed: ${next.error.message ?? next.error.kind}`);
    return next(e);
  });
};
