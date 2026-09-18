import { useState } from "react";
import { api, errorMessage, type MergeResult } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader, PlaylistMultiSelect } from "../components/PlaylistPicker";

export function MergePage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);
  const playlists = useApp((s) => s.playlists);

  const [ids, setIds] = useState<Set<string>>(new Set());
  const [name, setName] = useState("");
  const [dedupeByName, setDedupeByName] = useState(true);
  const [randomize, setRandomize] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<MergeResult | null>(null);

  const total = [...ids].reduce((n, id) => n + (playlists.find((p) => p.id === id)?.trackCount ?? 0), 0);

  async function merge() {
    setBusy(true);
    setResult(null);
    try {
      const r = await api.mergePlaylists([...ids], name, dedupeByName, randomize);
      setResult(r);
      toast("success", `Created "${r.playlist.name}" with ${r.playlist.trackCount} tracks.`);
      await refreshPlaylists(true);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Playlist Merge" />
      <div className="flex-1 space-y-5 overflow-auto p-6">
        <section className="rounded-lg border border-line bg-panel p-5">
          <h2 className="mb-3 text-base font-semibold">Sources</h2>
          <PlaylistMultiSelect selected={ids} onChange={setIds} disabled={busy} />
          <p className="mt-2 text-xs text-muted">
            {ids.size} playlist{ids.size === 1 ? "" : "s"} · {total} tracks before deduplication. Sources are left untouched.
          </p>
        </section>

        <section className="rounded-lg border border-line bg-panel p-5">
          <h2 className="mb-3 text-base font-semibold">New playlist</h2>
          <div className="flex flex-wrap items-center gap-3">
            <span className="text-sm text-muted">Utilify:</span>
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="name (default: Merge of …)"
              disabled={busy}
              className="w-72 rounded-md border border-line bg-ink px-3 py-1.5 text-sm outline-none focus:border-spotify"
            />
            <label className="flex items-center gap-2 text-sm text-zinc-300">
              <input type="checkbox" checked={dedupeByName} onChange={(e) => setDedupeByName(e.target.checked)} disabled={busy} />
              Also skip same title + artist
            </label>
            <label className="flex items-center gap-2 text-sm text-zinc-300">
              <input type="checkbox" checked={randomize} onChange={(e) => setRandomize(e.target.checked)} disabled={busy} />
              Randomize order
            </label>
            <div className="ml-auto">
              <Button onClick={merge} disabled={ids.size < 2 || busy}>
                {busy ? <Spinner /> : "Merge"}
              </Button>
            </div>
          </div>
        </section>

        {result && (
          <section className="rounded-lg border border-spotify/40 bg-panel p-5 text-sm">
            <div className="font-semibold">{result.playlist.name}</div>
            <div className="mt-1 text-muted">
              {result.playlist.trackCount} tracks from {result.sources} playlists · {result.tracksSeen} scanned ·{" "}
              {result.duplicatesSkipped} duplicate{result.duplicatesSkipped === 1 ? "" : "s"} skipped
              {result.playlist.trackCount < result.playlist.requested &&
                ` · ${result.playlist.requested - result.playlist.trackCount} not accepted by Spotify`}
            </div>
          </section>
        )}
      </div>
    </div>
  );
}
