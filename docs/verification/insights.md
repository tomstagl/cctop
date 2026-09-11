# cctop-insights — verification transcript

Date: 2026-09-11 · binary: `cctop 0.1.0` (debug build) · session: `fixtures/session-a`
(an anonymised real session: 15 turns, 143 API calls, $9.90). The five example
questions from `plugin/skills/cctop-insights/SKILL.md`, the commands the skill
prescribes, the raw answers, and the reply a session would give — each with at
least one number and one action.

## 1. "Why is this session so expensive?"

```
$ cctop query summary | jq -c '{cost: .cost.value, burn: .burn_rate.value, turns: .turns.value}'
{"cost":9.903498600000011,"burn":25.99123573283813,"turns":15}
$ cctop query ledger --last 3 | jq -c '.[] | [.turn, .api_calls.value, .cost.value]'
[13,16,1.0167792]
[14,52,4.0666826]
[15,0,null]
```

Reply: *$9.90 over 15 turns, burning ≈ $26/h at the end. Turn 14 alone cost
$4.07 across 52 API calls — a long browser-automation loop (214 chrome MCP
calls in the session). Action: delegate that kind of loop to a subagent on a
cheaper model, or batch the browser steps.*

## 2. "Why is my cache hit ratio low?"

```
$ cctop query summary | jq -c '.tokens.cache_hit_ratio.value'
0.9822053232392264
$ cctop query advice | jq -c '[.[] | .rule]'
["A15"]
```

Reply: *It isn't — 98 % of input came from cache, and rules A01/A02 are not
firing. The only recommendation is A15: `Bash make check` failed 3× with the
same input. Action: interrupt the retry loop and give the fix in one prompt.*

## 3. "What is filling my context?"

```
$ cctop query prefix | jq -c '.rows[:2] | map({name, tokens: .tokens.value})'
[{"name":"system prompt + other","tokens":57726},{"name":"skills listing","tokens":1000}]
$ cctop query tools | jq -c '.top_ctx[0] | {tool, input, tokens: .tokens.value}'
{"tool":"Read","input":"/home/user/project/src/a3d75d…","tokens":1000}
```

Reply: *396k of a 1M window. The fixed prefix is 60k on every request
(57.7k system prompt + tool schemas, 1k skills listing); the largest single
tool result is a 1k-token Read. Action: the prefix, not tool output, is the
lever here — trim CLAUDE.md and disable unused MCP servers.*

(Fixture strings are capped at 4 000 bytes, so `top_ctx` tops out at 1k.)

## 4. "Will I hit the rate limit?"

```
$ cctop query summary | jq -c .limits
{"hint":"run cctop install","source":"missing"}
```

Reply: *I can't see rate limits for this session: the status-line shim is not
installed. Action: run `cctop install` (it shows a diff and backs up
settings.json); afterwards this answer includes used %, reset time and a
projection.*

## 5. "What does TOKENS→CTX mean?"

```
$ cctop query explain tokens_to_ctx | jq -c '{formula, caveats}'
{"formula":"Σ len(result text) / 4 per tool","caveats":"Heuristic; exact with OpenTelemetry. Uses the truncated text in the transcript, not offloaded `tool-results/` files"}
```

Reply: *The context each tool's results occupy: result text length ÷ 4,
summed per tool. It is a heuristic (exact with OpenTelemetry) and counts the
truncated text in the transcript, not offloaded result files. Action: sort the
Tools panel by it (`s`) to find the noisiest tool.*

## Notes

- `cctop query … | head` no longer panics on a closed pipe (SIGPIPE restored).
- Prefix cost of the skill: its one-line listing entry only; no query runs
  unless asked.
