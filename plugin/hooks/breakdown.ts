// The context breakdown spool: what `$.session.usage({ breakdown: 'summary' })`
// says about the window, cut to what cctop reads (docs: PRD context
// breakdown §3.1). Pure: `SessionUsage` in, text out. The grid, the theme
// colours and the slash-command listing are dropped.
import type { SessionUsage } from 'claude-code';

/** The spool's schema version; Rust ignores one it does not know. */
export const SPOOL_VERSION = 1;

// The host reports counts as numbers that may carry a fraction; the spool holds whole ones.
const whole = (n: number): number => Math.round(n);

/** The spool's text (one JSON object and a newline) for a usage read at `at`, or null without a breakdown. */
export function spoolText(usage: SessionUsage, at: number): string | null {
  const b = usage.context.breakdown;
  if (b === undefined) return null;
  const spool = {
    v: SPOOL_VERSION,
    at: whole(at),
    tokens: whole(b.totalTokens),
    window: whole(b.rawMaxTokens),
    model: b.model,
    categories: b.categories.map((c) => ({ name: c.name, tokens: whole(c.tokens), kind: c.kind })),
    autocompact_threshold: b.autoCompactThreshold === undefined ? undefined : whole(b.autoCompactThreshold),
    is_autocompact_enabled: b.isAutoCompactEnabled,
    memory_files: b.memoryFiles.map((f) => ({ path: f.path, tokens: whole(f.tokens) })),
    mcp_tools: b.mcpTools.map((t) => ({ name: t.name, tokens: whole(t.tokens) })),
    // The skills the listing included: its tokens are theirs.
    skills: b.skills === undefined ? undefined : { count: whole(b.skills.includedSkills), tokens: whole(b.skills.tokens) },
    agents: b.agents.map((a) => ({ name: a.agentType, tokens: whole(a.tokens) })),
  };
  return `${JSON.stringify(spool)}\n`;
}
