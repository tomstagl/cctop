// Which of cctop's agent rows is which of the engine's agents. A row's `id`
// is the id cctop reads from the transcript's own files; `$.ui.view` and
// `$.agent.list()` speak `AgentInfo.id`. The two are expected to be one id
// (the d.ts: "the agent's one id"), but cctop has never read it from the
// engine, so the match is exact first, and a row the id does not find falls
// back on what the Agent call said of it (`description` and `type`), the
// oldest row taking the oldest live agent. A row with no match gets none, and
// so no Button: a press would put some other agent's transcript on screen.

/** What a cctop agent row says of itself; `order` is its place in spawn order. */
export type AgentRow = { id: string; type?: string; description?: string; order: number };

/** What `$.agent.list()` says of one agent, as much as the mapping reads. */
export type LiveAgent = { id: string; type?: string; description?: string };

/** cctop's agent id → `AgentInfo.id`; never two rows to one live id. */
export function mapAgents(rows: AgentRow[], live: LiveAgent[]): Map<string, string> {
  const out = new Map<string, string>();
  const taken = new Set<string>();
  const known = new Set(live.map((l) => l.id));
  for (const r of rows) {
    if (known.has(r.id) && !taken.has(r.id) && !out.has(r.id)) {
      out.set(r.id, r.id);
      taken.add(r.id);
    }
  }
  const rest = rows.filter((r) => !out.has(r.id) && r.description !== undefined && r.type !== undefined).sort((a, b) => a.order - b.order);
  for (const r of rest) {
    const hit = live.find((l) => !taken.has(l.id) && !known.has(r.id) && l.description === r.description && l.type === r.type);
    if (hit === undefined) continue;
    out.set(r.id, hit.id);
    taken.add(hit.id);
  }
  return out;
}
