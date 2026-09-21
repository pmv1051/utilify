import { useEffect, useState } from "react";
import type { QuotaScope } from "./api";
import { useApp } from "../stores/app";

/**
 * Spotify's Development Mode quota runs out per family of endpoints. Artist
 * and album lookups can be exhausted while playlist calls still work, so a
 * control is only disabled when the family it uses is the one that is paused.
 */
export interface Cooldown {
  /** True while this family of calls is paused. */
  active: boolean;
  until: number | null;
  /** Time until the next attempt, e.g. "41 min". */
  remaining: string;
  /** Full sentence for a disabled control's hover title. */
  reason: string;
}

const IDLE: Cooldown = { active: false, until: null, remaining: "", reason: "" };

export const SCOPE_LABEL: Record<QuotaScope, string> = {
  catalog: "artist and album lookups",
  playlists: "playlist requests",
  player: "playback control",
};

function remainingText(until: number, nowSec: number): string {
  const diff = Math.max(0, until - nowSec);
  const h = Math.floor(diff / 3600);
  const m = Math.ceil((diff % 3600) / 60);
  if (h > 0) return `${h} h ${m} min`;
  return `${Math.max(1, m)} min`;
}

/** Ticks once a minute while anything is paused, so countdowns stay honest. */
function useNow(active: boolean): number {
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 30_000);
    return () => clearInterval(t);
  }, [active]);
  return now;
}

/** Live view of one family's pause. */
export function useQuotaCooldown(scope: QuotaScope = "playlists"): Cooldown {
  const until = useApp((s) => s.quotaScopes[scope] ?? null);
  const now = useNow(until !== null);

  if (until === null || until <= now) return IDLE;
  const remaining = remainingText(until, now);
  return {
    active: true,
    until,
    remaining,
    reason: `Temporarily disabled: Spotify's quota for ${SCOPE_LABEL[scope]} is exhausted. Utilify tries again in ${remaining}; everything else keeps working.`,
  };
}

/** Every family currently paused, for the sidebar banner and Settings. */
export function usePausedScopes(): { scope: QuotaScope; until: number; remaining: string }[] {
  const scopes = useApp((s) => s.quotaScopes);
  const entries = Object.entries(scopes) as [QuotaScope, number][];
  const now = useNow(entries.length > 0);
  return entries
    .filter(([, until]) => until > now)
    .sort((a, b) => a[1] - b[1])
    .map(([scope, until]) => ({ scope, until, remaining: remainingText(until, now) }));
}
