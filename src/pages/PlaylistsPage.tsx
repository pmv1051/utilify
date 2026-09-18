import { useMemo, useState } from "react";
import { api, errorMessage, type Playlist } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";

export function PlaylistsPage() {
  const playlists = useApp((s) => s.playlists);
  const loading = useApp((s) => s.playlistsLoading);
  const refresh = useApp((s) => s.refreshPlaylists);
  const sessions = useApp((s) => s.sessions);
  const refreshSessions = useApp((s) => s.refreshSessions);
  const refreshPlayback = useApp((s) => s.refreshPlayback);
  const toast = useApp((s) => s.toast);
  const setPage = useApp((s) => s.setPage);
  const openBenchFor = useApp((s) => s.openBenchFor);

  const [query, setQuery] = useState("");
  const [showShadows, setShowShadows] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return playlists.filter((p) => (showShadows || !p.isShadow) && (q === "" || p.name.toLowerCase().includes(q)));
  }, [playlists, query, showShadows]);

  const sessionBySource = useMemo(() => {
    const m = new Map<string, (typeof sessions)[number]>();
    for (const s of sessions) m.set(s.sourcePlaylistId, s);
    return m;
  }, [sessions]);

  async function randomize(p: Playlist) {
    setBusyId(p.id);
    try {
      const result = await api.randomizePlaylist(p.id, true);
      await Promise.all([refreshSessions(), refresh(true)]);
      const missing = result.session.missingTracks.length;
      const summary =
        missing > 0
          ? `Shuffled ${result.session.trackCount} tracks into "${result.session.shadowName}" (${missing} not included, see Randomizer).`
          : `Shuffled ${result.session.trackCount} tracks into "${result.session.shadowName}".`;
      if (result.playbackStarted) {
        toast("success", `${summary} Playing now.`);
      } else {
        toast("info", result.warning ?? `Shuffled into "${result.session.shadowName}", but playback did not start.`);
      }
      setTimeout(() => refreshPlayback(), 1500);
      setPage("randomizer");
    } catch (e) {
      toast("error", errorMessage(e));
      // The shadow playlist may have been written before the failure.
      await Promise.all([refreshSessions(), refresh(true)]);
    } finally {
      setBusyId(null);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-6 py-4">
        <h1 className="text-xl font-bold">Playlists</h1>
        <span className="text-sm text-muted">{playlists.length}</span>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter playlists"
          className="ml-auto w-64 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
        />
        <label className="flex items-center gap-2 text-xs text-muted">
          <input type="checkbox" checked={showShadows} onChange={(e) => setShowShadows(e.target.checked)} />
          Show -Utilify playlists
        </label>
        <Button variant="secondary" onClick={() => refresh(true)} disabled={loading}>
          {loading ? <Spinner /> : "↻"} Refresh
        </Button>
      </header>

      <div className="flex-1 overflow-auto p-6">
        {loading && playlists.length === 0 ? (
          <div className="flex items-center gap-3 text-muted">
            <Spinner /> Loading playlists from Spotify…
          </div>
        ) : filtered.length === 0 ? (
          <p className="text-muted">No playlists match.</p>
        ) : (
          <ul className="grid grid-cols-1 gap-3 lg:grid-cols-2">
            {filtered.map((p) => {
              const session = sessionBySource.get(p.id);
              return (
                <li key={p.id} className="flex items-center gap-4 rounded-lg border border-line bg-panel p-3">
                  {p.imageUrl ? (
                    <img src={p.imageUrl} alt="" className="h-14 w-14 shrink-0 rounded object-cover" />
                  ) : (
                    <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded bg-panel-2 text-muted">♫</div>
                  )}
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="truncate font-medium">{p.name}</span>
                      {p.isShadow && <Tag>Utilify</Tag>}
                      {session && <Tag green>Randomized</Tag>}
                    </div>
                    <div className="truncate text-xs text-muted">
                      {p.trackCount} tracks · {p.ownerName ?? p.ownerId ?? "unknown owner"}
                      {!p.isOwn && " · followed"}
                    </div>
                  </div>
                  <Button
                    variant="ghost"
                    onClick={() => openBenchFor(p.id)}
                    disabled={p.trackCount === 0}
                    title="Temporarily remove a track from this playlist"
                  >
                    Bench…
                  </Button>
                  {!p.isShadow && (
                    <Button
                      variant={session ? "secondary" : "primary"}
                      onClick={() => randomize(p)}
                      disabled={busyId !== null || p.trackCount === 0}
                      title={p.trackCount === 0 ? "Playlist is empty" : `Shuffle into ${p.name}-Utilify and play`}
                    >
                      {busyId === p.id ? <Spinner /> : "⇄"} {session ? "Re-shuffle" : "Randomize"}
                    </Button>
                  )}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

function Tag({ children, green }: { children: string; green?: boolean }) {
  return (
    <span
      className={`shrink-0 rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-wide ${
        green ? "bg-spotify/20 text-spotify" : "bg-panel-2 text-muted"
      }`}
    >
      {children}
    </span>
  );
}
