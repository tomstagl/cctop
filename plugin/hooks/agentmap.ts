// Which of cctop's agent rows is which of the engine's agents. A row's `id`
// is the id cctop reads from the transcript's own files; `$.ui.view` and
// `$.agent.list()` speak `AgentInfo.id`. The two are expected to be one id
// (the d.ts: "the agent's one id"), but cctop has never read it from the
// engine, so the match is exact first. A row the id does not find falls back
// on what the Agent call said of it (`description` and `type`) only when that
// cannot be a guess: the row is running, and its pair is unique among the
// running rows and among the live agents no exact match took. Neither list's
// order is spawn order (the query sorts by spend), so order never decides.
// Anything else gets no entry, and so no Button: a press would put some other
// agent's transcript on screen.

/** What a cctop agent row says of itself. */
export type AgentRow = { id: string; type?: string; description?: string; running: boolean };

/** What `$.agent.list()` says of one agent, as much as the mapping reads. */
export type LiveAgent = { id: string; type?: string; description?: string };

const pair = (x: { type?: string; description?: string }): string | null =>
  x.type === undefined || x.description === undefined ? null : JSON.stringify([x.type, x.description]);

/** cctop's agent id -> `AgentInfo.id`; never two rows to one live id, never a guess. */
export function mapAgents(rows: AgentRow[], live: LiveAgent[]): Map<string, string> {
  const out = new Map<string, string>();
  const taken = new Set<string>();
  const known = new Set(live.map((l) => l.id));
  for (const r of rows) {
    if (known.has(r.id) && !taken.has(r.id)) {
      out.set(r.id, r.id);
      taken.add(r.id);
    }
  }
  const count = (keys: (string | null)[]): Map<string, number> => {
    const n = new Map<string, number>();
    for (const k of keys) if (k !== null) n.set(k, (n.get(k) ?? 0) + 1);
    return n;
  };
  const running = rows.filter((r) => r.running);
  // Every row's pair, finished ones too: a finished row whose agent is still
  // listed must not leave its pair to a new running row.
  const rowPairs = count(rows.map(pair));
  const free = live.filter((l) => !taken.has(l.id));
  const livePairs = count(free.map(pair));
  for (const r of running) {
    const k = pair(r);
    if (k === null || out.has(r.id) || known.has(r.id)) continue;
    if (rowPairs.get(k) !== 1 || livePairs.get(k) !== 1) continue;
    const hit = free.find((l) => pair(l) === k);
    if (hit !== undefined) out.set(r.id, hit.id);
  }
  return out;
}
