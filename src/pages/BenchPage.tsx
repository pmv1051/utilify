import { useEffect, useMemo, useState } from "react";
import { api, errorMessage, type BenchRow, type TrackRow } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { formatDateTime, formatDuration, formatUntil } from "../lib/format";
import { BENCH_PRESETS as PRESETS } from "../lib/bench";

type Unit = "minutes" | "hours" | "days";
const UNIT_SECS: Record<Unit, number> = { minutes: 60, hours: 3600, days: 86400 };

export function BenchPage() {
  const playlists = useApp((s) => s.playlists);
  const benches = useApp((s) => s.benches);
  const refreshBenches = useApp((s) => s.refreshBenches);
  const refreshSessions = useApp((s) => s.refreshSessions);
  const playlistId = useApp((s) => s.benchPlaylistId);
  const setPlaylistId = useApp((s) => s.setBenchPlaylist);
  const toast = useApp((s) => s.toast);

  const [tracks, setTracks] = useState<TrackRow[]>([]);
  const [loadingTracks, setLoadingTracks] = useState(false);
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<TrackRow | null>(null);
  const [preset, setPreset] = useState<number | "custom">(86400);
  const [customValue, setCustomValue] = useState("3");
  const [customUnit, setCustomUnit] = useState<Unit>("days");
  const [busy, setBusy] = useState(false);
  const [unbenching, setUnbenching] = useState<number | null>(null);

  // Re-render countdowns every 30 s.
  const [, setTick] = useState(0);
  useEffect(() => {
    const t = setInterval(() => setTick((n) => n + 1), 30_000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    setSelected(null);
    setQuery("");
    if (!playlistId) {
      setTracks([]);
      return;
    }
    let cancelled = false;
    setLoadingTracks(true);
    api
      .getPlaylistTracks(playlistId)
      .then((t) => {
        if (!cancelled) setTracks(t);
      })
      .catch((e) => toast("error", errorMessage(e)))
      .finally(() => {
        if (!cancelled) setLoadingTracks(false);
      });
    return () => {
      cancelled = true;
    };
  }, [playlistId, toast]);

  const benchedHere = useMemo(() => {
    const set = new Set<string>();
    for (const b of benches) if (b.playlistId === playlistId) set.add(b.trackUri);
    return set;
  }, [benches, playlistId]);

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

  const durationSecs =
    preset === "custom" ? Math.round(Number(customValue) * UNIT_SECS[customUnit]) : preset;
  const durationValid = Number.isFinite(durationSecs) && durationSecs >= 60 && durationSecs <= 366 * 86400;
  const playlist = playlists.find((p) => p.id === playlistId) ?? null;

  async function confirm() {
    if (!playlistId || !selected || !durationValid) return;
    setBusy(true);
    try {
      const row = await api.benchTrack({
        playlistId,
        trackUri: selected.uri,
        trackName: selected.name,
        artistName: selected.artists,
        position: selected.position,
        durationSecs,
      });
      toast("success", `Benched "${selected.name}". It returns ${formatUntil(row.restoreAt)}.`);
      setTracks((ts) => ts.filter((t) => t.uri !== selected.uri));
      setSelected(null);
      await Promise.all([refreshBenches(), refreshSessions()]);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  async function unbench(b: BenchRow) {
    setUnbenching(b.id);
    try {
      await api.unbenchTrack(b.id);
      toast("success", `Restored "${b.trackName ?? b.trackUri}" to "${b.playlistName ?? "playlist"}".`);
      await refreshBenches();
      if (b.playlistId === playlistId) {
        setTracks(await api.getPlaylistTracks(b.playlistId));
      }
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setUnbenching(null);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-6 py-4">
        <h1 className="text-xl font-bold">Bench</h1>
        <span className="text-sm text-muted">{benches.length} benched</span>
      </header>

      <div className="flex-1 space-y-6 overflow-auto p-6">
        <section className="rounded-lg border border-line bg-panel p-5">
          <h2 className="mb-1 text-base font-semibold">Bench a track</h2>
          <p className="mb-4 text-xs text-muted">
            The track is removed from the playlist now and put back automatically when the time is up, even if the
            app was closed in between. Benched tracks stay out of the next re-shuffle of a randomized playlist and
            rejoin on the re-shuffle after they return.
          </p>

          <div className="flex flex-wrap items-center gap-2">
            <select
              value={playlistId ?? ""}
              onChange={(e) => setPlaylistId(e.target.value || null)}
              className="min-w-64 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
            >
              <option value="">Choose a playlist…</option>
              {playlists.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name} ({p.trackCount})
                </option>
              ))}
            </select>
            {playlistId && (
              <input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Find a track"
                className="w-64 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
              />
            )}
            {loadingTracks && <Spinner />}
          </div>

          {playlistId && !loadingTracks && (
            <ul className="mt-3 max-h-72 divide-y divide-line overflow-auto rounded-md border border-line bg-ink">
              {filtered.length === 0 ? (
                <li className="px-3 py-4 text-sm text-muted">No tracks match.</li>
              ) : (
                filtered.map((t) => {
                  const isSel = selected?.uri === t.uri && selected?.position === t.position;
                  const already = benchedHere.has(t.uri);
                  return (
                    <li key={`${t.uri}-${t.position}`}>
                      <button
                        disabled={already}
                        onClick={() => setSelected(isSel ? null : t)}
                        className={`flex w-full items-center gap-3 px-3 py-2 text-left text-sm disabled:opacity-40 ${
                          isSel ? "bg-spotify/15" : "hover:bg-panel-2"
                        }`}
                      >
                        <span className="w-8 shrink-0 text-right text-xs text-muted">{t.position + 1}</span>
                        <span className="min-w-0 flex-1">
                          <span className="block truncate">{t.name}</span>
                          <span className="block truncate text-xs text-muted">
                            {t.artists}
                            {t.album ? ` · ${t.album}` : ""}
                          </span>
                        </span>
                        <span className="shrink-0 text-xs text-muted">{formatDuration(t.durationMs)}</span>
                        {already && <span className="shrink-0 text-xs text-amber-400">benched</span>}
                      </button>
                    </li>
                  );
                })
              )}
            </ul>
          )}

          {selected && (
            <div className="mt-4 rounded-md border border-line bg-panel-2 p-4">
              <div className="mb-3 text-sm">
                Bench <span className="font-semibold">{selected.name}</span>{" "}
                <span className="text-muted">by {selected.artists}</span> from{" "}
                <span className="font-semibold">{playlist?.name}</span> for
              </div>
              <div className="flex flex-wrap gap-2">
                {PRESETS.map((p) => (
                  <Chip key={p.secs} active={preset === p.secs} onClick={() => setPreset(p.secs)}>
                    {p.label}
                  </Chip>
                ))}
                <Chip active={preset === "custom"} onClick={() => setPreset("custom")}>
                  Custom
                </Chip>
              </div>
              {preset === "custom" && (
                <div className="mt-3 flex items-center gap-2 text-sm">
                  <input
                    type="number"
                    min={1}
                    value={customValue}
                    onChange={(e) => setCustomValue(e.target.value)}
                    className="w-24 rounded-md border border-line bg-ink px-3 py-1.5 outline-none focus:border-spotify"
                  />
                  <select
                    value={customUnit}
                    onChange={(e) => setCustomUnit(e.target.value as Unit)}
                    className="rounded-md border border-line bg-ink px-3 py-1.5 outline-none focus:border-spotify"
                  >
                    <option value="minutes">minutes</option>
                    <option value="hours">hours</option>
                    <option value="days">days</option>
                  </select>
                  {!durationValid && <span className="text-xs text-red-400">Between 1 minute and 1 year.</span>}
                </div>
              )}
              <div className="mt-4 flex items-center gap-3">
                <Button onClick={confirm} disabled={busy || !durationValid}>
                  {busy && <Spinner />} Bench it
                </Button>
                <span className="text-xs text-muted">
                  Returns {formatUntil(Math.floor(Date.now() / 1000) + (durationValid ? durationSecs : 0))}
                </span>
                <Button variant="ghost" onClick={() => setSelected(null)} disabled={busy}>
                  Cancel
                </Button>
              </div>
            </div>
          )}
        </section>

        <section className="rounded-lg border border-line bg-panel p-5">
          <h2 className="mb-3 text-base font-semibold">On the bench</h2>
          {benches.length === 0 ? (
            <p className="text-sm text-muted">Nothing benched right now.</p>
          ) : (
            <ul className="divide-y divide-line">
              {benches.map((b) => (
                <li key={b.id} className="flex items-center gap-4 py-3">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium">{b.trackName ?? b.trackUri}</div>
                    <div className="truncate text-xs text-muted">
                      {b.artistName && <>{b.artistName} · </>}
                      from <span className="text-zinc-300">{b.playlistName ?? b.playlistId}</span>
                      {b.originalPosition != null && <> · was #{b.originalPosition + 1}</>}
                    </div>
                  </div>
                  <div className="shrink-0 text-right text-xs">
                    <div className={b.restoreAt <= Date.now() / 1000 ? "text-amber-400" : "text-spotify"}>
                      Returns {formatUntil(b.restoreAt)}
                    </div>
                    <div className="text-muted">{formatDateTime(b.restoreAt)}</div>
                  </div>
                  <Button variant="secondary" onClick={() => unbench(b)} disabled={unbenching !== null}>
                    {unbenching === b.id ? <Spinner /> : "Restore now"}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </section>
      </div>
    </div>
  );
}

function Chip({ active, onClick, children }: { active: boolean; onClick: () => void; children: string }) {
  return (
    <button
      onClick={onClick}
      className={`rounded-full border px-3 py-1 text-xs transition ${
        active ? "border-spotify bg-spotify/20 text-spotify" : "border-line text-zinc-300 hover:bg-line"
      }`}
    >
      {children}
    </button>
  );
}
