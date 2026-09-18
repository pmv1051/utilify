import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage, type StatsSummary } from "../lib/api";
import { useApp } from "../stores/app";
import { Spinner } from "../components/Spinner";
import { PageHeader } from "../components/PlaylistPicker";
import { formatListening } from "../lib/format";
import { playlistIdFromContext } from "../lib/bench";

const RANGES: { label: string; days: number | null }[] = [
  { label: "7 days", days: 7 },
  { label: "30 days", days: 30 },
  { label: "90 days", days: 90 },
  { label: "All time", days: null },
];

export function StatsPage() {
  const toast = useApp((s) => s.toast);
  const playlists = useApp((s) => s.playlists);
  const [days, setDays] = useState<number | null>(30);
  const [stats, setStats] = useState<StatsSummary | null>(null);
  const [loading, setLoading] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      setStats(await api.getStats(days));
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setLoading(false);
    }
  }, [days, toast]);

  useEffect(() => {
    load();
  }, [load]);

  // Refresh when a play is finalized so the dashboard stays live.
  useEffect(() => {
    const un = listen("play-finished", () => load());
    return () => {
      un.then((f) => f());
    };
  }, [load]);

  const contextLabel = useMemo(
    () => (uri: string) => {
      const pid = playlistIdFromContext(uri);
      if (pid) return playlists.find((p) => p.id === pid)?.name ?? "Playlist (not in library)";
      if (uri.startsWith("spotify:album:")) return "An album";
      if (uri.startsWith("spotify:artist:")) return "An artist page";
      if (uri.startsWith("spotify:collection:")) return "Liked Songs";
      return uri;
    },
    [playlists],
  );

  const skipRate = stats && stats.plays > 0 ? Math.round((stats.skips / stats.plays) * 100) : 0;
  const hourMax = stats ? Math.max(1, ...stats.hours) : 1;
  const dayMax = stats ? Math.max(1, ...stats.days.map((d) => d.listenedMs)) : 1;

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Stats" subtitle={loading ? undefined : stats ? `${stats.plays} plays` : undefined}>
        {loading && <Spinner />}
      </PageHeader>

      <div className="flex flex-wrap items-center gap-2 border-b border-line bg-panel px-6 py-3">
        {RANGES.map((r) => (
          <button
            key={r.label}
            onClick={() => setDays(r.days)}
            className={`rounded-full border px-3 py-1 text-xs transition ${
              days === r.days ? "border-spotify bg-spotify/20 text-spotify" : "border-line text-zinc-300 hover:bg-line"
            }`}
          >
            {r.label}
          </button>
        ))}
        {stats?.firstLoggedAt && (
          <span className="ml-auto text-xs text-muted">
            Logging since {new Date(stats.firstLoggedAt * 1000).toLocaleDateString()}
          </span>
        )}
      </div>

      <div className="flex-1 space-y-5 overflow-auto p-6">
        {!stats ? null : stats.plays === 0 ? (
          <div className="rounded-lg border border-dashed border-line p-8 text-center text-sm text-muted">
            Nothing logged in this range yet. Plays are recorded while Utilify runs (window open or in the tray) and
            Spotify is playing. A play counts once 10 seconds are heard; anything shorter is a skip.
          </div>
        ) : (
          <>
            <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
              <Tile label="Listening time" value={formatListening(stats.listenedMs)} />
              <Tile label="Plays" value={stats.plays.toLocaleString()} />
              <Tile label="Unique tracks" value={stats.uniqueTracks.toLocaleString()} />
              <Tile label="Artists" value={stats.uniqueArtists.toLocaleString()} />
              <Tile label="Skip rate" value={`${skipRate}%`} hint={`${stats.skips} skipped`} />
            </div>

            <div className="grid grid-cols-1 gap-5 xl:grid-cols-2">
              <Section title="Top tracks">
                <BarList
                  rows={stats.topTracks.map((t) => ({
                    key: t.uri,
                    label: t.name ?? t.uri,
                    sub: t.artist ?? "",
                    value: t.listenedMs,
                    right: `${formatListening(t.listenedMs)} · ${t.plays}×`,
                  }))}
                />
              </Section>
              <Section title="Top artists">
                <BarList
                  rows={stats.topArtists.map((a) => ({
                    key: a.artistId ?? a.artist,
                    label: a.artist,
                    sub: `${a.plays} plays${a.skips ? ` · ${a.skips} skips` : ""}`,
                    value: a.listenedMs,
                    right: formatListening(a.listenedMs),
                  }))}
                />
              </Section>
              <Section title="By playlist / context">
                <BarList
                  rows={stats.contexts.map((c) => ({
                    key: c.contextUri,
                    label: contextLabel(c.contextUri),
                    sub: `${c.plays} plays`,
                    value: c.listenedMs,
                    right: formatListening(c.listenedMs),
                  }))}
                />
              </Section>
              <Section title="Most skipped">
                {stats.mostSkipped.length === 0 ? (
                  <p className="px-4 py-3 text-sm text-muted">No repeat skips yet.</p>
                ) : (
                  <BarList
                    rows={stats.mostSkipped.map((t) => ({
                      key: t.uri,
                      label: t.name ?? t.uri,
                      sub: t.artist ?? "",
                      value: t.skips,
                      right: `${t.skips} of ${t.plays} skipped`,
                    }))}
                  />
                )}
              </Section>
            </div>

            <Section title="Time of day">
              <div className="flex h-32 items-end gap-1 px-4 pb-2 pt-4">
                {stats.hours.map((ms, h) => (
                  <div key={h} className="flex flex-1 flex-col items-center gap-1" title={`${h}:00 · ${formatListening(ms)}`}>
                    <div className="w-full rounded-t bg-spotify/80" style={{ height: `${(ms / hourMax) * 100}%`, minHeight: ms > 0 ? 2 : 0 }} />
                    <span className="text-[10px] text-muted">{h % 3 === 0 ? h : ""}</span>
                  </div>
                ))}
              </div>
            </Section>

            <Section title="Daily listening">
              <div className="flex h-32 items-end gap-0.5 px-4 pb-2 pt-4">
                {stats.days.map((d) => (
                  <div
                    key={d.date}
                    className="flex-1 rounded-t bg-spotify/60 hover:bg-spotify"
                    title={`${d.date} · ${formatListening(d.listenedMs)} · ${d.plays} plays`}
                    style={{ height: `${(d.listenedMs / dayMax) * 100}%`, minHeight: d.listenedMs > 0 ? 2 : 0 }}
                  />
                ))}
              </div>
              <div className="flex justify-between px-4 pb-2 text-[10px] text-muted">
                <span>{stats.days[0]?.date}</span>
                <span>{stats.days[stats.days.length - 1]?.date}</span>
              </div>
            </Section>
          </>
        )}
      </div>
    </div>
  );
}

function Tile({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <div className="rounded-lg border border-line bg-panel p-4">
      <div className="text-xs uppercase tracking-wider text-muted">{label}</div>
      <div className="mt-1 text-2xl font-bold">{value}</div>
      {hint && <div className="text-xs text-muted">{hint}</div>}
    </div>
  );
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-line bg-panel">
      <h2 className="border-b border-line px-4 py-2 text-xs font-semibold uppercase tracking-wider text-muted">{title}</h2>
      {children}
    </section>
  );
}

function BarList({ rows }: { rows: { key: string; label: string; sub: string; value: number; right: string }[] }) {
  const max = Math.max(1, ...rows.map((r) => r.value));
  if (rows.length === 0) return <p className="px-4 py-3 text-sm text-muted">Nothing yet.</p>;
  return (
    <ul className="divide-y divide-line">
      {rows.slice(0, 15).map((r, i) => (
        <li key={r.key} className="relative px-4 py-2 text-sm">
          <div className="absolute inset-y-0 left-0 bg-spotify/10" style={{ width: `${(r.value / max) * 100}%` }} />
          <div className="relative flex items-center gap-3">
            <span className="w-5 shrink-0 text-right text-xs text-muted">{i + 1}</span>
            <span className="min-w-0 flex-1">
              <span className="block truncate">{r.label}</span>
              {r.sub && <span className="block truncate text-xs text-muted">{r.sub}</span>}
            </span>
            <span className="shrink-0 text-xs text-muted">{r.right}</span>
          </div>
        </li>
      ))}
    </ul>
  );
}
