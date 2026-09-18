import type { TrackRow } from "../lib/api";
import { formatDuration } from "../lib/format";

export function trackKey(t: TrackRow) {
  return `${t.uri}|${t.position}`;
}

/** Compact selectable track table used by the tools. */
export function TrackList({
  tracks,
  selected,
  onToggle,
  emptyText = "No tracks.",
  maxHeight = "max-h-80",
  note,
}: {
  tracks: TrackRow[];
  selected?: Set<string>;
  onToggle?: (key: string) => void;
  emptyText?: string;
  maxHeight?: string;
  /** Optional per-track annotation (e.g. "different version"). */
  note?: (t: TrackRow) => string | null;
}) {
  if (tracks.length === 0) return <p className="px-3 py-3 text-sm text-muted">{emptyText}</p>;
  return (
    <ul className={`${maxHeight} divide-y divide-line overflow-auto`}>
      {tracks.map((t) => {
        const key = trackKey(t);
        const row = (
          <>
            {selected && onToggle && <input type="checkbox" checked={selected.has(key)} onChange={() => onToggle(key)} />}
            <span className="w-10 shrink-0 text-right text-xs text-muted">#{t.position + 1}</span>
            <span className="min-w-0 flex-1">
              <span className="block truncate">{t.name}</span>
              <span className="block truncate text-xs text-muted">
                {t.artists}
                {t.album ? ` · ${t.album}` : ""}
              </span>
            </span>
            {note && note(t) && <span className="shrink-0 text-xs text-amber-300">{note(t)}</span>}
            <span className="shrink-0 text-xs text-muted">{formatDuration(t.durationMs)}</span>
          </>
        );
        return (
          <li key={key}>
            {selected && onToggle ? (
              <label className="flex cursor-pointer items-center gap-3 px-3 py-1.5 text-sm hover:bg-panel-2">{row}</label>
            ) : (
              <div className="flex items-center gap-3 px-3 py-1.5 text-sm">{row}</div>
            )}
          </li>
        );
      })}
    </ul>
  );
}
