import { useEffect, useMemo, useState } from "react";
import { api, errorMessage, type AlbumInfo, type ArtistHit, type DiscographyResult } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader } from "../components/PlaylistPicker";

const GROUP_LABEL: Record<string, string> = {
  album: "Albums",
  single: "Singles & EPs",
  compilation: "Compilations",
  appears_on: "Appears on",
};
const GROUP_ORDER = ["album", "single", "compilation", "appears_on"];

export function DiscographyPage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);

  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<ArtistHit[]>([]);
  const [searching, setSearching] = useState(false);
  const [artist, setArtist] = useState<ArtistHit | null>(null);

  const [includeCompilations, setIncludeCompilations] = useState(false);
  const [includeAppearsOn, setIncludeAppearsOn] = useState(false);
  const [albums, setAlbums] = useState<AlbumInfo[]>([]);
  const [loadingAlbums, setLoadingAlbums] = useState(false);
  const [chosen, setChosen] = useState<Set<string>>(new Set());

  const [name, setName] = useState("");
  const [onlyThisArtist, setOnlyThisArtist] = useState(true);
  const [dedupeByName, setDedupeByName] = useState(true);
  const [randomize, setRandomize] = useState(false);
  const [building, setBuilding] = useState(false);
  const [result, setResult] = useState<DiscographyResult | null>(null);

  async function search() {
    if (!query.trim()) return;
    setSearching(true);
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

  useEffect(() => {
    if (!artist) {
      setAlbums([]);
      setChosen(new Set());
      return;
    }
    let cancelled = false;
    setLoadingAlbums(true);
    setResult(null);
    api
      .getArtistAlbums(artist.id, includeCompilations, includeAppearsOn)
      .then((list) => {
        if (cancelled) return;
        setAlbums(list);
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
  }, [artist, includeCompilations, includeAppearsOn, toast]);

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
        albumIds: albums.filter((a) => chosen.has(a.id)).map((a) => a.id),
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
        <section className="rounded-lg border border-line bg-panel p-5">
          <div className="flex items-center gap-2">
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && search()}
              placeholder="Search for an artist"
              className="w-80 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
            />
            <Button onClick={search} disabled={searching || !query.trim()}>
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
              <label className="flex items-center gap-2 text-sm text-zinc-300">
                <input type="checkbox" checked={includeAppearsOn} onChange={(e) => setIncludeAppearsOn(e.target.checked)} />
                Include "appears on"
              </label>
              <span className="ml-auto text-xs text-muted">
                {chosen.size} of {albums.length} releases · about {chosenTracks} tracks before deduplication
              </span>
            </div>
            {loadingAlbums ? (
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
                <Button onClick={build} disabled={chosen.size === 0 || building}>
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
              {result.playlist.trackCount < result.playlist.requested &&
                ` · ${result.playlist.requested - result.playlist.trackCount} not accepted by Spotify`}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
