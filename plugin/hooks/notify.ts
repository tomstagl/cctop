// Opt-in native notifications (CCTOP_PANE_NOTIFY=1): the bookkeeping, pure.
// Two moments are worth interrupting someone for: a NOW coach nudge taking
// the slot (once per fire, taken back when it is snoozed or acted on) and a
// rate-limit window that is nearly spent (one notification booked for the
// reset, delivered only to someone who left). The calls to `$.ui.notify` and
// `$.ui.cancelNotify` are made by pane.tsx from what these functions say.

export type NotifyState = {
  /** `<rule id>:<fire id>` of every NOW nudge notified, and `~<same>` once it was taken back. */
  sent: Set<string>;
  /** Window key (`reset-5h`, `reset-7d`) → the reset time (epoch ms) booked for it. */
  scheduledReset: Map<string, number>;
  /** A `disabled` or `no-channel` answer: every further call would say the same, so none is made. */
  disabled: boolean;
  /** Keys whose first result or failure was logged. */
  logged: Set<string>;
};

export function initialNotifyState(): NotifyState {
  return { sent: new Set(), scheduledReset: new Map(), disabled: false, logged: new Set() };
}

/** The text of a reset notification. */
export const RESET_TEXT = 'rate limit resets now';
/** Percent of a window used from which its reset is booked. */
export const RESET_AT_PCT = 90;
/** The d.ts: an `at` more than 7 days ahead rejects the call. */
const MAX_AHEAD_MS = 7 * 24 * 3600 * 1000;

export type NudgeInput = { ruleId: string; fireId: string; urgency: 'NOW' | 'NEXT' | 'LATER'; headline: string };

/** The key a nudge's notification is sent, replaced and cancelled under. */
export const nudgeKey = (ruleId: string): string => `nudge-${ruleId}`;

/**
 * What to notify for the nudge in the slot, and which notifications to take
 * back: those of `snoozedOrActed` rules that were sent and not yet cancelled.
 * A nudge that is snoozed or acted on is never notified.
 */
export function nudgeActions(
  s: NotifyState,
  nudge: NudgeInput | null,
  snoozedOrActed: string[],
): { notify: { text: string; key: string }[]; cancel: string[] } {
  const out: { notify: { text: string; key: string }[]; cancel: string[] } = { notify: [], cancel: [] };
  if (s.disabled) return out;
  for (const rule of snoozedOrActed) {
    for (const sent of [...s.sent]) {
      if (sent.startsWith('~') || !sent.startsWith(`${rule}:`) || s.sent.has(`~${sent}`)) continue;
      s.sent.add(`~${sent}`);
      if (!out.cancel.includes(nudgeKey(rule))) out.cancel.push(nudgeKey(rule));
    }
  }
  if (nudge !== null && nudge.urgency === 'NOW' && !snoozedOrActed.includes(nudge.ruleId)) {
    const sent = `${nudge.ruleId}:${nudge.fireId}`;
    if (!s.sent.has(sent)) {
      s.sent.add(sent);
      out.notify.push({ text: nudge.headline, key: nudgeKey(nudge.ruleId) });
    }
  }
  return out;
}

/**
 * The reset notifications to book now: a window at or above RESET_AT_PCT
 * whose reset is ahead (and within the 7 days the host accepts) and is not
 * the one already booked. A new `resets_at` replaces the booking (same key);
 * a window that fell below the threshold forgets its booking.
 */
export function resetActions(
  s: NotifyState,
  windows: { key: 'reset-5h' | 'reset-7d'; usedPct: number; resetsAtMs: number }[],
  nowMs: number = Date.now(),
): { text: string; key: string; at: number }[] {
  const out: { text: string; key: string; at: number }[] = [];
  if (s.disabled) return out;
  for (const w of windows) {
    if (w.usedPct < RESET_AT_PCT) {
      s.scheduledReset.delete(w.key);
      continue;
    }
    if (!(w.resetsAtMs > nowMs) || w.resetsAtMs > nowMs + MAX_AHEAD_MS) continue;
    if (s.scheduledReset.get(w.key) === w.resetsAtMs) continue;
    s.scheduledReset.set(w.key, w.resetsAtMs);
    out.push({ text: RESET_TEXT, key: w.key, at: w.resetsAtMs });
  }
  return out;
}

/** Reset bookings whose window has since fallen below the threshold: forgotten, and their keys returned so the caller takes them back. A rejected notify call is not retried (the failure is logged once; the next change books again). */
export function resetCancels(s: NotifyState, windows: { key: 'reset-5h' | 'reset-7d'; usedPct: number }[]): string[] {
  const keys: string[] = [];
  for (const w of windows) {
    if (w.usedPct < RESET_AT_PCT && s.scheduledReset.delete(w.key)) keys.push(w.key);
  }
  return keys;
}

/** `$.ui.notify`'s answer for `key`: stops all further calls on `disabled` / `no-channel`; returns the line to log the first time for this key, else null. */
export function noteResult(
  s: NotifyState,
  key: string,
  result: { isSent: true; channel: string } | { isSent: false; reason: string },
): string | null {
  if (!result.isSent && (result.reason === 'disabled' || result.reason === 'no-channel')) s.disabled = true;
  if (s.logged.has(key)) return null;
  s.logged.add(key);
  return result.isSent ? `cctop: notify ${key} sent (${result.channel})` : `cctop: notify ${key} not sent (${result.reason})`;
}

/** A rejected `$.ui.notify`: the line to log the first time for this key, else null. */
export function noteFailure(s: NotifyState, key: string, err: unknown): string | null {
  if (s.logged.has(key)) return null;
  s.logged.add(key);
  return `cctop: notify ${key} failed: ${err instanceof Error ? err.message : String(err)}`;
}

/** The rule ids of a raw `cctop query coach` object that are snoozed, or acted on since the fire in question: the current nudge's if it is that rule's, else the latest one notified. An older acted entry never settles a re-fire. */
export function settledRules(coach: unknown, s: NotifyState, current: { ruleId: string; fireId: string } | null = null): string[] {
  const rules = new Set<string>();
  if (coach === null || typeof coach !== 'object') return [];
  const snoozed = (coach as { snoozed?: unknown }).snoozed;
  if (Array.isArray(snoozed)) {
    for (const x of snoozed) {
      const id = (x as { id?: unknown } | null)?.id;
      if (typeof id === 'string') rules.add(id);
    }
  }
  const recent = (coach as { recent?: unknown }).recent;
  if (Array.isArray(recent)) {
    for (const x of recent) {
      const r = x as { what?: unknown; id?: unknown; at_ms?: unknown } | null;
      if (r?.what !== 'acted' || typeof r.id !== 'string' || typeof r.at_ms !== 'number') continue;
      let fire: number | null = current !== null && current.ruleId === r.id ? Number(current.fireId) : null;
      if (fire === null) {
        for (const sent of s.sent) {
          if (sent.startsWith(`${r.id}:`)) fire = Math.max(fire ?? -Infinity, Number(sent.slice(r.id.length + 1)));
        }
      }
      if (fire !== null && fire <= r.at_ms) rules.add(r.id);
    }
  }
  return [...rules];
}
