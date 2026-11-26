# PRD 6: the pane on the 2.1.332 API — session facts, agent limits, jump to an agent, notifications, Stop

**Status:** v1.0 · 2026-11-26 — ready for implementation.
**Target:** plugin 0.11.x (TypeScript), with small Rust reads.
**Depends on:** PRD 0 (`prd-cctop-contract-2-1-332.md`): every call here is declared only in the 2.1.332 d.ts.
**Source:** the 2.1.332 d.ts. Nouns new since 2.1.284: `$.session.info/identity/machine/append/redact`, `$.agent.limits/call`, `$.ui.notify/cancelNotify/place/view/selection`, `$.vcs.*`, `$.theme.*`, `$.prompt.compose/media/paint`, `$.process.hookEnv`, `$.wasm.compile`. Events new since 2.1.284: `agent.report`, `session.stop`, `session.append`, `tool.judge/run/offer`, `ui.fault/offer/pointer/tip`, `prompt.autocomplete/change/compose/mention`, `vcs.*`.

> Decisions taken with the user on 2026-11-26:
> 1. Read-only holds. A call that puts cctop in the path of something Claude Code delivers (`agent.report`), or changes the session (`theme`, `agent.call`, `session.append`, `prompt.*`), is out.
> 2. One state, many surfaces holds. Facts only the pane can read reach the Rust side through the pane's marker file, and from there the dashboard object.

---

## 1. Introduction

The pane (`plugin/hooks/pane.tsx`) hooks nine events today: `session.start`, `turn.start`, `turn.complete`, `session.compact`, `tool.call`, `command.run`, `skill.prompt`, `ui.close`, `ui.render`. It calls `$.clock.*`, `$.command.register`, `$.env.get`, `$.fs.read/write`, `$.process.run`, `$.prompt.fill`, `$.session.id/model/usage`, `$.store.*` and `$.ui.close/invalidate/log/open/resolve/status/toast`. It writes a marker to `~/.cctop/pane/<session>.json` (`poller.ts:102`) that `cctop pane status` and `src/pane.rs` read.

Five 2.1.332 additions serve a read-only dashboard:

| API | d.ts summary | Use in cctop |
|---|---|---|
| `$.session.info()` | transcript file, scratchpad, prompt id, agent type, config root, platform, hosting, screen reader, remote workspace, `dataRules` (`hipaa`, `zdr`, …) | Pass the transcript **path** to `cctop query --session <path>` instead of an id that cctop must resolve. Show a `ZDR` / `HIPAA` badge. Honour `isScreenReader`. |
| `$.agent.limits()` | `{ concurrent, depth, isBackgroundEnabled }` | The agents view header: `3 / 10 running · depth ≤ 2` |
| `$.ui.view({ agentId })` | puts an agent's transcript in the main view, *"only while the call answers the person's own input (a press…)"*; `{}` returns to main | A Button on each agent row in the pane: jump to that agent. `back to main` returns. |
| `$.ui.notify(text, { title, key, delayMs, at, untilBack })` | a native notification through the person's own channel (iTerm2, kitty, Ghostty, bell, a client); `Notification` hooks run first | Opt-in: notify when a NOW coach nudge appears, and when a rate-limit window resets (`at` = reset time, `untilBack: true`, so it only reaches someone who left) |
| `session.stop` event | fires when the person presses Stop (Esc / Ctrl+C on a turn); observe; payload has `isTurnRunning` and the usage figures | End the pane's "turn running" state at once instead of on the next poll, and poll immediately |

## 2. Goals

- The pane resolves its session by path, never by guess.
- Agent concurrency limits and data rules are visible on both surfaces.
- A person can jump from an agent row in the pane to that agent's transcript.
- Opt-in notifications for the two moments worth interrupting someone: a NOW nudge, a rate-limit reset.
- Stop is reflected immediately.

## 3. User stories

### US-001: Session facts through `$.session.info()`
**Description:** As a pane user, I want the pane to tell cctop exactly which transcript to read, and to show data-handling rules, so that the pane never attaches to the wrong session and I know what rules apply.

**Acceptance criteria:**
- [ ] On `session.start` and when the session id changes (`followSession`, `pane.tsx:452`), the pane calls `$.session.info()` and keeps `transcriptPath`, `platform`, `isScreenReader`, `dataRules`.
- [ ] `poller.ts` passes `--session <transcriptPath>` when known, else the id as today. Test with a fake engine: the argv carries the path.
- [ ] The marker gains `facts: { dataRules?: string[], platform?: string, isScreenReader?: boolean }`. `src/pane.rs` parses it, and `dashboard.rs`'s header shows `ZDR` / `HIPAA` badges for known rule names and `rules` for an unknown one. The d.ts: *"treat absent, and any name you do not know, as a rule in force."* Snapshot.
- [ ] When `isScreenReader` is true, the pane's view bar uses words instead of glyph-only badges (where a glyph-only badge exists). Test in `tests/pane/`.
- [ ] `claude plugin validate --strict` clean; `npm test` green.

### US-002: Agent limits on the agents view
**Description:** As a person using cctop, I want to see how many agents can run at once and how deep they can nest, so that a queue of waiting agents is explained.

**Acceptance criteria:**
- [ ] The pane calls `$.agent.limits()` on `session.start` and on each `turn.complete`, and writes `facts.agentLimits` into the marker.
- [ ] `src/pane.rs` parses it. The agents-view header on both surfaces reads `<running> / <concurrent> running · depth ≤ <depth>`, plus `· no background` when `isBackgroundEnabled` is false. Without a marker: unchanged. Snapshots both ways.
- [ ] `cctop query agents` carries `limits` with a registry id `agent_limits`; docs regenerated.

### US-003: Jump to an agent's transcript
**Description:** As a pane user, I want to press an agent row and see that agent's transcript in the main view, so that I can look at what it is doing without leaving Claude Code.

**Acceptance criteria:**
- [ ] Each agent row in `views/agents.tsx` that has a live entry in `$.agent.list()` gets a `plain` Button; `onPress` calls `$.ui.view({ agentId })` with the `AgentInfo.id`. While an agent is viewed, the view bar shows `back to main`, which calls `$.ui.view({})`.
- [ ] Mapping cctop's agent id to `AgentInfo.id`: by exact match first, else by `AgentInfo.description` + `type` + spawn order. The mapping is a pure function with tests. When none matches, the row has no Button.
- [ ] A `{ deny }` result is shown as a toast with the reason, once.
- [ ] `$.agent.list()` is read on `turn.start` / `turn.complete` / the agents view opening, not on every render.
- [ ] `docs/verification/pane.md` item 43: *"Pressing an agent row shows its transcript; back to main returns"*, with the setup (a session with a running `Explore` agent), `Result: pending`. It also records whether cctop's ids equal `AgentInfo.id`.

### US-004: Opt-in notifications
**Description:** As a person who steps away, I want a notification when cctop has something urgent or my rate limit resets, so that I come back at the right time, and nothing otherwise.

**Acceptance criteria:**
- [ ] Off by default. On when `$.env.get("CCTOP_PANE_NOTIFY")` returns `1`, spelled literally in `pane.tsx` as the validator requires.
- [ ] A coach nudge of urgency NOW raises `$.ui.notify(<nudge headline>, { title: 'cctop', key: 'nudge-<rule id>' })` once per nudge instance (the advisor's fire id). A snooze or `acted` raises `$.ui.cancelNotify` for that key.
- [ ] When a five-hour or seven-day window is ≥ 90 % used, the pane schedules `$.ui.notify('rate limit resets now', { key: 'reset-5h', at: <resets_at ms>, untilBack: true })`, replacing any earlier schedule for that key.
- [ ] The result is logged once per key (`isSent`, `channel` or `reason`). A `no-channel` / `disabled` result stops further attempts for the session.
- [ ] The headline text comes from the coach object (fixed width), so it is identical to what the pane shows.
- [ ] Tests with a fake `$.ui.notify`: once per nudge, cancel on snooze, the reset schedule replaced and not duplicated.
- [ ] `plugin/README.md` documents `CCTOP_PANE_NOTIFY`.

### US-005: Stop ends the turn at once
**Description:** As a pane user, I want the pane to stop showing a running turn the moment I press Esc, so that it matches the screen.

**Acceptance criteria:**
- [ ] `on('session.stop', ($, e, next) => { apply($, { type: 'turn.stop', at }); poller?.pollNow(); return next(e); })`. It observes only and returns `next(e)` unchanged, noting the stop before any `await` as the d.ts asks.
- [ ] The reducer's `turn.stop` ends the running state the way `turn.complete` does, without a duration; test.
- [ ] `poller.ts` gains `pollNow()` (coalesced with a poll in flight); test.
- [ ] The validator lists `session.stop` among the hooks; no "gating hook" note for it.

## 4. Functional requirements

- FR-1: The pane must call `$.session.info()` and use `transcriptPath` for `cctop query` when present.
- FR-2: The marker must carry `facts.{dataRules, platform, isScreenReader, agentLimits}`; Rust must surface data rules and agent limits on both surfaces.
- FR-3: Agent rows must offer a jump only when an `AgentInfo.id` is matched, and only from a press.
- FR-4: Notifications must be opt-in, once per nudge, cancelled on snooze, and scheduled with `untilBack` for resets.
- FR-5: `session.stop` must be observed with `next(e)` returned unchanged.

## 5. Non-goals

| API | Why not |
|---|---|
| `agent.report` hook | *"What the outermost hook returns is delivered."* Hooking it puts cctop in the delivery path of every subagent report. SubagentStop and the agent transcripts already give completed/failed. |
| `$.vcs.*` | Experimental. The coach's outcome rules run in Rust, which reads git itself (`git.rs`); the pane cannot feed them a better answer. |
| `$.theme.*`, `$.agent.call`, `$.session.append`, `$.prompt.compose/paint` | They change the session or its look. |
| `$.session.identity()` | Account id, org and email: not cctop's to hold. |
| `$.ui.place`, `$.ui.selection` | No read-only use worth the surface. A placed row is reconsidered for coach nudges later. |
| `$.session.machine()`, `$.process.hookEnv()` | Nothing cctop shows depends on them. |

## 6. Technical considerations

- The validator follows `$` only into top-level functions of `pane.tsx`. New calls go in top-level functions or through the facade (`pollerEngine($)` pattern). Keep module state in module-scope `let`s; no local named `h` or `Fragment`.
- Failures in a `.catch` are `next.error`, not `failure`.
- `$.ui.view` is refused outside the person's own input, so it is called only from `onPress`.
- `$.ui.notify` runs the person's `Notification` hooks first, which may include cctop's own `cctop hook` for `Notification`. Such an event carries `plugin_notification`; `apply_hook` must not treat it as Claude Code's own idle/attention notification (`ui/state.rs:873`). Add a test.

## 7. Success metrics

- `cctop pane status` reports the session by path on 2.1.332 every time.
- With `CCTOP_PANE_NOTIFY=1`, one notification per NOW nudge and none for LATER/NEXT, over a week of use.

## 8. Open questions

- Do cctop's agent ids (from `subagents/agent-<id>.jsonl`) equal `AgentInfo.id`? Item 43 answers it. The mapping fallback exists for when they don't.
