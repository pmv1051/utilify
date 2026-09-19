import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  errorMessage,
  type StatsImportProgress,
  type StatsStatus,
  type StatsSummary,
} from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader } from "../components/PlaylistPicker";
import { Columns, RankedList, StatTile, type ChartPoint, type RankRow } from "../components/Chart";
import {
  formatCount,
  formatDate,
  formatHoursShort,
  formatListeningLong,
  formatMonthLabel,
  formatRelative,
  plural,
} from "../lib/format";

/**
 * Stats are built from Spotify's data export, not from the Web API: there is
 * no history endpoint, and polling only ever saw 30-second snapshots. Every
 * command on this page is local, so it keeps working during an API quota
 * pause.
 */

type RangeKey = "all" | "last30" | "last12" | "custom" | string; // "y2025"

const THRESHOLDS = [
  { secs: 10, label: "10 seconds" },
  { secs: 30, label: "30 seconds" },
  { secs: 60, label: "1 minute" },
  { secs: 120, label: "2 minutes" },
];

const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const WEEKDAYS_SHORT = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

function startOfDay(iso: string): number | null {
  const [y, m, d] = iso.split("-").map(Number);
  if (!y || !m || !d) return null;
  return Math.floor(new Date(y, m - 1, d).getTime() / 1000);
}

function endOfDay(iso: string): number | null {
  const start = startOfDay(iso);
  return start === null ? null : start + 86399;
}

/** Local-time bounds for the chosen preset; `null` means "open end". */
function rangeBounds(key: RangeKey, from: string, to: string): [number | null, number | null] {
  const now = Math.floor(Date.now() / 1000);
  if (key === "all") return [null, null];
  if (key === "last30") return [now - 30 * 86400, now];
  if (key === "last12") return [now - 365 * 86400, now];
  if (key === "custom") return [from ? startOfDay(from) : null, to ? endOfDay(to) : null];
  const year = Number(key.slice(1));
  if (!year) return [null, null];
  return [
    Math.floor(new Date(year, 0, 1).getTime() / 1000),
    Math.floor(new Date(year + 1, 0, 1).getTime() / 1000) - 1,
  ];
}

function hourTick(h: number): string {
  const suffix = h < 12 ? "a" : "p";
  const hour = h % 12 === 0 ? 12 : h % 12;
  return `${hour}${suffix}`;
}

function hourTitle(h: number): string {
  const label = (n: number) => {
    const suffix = n < 12 || n === 24 ? "AM" : "PM";
    const hour = n % 12 === 0 ? 12 : n % 12;
    return `${hour} ${suffix}`;
  };
  return `${label(h)} – ${label((h + 1) % 24)}`;
}

export function StatsPage() {
  const toast = useApp((s) => s.toast);

  const [status, setStatus] = useState<StatsStatus | null>(null);
  const [summary, setSummary] = useState<StatsSummary | null>(null);
  const [loading, setLoading] = useState(true);
  const [importing, setImporting] = useState(false);
  const [progress, setProgress] = useState<StatsImportProgress | null>(null);
  const [confirmClear, setConfirmClear] = useState(false);

  const [range, setRange] = useState<RangeKey>("all");
  const [customFrom, setCustomFrom] = useState("");
  const [customTo, setCustomTo] = useState("");
  const [threshold, setThreshold] = useState(30);

  const [from, to] = useMemo(() => rangeBounds(range, customFrom, customTo), [range, customFrom, customTo]);

  useEffect(() => {
    const un = listen<StatsImportProgress>("stats-import-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const loadStatus = useCallback(async () => {
    const s = await api.getStatsStatus();
    setStatus(s);
    return s;
  }, []);

  useEffect(() => {
    loadStatus()
      .catch((e) => toast("error", errorMessage(e)))
      .finally(() => setLoading(false));
  }, [loadStatus, toast]);

  // Refetch on any filter change. The previous render stays on screen at
  // reduced opacity so the page never flashes a skeleton.
  useEffect(() => {
    if (!status || status.plays === 0) {
      setSummary(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    api
      .getStats(from, to, threshold)
      .then((s) => {
        if (!cancelled) setSummary(s);
      })
      .catch((e) => {
        if (!cancelled) toast("error", errorMessage(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [status, from, to, threshold, toast]);

  async function runImport() {
    setImporting(true);
    setProgress(null);
    try {
      const result = await api.importStreamingHistory();
      if (!result) return; // cancelled
      const parts = [`Imported ${formatCount(result.playsAdded)} plays from ${result.files} file(s)`];
      if (result.playsAdded < result.playsRead) {
        parts.push(`${formatCount(result.playsRead - result.playsAdded)} were already stored`);
      }
      if (result.otherRows > 0) {
        parts.push(`${formatCount(result.otherRows)} podcast or audiobook rows ignored`);
      }
      toast("success", `${parts.join(". ")}.`);
      await loadStatus();
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setImporting(false);
      setProgress(null);
    }
  }

  async function runClear() {
    try {
      await api.clearStreamingHistory();
      setSummary(null);
      await loadStatus();
      toast("success", "Imported listening history removed.");
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setConfirmClear(false);
    }
  }

  const hasData = (status?.plays ?? 0) > 0;

  // ---- charts -------------------------------------------------------------

  const hourPoints: ChartPoint[] = useMemo(
    () =>
      (summary?.byHour ?? []).map((b, i) => ({
        label: b.label,
        value: b.ms,
        title: hourTitle(i),
        detail: plural(b.plays, "play"),
        tick: i % 3 === 0 ? hourTick(i) : undefined,
      })),
    [summary],
  );

  const weekdayPoints: ChartPoint[] = useMemo(
    () =>
      (summary?.byWeekday ?? []).map((b, i) => ({
        label: b.label,
        value: b.ms,
        title: WEEKDAYS[i] ?? b.label,
        detail: plural(b.plays, "play"),
        tick: WEEKDAYS_SHORT[i],
      })),
    [summary],
  );

  const monthPoints: ChartPoint[] = useMemo(() => {
    const months = summary?.byMonth ?? [];
    // With years of history a tick per month collides; label each January,
    // and every month when the range is short enough to fit.
    const dense = months.length <= 14;
    return months.map((b) => ({
      label: b.label,
      value: b.ms,
      title: formatMonthLabel(b.label),
      detail: plural(b.plays, "play"),
      tick: dense
        ? formatMonthLabel(b.label).split(" ")[0]
        : b.label.endsWith("-01")
          ? b.label.slice(0, 4)
          : undefined,
    }));
  }, [summary]);

  const songRows: RankRow[] = useMemo(
    () =>
      (summary?.topSongs ?? []).map((s, i) => ({
        key: `${s.name}|${s.artist}|${i}`,
        primary: s.name,
        secondary: s.artist,
        value: s.plays,
        valueLabel: `${formatCount(s.plays)}`,
        extra: formatListeningLong(s.ms),
      })),
    [summary],
  );

  const artistRows: RankRow[] = useMemo(
    () =>
      (summary?.topArtists ?? []).map((a, i) => ({
        key: `${a.name}|${i}`,
        primary: a.name,
        secondary: plural(a.songs, "song"),
        value: a.ms,
        valueLabel: formatListeningLong(a.ms),
        extra: plural(a.plays, "play"),
      })),
    [summary],
  );

  const albumRows: RankRow[] = useMemo(
    () =>
      (summary?.topAlbums ?? []).map((a, i) => ({
        key: `${a.name}|${a.secondary}|${i}`,
        primary: a.name,
        secondary: a.secondary,
        value: a.ms,
        valueLabel: formatListeningLong(a.ms),
        extra: plural(a.plays, "play"),
      })),
    [summary],
  );

  const skippedRows: RankRow[] = useMemo(
    () =>
      (summary?.mostSkipped ?? []).map((s, i) => ({
        key: `${s.name}|${s.artist}|${i}`,
        primary: s.name,
        secondary: s.artist,
        value: s.skips,
        valueLabel: formatCount(s.skips),
        extra: `${Math.round((s.skips / Math.max(1, s.skips + s.plays)) * 100)}% of tries`,
      })),
    [summary],
  );

  const platformRows: RankRow[] = useMemo(
    () =>
      (summary?.byPlatform ?? []).map((b) => ({
        key: b.label,
        primary: b.label,
        value: b.ms,
        valueLabel: formatListeningLong(b.ms),
        extra: plural(b.plays, "play"),
      })),
    [summary],
  );

  const totals = summary?.totals;
  const skipRate = totals && totals.streams > 0 ? Math.round((totals.skips / totals.streams) * 100) : 0;
  const shuffleRate =
    totals && totals.streams > 0 ? Math.round((totals.shuffleStreams / totals.streams) * 100) : 0;
  const perDayMs = totals && totals.days > 0 ? totals.ms / totals.days : 0;

  return (
    <div className="flex h-full flex-col">
      <PageHeader
        title="Stats"
        subtitle={
          hasData && status?.firstTs && status?.lastTs
            ? `${formatCount(status.plays)} plays · ${formatDate(status.firstTs)} – ${formatDate(status.lastTs)}`
            : undefined
        }
      >
        {hasData && (
          <Button local variant="secondary" onClick={runImport} disabled={importing}>
            {importing ? <Spinner /> : null}
            {importing ? "Importing…" : "Import more…"}
          </Button>
        )}
      </PageHeader>

      <div className="flex-1 overflow-auto p-6">
        {!hasData ? (
          <EmptyState onImport={runImport} importing={importing} progress={progress} loading={loading} />
        ) : (
          <div className="mx-auto max-w-5xl space-y-4">
            {/* One filter row, above everything it scopes. */}
            <div className="flex flex-wrap items-center gap-3 rounded-lg border border-line bg-panel px-4 py-3">
              <label className="flex items-center gap-2 text-xs text-muted">
                Range
                <select
                  value={range}
                  onChange={(e) => setRange(e.target.value)}
                  className="rounded-md border border-line bg-panel-2 px-2 py-1 text-xs text-zinc-100"
                >
                  <option value="all">All time</option>
                  <option value="last30">Last 30 days</option>
                  <option value="last12">Last 12 months</option>
                  {(status?.years ?? []).map((y) => (
                    <option key={y} value={`y${y}`}>
                      {y}
                    </option>
                  ))}
                  <option value="custom">Custom…</option>
                </select>
              </label>

              {range === "custom" && (
                <div className="flex items-center gap-2 text-xs text-muted">
                  <input
                    type="date"
                    value={customFrom}
                    onChange={(e) => setCustomFrom(e.target.value)}
                    className="rounded-md border border-line bg-panel-2 px-2 py-1 text-xs text-zinc-100"
                  />
                  <span>to</span>
                  <input
                    type="date"
                    value={customTo}
                    onChange={(e) => setCustomTo(e.target.value)}
                    className="rounded-md border border-line bg-panel-2 px-2 py-1 text-xs text-zinc-100"
                  />
                </div>
              )}

              <label className="flex items-center gap-2 text-xs text-muted">
                Counts as a play after
                <select
                  value={threshold}
                  onChange={(e) => setThreshold(Number(e.target.value))}
                  className="rounded-md border border-line bg-panel-2 px-2 py-1 text-xs text-zinc-100"
                >
                  {THRESHOLDS.map((t) => (
                    <option key={t.secs} value={t.secs}>
                      {t.label}
                    </option>
                  ))}
                </select>
              </label>

              {loading && <Spinner />}

              {status?.lastImport && (
                <span className="ml-auto text-[11px] text-muted">
                  {status.lastImport.source}, imported {formatRelative(status.lastImport.importedAt)}
                </span>
              )}
            </div>

            <div className={`space-y-4 transition-opacity ${loading ? "opacity-60" : ""}`}>
              {totals && (
                <>
                  <div className="grid gap-4 lg:grid-cols-[minmax(0,1.1fr)_minmax(0,2fr)]">
                    <div className="rounded-lg border border-line bg-panel px-5 py-4">
                      <div className="text-xs text-muted">Listening time</div>
                      <div className="mt-1 text-5xl font-semibold leading-tight text-zinc-100">
                        {formatListeningLong(totals.ms)}
                      </div>
                      <div className="mt-1 text-xs text-muted">
                        about {formatCount(Math.round(totals.ms / 86_400_000))} full days of music
                      </div>
                      <div className="text-xs text-muted">
                        {formatListeningLong(perDayMs)} on each of {formatCount(totals.days)} days you listened
                      </div>
                    </div>

                    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3">
                      <StatTile label="Plays" value={formatCount(totals.plays)} hint="past the threshold" />
                      <StatTile label="Different songs" value={formatCount(totals.songs)} />
                      <StatTile label="Artists" value={formatCount(totals.artists)} />
                      <StatTile
                        label="Skipped past"
                        value={`${skipRate}%`}
                        hint={`${formatCount(totals.skips)} of ${formatCount(totals.streams)} starts`}
                      />
                      <StatTile label="Albums" value={formatCount(totals.albums)} />
                      <StatTile label="On shuffle" value={`${shuffleRate}%`} hint="of starts" />
                    </div>
                  </div>

                  <Columns
                    title="Time of day"
                    subtitle="Listening time by hour, in your local timezone"
                    points={hourPoints}
                    formatValue={formatListeningLong}
                    formatTick={formatHoursShort}
                    bucketHeading="Hour"
                    valueHeading="Hours"
                    detailHeading="Plays"
                    unit={3_600_000}
                  />

                  <div className="grid gap-4 lg:grid-cols-2">
                    <Columns
                      title="Day of the week"
                      subtitle="Listening time by weekday"
                      points={weekdayPoints}
                      formatValue={formatListeningLong}
                      formatTick={formatHoursShort}
                      bucketHeading="Day"
                      valueHeading="Hours"
                      detailHeading="Plays"
                      unit={3_600_000}
                    />
                    <Columns
                      title="Over time"
                      subtitle="Listening time per month"
                      points={monthPoints}
                      formatValue={formatListeningLong}
                      formatTick={formatHoursShort}
                      bucketHeading="Month"
                      valueHeading="Hours"
                      detailHeading="Plays"
                      unit={3_600_000}
                    />
                  </div>

                  <div className="grid gap-4 lg:grid-cols-2">
                    <RankedList
                      title="Most played songs"
                      subtitle="By play count. Editions of one song count together."
                      rows={songRows}
                      empty="No plays in this range."
                    />
                    <RankedList
                      title="Most played artists"
                      subtitle="By listening time"
                      rows={artistRows}
                      empty="No plays in this range."
                    />
                    <RankedList
                      title="Most played albums"
                      subtitle="By listening time"
                      rows={albumRows}
                      empty="No plays in this range."
                    />
                    <RankedList
                      title="Skipped past most"
                      subtitle="Songs you started and left before the threshold"
                      rows={skippedRows}
                      empty="Nothing was skipped at least three times."
                    />
                  </div>

                  <RankedList
                    title="Devices"
                    subtitle="Listening time by the device Spotify recorded"
                    rows={platformRows}
                    empty="No devices in this range."
                    initial={6}
                  />

                  <footer className="flex flex-wrap items-center gap-3 pb-2 text-[11px] text-muted">
                    <span>
                      The export records what played, not which playlist it came from, so listening per playlist
                      is not available.
                    </span>
                    {confirmClear ? (
                      <span className="ml-auto flex items-center gap-2">
                        Remove all imported history?
                        <Button local variant="danger" className="px-2 py-1 text-[11px]" onClick={runClear}>
                          Remove
                        </Button>
                        <Button
                          local
                          variant="ghost"
                          className="px-2 py-1 text-[11px]"
                          onClick={() => setConfirmClear(false)}
                        >
                          Cancel
                        </Button>
                      </span>
                    ) : (
                      <button
                        type="button"
                        onClick={() => setConfirmClear(true)}
                        className="ml-auto underline-offset-2 hover:text-zinc-200 hover:underline"
                      >
                        Remove imported history
                      </button>
                    )}
                  </footer>
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function EmptyState({
  onImport,
  importing,
  progress,
  loading,
}: {
  onImport: () => void;
  importing: boolean;
  progress: StatsImportProgress | null;
  loading: boolean;
}) {
  return (
    <section className="max-w-2xl rounded-lg border border-line bg-panel p-6">
      <h2 className="text-base font-semibold">Import your listening history</h2>
      <p className="mt-2 text-sm text-muted">
        Spotify's Web API cannot look back past the last few tracks, so these stats come from your data export.
        On Spotify: Account → Privacy settings → Download your data, tick{" "}
        <span className="text-zinc-300">Extended streaming history</span>, and confirm the email. It arrives
        within a few days as <span className="text-zinc-300">my_spotify_data.zip</span>.
      </p>
      <p className="mt-3 text-sm text-muted">
        Pick that zip below. Nothing leaves your computer: the plays are stored in Utilify's local database, and
        the IP addresses in the export are not read.
      </p>

      {importing && progress && (
        <p className="mt-4 text-xs text-muted">
          Reading {progress.file} ({progress.done}/{progress.total}) · {formatCount(progress.plays)} plays so far
        </p>
      )}

      <Button local className="mt-4" onClick={onImport} disabled={importing || loading}>
        {importing ? <Spinner /> : null}
        {importing ? "Importing…" : "Import data export…"}
      </Button>
    </section>
  );
}
