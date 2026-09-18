import { useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage, type EditorProgress, type TrackRow } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader, PlaylistSelect } from "../components/PlaylistPicker";
import { formatDuration } from "../lib/format";

type SortKey = "artist" | "album" | "name" | "added" | "duration";
const SORT_LABEL: Record<SortKey, string> = {
  artist: "Artist",
  album: "Album",
  name: "Track",
  added: "Date added",
  duration: "Duration",
};

function sortValue(t: TrackRow, key: SortKey): string | number {
  switch (key) {
    case "artist":
      return t.artists.toLowerCase();
    case "album":
      return (t.album ?? "").toLowerCase();
    case "name":
      return t.name.toLowerCase();
    case "added":
      return t.addedAt ?? "";
    case "duration":
      return t.durationMs ?? 0;
  }
}

export function EditorPage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);

  const [playlistId, setPlaylistId] = useState<string | null>(null);
  const [tracks, setTracks] = useState<TrackRow[]>([]);
  const [loading, setLoading] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<Set<number>>(new Set());
  const lastClicked = useRef<number | null>(null);

  const [afterQuery, setAfterQuery] = useState("");
  const [afterOpen, setAfterOpen] = useState(false);
  const [sortKey, setSortKey] = useState<SortKey>("artist");
  const [sortDesc, setSortDesc] = useState(false);

  const [applying, setApplying] = useState(false);
  const [progress, setProgress] = useState<EditorProgress | null>(null);

  async function load(id: string | null) {
    setSelected(new Set());
    setQuery("");
    if (!id) {
      setTracks([]);
      return;
    }
    setLoading(true);
    try {
      setTracks(await api.loadPlaylistForEditor(id));
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    load(playlistId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playlistId]);

  useEffect(() => {
    const un = listen<EditorProgress>("editor-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return tracks;
    return tracks.filter(
      (t) =>
        t.name.toLowerCase().includes(q) ||
        t.artists.toLowerCase().includes(q) ||
        (t.album ?? "").toLowerCase().includes(q),
    );
  }, [tracks, query]);

  const afterMatches = useMemo(() => {
    const q = afterQuery.trim().toLowerCase();
    if (!q) return [];
    return tracks
      .filter((t) => !selected.has(t.position))
      .filter((t) => t.name.toLowerCase().includes(q) || t.artists.toLowerCase().includes(q))
      .slice(0, 8);
  }, [tracks, afterQuery, selected]);

  function toggle(position: number, shift: boolean) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (shift && lastClicked.current !== null) {
        // Range select within the filtered view.
        const idxA = filtered.findIndex((t) => t.position === lastClicked.current);
        const idxB = filtered.findIndex((t) => t.position === position);
        if (idxA >= 0 && idxB >= 0) {
          const [lo, hi] = idxA < idxB ? [idxA, idxB] : [idxB, idxA];
          for (let i = lo; i <= hi; i++) next.add(filtered[i].position);
          return next;
        }
      }
      if (next.has(position)) next.delete(position);
      else next.add(position);
      return next;
    });
    lastClicked.current = position;
  }

  async function apply(target: number[], label: string) {
    if (!playlistId) return;
    // Skip no-ops.
    if (target.every((p, i) => p === i)) {
      toast("info", "Already in that order.");
      return;
    }
    setApplying(true);
    setProgress(null);
    try {
      const r = await api.applyPlaylistOrder(playlistId, target);
      toast("success", `${label}: ${r.moves} reorder call${r.moves === 1 ? "" : "s"}.`);
      // The backend applied exactly this order; mirror it locally instead of
      // re-reading the playlist (one request per 50 tracks).
      setTracks((prev) => {
        const byPos = new Map(prev.map((t) => [t.position, t]));
        return target.map((p, i) => ({ ...byPos.get(p)!, position: i }));
      });
      setSelected(new Set());
      await refreshPlaylists(false);
    } catch (e) {
      toast("error", errorMessage(e));
      await load(playlistId);
    } finally {
      setApplying(false);
      setProgress(null);
    }
  }

  const order = () => tracks.map((t) => t.position); // current order (identity)
  const sel = () => order().filter((p) => selected.has(p));
  const rest = () => order().filter((p) => !selected.has(p));

  const moveToTop = () => apply([...sel(), ...rest()], "Moved to top");
  const moveToBottom = () => apply([...rest(), ...sel()], "Moved to bottom");
  const moveAfter = (targetPos: number) => {
    const r = rest();
    const idx = r.indexOf(targetPos);
    if (idx < 0) return;
    setAfterOpen(false);
    setAfterQuery("");
    apply([...r.slice(0, idx + 1), ...sel(), ...r.slice(idx + 1)], "Moved after track");
  };
  const sort = () => {
    const sorted = [...tracks].sort((a, b) => {
      const va = sortValue(a, sortKey);
      const vb = sortValue(b, sortKey);
      const c = va < vb ? -1 : va > vb ? 1 : a.position - b.position;
      return sortDesc ? -c : c;
    });
    apply(
      sorted.map((t) => t.position),
      `Sorted by ${SORT_LABEL[sortKey].toLowerCase()} ${sortDesc ? "descending" : "ascending"}`,
    );
  };

  const n = selected.size;
  const busy = applying || loading;

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Playlist Editor" subtitle={tracks.length ? `${tracks.length} tracks` : undefined} />

      <div className="flex flex-wrap items-center gap-3 border-b border-line bg-panel px-6 py-3">
        <PlaylistSelect value={playlistId} onChange={setPlaylistId} disabled={applying} />
        <Button variant="secondary" onClick={() => load(playlistId)} disabled={!playlistId || busy}>
          {loading ? <Spinner /> : "↻"} Reload
        </Button>
      </div>

      {playlistId && (
        <div className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-6 py-2 text-sm">
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter by title, artist, album"
            className="w-64 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
          />
          <span className="text-xs text-muted">{n} selected</span>
          <button className="text-xs text-zinc-300 hover:text-white" onClick={() => setSelected(new Set(filtered.map((t) => t.position)))}>
            Select shown
          </button>
          <button className="text-xs text-zinc-300 hover:text-white" onClick={() => setSelected(new Set())}>
            Clear
          </button>
          <span className="mx-1 h-5 w-px bg-line" />
          <Button variant="secondary" onClick={moveToTop} disabled={n === 0 || busy}>
            ⤒ Top
          </Button>
          <Button variant="secondary" onClick={moveToBottom} disabled={n === 0 || busy}>
            ⤓ Bottom
          </Button>
          <div className="relative">
            <input
              value={afterQuery}
              onChange={(e) => {
                setAfterQuery(e.target.value);
                setAfterOpen(true);
              }}
              onFocus={() => setAfterOpen(true)}
              onBlur={() => setTimeout(() => setAfterOpen(false), 150)}
              placeholder="Move after…"
              disabled={n === 0 || busy}
              className="w-48 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify disabled:opacity-50"
            />
            {afterOpen && afterMatches.length > 0 && (
              <ul className="absolute left-0 top-full z-20 mt-1 w-80 rounded-md border border-line bg-panel-2 shadow-xl">
                {afterMatches.map((t) => (
                  <li key={t.position}>
                    <button
                      onMouseDown={(e) => e.preventDefault()}
                      onClick={() => moveAfter(t.position)}
                      className="flex w-full items-center gap-2 px-3 py-1.5 text-left text-sm hover:bg-line"
                    >
                      <span className="w-8 text-right text-xs text-muted">#{t.position + 1}</span>
                      <span className="min-w-0 flex-1 truncate">
                        {t.name} <span className="text-muted">· {t.artists}</span>
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
          <span className="mx-1 h-5 w-px bg-line" />
          <select
            value={sortKey}
            onChange={(e) => setSortKey(e.target.value as SortKey)}
            disabled={busy}
            className="rounded-md border border-line bg-ink px-2 py-1.5 text-sm outline-none focus:border-spotify"
          >
            {(Object.keys(SORT_LABEL) as SortKey[]).map((k) => (
              <option key={k} value={k}>
                {SORT_LABEL[k]}
              </option>
            ))}
          </select>
          <button
            className="rounded-md border border-line px-2 py-1.5 text-xs text-zinc-300 hover:text-white"
            onClick={() => setSortDesc((d) => !d)}
            disabled={busy}
            title={sortDesc ? "Descending" : "Ascending"}
          >
            {sortDesc ? "Z→A" : "A→Z"}
          </button>
          <Button variant="secondary" onClick={sort} disabled={tracks.length < 2 || busy}>
            Sort whole playlist
          </Button>
        </div>
      )}

      {applying && (
        <div className="border-b border-line bg-panel px-6 py-2 text-xs text-muted">
          <div className="mb-1 flex items-center gap-2">
            <Spinner /> Reordering
            {progress ? ` ${progress.done} / ${progress.total} moves` : "…"}
          </div>
          <div className="h-1 w-full overflow-hidden rounded bg-line">
            <div
              className="h-full bg-spotify transition-all"
              style={{ width: progress && progress.total > 0 ? `${(progress.done / progress.total) * 100}%` : "5%" }}
            />
          </div>
        </div>
      )}

      <div className="flex-1 overflow-auto">
        {!playlistId ? (
          <p className="p-6 text-sm text-muted">Pick a playlist above. Reordering keeps each track's "date added".</p>
        ) : loading ? (
          <div className="flex items-center gap-2 p-6 text-sm text-muted">
            <Spinner /> Loading tracks…
          </div>
        ) : (
          <table className="w-full border-collapse text-sm">
            <thead className="sticky top-0 bg-panel text-left text-xs uppercase tracking-wider text-muted">
              <tr>
                <th className="w-8 px-3 py-2"></th>
                <th className="w-12 px-2 py-2 text-right">#</th>
                <th className="px-2 py-2">Title</th>
                <th className="px-2 py-2">Album</th>
                <th className="w-28 px-2 py-2">Added</th>
                <th className="w-16 px-2 py-2 text-right">Time</th>
              </tr>
            </thead>
            <tbody>
              {filtered.map((t) => {
                const on = selected.has(t.position);
                return (
                  <tr
                    key={t.position}
                    onClick={(e) => toggle(t.position, e.shiftKey)}
                    className={`cursor-pointer border-t border-line ${on ? "bg-spotify/10" : "hover:bg-panel-2"} ${
                      t.playable ? "" : "opacity-50"
                    }`}
                  >
                    <td className="px-3 py-1.5">
                      <input type="checkbox" checked={on} readOnly />
                    </td>
                    <td className="px-2 py-1.5 text-right text-xs text-muted">{t.position + 1}</td>
                    <td className="px-2 py-1.5">
                      <div className="truncate">{t.name}</div>
                      <div className="truncate text-xs text-muted">
                        {t.artists}
                        {t.isLocal ? " · local file" : ""}
                      </div>
                    </td>
                    <td className="max-w-56 truncate px-2 py-1.5 text-zinc-300">{t.album ?? "—"}</td>
                    <td className="px-2 py-1.5 text-xs text-muted">{t.addedAt?.slice(0, 10) ?? ""}</td>
                    <td className="px-2 py-1.5 text-right text-xs text-muted">{formatDuration(t.durationMs)}</td>
                  </tr>
                );
              })}
              {filtered.length === 0 && (
                <tr>
                  <td colSpan={6} className="px-6 py-4 text-muted">
                    No tracks match.
                  </td>
                </tr>
              )}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
