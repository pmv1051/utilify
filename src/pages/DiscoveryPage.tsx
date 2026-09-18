import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  errorMessage,
  type ArtistHit,
  type DiscoverResult,
  type DiscoveryProgress,
  type DiscoveryStatus,
} from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader, PlaylistSelect } from "../components/PlaylistPicker";
import { formatRelative } from "../lib/format";

const KIND_LABEL: Record<string, string> = {
  followed_artist: "Followed artist",
  seed_artist: "Seed artist",
  seed_playlist: "Seed playlist",
};

export function DiscoveryPage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);

  const [status, setStatus] = useState<DiscoveryStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [progress, setProgress] = useState<DiscoveryProgress | null>(null);

  const [followed, setFollowed] = useState<ArtistHit[] | null>(null);
  const [pickedArtists, setPickedArtists] = useState<Set<string>>(new Set());
  const [maxReleases, setMaxReleases] = useState(5);
  const [artistQuery, setArtistQuery] = useState("");
  const [artistHits, setArtistHits] = useState<ArtistHit[]>([]);
  const [seedRef, setSeedRef] = useState("");
  const [seedIncludeTracks, setSeedIncludeTracks] = useState(false);
  const [seedExpand, setSeedExpand] = useState(true);
  const [seedMaxArtists, setSeedMaxArtists] = useState(10);

  const [count, setCount] = useState(50);
  const [mode, setMode] = useState<"queue" | "playlist">("playlist");
  const [lastResult, setLastResult] = useState<DiscoverResult | null>(null);

  const load = useCallback(async () => {
    try {
      setStatus(await api.getDiscoveryStatus());
    } catch (e) {
      toast("error", errorMessage(e));
    }
  }, [toast]);

  useEffect(() => {
    load();
    const un1 = listen<DiscoveryProgress>("discovery-progress", (e) => setProgress(e.payload));
    const un2 = listen("play-finished", () => load());
    return () => {
      un1.then((f) => f());
      un2.then((f) => f());
    };
  }, [load]);

  async function run(label: string, fn: () => Promise<string | void>) {
    setBusy(label);
    setProgress(null);
    try {
      const msg = await fn();
      if (msg) toast("success", msg);
      await load();
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
      setProgress(null);
    }
  }

  const rebuildIndex = () =>
    run("library", async () => {
      const r = await api.rebuildLibraryIndex();
      return `Indexed ${r.tracks} tracks across ${r.playlists} playlists.`;
    });

  const loadFollowed = () =>
    run("followed", async () => {
      const list = await api.listFollowedArtists();
      setFollowed(list);
      setPickedArtists(new Set(list.map((a) => a.id)));
      if (list.length === 0) return "You do not follow any artists on Spotify.";
    });

  const indexFollowed = () =>
    run("index", async () => {
      const chosen = (followed ?? []).filter((a) => pickedArtists.has(a.id)).map((a) => ({ id: a.id, name: a.name }));
      const r = await api.indexDiscoveryArtists(chosen, "followed_artist", maxReleases, false);
      if (r.warning) toast("info", r.warning);
      return `Indexed ${r.indexed} artist${r.indexed === 1 ? "" : "s"}${r.skippedFresh ? ` (${r.skippedFresh} already fresh)` : ""}.`;
    });

  const searchArtist = () =>
    run("search", async () => {
      setArtistHits(await api.searchArtists(artistQuery));
    });

  const addSeedArtist = (a: ArtistHit) =>
    run("seed-artist", async () => {
      const r = await api.indexDiscoveryArtists([{ id: a.id, name: a.name }], "seed_artist", maxReleases, true);
      setArtistHits([]);
      setArtistQuery("");
      if (r.warning) toast("info", r.warning);
      return r.indexed > 0 ? `Indexed ${a.name}.` : undefined;
    });

  const addSeedPlaylist = () =>
    run("seed-playlist", async () => {
      const r = await api.addSeedPlaylist({
        reference: seedRef,
        includeTracks: seedIncludeTracks,
        expandArtists: seedExpand,
        maxArtists: seedMaxArtists,
        maxReleases,
      });
      setSeedRef("");
      if (r.warning) toast("info", r.warning);
      const parts = [`"${r.source.label}"`];
      if (seedIncludeTracks) parts.push(`${r.source.trackCount} of its tracks`);
      if (seedExpand) parts.push(`${r.artistsIndexed} of ${r.artistsTotal} artists indexed`);
      return `Seeded from ${parts.join(", ")}.`;
    });

  const removeSource = (key: string) => run(`remove-${key}`, () => api.removeDiscoverySource(key));

  const discover = () =>
    run("discover", async () => {
      const r = await api.discover(count, mode);
      setLastResult(r);
      if (r.playlist) await refreshPlaylists(true);
      return r.mode === "queue"
        ? `Queued ${r.offered.length} unheard tracks on your active device.`
        : `Created "${r.playlist?.name}" with ${r.offered.length} unheard tracks.`;
    });

  const skipRate =
    status && status.totals.listened + status.totals.skipped > 0
      ? Math.round((status.totals.skipped / (status.totals.listened + status.totals.skipped)) * 100)
      : null;

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Discovery" subtitle={status ? `${status.poolUnseen} unheard of ${status.poolTotal} in pool` : undefined}>
        {busy && <Spinner />}
      </PageHeader>

      {busy && progress && (
        <div className="border-b border-line bg-panel px-6 py-2 text-xs text-muted">
          {progress.phase === "library" ? "Indexing library" : progress.phase === "artists" ? "Indexing artists" : "Indexing"}
          {progress.total > 0 ? ` ${progress.done} / ${progress.total}` : ""} · {progress.label}
          <div className="mt-1 h-1 w-full overflow-hidden rounded bg-line">
            <div className="h-full bg-spotify transition-all" style={{ width: progress.total ? `${(progress.done / progress.total) * 100}%` : "5%" }} />
          </div>
        </div>
      )}

      <div className="flex-1 space-y-5 overflow-auto p-6">
        <p className="max-w-3xl text-sm text-muted">
          A track counts as heard once it has been played for 10 seconds or skipped while Utilify was logging, once
          Discovery has offered it, or if it sits in any playlist of your library. Everything else in the pool is fair
          game. Playlist mode is recommended: Utilify knows the order, so tracks you play through between two
          30-second checks are still recorded as listened.
        </p>

        {/* 1. Discover */}
        <section className="rounded-lg border border-spotify/40 bg-panel p-5">
          <div className="flex flex-wrap items-center gap-3">
            <label className="flex items-center gap-2 text-sm">
              Play
              <input
                type="number"
                min={1}
                max={mode === "queue" ? 20 : 100}
                value={count}
                onChange={(e) => setCount(Number(e.target.value))}
                className="w-16 rounded-md border border-line bg-ink px-2 py-1 text-sm outline-none focus:border-spotify"
              />
              unheard tracks
            </label>
            <select
              value={mode}
              onChange={(e) => {
                const m = e.target.value as "queue" | "playlist";
                setMode(m);
                setCount((c) => (m === "queue" ? Math.min(c, 20) : c));
              }}
              className="rounded-md border border-line bg-ink px-2 py-1.5 text-sm outline-none focus:border-spotify"
            >
              <option value="playlist">as a new "Utilify: Discovery" playlist (recommended)</option>
              <option value="queue">by adding them to the Spotify queue (max 20)</option>
            </select>
            <Button onClick={discover} disabled={busy !== null || !status || status.poolUnseen === 0}>
              {busy === "discover" ? <Spinner /> : "✦ Discover"}
            </Button>
            {status && status.poolUnseen === 0 && <span className="text-xs text-amber-400">Add a source below first.</span>}
          </div>
          {lastResult && (
            <ul className="mt-3 grid grid-cols-1 gap-1 text-sm md:grid-cols-2">
              {lastResult.offered.map((c) => (
                <li key={c.trackUri} className="truncate">
                  <span className="text-zinc-200">{c.name ?? c.trackUri}</span>
                  <span className="text-muted"> · {c.artists ?? ""}</span>
                </li>
              ))}
            </ul>
          )}
        </section>

        {/* 2. Library index */}
        <section className="rounded-lg border border-line bg-panel p-5">
          <div className="flex flex-wrap items-center gap-3">
            <h2 className="text-base font-semibold">Your library</h2>
            <span className="text-sm text-muted">
              {status ? `${status.library.tracks} tracks in ${status.library.playlists} playlists indexed` : "…"}
              {status?.library.updatedAt ? ` · updated ${formatRelative(status.library.updatedAt)}` : " · never indexed"}
            </span>
            <div className="ml-auto">
              <Button variant="secondary" onClick={rebuildIndex} disabled={busy !== null}>
                {busy === "library" ? <Spinner /> : "Rebuild index"}
              </Button>
            </div>
          </div>
          <p className="mt-2 text-xs text-muted">
            Tracks in your playlists are treated as already heard. Rebuild after adding a lot of music; it reads every
            playlist (one request per 50 tracks).
          </p>
        </section>

        {/* 3. Sources */}
        <section className="rounded-lg border border-line bg-panel p-5">
          <h2 className="mb-3 text-base font-semibold">Sources</h2>
          {status && status.sources.length > 0 ? (
            <ul className="mb-4 divide-y divide-line rounded-md border border-line">
              {status.sources.map((s) => (
                <li key={s.key} className="flex items-center gap-3 px-3 py-1.5 text-sm">
                  <span className="w-28 shrink-0 text-xs text-muted">{KIND_LABEL[s.kind] ?? s.kind}</span>
                  <span className="min-w-0 flex-1 truncate">{s.label}</span>
                  <span className="shrink-0 text-xs text-muted">
                    {s.kind === "seed_playlist" && s.trackCount === 0 ? "expanded to its artists" : `${s.trackCount} tracks`}
                    {s.indexedAt > 0 ? ` · ${formatRelative(s.indexedAt)}` : " · incomplete (quota), re-run to finish"}
                  </span>
                  <button className="text-xs text-muted hover:text-red-400" onClick={() => removeSource(s.key)} disabled={busy !== null}>
                    remove
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="mb-4 text-sm text-muted">No sources yet.</p>
          )}

          <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
            <div className="rounded-md border border-line p-3">
              <div className="mb-2 text-sm font-medium">Followed artists</div>
              {followed === null ? (
                <Button variant="secondary" onClick={loadFollowed} disabled={busy !== null}>
                  {busy === "followed" ? <Spinner /> : "Load followed artists"}
                </Button>
              ) : (
                <>
                  <div className="mb-2 flex items-center gap-2 text-xs text-muted">
                    <span>{pickedArtists.size} of {followed.length} selected</span>
                    <button className="hover:text-white" onClick={() => setPickedArtists(new Set(followed.map((a) => a.id)))}>All</button>
                    <button className="hover:text-white" onClick={() => setPickedArtists(new Set())}>None</button>
                  </div>
                  <ul className="mb-2 max-h-40 overflow-auto rounded border border-line">
                    {followed.map((a) => (
                      <li key={a.id}>
                        <label className="flex cursor-pointer items-center gap-2 px-2 py-1 text-sm hover:bg-panel-2">
                          <input
                            type="checkbox"
                            checked={pickedArtists.has(a.id)}
                            onChange={() =>
                              setPickedArtists((prev) => {
                                const n = new Set(prev);
                                if (n.has(a.id)) n.delete(a.id);
                                else n.add(a.id);
                                return n;
                              })
                            }
                          />
                          <span className="truncate">{a.name}</span>
                        </label>
                      </li>
                    ))}
                  </ul>
                  <Button onClick={indexFollowed} disabled={busy !== null || pickedArtists.size === 0}>
                    {busy === "index" ? <Spinner /> : `Index ${pickedArtists.size} artists`}
                  </Button>
                </>
              )}
              <div className="mt-2 flex items-center gap-2 text-xs text-muted">
                Newest
                <input
                  type="number"
                  min={1}
                  max={50}
                  value={maxReleases}
                  onChange={(e) => setMaxReleases(Number(e.target.value))}
                  className="w-14 rounded-md border border-line bg-ink px-2 py-0.5 text-xs outline-none focus:border-spotify"
                />
                releases per artist (about {maxReleases + 1} requests each)
              </div>
            </div>

            <div className="rounded-md border border-line p-3">
              <div className="mb-2 text-sm font-medium">Seed artist</div>
              <div className="flex gap-2">
                <input
                  value={artistQuery}
                  onChange={(e) => setArtistQuery(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && searchArtist()}
                  placeholder="Search an artist"
                  className="min-w-0 flex-1 rounded-md border border-line bg-ink px-2 py-1 text-sm outline-none focus:border-spotify"
                />
                <Button variant="secondary" onClick={searchArtist} disabled={busy !== null || !artistQuery.trim()}>
                  {busy === "search" ? <Spinner /> : "Search"}
                </Button>
              </div>
              {artistHits.length > 0 && (
                <ul className="mt-2 max-h-40 overflow-auto rounded border border-line">
                  {artistHits.map((a) => (
                    <li key={a.id}>
                      <button
                        onClick={() => addSeedArtist(a)}
                        disabled={busy !== null}
                        className="flex w-full items-center gap-2 px-2 py-1 text-left text-sm hover:bg-panel-2"
                      >
                        <span className="truncate">{a.name}</span>
                        <span className="ml-auto shrink-0 text-xs text-spotify">+ index</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>

            <div className="rounded-md border border-line p-3">
              <div className="mb-2 text-sm font-medium">Seed playlist</div>
              <PlaylistSelect
                value={null}
                onChange={(id) => id && setSeedRef(id)}
                placeholder="Pick one from your library…"
                disabled={busy !== null}
                className="mb-2 w-full min-w-0"
              />
              <div className="flex gap-2">
                <input
                  value={seedRef}
                  onChange={(e) => setSeedRef(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && addSeedPlaylist()}
                  placeholder="or paste a link, URI or id"
                  className="min-w-0 flex-1 rounded-md border border-line bg-ink px-2 py-1 text-sm outline-none focus:border-spotify"
                />
                <Button variant="secondary" onClick={addSeedPlaylist} disabled={busy !== null || !seedRef.trim()}>
                  {busy === "seed-playlist" ? <Spinner /> : "Add"}
                </Button>
              </div>
              <p className="mt-1 text-xs text-amber-400/80">
                Spotify's development mode only lets the app read playlists you own. To seed from someone else's
                playlist, copy its tracks into a playlist of yours in Spotify, refresh Playlists, then pick the copy.
              </p>
              <div className="mt-2 space-y-1 text-xs text-zinc-300">
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={seedExpand} onChange={(e) => setSeedExpand(e.target.checked)} />
                  Explore its artists' other releases (up to
                  <input
                    type="number"
                    min={1}
                    max={50}
                    value={seedMaxArtists}
                    onChange={(e) => setSeedMaxArtists(Number(e.target.value))}
                    className="w-12 rounded-md border border-line bg-ink px-1 py-0.5 text-xs outline-none focus:border-spotify"
                  />
                  artists, newest {maxReleases} releases each)
                </label>
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={seedIncludeTracks} onChange={(e) => setSeedIncludeTracks(e.target.checked)} />
                  Also use the playlist's own tracks as candidates
                </label>
              </div>
              <p className="mt-2 text-xs text-muted">
                Someone else's playlist: an editorial mix, a friend's, a chart. It is a starting point; picks are
                spread evenly across all sources.
              </p>
            </div>
          </div>
        </section>

        {/* 4. Log */}
        <section className="rounded-lg border border-line bg-panel p-5">
          <div className="mb-3 flex flex-wrap items-center gap-4">
            <h2 className="text-base font-semibold">Discovery log</h2>
            {status && (
              <span className="text-sm text-muted">
                {status.totals.offered} offered · {status.totals.listened} listened · {status.totals.skipped} skipped ·{" "}
                {status.totals.pending} pending{skipRate !== null ? ` · ${skipRate}% skip rate` : ""}
              </span>
            )}
          </div>
          {status && status.recent.length > 0 ? (
            <ul className="max-h-72 divide-y divide-line overflow-auto rounded-md border border-line">
              {status.recent.map((r) => (
                <li key={r.id} className="flex items-center gap-3 px-3 py-1.5 text-sm">
                  <span
                    className={`w-16 shrink-0 text-xs ${
                      r.status === "listened" ? "text-spotify" : r.status === "skipped" ? "text-red-400" : "text-muted"
                    }`}
                  >
                    {r.status}
                  </span>
                  <span className="min-w-0 flex-1 truncate">
                    {r.trackName ?? r.trackUri}
                    <span className="text-muted"> · {r.artistName ?? ""}</span>
                  </span>
                  <span className="shrink-0 text-xs text-muted">{formatRelative(r.firstSeenAt)}</span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted">Nothing discovered yet.</p>
          )}
        </section>
      </div>
    </div>
  );
}
