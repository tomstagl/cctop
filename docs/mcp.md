# `cctop mcp`

The query interface as MCP tools over stdio (JSON-RPC 2.0, protocol
`2024-11-05`). Each tool returns exactly the JSON its `cctop query`
counterpart prints, so `docs/query.md` describes the shapes.

| Tool | Equivalent |
|---|---|
| `cctop_summary` | `cctop query summary` |
| `cctop_ledger` (`last`) | `cctop query ledger --last N` |
| `cctop_tools` | `cctop query tools` |
| `cctop_agents` | `cctop query agents` |
| `cctop_advice` | `cctop query advice` |
| `cctop_coach` | `cctop query coach` |
| `cctop_prefix` | `cctop query prefix`, plus `cctop query sources` as the `sources` key — the whole window, both halves |
| `cctop_events` (`since`) | `cctop query events --since` |
| `cctop_explain_metric` (`metric_id`) | `cctop query explain <id>` |

All session tools take an optional `session` argument (id, name, pid or a
fixture path); without it the server attaches to the `claude` process that
spawned it (its parent pid), falling back to the same discovery the TUI uses.

## Cost of registering it

Measured after `cctop_agents` was added (0.4.0+; the team in 0.5.0): the nine tool schemas serialise to **3 143 bytes ≈ 785
tokens** (`cctop mcp` → `tools/list`; a unit test keeps it under 800). That rides on every request of any
session that has the server registered — which is exactly what Advisor rule
A07 warns about. So:

**Register it as a deferred server**, or prefer the `cctop-insights` skill
(zero prefix cost: it calls `cctop query` through Bash only when you ask).
The skill is the default; the MCP server is for clients and fleets that want
typed tools.

## `.mcp.json` snippet

```json
{
  "mcpServers": {
    "cctop": {
      "command": "cctop",
      "args": ["mcp"]
    }
  }
}
```

Claude Code loads MCP tool schemas lazily when tool search is on (the
`deferred_tools_delta` mechanism cctop itself reads), so with tool search
enabled the schemas cost their listing line rather than the full 614 tokens
until a tool is used.

## Verified

`src/mcp.rs` tests drive the server over an in-memory stdio: `initialize` →
`notifications/initialized` (silent) → `tools/list` (7 tools) → `tools/call`
for `cctop_summary`, `cctop_explain_metric`, `cctop_ledger`, an unknown tool
(`isError: true`), an unknown method (`-32601`) and a parse error (`-32700`).
