export function formatDuration(ms: number | null | undefined): string {
  if (ms == null) return "--:--";
  const total = Math.floor(ms / 1000);
  const m = Math.floor(total / 60);
  const s = total % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function formatRelative(unixSeconds: number): string {
  const diff = Math.floor(Date.now() / 1000) - unixSeconds;
  if (diff < 60) return "just now";
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} h ago`;
  return `${Math.floor(diff / 86400)} d ago`;
}

/** "in 3 h 20 min", "in 2 d 4 h", or "due now". */
export function formatUntil(unixSeconds: number): string {
  const diff = unixSeconds - Math.floor(Date.now() / 1000);
  if (diff <= 0) return "due now";
  const d = Math.floor(diff / 86400);
  const h = Math.floor((diff % 86400) / 3600);
  const m = Math.floor((diff % 3600) / 60);
  if (d > 0) return `in ${d} d ${h} h`;
  if (h > 0) return `in ${h} h ${m} min`;
  if (m > 0) return `in ${m} min`;
  return "in under a minute";
}

export function formatDateTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

/** "3 h 12 min", "45 min", "0 min". */
export function formatListening(ms: number): string {
  const min = Math.round(ms / 60000);
  const h = Math.floor(min / 60);
  const m = min % 60;
  if (h > 0) return `${h} h ${m} min`;
  return `${m} min`;
}

export function artistNames(artists: { name: string }[] | undefined): string {
  if (!artists || artists.length === 0) return "";
  return artists.map((a) => a.name).join(", ");
}

/** "1,811 h" for long spans, "3 h 12 min" below a hundred hours. */
export function formatListeningLong(ms: number): string {
  const hours = ms / 3600000;
  if (hours >= 100) return `${Math.round(hours).toLocaleString()} h`;
  return formatListening(ms);
}

/** Hours only, for axis ticks: "0", "12", "1.5k". */
export function formatHoursShort(ms: number): string {
  const hours = ms / 3600000;
  if (hours >= 1000) return `${(hours / 1000).toFixed(1)}k`;
  if (hours >= 10) return Math.round(hours).toString();
  if (hours >= 1) return hours.toFixed(1);
  if (hours === 0) return "0";
  return `${Math.round(hours * 60)}m`;
}

export function formatCount(n: number): string {
  return n.toLocaleString();
}

/** "Jan 2024" from the export's "2024-01" bucket label. */
export function formatMonthLabel(label: string): string {
  const [y, m] = label.split("-").map(Number);
  if (!y || !m) return label;
  return new Date(y, m - 1, 1).toLocaleString(undefined, { month: "short", year: "numeric" });
}

export function formatDate(unixSeconds: number): string {
  return new Date(unixSeconds * 1000).toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

/** "1 play", "2,453 plays". */
export function plural(n: number, singular: string, many?: string): string {
  return `${formatCount(n)} ${n === 1 ? singular : (many ?? `${singular}s`)}`;
}

/** "812 KB", "2.4 MB". */
export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
