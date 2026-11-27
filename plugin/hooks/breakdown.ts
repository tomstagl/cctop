// The context breakdown spool: what `$.session.usage({ breakdown: 'summary' })`
// says about the window, cut to what cctop reads (docs: PRD context
// breakdown §3.1). Pure: `SessionUsage` in, text out. The grid, the theme
// colours and the slash-command listing are dropped.
import type { SessionUsage } from 'claude-code';

/** The spool's schema version; Rust ignores one it does not know. */
export const SPOOL_VERSION = 1;

/** The spool's text (one JSON object and a newline) for a usage read at `at`, or null without a breakdown. */
export function spoolText(usage: SessionUsage, at: number): string | null {
  const b = usage.context.breakdown;
  if (b === undefined) return null;
  const spool = {
    v: SPOOL_VERSION,
    at,
    tokens: b.totalTokens,
    window: b.rawMaxTokens,
    model: b.model,
    categories: b.categories.map((c) => ({ name: c.name, tokens: c.tokens, kind: c.kind })),
    autocompact_threshold: b.autoCompactThreshold,
    is_autocompact_enabled: b.isAutoCompactEnabled,
    memory_files: b.memoryFiles.map((f) => ({ path: f.path, tokens: f.tokens })),
    mcp_tools: b.mcpTools.map((t) => ({ name: t.name, tokens: t.tokens })),
    // The skills the listing included: its tokens are theirs.
    skills: b.skills === undefined ? undefined : { count: b.skills.includedSkills, tokens: b.skills.tokens },
    agents: b.agents.map((a) => ({ name: a.agentType, tokens: a.tokens })),
  };
  return `${JSON.stringify(spool)}\n`;
}
