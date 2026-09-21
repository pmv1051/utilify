import { useEffect, useMemo, useRef, useState } from "react";
import {
  api,
  errorMessage,
  type AlbumInfo,
  type ArtistHit,
  type DiscographyResult,
  type SkippedTrack,
} from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader } from "../components/PlaylistPicker";
import { useQuotaCooldown } from "../lib/quota";

const GROUP_LABEL: Record<string, string> = {
  album: "Albums",
  single: "Singles & EPs",
  compilation: "Compilations",
  appears_on: "Appears on",
};
const GROUP_ORDER = ["album", "single", "compilation", "appears_on"];

/**
 * "Appears on" is switched off for now. Unlike compilations, which ride along
 * in the listing Utilify already makes, it needs its own paged listing of
 * every release by other artists that credits this one, and then one listing
 * per release you pick. At ten items per page in Development Mode that is the
 * fastest way to exhaust the app's API quota.
 */
const APPEARS_ON_REASON =
  'Turned off for now. Burns through Spotify\'s API quota quickly';

export function DiscographyPage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);
  const cooldown = useQuotaCooldown("catalog");
  const paused = cooldown.active;

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<ArtistHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [artist, setArtist] = useState<ArtistHit | null>(null);

  const [includeCompilations, setIncludeCompilations] = useState(false);
  const [includeAppearsOn, setIncludeAppearsOn] = useState(false);
  // Fetched once per artist: albums + singles + compilations. "Appears on" is
  // fetched only when first requested. Both are cached for the session so
  // toggling or revisiting an artist never re-hits the API.
  const [base, setBase] = useState<AlbumInfo[]>([]);
  const [appearsOn, setAppearsOn] = useState<AlbumInfo[] | null>(null);
  const [loadingAlbums, setLoadingAlbums] = useState(false);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const albumCache = useRef(new Map<string, AlbumInfo[]>());

  async function fetchGroups(
    artistId: string,
    cacheKey: string,
    groups: string[],
    refresh = false,
  ): Promise<AlbumInfo[]> {
    const key = `${artistId}|${cacheKey}`;
    if (!refresh) {
      const hit = albumCache.current.get(key);
      if (hit) return hit;
    }
    const list = await api.getArtistAlbums(artistId, groups, refresh);
    albumCache.current.set(key, list);
    return list;
  }

  /** Re-read this artist's releases from Spotify, past both caches. */
  async function refreshReleases() {
    if (!artist) return;
    setLoadingAlbums(true);
    setResult(null);
    try {
      const list = await fetchGroups(artist.id, "base", ["album", "single", "compilation"], true);
      setBase(list);
      setChosen(new Set(list.filter((a) => a.group === "album" || a.group === "single").map((a) => a.id)));
      toast("success", `${list.length} releases re-read from Spotify.`);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setLoadingAlbums(false);
    }
  }

  const [name, setName] = useState("");
  const [onlyThisArtist, setOnlyThisArtist] = useState(true);
  const [dedupeByName, setDedupeByName] = useState(true);
  const [randomize, setRandomize] = useState(false);
  const [building, setBuilding] = useState(false);
  const [result, setResult] = useState<DiscographyResult | null>(null);

  async function search() {
    if (!query.trim()) return;
    setSearching(true);
    // Drop the artist that was picked before: the hit list is hidden while one
    // is selected, so leaving it set made a new search look like it did
    // nothing while the old releases stayed on screen.
    setArtist(null);
    setHits([]);
    setResult(null);
    try {
      const r = await api.searchArtists(query);
      setHits(r);
      if (r.length === 0) toast("info", "No artists found.");
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setSearching(false);
    }
  }

  // Base listing: once per artist.
  useEffect(() => {
    if (!artist) {
      setBase([]);
      setAppearsOn(null);
      setChosen(new Set());
      return;
    }
    let cancelled = false;
    setLoadingAlbums(true);
    setResult(null);
    setAppearsOn(null);
    fetchGroups(artist.id, "base", ["album", "single", "compilation"])
      .then((list) => {
        if (cancelled) return;
        setBase(list);
        // Albums and singles on by default; compilations / appears-on opt-in.
        setChosen(new Set(list.filter((a) => a.group === "album" || a.group === "single").map((a) => a.id)));
      })
      .catch((e) => toast("error", errorMessage(e)))
      .finally(() => {
        if (!cancelled) setLoadingAlbums(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [artist, toast]);

  // "Appears on": only when first switched on for this artist.
  useEffect(() => {
    if (!artist || !includeAppearsOn || appearsOn !== null) return;
    let cancelled = false;
    setLoadingAlbums(true);
    fetchGroups(artist.id, "appears_on", ["appears_on"])
      .then((list) => {
        if (!cancelled) setAppearsOn(list);
      })
      .catch((e) => toast("error", errorMessage(e)))
      .finally(() => {
        if (!cancelled) setLoadingAlbums(false);
      });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [artist, includeAppearsOn, appearsOn, toast]);

  const albums = useMemo(() => {
    const list = base.filter((a) => includeCompilations || a.group !== "compilation");
    return includeAppearsOn && appearsOn ? [...list, ...appearsOn] : list;
  }, [base, appearsOn, includeCompilations, includeAppearsOn]);

  const grouped = useMemo(() => {
    const m = new Map<string, AlbumInfo[]>();
    for (const a of albums) m.set(a.group, [...(m.get(a.group) ?? []), a]);
    return GROUP_ORDER.filter((g) => m.has(g)).map((g) => ({ group: g, albums: m.get(g)! }));
  }, [albums]);

  const chosenTracks = albums.filter((a) => chosen.has(a.id)).reduce((n, a) => n + a.totalTracks, 0);

  function toggleAlbum(id: string) {
    setChosen((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function setGroup(group: string, on: boolean) {
    setChosen((prev) => {
      const next = new Set(prev);
      for (const a of albums) if (a.group === group) on ? next.add(a.id) : next.delete(a.id);
      return next;
    });
  }

  async function build() {
    if (!artist) return;
    setBuilding(true);
    setResult(null);
    try {
      const r = await api.createDiscography({
        artistId: artist.id,
        artistName: artist.name,
        albums: albums.filter((a) => chosen.has(a.id)).map((a) => ({ id: a.id, name: a.name })),
        name,
        onlyThisArtist,
        dedupeByName,
        randomize,
      });
      setResult(r);
      toast("success", `Created "${r.playlist.name}" with ${r.playlist.trackCount} tracks.`);
      await refreshPlaylists(true);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBuilding(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Artist Discography" />
      <div className="flex-1 space-y-5 overflow-auto p-6">
        {paused && (
          <div className="rounded-lg border border-amber-400/40 bg-amber-400/10 p-4 text-sm text-amber-200">
            <div className="font-semibold text-amber-300">
              Paused: Spotify's quota for artist and album lookups is exhausted.
            </div>
            This tool is the one that uses them. Spotify's budget resets daily, so it waits {cooldown.remaining}
            before sending any more. The other tools are unaffected, and releases you have already looked up stay
            available.
          </div>
        )}
        <section className="rounded-lg border border-line bg-panel p-5">
          <div className="flex items-center gap-2">
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && search()}
              placeholder="Search for an artist"
              disabled={paused}
              title={paused ? cooldown.reason : undefined}
              className="w-80 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify disabled:opacity-50"
            />
            <Button
              scope="catalog"
              onClick={search}
              disabled={paused || searching || !query.trim()}
              title={paused ? cooldown.reason : undefined}
            >
              {searching ? <Spinner /> : "Search"}
            </Button>
            {artist && (
              <span className="ml-3 flex items-center gap-2 text-sm">
                {artist.imageUrl && <img src={artist.imageUrl} alt="" className="h-7 w-7 rounded-full object-cover" />}
                <span className="font-semibold">{artist.name}</span>
                <button className="text-xs text-muted hover:text-white" onClick={() => setArtist(null)}>
                  change
                </button>
              </span>
            )}
          </div>
          {!artist && hits.length > 0 && (
            <ul className="mt-3 grid grid-cols-1 gap-2 md:grid-cols-2">
              {hits.map((h) => (
                <li key={h.id}>
                  <button
                    onClick={() => {
                      setArtist(h);
                      setName("");
                    }}
                    className="flex w-full items-center gap-3 rounded-md border border-line bg-ink px-3 py-2 text-left text-sm hover:border-spotify"
                  >
                    {h.imageUrl ? (
                      <img src={h.imageUrl} alt="" className="h-10 w-10 rounded-full object-cover" />
                    ) : (
                      <span className="flex h-10 w-10 items-center justify-center rounded-full bg-panel-2 text-muted">♪</span>
                    )}
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">{h.name}</span>
                      <span className="block truncate text-xs text-muted">
                        {h.followers != null ? `${h.followers.toLocaleString()} followers` : ""}
                        {h.genres.length > 0 ? ` · ${h.genres.slice(0, 3).join(", ")}` : ""}
                      </span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        {artist && (
          <section className="rounded-lg border border-line bg-panel p-5">
            <div className="mb-3 flex flex-wrap items-center gap-4">
              <h2 className="text-base font-semibold">Releases</h2>
              <label className="flex items-center gap-2 text-sm text-zinc-300">
                <input type="checkbox" checked={includeCompilations} onChange={(e) => setIncludeCompilations(e.target.checked)} />
                Include compilations
              </label>
              <label
                className="flex cursor-not-allowed items-center gap-2 text-sm text-muted"
                title={APPEARS_ON_REASON}
              >
                <input
                  type="checkbox"
                  checked={includeAppearsOn}
                  onChange={(e) => setIncludeAppearsOn(e.target.checked)}
                  disabled
                  className="cursor-not-allowed"
                />
                Include "appears on"
                <span className="text-xs">(disabled)</span>
                {loadingAlbums && base.length > 0 && <Spinner />}
              </label>
              <span className="ml-auto flex items-center gap-3 text-xs text-muted">
                <span>
                  {chosen.size} of {albums.length} releases · about {chosenTracks} tracks before deduplication
                </span>
                <button
                  type="button"
                  onClick={refreshReleases}
                  disabled={paused || loadingAlbums}
                  title={
                    paused
                      ? cooldown.reason
                      : "Releases are kept for a week so re-opening an artist costs no requests. Use this after a new release."
                  }
                  className="rounded border border-line px-2 py-1 transition hover:bg-panel-2 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  Refresh
                </button>
              </span>
            </div>
            <p className="mb-3 text-xs text-muted">{APPEARS_ON_REASON}</p>
            {loadingAlbums && base.length === 0 ? (
              <div className="flex items-center gap-2 text-sm text-muted">
                <Spinner /> Loading releases…
              </div>
            ) : (
              <div className="space-y-4">
                {grouped.map(({ group, albums: list }) => (
                  <div key={group}>
                    <div className="mb-1 flex items-center gap-3">
                      <span className="text-xs font-semibold uppercase tracking-wider text-muted">
                        {GROUP_LABEL[group] ?? group} ({list.length})
                      </span>
                      <button className="text-xs text-zinc-300 hover:text-white" onClick={() => setGroup(group, true)}>
                        All
                      </button>
                      <button className="text-xs text-zinc-300 hover:text-white" onClick={() => setGroup(group, false)}>
                        None
                      </button>
                    </div>
                    <ul className="grid grid-cols-1 gap-1 md:grid-cols-2 xl:grid-cols-3">
                      {list.map((a) => (
                        <li key={a.id}>
                          <label className="flex cursor-pointer items-center gap-3 rounded-md px-2 py-1.5 text-sm hover:bg-panel-2">
                            <input type="checkbox" checked={chosen.has(a.id)} onChange={() => toggleAlbum(a.id)} />
                            {a.imageUrl ? (
                              <img src={a.imageUrl} alt="" className="h-9 w-9 rounded object-cover" />
                            ) : (
                              <span className="h-9 w-9 rounded bg-panel-2" />
                            )}
                            <span className="min-w-0 flex-1">
                              <span className="block truncate">{a.name}</span>
                              <span className="block truncate text-xs text-muted">
                                {a.releaseDate?.slice(0, 4) ?? "—"} · {a.totalTracks} tracks
                                {group === "appears_on" ? ` · ${a.artists}` : ""}
                              </span>
                            </span>
                          </label>
                        </li>
                      ))}
                    </ul>
                  </div>
                ))}
              </div>
            )}
          </section>
        )}

        {artist && albums.length > 0 && (
          <section className="rounded-lg border border-line bg-panel p-5">
            <h2 className="mb-3 text-base font-semibold">New playlist</h2>
            <div className="flex flex-wrap items-center gap-3">
              <span className="text-sm text-muted">Utilify:</span>
              <input
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder={`${artist.name} discography`}
                disabled={building}
                className="w-72 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
              />
              <label className="flex items-center gap-2 text-sm text-zinc-300" title="Skip tracks on these releases that do not credit this artist">
                <input type="checkbox" checked={onlyThisArtist} onChange={(e) => setOnlyThisArtist(e.target.checked)} disabled={building} />
                Only this artist's tracks
              </label>
              <label className="flex items-center gap-2 text-sm text-zinc-300" title="Collapse the same song across deluxe/standard editions">
                <input type="checkbox" checked={dedupeByName} onChange={(e) => setDedupeByName(e.target.checked)} disabled={building} />
                Skip same title across editions
              </label>
              <label className="flex items-center gap-2 text-sm text-zinc-300">
                <input type="checkbox" checked={randomize} onChange={(e) => setRandomize(e.target.checked)} disabled={building} />
                Randomize order
              </label>
              <div className="ml-auto">
                <Button
                  scope="catalog"
                  onClick={build}
                  disabled={paused || chosen.size === 0 || building}
                  title={paused ? cooldown.reason : undefined}
                >
                  {building ? <Spinner /> : "Create playlist"}
                </Button>
              </div>
            </div>
            <p className="mt-3 text-xs text-muted">
              Fetches every chosen release (one request each), so large catalogs take a little while.
            </p>
          </section>
        )}

        {result && (
          <section className="rounded-lg border border-spotify/40 bg-panel p-5 text-sm">
            <div className="font-semibold">{result.playlist.name}</div>
            <div className="mt-1 text-muted">
              {result.playlist.trackCount} tracks from {result.albums} releases · {result.tracksSeen} scanned ·{" "}
              {result.duplicatesSkipped} duplicate{result.duplicatesSkipped === 1 ? "" : "s"} skipped
              {result.otherArtistSkipped > 0 && ` · ${result.otherArtistSkipped} by other artists skipped`}
              {result.unplayableSkipped > 0 && ` · ${result.unplayableSkipped} unplayable`}
              {result.releasesFromCache > 0 && ` · ${result.releasesFromCache} read from cache`}
              {result.playlist.trackCount < result.playlist.requested &&
                ` · ${result.playlist.requested - result.playlist.trackCount} not accepted by Spotify`}
            </div>
            <SkippedTracks skipped={result.skipped} />
          </section>
        )}
      </div>
    </div>
  );
}

/** Order the reasons so the surprising one is read first. */
const REASON_RANK: Record<SkippedTrack["reason"], number> = {
  "same-title": 0,
  "same-recording": 1,
  "other-artist": 2,
  unplayable: 3,
};

function reasonText(s: SkippedTrack): string {
  switch (s.reason) {
    case "same-title":
      return s.keptFrom ? `same title as the copy on "${s.keptFrom}"` : "same title as an earlier release";
    case "same-recording":
      return s.keptFrom ? `already added from "${s.keptFrom}"` : "already added";
    case "other-artist":
      return "not credited to this artist";
    default:
      return "not playable from Spotify's catalogue";
  }
}

/**
 * Names the tracks that were read but left out. Without this the result only
 * said how many went missing, so a song vanishing from a discography could
 * not be explained without rebuilding with the options changed.
 */
function SkippedTracks({ skipped }: { skipped: SkippedTrack[] }) {
  const [expanded, setExpanded] = useState(false);
  if (skipped.length === 0) return null;

  const sorted = [...skipped].sort((a, b) => REASON_RANK[a.reason] - REASON_RANK[b.reason]);
  const shown = expanded ? sorted : sorted.slice(0, 5);
  const byTitle = sorted.some((s) => s.reason === "same-title");

  return (
    <div className="mt-3 border-t border-line pt-3">
      <div className="text-xs font-medium text-zinc-300">Left out</div>
      <ul className="mt-1.5 max-h-64 space-y-1 overflow-auto text-xs">
        {shown.map((s, i) => (
          <li key={`${s.release}|${s.name}|${i}`} className="flex flex-wrap items-baseline gap-x-2">
            <span className="text-zinc-100">{s.name}</span>
            <span className="text-muted">{s.artists}</span>
            <span className="text-muted">· from "{s.release}"</span>
            <span className="text-muted">· {reasonText(s)}</span>
          </li>
        ))}
      </ul>
      {sorted.length > 5 && (
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="mt-1.5 text-xs text-muted underline-offset-2 hover:text-zinc-200 hover:underline"
        >
          {expanded ? "Show fewer" : `Show all ${sorted.length}`}
        </button>
      )}
      {byTitle && (
        <p className="mt-2 text-xs text-muted">
          Releases are read oldest first and the first copy of a title wins, so a newer single loses to an older
          version. Untick "Skip same title across editions" to keep every copy.
        </p>
      )}
    </div>
  );
}
