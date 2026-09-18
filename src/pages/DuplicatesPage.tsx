import { useMemo, useState } from "react";
import { api, errorMessage, type DuplicateReport, type RemovalRequest } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader, PlaylistMultiSelect, PlaylistSelect } from "../components/PlaylistPicker";
import { formatDuration } from "../lib/format";

type Mode = "single" | "cross";

function occKey(playlistId: string, uri: string, position: number) {
  return `${playlistId}|${uri}|${position}`;
}

export function DuplicatesPage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);

  const [mode, setMode] = useState<Mode>("single");
  const [singleId, setSingleId] = useState<string | null>(null);
  const [multiIds, setMultiIds] = useState<Set<string>>(new Set());
  const [matchByName, setMatchByName] = useState(true);
  const [report, setReport] = useState<DuplicateReport | null>(null);
  const [scanning, setScanning] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());

  const ids = mode === "single" ? (singleId ? [singleId] : []) : [...multiIds];
  const canScan = mode === "single" ? ids.length === 1 : ids.length >= 2;

  async function scan() {
    setScanning(true);
    setReport(null);
    setSelected(new Set());
    try {
      const r = await api.scanDuplicates(ids, matchByName);
      setReport(r);
      if (r.mode === "single") setSelected(allButFirst(r));
      if (r.groups.length === 0) toast("success", "No duplicates found.");
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setScanning(false);
    }
  }

  function allButFirst(r: DuplicateReport): Set<string> {
    const s = new Set<string>();
    for (const g of r.groups) {
      g.occurrences.slice(1).forEach((o) => s.add(occKey(o.playlistId, o.track.uri, o.track.position)));
    }
    return s;
  }

  function toggle(key: string) {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
  }

  const removals = useMemo<RemovalRequest[]>(() => {
    if (!report) return [];
    const byPair = new Map<string, RemovalRequest>();
    for (const g of report.groups) {
      for (const o of g.occurrences) {
        const key = occKey(o.playlistId, o.track.uri, o.track.position);
        if (!selected.has(key)) continue;
        const pairKey = `${o.playlistId}|${o.track.uri}`;
        const req = byPair.get(pairKey) ?? { playlistId: o.playlistId, uri: o.track.uri, positions: [] };
        if (!req.positions.includes(o.track.position)) req.positions.push(o.track.position);
        byPair.set(pairKey, req);
      }
    }
    return [...byPair.values()];
  }, [report, selected]);

  const selectedCount = removals.reduce((n, r) => n + r.positions.length, 0);

  async function remove() {
    if (removals.length === 0) return;
    setRemoving(true);
    try {
      const s = await api.removeDuplicates(removals);
      toast("success", `Removed ${s.removed} cop${s.removed === 1 ? "y" : "ies"} across ${s.playlists} playlist${s.playlists === 1 ? "" : "s"}.`);
      await refreshPlaylists(true);
      await scan();
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setRemoving(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Duplicate Scanner" subtitle={report ? `${report.groups.length} group${report.groups.length === 1 ? "" : "s"}` : undefined} />

      <div className="flex-1 space-y-5 overflow-auto p-6">
        <section className="rounded-lg border border-line bg-panel p-5">
          <div className="mb-3 flex flex-wrap items-center gap-2">
            <ModeButton active={mode === "single"} onClick={() => setMode("single")}>
              Within one playlist
            </ModeButton>
            <ModeButton active={mode === "cross"} onClick={() => setMode("cross")}>
              Across playlists
            </ModeButton>
            <label className="ml-4 flex items-center gap-2 text-sm text-zinc-300">
              <input type="checkbox" checked={matchByName} onChange={(e) => setMatchByName(e.target.checked)} />
              Also match by title + artist
            </label>
            <div className="ml-auto">
              <Button onClick={scan} disabled={!canScan || scanning || removing}>
                {scanning ? <Spinner /> : "Scan"}
              </Button>
            </div>
          </div>

          {mode === "single" ? (
            <PlaylistSelect value={singleId} onChange={setSingleId} disabled={scanning} />
          ) : (
            <PlaylistMultiSelect selected={multiIds} onChange={setMultiIds} disabled={scanning} />
          )}
          <p className="mt-3 text-xs text-muted">
            {mode === "single"
              ? "Finds tracks that appear more than once in the playlist. By default every copy but the first is selected for removal."
              : "Finds tracks that appear in two or more of the selected playlists. Tick the copies you want removed; nothing is selected by default."}
            {" "}Title + artist matching catches re-releases and deluxe editions that carry different Spotify URIs.
          </p>
        </section>

        {report && report.groups.length > 0 && (
          <section className="rounded-lg border border-line bg-panel">
            <div className="flex flex-wrap items-center gap-3 border-b border-line px-5 py-3 text-sm">
              <span className="text-muted">
                Scanned {report.tracksScanned} tracks in {report.playlistsScanned} playlist{report.playlistsScanned === 1 ? "" : "s"}.
              </span>
              <button className="text-xs text-zinc-300 hover:text-white" onClick={() => setSelected(allButFirst(report))}>
                Select all but first
              </button>
              <button className="text-xs text-zinc-300 hover:text-white" onClick={() => setSelected(new Set())}>
                Clear selection
              </button>
              <div className="ml-auto">
                <Button variant="danger" onClick={remove} disabled={selectedCount === 0 || removing || scanning}>
                  {removing ? <Spinner /> : `Remove ${selectedCount} selected`}
                </Button>
              </div>
            </div>
            <ul className="divide-y divide-line">
              {report.groups.map((g) => (
                <li key={`${g.matchKind}:${g.key}`} className="px-5 py-3">
                  <div className="mb-2 flex items-center gap-2">
                    <span className="font-medium">{g.name}</span>
                    <span className="text-sm text-muted">{g.artists}</span>
                    <span
                      className={`rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase ${
                        g.matchKind === "uri" ? "bg-spotify/20 text-spotify" : "bg-amber-400/20 text-amber-300"
                      }`}
                      title={g.matchKind === "uri" ? "Identical Spotify track" : "Same title and artist, different Spotify tracks"}
                    >
                      {g.matchKind === "uri" ? "same track" : "same title"}
                    </span>
                    <span className="text-xs text-muted">× {g.occurrences.length}</span>
                  </div>
                  <ul className="space-y-1">
                    {g.occurrences.map((o) => {
                      const key = occKey(o.playlistId, o.track.uri, o.track.position);
                      return (
                        <li key={key}>
                          <label className="flex cursor-pointer items-center gap-3 rounded px-2 py-1 text-sm hover:bg-panel-2">
                            <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key)} />
                            {report.mode === "cross" && (
                              <span className="w-48 shrink-0 truncate text-zinc-300">{o.playlistName}</span>
                            )}
                            <span className="w-10 shrink-0 text-right text-xs text-muted">#{o.track.position + 1}</span>
                            <span className="min-w-0 flex-1 truncate text-zinc-300">
                              {o.track.name}
                              <span className="text-muted"> · {o.track.album ?? "—"}</span>
                            </span>
                            <span className="shrink-0 text-xs text-muted">{formatDuration(o.track.durationMs)}</span>
                            <span className="w-24 shrink-0 text-right text-xs text-muted">
                              {o.track.addedAt ? o.track.addedAt.slice(0, 10) : ""}
                            </span>
                          </label>
                        </li>
                      );
                    })}
                  </ul>
                </li>
              ))}
            </ul>
          </section>
        )}

        {report && report.groups.length === 0 && (
          <p className="text-sm text-muted">No duplicates in {report.tracksScanned} tracks.</p>
        )}
      </div>
    </div>
  );
}

function ModeButton({ active, onClick, children }: { active: boolean; onClick: () => void; children: string }) {
  return (
    <button
      onClick={onClick}
      className={`rounded-md px-3 py-1.5 text-sm transition ${
        active ? "bg-panel-2 text-white" : "text-zinc-400 hover:bg-panel-2 hover:text-zinc-100"
      }`}
    >
      {children}
    </button>
  );
}
