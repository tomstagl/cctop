# PRD: subagent spend — the combined figure on Panel 2 and the agents view

**Status:** v1.1 · 2026-09-15 — draft, not implemented. v1.0 was written from the code alone; v1.1 checked every claim against `main` at v0.3.1 and against the sessions on the user's machine (79 with a `cost-state`, 8 of them with subagent usage, 2 forks, 387 task notifications). What changed is in the second box.
**Target:** cctop ≥ 0.3.1 attached to Claude Code CLI ≥ 2.1.271; TUI first, then `cctop query` / MCP and the pane.
**Depends on:** `tasks/prd-cctop.md` v1.1 (Panels 2 and 6, the metrics registry), `tasks/prd-cctop-coach.md` v1.3 (US-005 shipped `agents_cost`, the recursive `subagents/**` scan and the dashboard object). The teammate collector is a separate PRD: `tasks/prd-cctop-team-costs.md`.

> Decisions taken with the user on 2026-09-15:
> 1. Panel 2's headline becomes the **combined** figure: main session plus subagents (plus teammates once the second PRD lands). Main-only stays reachable with the existing `a` toggle.
> 2. Panel 6 **stays as it is**; the new agents view opens from it with `Enter`, the way the ledger opens from Panel 2.
> 3. The insight the view is built around is **wasted spend**: money that went to agents whose work did not come back.
> 4. Subagents and teammates ship as **two changes with two PRDs**. This is the first.

> v1.1 — what the check changed (details in §3):
> - **`cost-state` includes subagent calls** (and calls no transcript shows). v1.0's open question is closed and its two-branch design collapses to one: the ledger, where it exists, is the whole session's spend up to the moment it was written; cctop prices only what came after — main *and* agent calls — by time. A live session has no ledger until it ends or is bridged, so the live headline today is priced main-only and the "order of magnitude low" case is real.
> - **What "came back" is a `<task-notification>`, not the `tool_result`.** Agents launch in the background (`async_launched`); the result, its length, the status (`completed` / `failed` / `killed`) and Claude Code's own usage figure arrive later in a system-originated user line. The waste reasons now rest on those fields.
> - **A fork's first assistant line is the parent's launching message** (same `message.id`, verified on both live forks). cctop counts it as the fork's today: `agents_cost` double counts it. The combined figure must skip it.
> - **The 7-day baseline is already combined** (`cost-state.totalCostUSD ÷ turns`), so `×N$` compares a main-only live figure with a combined baseline today. The combined headline *removes* that bias; v1.0 had it backwards.
> - **Subagent transcripts stream `output_tokens`**: the several lines of one message carry growing counts and only the last is complete (59 % of 2 292 subagent messages on this machine; main transcripts never do this). `Agent::push` keeps the first line's usage — 3.2 M output tokens short across 357 agent files here.
> - Fixture A cannot exercise the return path: its fork was taken from a different, later session than its main transcript. US-001's "the fork returns 310 tokens" was invented.
> - Mechanics corrected: overlays are `state.overlay = Some(panel id)` + `Panel::render_overlay`, not `ContextView`; there is no header cost tile (cost lives in dashboard row 2's detail); `IDLE_MS` is Panel 6's *MCP* idle constant; `state.tools.agent_spawns` already links an agent id to the launching turn and nobody reads it; the journal parser drops the `agentId` of `failed` entries.

> Found while implementing US-003 (2026-09-16, 505 transcripts on the same machine; `harness_facts::task_notification`):
> - **A notification is delivered three ways**, all with the same text: as a `user` line when the model is idle, and when it is busy as a `queue-operation` `enqueue` (its `content`) followed by a `queued_command` attachment (its `prompt`) or, after a `dequeue`, the user line. Every `killed` agent notification on this machine came as an attachment, none as a user line — a parser that reads user lines alone never sees one. `Line::task_notification()` reads all three; `State` keys by `<task-id>` so a repeat overwrites.
> - **Background shell commands' notifications do carry `<tool-use-id>`** (it names the `Bash` call); what tells them from an agent's is the id: an agent's `<task-id>` is its 17-hex agent id, a shell task's 9 base-36 characters. `State` takes a notification as an agent's when the id is a known agent's, the tool_use was an `Agent` call, or (no launch seen) the id has the agent shape.
> - **No `failed` agent notification exists on this machine** (the 25 of v1.1 were shell tasks and tool-result text); fixture C carries `completed` and `killed` agents and a shell `failed`; the `failed` reason is pinned on synthetic lines. `<result>` is present on a `killed` notification too (the partial result, 165 chars on the fixture).

---

## 1. Introduction

Every subagent transcript under `<session>/subagents/**` is already parsed: tokens, model, state, elapsed, tool calls per agent (`src/agents.rs`). The money is not. Panel 2's headline cost is `CostTracker::current()`: Claude Code's last `cost-state` plus a priced estimate of the main-transcript responses newer than that line. The subagents' share appears only as a dim `agents $X (N %)` fragment on Panel 2's last line, is never added to the headline, the burn rate, the per-turn figure or the baseline multipliers, and `cctop query summary` / `dashboard` export the same `cost`. Panel 6 shows one row per agent with tokens and no dollars; `cctop query agents` has no cost field at all.

At the scale the user runs (20 subagents, or a workflow run of 41 agents that spent $118 against $10 in the main thread, coach PRD §3.1), the number a person reads as "what this session cost" can be an order of magnitude low **while the session runs**, and the panel that lists the agents gives no way to tell which of them earned its money.

Three facts found while checking v1.0 decide the design:

- **Claude Code's `cost-state` already contains the subagents' calls** — and calls that appear in no transcript at all (haiku side calls: 1.5 M input tokens on one session). But it is written only at session end (after `last-prompt`) or at a `bridge-session`, never periodically, and it carries no timestamp. So: a live session has no ledger, and everything cctop shows is priced; a resumed session has a ledger for its earlier life and prices the rest. The combined figure is `ledger + priced(main after the ledger) + priced(agent calls after the ledger)`, one formula, no branch.
- **Subagent results come back as `<task-notification>` lines.** The `Agent` tool's `tool_result` is a launch notice (`toolUseResult.status = "async_launched"`); what the agent produced, whether it `completed` / `failed` / was `killed`, the length of its `<result>`, and Claude Code's own `<subagent_tokens>` / `<tool_uses>` / `<duration_ms>` arrive later in a user line with `origin.kind = "task-notification"`, keyed by `<task-id>` (the agent id) and `<tool-use-id>`. cctop already classifies that line as a machine turn (`PromptKind::TaskNotification`) and parses nothing inside it.
- **A fork's transcript starts with the parent's own launching response.** Its first assistant message (the `Agent` tool_use whose id is `Meta.tool_use_id`) has the same `message.id` as a line in the parent transcript and was billed there; the fork's own calls start with the second message. `Agent::push` counts the first one today — on fixture A that is 1 227 of the fork's 1 415 "output" tokens.

## 2. Goals

- **One number for the session's money**, on Panel 2, dashboard row 2, `cctop query`, the MCP tool, `cctop report` and the pane, with the same provenance rules everywhere: exact where Claude Code wrote it, `≈` where cctop priced it, and never double counted.
- **A per-agent ledger** that answers, at 20 or 40 agents, which agents cost what and which of that was wasted.
- **Wasted spend as a first-class metric**, computed only from structural evidence: the task notification's status and result length, the workflow journal, the agent's state, the hook spool, the cache profile of its calls. No prompt text, no description text, no model calls.
- **No change to the rest of cctop's meaning.** The coach rules, baselines and the cost gradient keep their inputs unless a story below names them.

## 3. Findings

Everything below is from the code on `main` at v0.3.1, fixtures A and B, and the user's `~/.claude` (Claude Code 2.1.247 – 2.1.272 transcripts).

### 3.1 What the code does today

| Fact | Where | Consequence |
|---|---|---|
| Headline cost = `cost-state.totalCostUSD` + priced main responses since that line; `since`/`seen` are cleared on every cost-state | `metrics/cost.rs` `CostTracker::{push, current}` | With no cost-state (every live session until it ends) the figure is priced main-only; agents are never in it |
| `Cost { usd, approx }` — no source | `metrics/cost.rs` | The provenance model of §4.1 adds one |
| `agents_cost()` = Σ `pricing.estimate(a.usage, a.model)` over `state.agents`; share = usd / (`cost.current()` + usd) | `ui/state.rs` `agents_cost` | One figure, not in the headline; the share is understated once a cost-state exists (the denominator already contains the agents); `None` when no agent model is priceable |
| `a` on Panel 2 toggles `tokens_include_agents` for the token bars (`Tokens::usage`) only | `ui/panels/tokens.rs` | Cost lines ignore the toggle |
| Burn rate `$/h` and `in tok/min` come from `agg.turns` in a 15-minute window, priced per turn | `metrics/cost.rs` `rates` | Main-only; agents have no turn membership — but see `agent_spawns` below |
| `×N$` = `cost.current().usd ÷ human turns` against `baseline.cost_per_turn`; the baseline's per-session cost is **`cost-state.totalCostUSD ÷ human turns`** | `ui/panels/tokens.rs` line 7, `baseline.rs` `figures` | The baseline is combined (the ledger contains the agents, §3.2); the live figure is main-only. Today's multiplier is biased low in sessions with agents. `×N.Nt` compares main tokens with main tokens and is consistent |
| Overlays: `state.overlay = Some(panel id)`; the owning panel's `has_overlay` / `render_overlay` / `handle_key`; the ledger view is `ui/ledger_view.rs` (`open`, `handle_key`, `render`) over pure rows from `src/ledger.rs` (`ledger::sorted`, `Sort`) with its UI state in `State.ledger_ui: LedgerUi` | `ui/panel.rs`, `ui/panels/context.rs`, `ui/panels/tokens.rs` | `ContextView` (`Ledger` / `Prefix`) is Panel 1's sub-view selector, not a view registry. The agents view is a new overlay owned by Panel 6, which has no `handle_key` today |
| Panel 6 lists agents newest-first: glyph, type, description (28 cols), elapsed, tokens, model family, `↰N` | `ui/panels/agents.rs` `render` | No dollars, no sort, no total; a fixed-height panel overflows at 20 agents |
| `IDLE_MS` (5 min) in Panel 6 is the **MCP server** idle threshold | `ui/panels/agents.rs` | Reusable as the agent idle threshold; not an agent constant today |
| `Agent::state`: Done when no pending tool_use and the last response ended with text / `end_turn`; Failed when the last result was an error and nothing followed for `FAILED_AFTER_MS` (60 s); else Running | `agents.rs` | A heuristic; the task notification's `<status>` is the authoritative signal and is not read |
| `Agent::new` drops `Meta.tool_use_id`; `Agent` has no field for it | `agents.rs` | The link to the launching call must be added |
| `Agent::push` adds a message's usage on its first line (`agg.api_calls()` grew) and ignores the later lines of the same id | `agents.rs` `push` | Correct for main transcripts, wrong for subagent ones (§3.2): output is under-counted |
| `workflow_journals` counts `launched` / `started` / `result` / `failed` per run; a `failed` entry carries `agentId` (the test fixture writes one) and the parser drops it | `agents.rs` `workflow_journals` | Per-agent `failed` from the journal needs the id kept |
| `state.tools.agent_spawns: Vec<AgentSpawn { at, turn, agent_id, agent_type, resolved_model, tool_uses, is_async, tokens }>` is filled from every `Agent` tool result and **read by nothing** | `tools.rs` | The main-transcript side of the link exists: agent id → launching turn. It lacks the `tool_use_id` |
| `ToolUseDetail::Agent(AgentResult)` parses `toolUseResult` with `status`, `agent_id`, `is_async`, `usage` (synchronous completions only), `result_chars` | `transcript/tool_result.rs` | On this machine's corpus every `Agent` result is `async_launched` (9) or `teammate_spawned` (18) or an older plain-string result (2); no synchronous completion with `usage` was seen. `result_chars` of a launch notice is not what came back |
| `<task-notification>` lines are classified as machine turns (`PromptKind::TaskNotification`, coach A27) and nothing inside them is parsed | `transcript/mod.rs`, `metrics/usage.rs` | The structural fields below need a parser |
| Hook spool: any event with `agent_id` (except `SubagentStart`/`Stop`) updates the agent's `started_at` / `last_line_at`; `PostToolUse` / `PostToolUseFailure` add exact tool time and errors | `ui/state.rs` `apply_hook`, `agents.rs` `note_hook` | `PreToolUse` per agent is seen but not counted; pending = pre − post is one line of code |
| Cost is drawn on: Panel 2 l6 + l10; dashboard row 2 detail (`≈$X`, `$/h`, `agents $X (N %)`) and row 6; `query summary` / `dashboard` (`cost`) / `agents`; MCP; the pane; `cctop report` (`## Cost`); the coach's limits light `lines` (`agents ≈$X · N ran · N failed`) and header cell (`agents N run · N %`) | `dashboard.rs`, `query.rs`, `report.rs`, `coach.rs` | All of these are "the session's cost" surfaces; there is no header tile and no cost light |
| Registry: `agents_cost` (Panel 2, tagged D6 + D9 — D6 is `tasks/`, the tag is wrong; D2a is the subagent transcripts), `agent_tokens` (Panel 6, D2a) | `metrics/registry.rs` | New ids go beside them; fix the tag; `docs/metrics.md` regenerates |
| Rule ids run to A47; LATER rules are ordered in `ORDER_LATER`; a rule has `id`, `family`, `urgency`, `evaluate`, `acted`, `ttl`, `cooldown_turns` | `advisor/mod.rs`, `advisor/rules/token.rs` (A14 `subagent-model` is the nearest sibling) | The new rule is A48, family `agents-waste`, listed in `ORDER_LATER` |
| The ledger view's snapshot tests render at 120 × 30; the coach view's at 56 × 20 | `ui/ledger_view.rs`, `coach.rs` tests | The agents view uses the same two sizes |

### 3.2 What Claude Code writes (measured)

| Fact | Evidence | Consequence |
|---|---|---|
| **`cost-state.modelUsage` ≥ main + subagents on every session; it matches main + agents where the agents dominate** | 8 sessions with a cost-state and subagent usage. A 336-agent workflow session (fable): cache-read 166 436 106 in the ledger vs 31 434 433 main vs 166 115 416 main + agents (0.2 % off); cache-creation 19 701 117 vs 4 397 886 vs 19 848 266 (0.7 %). Never a session where the ledger was below main + agents | The ledger is the session's whole spend to the moment it was written. Adding priced agents on top of it double counts |
| **The ledger also holds calls no transcript shows** | The same session: 1 528 881 haiku input tokens and 152 134 haiku output in the ledger, 10 / 56 across every transcript. Every session's ledger `inputTokens` is 20–100× the transcript's | `ledger − priced(main)` is **not** the agents' share; never derive `main` or `agents` by subtraction |
| **`cost-state` is written at session end or on a bridge, never periodically, and has no timestamp** | 115 cost-states in 79 sessions: 68 are the file's last line (after `last-prompt`, `turn_duration`, `stop_hook_summary`); 26 follow `bridge-session`; 1–5 per session; none carry `timestamp`. The nearest timestamped line is a `system` line 1–5 lines above | A live session runs on priced figures until it ends; a resumed one has a ledger for its earlier life. The ledger's moment is the last timestamped main line before it |
| **`Agent` launches are asynchronous; the result is a task notification** | `toolUseResult.status = "async_launched"`, `isAsync: true`, `agentId`, `outputFile`. Later, a `user` line with `promptSource: "system"`, `origin.kind: "task-notification"` and content `<task-notification><task-id>…</task-id><tool-use-id>…</tool-use-id><status>…</status><usage><subagent_tokens>N</subagent_tokens><tool_uses>N</tool_uses><duration_ms>N</duration_ms></usage><output-file>…</output-file><summary>…</summary><result>…</result><note>…</note></task-notification>` | `<status>` ∈ {`completed` 294, `failed` 25, `killed` 9} across 387 notifications; `<result>` present in 103, `<usage>` in 100 (agent notifications; the rest are background shell tasks, which have a `<task-id>` and no `<tool-use-id>`); `<usage>` is optional on agent notifications of every version seen. Workflow notifications carry `<event>`, `<agent_count>`, `<agents_done>`, `<agents_error>`, `<agents_skipped>`, `<agents_empty_result>`, `<failures>` instead |
| **A fork's first assistant message is the parent's** | Both live forks: exactly the first distinct `message.id` of the fork transcript exists in the parent; the message carries the `Agent` tool_use whose id is `Meta.tool_use_id`; every later id is the fork's own. The fork's first own call reads the parent's cache (62 690 cache-read against a 62 641 parent context) | Skip the first assistant message of any transcript that began with `fork-context-ref`. The fork is warm from its first own call |
| **A subagent message's lines carry a growing `output_tokens`; the last line is complete** | 357 agent transcripts, 2 292 messages: 1 354 have lines that disagree, only on `output_tokens`, and the last line is the maximum every time; the first line is 3 234 102 output tokens short in total. 141 main transcripts, 9 028 messages: no line ever disagrees | `Agent::push` must keep the **last** usage per `message.id` (`Aggregate` for the main transcript can stay as it is; a test pins both behaviours) |
| **`fork-context-ref.contextLength` is not tokens** | 32 and 789 against first-call cache reads of 62 690 and 279 924 (and 44 / 748 parent lines) | Panel 6's `↰32` is mislabelled today; the agents view shows the inherited context as the fork's first own `cache_read` (side fix, §11) |
| **Fixture A cannot exercise the link** | `fixtures/session-a/subagents/agent-a9a92645226d3a561` carries message ids (`msg_011Cef8…`) that post-date every main-transcript id (`msg_011CeT1…`); its `toolUseId` and task notification are absent from `session-a.jsonl`; its `sessionId` differs. The real launching call (line 46 of a 224-line session in another project) is `async_launched`, and the notification (line 54) reports `completed`, `<subagent_tokens>75218`, `<tool_uses>6`, a 2 069-char `<result>` | Fixture A's fork is a `state == Done`, notification-absent agent — which is why "absent" must not mean `no return` (§4.4). A fixture with the full path is composed in US-003 |

## 4. Design

### 4.1 Provenance model

Every dollar figure on every surface carries one of three sources, and the surfaces show them the same way:

| Source | Meaning | Mark |
|---|---|---|
| `ledger` | Claude Code's own `cost-state` (`totalCostUSD`, `modelUsage[*].costUSD`): main, subagents and hidden calls, up to the moment it was written | none |
| `priced` | cctop's estimate: usage × `pricing.toml` | `≈` |
| `unpriced` | usage on a model the table does not know | `—` and the token count |

The mark of a sum is the worst of its parts. `Cost` gains `source: Source` (`Ledger` / `Priced` / `Mixed` / `Unpriced`); `cctop query` already tags every value with `approx` and this PRD adds `source` to the cost values it introduces so the pane and the MCP tool can say *why* a number is approximate (FR-3).

### 4.2 The combined figure

```
combined = ledger.totalCostUSD                 (if a cost-state exists; else 0)
         + priced(main responses after the ledger)     — what CostTracker::since already holds
         + priced(agent calls after the ledger's moment)
```

- **The ledger's moment** (`CostTracker::authoritative_at_ms`) is the timestamp of the last timestamped main line seen before the `cost-state` was pushed (`State::apply` already tracks the last line time). An agent call counts as "after" when its own line timestamp is later. Calls in the seconds between that line and the write are counted twice; that window is the mark's honest reason and is not corrected.
- With no cost-state: `combined = priced(main) + priced(agents)`, every part `≈`. With no agents: `combined` is exactly today's figure, mark included (FR-6).
- **Forks**: `Agent::push` skips the first assistant message of a transcript that began with `fork-context-ref` (by `message.id`, so its several lines are all skipped). `agents_usage`, `agents_cost`, `behaviour_flags` and the A14 `subagent-model` rule inherit the fix; the registry text of `agent_tokens` says so.
- **Agents' share** on the breakdown line is `priced(all agent calls) ÷ combined` — the whole session's agent spend, including the part inside the ledger — so the percentage answers "how much of it went to agents" whether or not a ledger exists.

Panel 2, line 6 today: `cache hit 98 % · $9.90 ($3.10/h) · in 12k/min`. It becomes, on a live session (no cost-state yet):

```
 cache hit 98 %  ·  ≈$10.05 ($3.10/h main)  ·  in 12k/min
 main ≈$9.90 · agents ≈$0.15 (2 %)
```

and on a resumed session (a ledger, then more work):

```
 cache hit 98 %  ·  ≈$41.20 ($3.10/h main)  ·  in 12k/min
 ledger $38.10 · since ≈$3.10 · agents ≈$4.80 (12 %)
```

- The breakdown line is new and replaces the `agents $X (N %)` fragment on line 10; the `where:` attribution and the `weight ×N` stay on line 10. `main ≈$` is printed only when there is no ledger (it is then a real figure, not a subtraction); with a ledger the line names the ledger and `since` — everything priced after it, main and agents together — so the first two numbers always add up to the headline, and `agents` is the whole session's agent spend as a share of it.
- `a` toggles agents out of both the token bars and the cost lines; with `a` off the headline is `ledger + since` (today's figure) and the existing `main only` marker on line 7 covers both. The toast text is unchanged.
- `$/h` stays main-only and gets a dim `main` suffix when any agent has usage; `last turn $` is unchanged. Giving agents a turn membership is a follow-up — `agent_spawns[].turn` already knows the launching turn, so it is a small one (§11).
- `×N$` divides the **combined** figure by human turns, because the 7-day baseline is `cost-state ÷ turns` and the ledger is combined (§3.2). This corrects today's bias; the docstring on both sides says why. `×N.Nt` is unchanged.
- Dashboard row 2's detail (the pane's Overview and the TUI dashboard) shows the combined figure with the same mark, then `agents ≈$X (N %)`; `cctop report`'s `## Cost` and the coach's limits-light lines follow.

### 4.3 The agents view (from Panel 6, `Enter`)

An overlay in the pattern of the ledger view: rows come from a pure module `src/agent_ledger.rs` (`agent_ledger::rows(state, sort) -> Vec<AgentRow>`, `Sort`, `Totals`), the view is `ui/agents_view.rs` (`open`, `handle_key`, `render`), its UI state is `State.agents_ui: AgentsUi { sort, ascending, selected, expanded: BTreeSet<String> }`. Panel 6 gains `has_overlay`, `render_overlay` and a `handle_key` that opens it on `Enter` (`state.overlay = Some(6)`) and delegates while open; `Esc` closes it as the App does for every overlay. `j`/`k` scroll, `s` cycles the sort (spend › waste › elapsed › started), `S` flips it. Fixed column widths cut by `fmt::clip`, 60 columns of content:

```
 Agents ─ 12 · 3 running · ≈$4.82 · wasted ≈$1.94 (40 %)   [s]ort
 ─────────────────────────────────────────────────────────
   type      model    time    tok   ≈$     ret   waste
 ✗ Explore   haiku    1m 02  212k  0.19     —   0.19 failed
 ◐ claude    opus    12m 40  1.9M  1.41   ...   1.11 idle 6m
 ✓ Explore   haiku      44s  180k  0.16   517   0.00
 ✓ general   opus     3m 10  620k  0.92   2.1k  0.00
 ✓ Explore   haiku      51s  201k  0.18     0   0.18 no ret
 ✓ fork      sonnet     42s  548k  0.15   517   0.00  ↰63k
 ─────────────────────────────────────────────────────────
 wf wf_abc-123   9 launched · 7 done · 2 failed · 1 empty   ≈$2.40
 ─────────────────────────────────────────────────────────
 waste: failed 0.19 · killed 0.00 · no return 0.18 · idle 1.11
 cold starts 4 (≈$0.61 of cache writes) · 0.8 % of output returned
```

Columns: state glyph (as Panel 6, but from the notification's `<status>` when one arrived: `✗` for `failed` *and* `killed`), agent type, model family, elapsed, tokens (fork-skip applied), priced cost, **ret** (the `<result>` length ÷ 4 for a notified agent, `result_chars ÷ 4` for a synchronous one, `—` until a result exists, `0` for an empty one), **waste** (dollars with the reason word), `↰` the fork's inherited context (its first own call's cache read). Workflow runs fold into one group row with the journal's counts, the notification's `agents_empty_result` when one arrived, and a subtotal; `Enter` on the row expands it. The footer sums waste by reason and gives, for this session, the two ratios the coach PRD measured across the corpus (cold starts, share of output returned).

### 4.4 Wasted spend — definitions

All are computed per agent from `State` and are deterministic. A dollar of spend is counted under at most one reason, tested in this order:

| Reason | Evidence | Amount |
|---|---|---|
| `failed` | the agent's task notification says `<status>failed</status>`; **or** the workflow journal has a `failed` entry with this agent's id; **or** (no notification yet) `Agent::state == Failed` — the 60 s heuristic, re-classified when a notification lands | the agent's whole priced cost |
| `killed` | `<status>killed</status>` (a `TaskStop`, or the session ended under it) | the whole priced cost |
| `no return` | `<status>completed</status>` with `<result>` absent or empty; or a synchronous `AgentResult` with `result_chars == 0` | the whole priced cost |
| `idle` | no notification, `Agent::state == Running`, `last_line_at` older than `IDLE_MS` (5 min, shared with the MCP rows), and no hook `PreToolUse` for the agent without its `PostToolUse` (a 10-minute build is not idle) | the priced cost so far, labelled `idle <age>`; re-classified when the agent moves or a notification lands |
| — | everything else, including a `Done` agent whose notification has not arrived (fixture A's fork; a notification that was cut with the transcript) | 0 |

Two more figures are shown but not counted as waste, because they are costs of the design rather than of a failure: **cold start** (an agent that is not a fork whose first call has `cache_write > cache_read`: the cache-write dollars of that call) and **return ratio** (Σ `ret` ÷ Σ agent output tokens).

Nothing here reads prompt, description, summary or result text: lengths, enum values and counts only (FR-4, FR-5). v1.0's `dup` reason (same type and description hash) is dropped from v1: it is the only reason without a Claude Code signal, and a relaunch after a `failed` agent is the fix, not the waste. It stays in §11 with a measurement first.

### 4.5 `cctop query agents`, dashboard row 6, pane

- Each agent entry gains `cost` (USD, `approx`, `source`), `returned_tokens` (or `null`), `status` (the notification's, or `null`), `waste` (`{usd, reason}` or `null`), `cold_start` (bool), `launched_turn` (from `agent_spawns`), and for forks `inherited_context` becomes the first own call's cache read (the old `contextLength` value moves to `fork_context_messages`). The object gains `totals` (`cost`, `waste`, `waste_by_reason`, `returned_ratio`, `cold_starts`).
- `cctop query summary` and `dashboard` gain `cost_combined` (`metric_id: cost_combined`, `approx`, `source`) beside `cost`, which keeps today's meaning (`ledger + since`) for one release; `docs/query.md` and the README note the rename plan. Dashboard row 2's detail reads `cost_combined`.
- Dashboard row 6 sorts its top 3 by waste, then spend, and appends `wasted ≈$X` when non-zero, inside `ROW_WIDTH`, so the pane's Overview and the TUI stay row-identical (`tests/pane`).
- The pane's Agents view draws the new `query agents` fields in the column order of §4.3; no new engine calls.

## 5. User stories

### US-001: The ledger's moment, the fork skip, and the facts
**Description:** As the implementer, I need the combined figure to be one formula that never counts a call twice, with the facts it rests on recorded where cctop keeps them.

**Acceptance Criteria:**
- [ ] `scripts/ledger-vs-agents.py` (read-only, counts only) is committed: for every session under `~/.claude/projects` with a `cost-state` and subagent usage it compares `modelUsage[*]` per model with the deduplicated main sums and main + subagent sums and prints the table of §3.2. Its output on this machine is summarised in `harness_facts.rs`.
- [ ] `harness_facts.rs` gains a `cost_state` module: `INCLUDES_SUBAGENTS: bool = true`, `INCLUDES_UNTRANSCRIBED_CALLS: bool = true`, `WRITTEN_AT: &str = "session end (after last-prompt) or bridge-session; no timestamp"`, and a `task_notification` module noting that `<usage>` is optional on agent notifications of every version seen (2.1.231 – 2.1.270; 18 with, 31 without, both in 2.1.269) and that `<status>` is one of `completed` / `failed` / `killed`; `READ_FROM` is bumped to the `claude` the check ran on.
- [ ] `CostTracker` records `authoritative_at_ms` (the last line timestamp seen before the cost-state) and gains `combined(&self, agents: impl Iterator<Item = &Agent>) -> Option<Cost>` implementing §4.2 with `source`; unit tests: no ledger; ledger with agent calls before, after and straddling its moment; unpriced agent model; no agents (equals `current()` exactly).
- [ ] `Agent::push` keeps the last usage per `message.id` (a later line of the same message replaces the earlier one; `api_calls` still counts ids) and skips the first assistant `message.id` after a `fork-context-ref` line; the fixture A test's hand counts change (7 own calls, 304 output tokens: 188 by first line, 304 by last) and say why; `agents_usage` / `agents_cost` / `behaviour_flags` / A14 follow without changes of their own.

### US-002: Combined headline on Panel 2, dashboard row 2, query, report and the pane
**Description:** As a user, I want the cost I see first to be the session's whole spend, so that 20 subagents cannot hide $100 while the session runs.

**Acceptance Criteria:**
- [ ] Panel 2 line 6 shows the combined figure with its mark; the breakdown line of §4.2 renders under it in both its forms (no ledger / ledger); line 7's `main only` covers cost when `a` is off; the `agents $X (N %)` fragment leaves line 10.
- [ ] `$/h` carries a dim `main` suffix when any agent has usage; `×N$` divides the combined figure and its docstring cites the baseline's definition; `×N.Nt` is unchanged.
- [ ] `cctop query summary` / `dashboard` export `cost_combined` with `metric_id`, `approx`, `source`; dashboard row 2's detail, `cctop report`'s `## Cost`, the coach's limits-light lines and header cell read it; `docs/metrics.md` regenerates with `cost_combined` (Panel 2, D2 + D2a + D9 + D11, `≈ when any priced part is non-zero`).
- [ ] Insta snapshots for Panel 2 and the dashboard on fixtures A and B are updated and reviewed; `tests/pane` fixtures regenerated via `scripts/pane-fixtures.sh`.
- [ ] With no subagents, the rendered characters of Panel 2, the dashboard and `cctop query summary` are identical to v0.3.1 except for the added `cost_combined` key (FR-6).

### US-003: Per-agent cost, return and waste in `State`
**Description:** As every surface, I need one place that prices an agent, knows what came back and classifies its waste, so that the TUI, query and pane cannot disagree.

**Acceptance Criteria:**
- [ ] `transcript/task_notification.rs`: `TaskNotification { task_id, tool_use_id, status: TaskStatus (Completed / Failed / Killed / Other), result_chars, summary_chars, subagent_tokens, tool_uses, duration_ms }` and `WorkflowNotification { agent_count, done, error, skipped, empty_result }`, parsed from a `PromptKind::TaskNotification` line's content by element name; the text is never kept. Unit tests on synthetic lines for each status and for a background-shell notification (no `<usage>`, ignored).
- [ ] `AgentSpawn` gains `tool_use_id`; `Agent` gains `tool_use_id: Option<String>` (from meta, or from the spawn by `agent_id`), `launched_turn: Option<usize>`, `notified: Option<TaskNotification>`, `sync_result_chars: Option<usize>`, `first_own_call: Option<Usage>`; `State::apply` routes a notification to `state.agents[task_id]` (creating a placeholder when the transcript is not there yet, the way hooks do) and a synchronous `AgentResult` by `agent_id`.
- [ ] `workflow_journals` keeps the `agentId` of `failed` entries (`WorkflowJournal.failed_ids`).
- [ ] `src/agent_ledger.rs`: `rows(state, sort)` and `totals(state)` implement §4.3 / §4.4; one unit test per reason on synthetic lines, one for the order (a killed agent with an empty result is `killed`), one for "no notification, Done → 0", and the fixture A fork: 7 own calls, waste 0, not a cold start, `ret` `—`.
- [ ] `AgentWatcher::poll` and `agents::load` populate the new fields identically: a test loads fixture A both ways and compares.
- [ ] A fixture with the full path: `fixtures/session-c.jsonl` composed with `scripts/compose-fixture.py --shared-shift` from the 224-line session on this machine whose fork fixture A already ships (its `async_launched` call at line 46, the `<task-notification>` at line 54 — `completed`, with `<usage>` and a `<result>` — and `agent-a9a92645226d3a561` under `fixtures/session-c/subagents/`), plus one spliced `failed` notification and one `killed` from other sessions; the anonymiser keeps the notification's element names and numbers and replaces its text. Its test asserts `ret`, `status` and the three waste rows. (The team PRD's fixture is `session-d`.)
- [ ] Registry ids added: `agent_cost` (Panel 6, D2a + D9, `≈`), `agent_returned` (D2: the notification's `<result>` length ÷ 4), `agent_status` (D2: the notification's status), `agent_waste` (D2 + D2a + D4 + D9, `≈`), `agents_waste`, `agents_cold_starts`, `agents_return_ratio`; `agents_cost`'s source tag becomes D2a + D9.

### US-004: The agents view
**Description:** As a user with 20 agents, I want a sortable ledger of them with dollars and waste, so that I can see which ones I should not have launched.

**Acceptance Criteria:**
- [ ] `ui/agents_view.rs` + `State.agents_ui`; Panel 6 opens it on `Enter` (`overlay = Some(6)`), `Esc` closes, `j`/`k` scroll, `s`/`S` sort; `BINDINGS` and the help overlay updated (the help test passes).
- [ ] Renders §4.3 at 120 × 30 and 56 × 20 with fixed column widths cut by `fmt::clip`; workflow runs fold into a group row with the journal counts, `agents_empty_result` and a subtotal, `Enter` expands; the footer shows waste by reason, cold starts and the return ratio.
- [ ] Insta snapshots on fixture C and on a synthetic 25-agent state (built in the test from `Agent::from_lines` and synthetic notifications: two failed, one killed, one idle, one empty result, three cold starts, one workflow run) so the overflow and sort paths are covered.
- [ ] Panel 6's own rendering is unchanged except `↰` for forks, which shows the inherited context in tokens (§3.2).

### US-005: `cctop query agents`, dashboard row 6, the pane
**Description:** As the pane and the MCP tool, I want the same rows the TUI draws.

**Acceptance Criteria:**
- [ ] `query::agents` exports the fields of §4.5; `tests/pane/fixtures/agents.json` and `agents-c.json` regenerated; the pane's Agents view shows type, model, time, tokens, `≈$`, ret, waste in that order and its test asserts the columns against the fixture.
- [ ] Dashboard row 6 sorts by waste then spend and appends `wasted ≈$X`; `tests/pane` still asserts row-identical text on fixture B's moments and on fixture C.
- [ ] The MCP `agents` tool (`mcp.rs`) carries the same object; its schema text names the new fields; `docs/mcp.md` and `docs/query.md` updated.
- [ ] README's metrics block regenerated (`cctop metrics --readme`).

### US-006: The coach hears about waste
**Description:** As a user, I want the coach to say so once when agent waste crosses a threshold, so that the view is not the only place it is visible.

**Acceptance Criteria:**
- [ ] One LATER rule, id `A48`, family `agents-waste`, in `advisor/rules/token.rs` beside A14, listed in `ORDER_LATER`: fires when `agents_waste.usd ≥ max($1, 25 % of agent spend)` and at least two agents are classified; the headline names the top reason and the money, the action names the key (`6` then `Enter`); TTL three human turns; `acted` when the view was opened or the waste ratio drops below 10 %; cooldown 10 human turns; silent in `machine`, `workflow` and `loop` modes like every other rule (coach PRD FR-4).
- [ ] Replay on fixtures A and B: fires zero times (no notifications, no waste), asserted in `advisor` tests; on fixture C once. The rule count in CLAUDE.md's architecture line (35 → 36) and `docs/metrics.md` update.

## 6. Functional requirements

- FR-1: The combined cost must never count a call twice: the ledger is taken as it is, priced parts are only calls after the ledger's moment, and a fork's replayed first message is never priced.
- FR-2: Every cost value on every surface must carry `approx` and the `source` of §4.1; the mark of a sum is the worst of its parts; `main` and `agents` are never derived by subtracting from the ledger.
- FR-3: `cctop query` values introduced here must carry `metric_id`, `approx` and `source`; existing values keep their shape.
- FR-4: Waste classification must use only the evidence listed in §4.4: notification status and lengths, journal entries, `Agent::state`, hook pending counts, usage. No rule may read description, prompt, summary or result text.
- FR-5: An agent's `returned_tokens` must be a count over the `<result>` element (or the synchronous result content); the text is never stored (cctop's read-only, no-prose rule), and a background shell task's notification never creates an agent.
- FR-6: With no subagents, Panel 2, the dashboard, `cctop report` and `cctop query summary` must render character-identical to v0.3.1 except for the added `cost_combined` key.
- FR-7: The view must be usable at 20 rows with 25 agents (scroll, sort, group rows) and must not allocate per tick beyond the existing 2 s agent scan; the per-agent `Cost` is cached on the `Agent` and recomputed on `push`.
- FR-8: New metrics must exist in the registry with sources and caveats before they are drawn; `docs/metrics.md` and the README block regenerate; CI fails on drift.

## 7. Non-goals

- No turn membership for agents, so `$/h`, `last turn $` and the cost gradient stay main-only in this PRD (§11 has the small version).
- No naming of the ledger's hidden calls (`ledger − main − agents`) on any surface; the explain overlay for `cost_combined` says they exist.
- No reading of teammates' transcripts; that is `tasks/prd-cctop-team-costs.md`.
- No semantic judgement of whether an agent's *result* was useful; `ret` is a size.
- No changes to Panel 6's inline list beyond opening the view and the `↰` fix.
- No `dup` reason (§11).

## 8. Design considerations

- **One figure first.** The person reads the headline while typing; the breakdown line is there for the 2 % of the time they wonder where the money went.
- **Waste has a reason word.** A dollar figure without `failed` / `killed` / `no ret` / `idle` next to it would just be a second cost column.
- **Claude Code's word beats cctop's guess.** Where a notification exists its `<status>` sets the glyph and the reason; the 60 s heuristic is the fallback, not the rule.
- **Same glyphs, same clip widths** as Panel 6 and the ledger view, so the pane can mirror the rows.
- **Honest marks.** Every agent dollar is `≈` and the combined headline inherits it; the view header says `≈` once rather than per cell.

## 9. Technical considerations

- **Linking.** Three keys meet on the agent id: the transcript file name (`agent-<id>.jsonl`), `toolUseResult.agentId` on the launch (`agent_spawns`), and `<task-id>` on the notification. `Meta.tool_use_id` and `<tool-use-id>` are the second key, for a notification that lands before the transcript or the meta. `State` keeps a `tool_use_id → agent id` index built from spawns and metas, and a pending map for notifications that arrive first (`AgentWatcher::poll` already re-reads empty metas; a placeholder agent from a notification gets its meta the same way).
- **Ordering across files.** Agent line timestamps and main line timestamps are compared directly; both are Claude Code's UTC ISO strings. Fixture files are shifted per file by the anonymiser (fixture A's fork is a week off its main), so fixture C is composed with one shared shift (`compose-fixture.py --shared-shift`), which is also what the team PRD needs.
- **Cost of pricing.** `pricing.estimate` is a prefix match over a small table; 40 agents × 2 s tick is negligible. Cache the per-agent `Cost` on the `Agent`.
- **Rustfmt / clippy.** `agents.rs`, `state.rs` and `tools.rs` are large; fold formatting drift in.

## 10. Open questions

1. **Idle vs pending tool.** The hook spool's `PreToolUse` without `PostToolUse` per agent suppresses `idle`; check on a live session that the pairing holds for background Bash inside an agent (a `PostToolUse` may come only at the end).
2. **`<subagent_tokens>`.** Claude Code's own figure (75 218 on fixture A's real fork against cctop's 485 084 total or 74 447 last-call context) has no formula cctop can reproduce yet; the view does not show it until one is established. It could become the exactness cross-check for `agent_tokens`.
3. **Which agent notifications carry `<usage>`.** It is absent on 31 of 49 agent notifications on this machine, in the same versions that have it elsewhere; the parser treats it as optional and nothing in §4.4 depends on it. Whether its absence marks a kind of agent (forks, killed ones) is worth a look when the parser exists.

## 11. Follow-ups

- Turn membership for agents: `agent_spawns[].turn` names the launching turn; attribute an agent's spend to it so `$/h`, `last turn $` and the gradient become combined.
- `dup`: measure on the corpus how often two agents of the same type and description hash overlap and whether the second's result was empty before deciding a reason exists.
- `↰` on Panel 6: show the fork's inherited context as its first own call's cache read (a display fix independent of this PRD; can ship first).
- `tasks/prd-cctop-team-costs.md`: teammates.
