import { useEffect, useState } from "react";
import { useApp } from "../stores/app";

export interface Cooldown {
  /** True while Spotify API calls are paused. */
  active: boolean;
  until: number | null;
  /** Ceiling on the pause, e.g. "23 h 41 min"; it usually ends much sooner. */
  remaining: string;
  /** Full sentence for a disabled control's hover title. */
  reason: string;
}

function remainingText(until: number, nowSec: number): string {
  const diff = Math.max(0, until - nowSec);
  const h = Math.floor(diff / 3600);
  const m = Math.ceil((diff % 3600) / 60);
  if (h > 0) return `${h} h ${m} min`;
  return `${Math.max(1, m)} min`;
}

/** Live view of the quota pause; re-renders every 30 s while active. */
export function useQuotaCooldown(): Cooldown {
  const until = useApp((s) => s.quotaCooldownUntil);
  const [now, setNow] = useState(() => Math.floor(Date.now() / 1000));

  useEffect(() => {
    if (!until) return;
    const t = setInterval(() => setNow(Math.floor(Date.now() / 1000)), 30_000);
    return () => clearInterval(t);
  }, [until]);

  const active = until !== null && until > now;
  if (!active || until === null) return { active: false, until: null, remaining: "", reason: "" };
  const remaining = remainingText(until, now);
  return {
    active: true,
    until,
    remaining,
    reason: `Temporarily disabled: Spotify's API quota for this app is exhausted. Utilify checks once an hour and re-enables everything as soon as Spotify answers again (at most ${remaining}).`,
  };
}
