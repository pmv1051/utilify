export const BENCH_PRESETS: { label: string; secs: number }[] = [
  { label: "1 hour", secs: 3600 },
  { label: "12 hours", secs: 12 * 3600 },
  { label: "1 day", secs: 86400 },
  { label: "2 days", secs: 2 * 86400 },
  { label: "7 days", secs: 7 * 86400 },
  { label: "1 month", secs: 30 * 86400 },
];

/** Playlist id when a playback context is a playlist, else null. */
export function playlistIdFromContext(uri: string | null | undefined): string | null {
  if (!uri) return null;
  const m = /^spotify:playlist:([A-Za-z0-9]+)$/.exec(uri);
  return m ? m[1] : null;
}
