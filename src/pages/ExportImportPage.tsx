import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, errorMessage, type ImportMatch, type ImportProgress } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader, PlaylistSelect } from "../components/PlaylistPicker";
import { formatDuration } from "../lib/format";

const STATUS_STYLE: Record<string, string> = {
  exact: "text-spotify",
  fuzzy: "text-amber-300",
  unsure: "text-red-400",
  none: "text-red-400",
};

export function ExportImportPage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);

  // ---- export ----
  const [exportId, setExportId] = useState<string | null>(null);
  const [format, setFormat] = useState<"csv" | "txt">("csv");
  const [exporting, setExporting] = useState<"save" | "copy" | null>(null);

  async function doExport(mode: "save" | "copy") {
    if (!exportId) return;
    setExporting(mode);
    try {
      const c = await api.exportPlaylist(exportId, format);
      if (mode === "copy") {
        await navigator.clipboard.writeText(c.content);
        toast("success", `Copied ${c.tracks} tracks as ${format.toUpperCase()} to the clipboard.`);
      } else {
        const path = await api.saveTextFile(c.fileName, c.content);
        if (path) toast("success", `Saved ${c.tracks} tracks to ${path}`);
      }
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setExporting(null);
    }
  }

  // ---- import ----
  const [text, setText] = useState("");
  const [matching, setMatching] = useState(false);
  const [progress, setProgress] = useState<ImportProgress | null>(null);
  const [matches, setMatches] = useState<ImportMatch[] | null>(null);
  const [choice, setChoice] = useState<Record<number, number | null>>({}); // line index → candidate index or null (skip)
  const [dest, setDest] = useState<"new" | "existing">("new");
  const [newName, setNewName] = useState("");
  const [existingId, setExistingId] = useState<string | null>(null);
  const [randomize, setRandomize] = useState(false);
  const [building, setBuilding] = useState(false);

  useEffect(() => {
    const un = listen<ImportProgress>("import-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const lineCount = useMemo(
    () => text.split(/\r?\n/).filter((l) => l.trim() && !l.trim().startsWith("#")).length,
    [text],
  );

  async function findMatches() {
    setMatching(true);
    setMatches(null);
    setProgress(null);
    try {
      const m = await api.importSearch(text.split(/\r?\n/));
      setMatches(m);
      const c: Record<number, number | null> = {};
      m.forEach((x, i) => {
        c[i] = x.status === "none" || x.status === "unsure" ? null : x.best;
      });
      setChoice(c);
      const found = m.filter((x) => x.status === "exact" || x.status === "fuzzy").length;
      toast("info", `${found} of ${m.length} lines matched confidently. Review the rest below.`);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setMatching(false);
    }
  }

  const chosenUris = useMemo(() => {
    if (!matches) return [];
    const uris: string[] = [];
    matches.forEach((m, i) => {
      const ci = choice[i];
      if (ci != null && m.candidates[ci]) uris.push(m.candidates[ci].uri);
    });
    return uris;
  }, [matches, choice]);

  async function build() {
    setBuilding(true);
    try {
      if (dest === "new") {
        const p = await api.createPlaylistFromTracks(newName || "Imported", chosenUris, randomize);
        toast("success", `Created "${p.name}" with ${p.trackCount} tracks.`);
      } else if (existingId) {
        const n = await api.addTracksToPlaylist(existingId, chosenUris);
        toast("success", `Added ${n} tracks.`);
      }
      await refreshPlaylists(true);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBuilding(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Export / Import" />
      <div className="flex-1 space-y-5 overflow-auto p-6">
        <section className="rounded-lg border border-line bg-panel p-5">
          <h2 className="mb-3 text-base font-semibold">Export</h2>
          <div className="flex flex-wrap items-center gap-3">
            <PlaylistSelect value={exportId} onChange={setExportId} disabled={exporting !== null} />
            <select
              value={format}
              onChange={(e) => setFormat(e.target.value as "csv" | "txt")}
              className="rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
            >
              <option value="csv">CSV (title, artists, album, duration, added, URI)</option>
              <option value="txt">Text (Artist — Track per line)</option>
            </select>
            <Button onClick={() => doExport("save")} disabled={!exportId || exporting !== null}>
              {exporting === "save" ? <Spinner /> : "Save file…"}
            </Button>
            <Button variant="secondary" onClick={() => doExport("copy")} disabled={!exportId || exporting !== null}>
              {exporting === "copy" ? <Spinner /> : "Copy to clipboard"}
            </Button>
          </div>
        </section>

        <section className="rounded-lg border border-line bg-panel p-5">
          <h2 className="mb-1 text-base font-semibold">Import</h2>
          <p className="mb-3 text-xs text-muted">
            One track per line as <span className="text-zinc-300">Artist — Track</span> (a hyphen or a tab works too).
            Lines without a separator are searched as a title. Each line costs one search request; up to 500 lines.
          </p>
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={"Daft Punk — One More Time\nRadiohead - Karma Police"}
            rows={8}
            disabled={matching}
            className="w-full rounded-md border border-line bg-ink px-3 py-2 font-mono text-sm outline-none focus:border-spotify"
          />
          <div className="mt-2 flex items-center gap-3">
            <Button scope="catalog" onClick={findMatches} disabled={lineCount === 0 || matching || building}>
              {matching ? <Spinner /> : `Find matches (${lineCount})`}
            </Button>
            {matching && progress && (
              <span className="text-xs text-muted">
                {progress.done} / {progress.total}
              </span>
            )}
          </div>
        </section>

        {matches && (
          <section className="rounded-lg border border-line bg-panel">
            <div className="flex flex-wrap items-center gap-3 border-b border-line px-5 py-3 text-sm">
              <span className="text-muted">{chosenUris.length} of {matches.length} lines will be imported.</span>
              <div className="ml-auto flex flex-wrap items-center gap-2">
                <select
                  value={dest}
                  onChange={(e) => setDest(e.target.value as "new" | "existing")}
                  className="rounded-md border border-line bg-ink px-2 py-1.5 text-sm outline-none focus:border-spotify"
                >
                  <option value="new">New playlist</option>
                  <option value="existing">Add to existing</option>
                </select>
                {dest === "new" ? (
                  <>
                    <span className="text-xs text-muted">Utilify:</span>
                    <input
                      value={newName}
                      onChange={(e) => setNewName(e.target.value)}
                      placeholder="Imported"
                      className="w-44 rounded-md border border-line bg-ink px-2 py-1.5 text-sm outline-none focus:border-spotify"
                    />
                    <label className="flex items-center gap-1 text-xs text-zinc-300">
                      <input type="checkbox" checked={randomize} onChange={(e) => setRandomize(e.target.checked)} />
                      Randomize
                    </label>
                  </>
                ) : (
                  <PlaylistSelect value={existingId} onChange={setExistingId} />
                )}
                <Button onClick={build} disabled={chosenUris.length === 0 || building || (dest === "existing" && !existingId)}>
                  {building ? <Spinner /> : dest === "new" ? "Create playlist" : "Add tracks"}
                </Button>
              </div>
            </div>
            <ul className="divide-y divide-line">
              {matches.map((m, i) => {
                const ci = choice[i];
                return (
                  <li key={i} className="flex items-center gap-3 px-5 py-2 text-sm">
                    <input
                      type="checkbox"
                      checked={ci != null}
                      disabled={m.candidates.length === 0}
                      onChange={(e) => setChoice((c) => ({ ...c, [i]: e.target.checked ? (m.best ?? 0) : null }))}
                    />
                    <span className="w-64 shrink-0 truncate font-mono text-xs text-zinc-300" title={m.line}>
                      {m.line}
                    </span>
                    <span className={`w-14 shrink-0 text-xs ${STATUS_STYLE[m.status]}`}>{m.status}</span>
                    {m.candidates.length === 0 ? (
                      <span className="text-muted">No results</span>
                    ) : (
                      <select
                        value={ci ?? ""}
                        onChange={(e) => setChoice((c) => ({ ...c, [i]: e.target.value === "" ? null : Number(e.target.value) }))}
                        className="min-w-0 flex-1 rounded-md border border-line bg-ink px-2 py-1 text-sm outline-none focus:border-spotify"
                      >
                        <option value="">— skip —</option>
                        {m.candidates.map((cand, k) => (
                          <option key={cand.uri} value={k}>
                            {Math.round(cand.score * 100)}% · {cand.name} — {cand.artists}
                            {cand.album ? ` (${cand.album})` : ""} · {formatDuration(cand.durationMs)}
                          </option>
                        ))}
                      </select>
                    )}
                  </li>
                );
              })}
            </ul>
          </section>
        )}
      </div>
    </div>
  );
}
