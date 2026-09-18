import { useState } from "react";
import { api, errorMessage, type DiffResult, type TrackRow } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { PageHeader, PlaylistSelect } from "../components/PlaylistPicker";
import { TrackList, trackKey } from "../components/TrackList";

type Section = "onlyA" | "onlyB" | "both";

export function DiffPage() {
  const toast = useApp((s) => s.toast);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);

  const [aId, setAId] = useState<string | null>(null);
  const [bId, setBId] = useState<string | null>(null);
  const [matchByName, setMatchByName] = useState(false);
  const [result, setResult] = useState<DiffResult | null>(null);
  const [comparing, setComparing] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [selected, setSelected] = useState<Record<Section, Set<string>>>({
    onlyA: new Set(),
    onlyB: new Set(),
    both: new Set(),
  });
  const [naming, setNaming] = useState<{ section: Section; name: string } | null>(null);

  async function compare() {
    if (!aId || !bId) return;
    setComparing(true);
    setResult(null);
    try {
      const r = await api.diffPlaylists(aId, bId, matchByName);
      setResult(r);
      setSelected({
        onlyA: new Set(r.onlyA.map(trackKey)),
        onlyB: new Set(r.onlyB.map(trackKey)),
        both: new Set(r.both.map((p) => trackKey(p.a))),
      });
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setComparing(false);
    }
  }

  function tracksOf(section: Section): TrackRow[] {
    if (!result) return [];
    if (section === "onlyA") return result.onlyA;
    if (section === "onlyB") return result.onlyB;
    return result.both.map((p) => p.a);
  }

  /** URIs of the selected tracks in a section; for "both" pick A's or B's copy. */
  function selectedUris(section: Section, side: "a" | "b" = "a"): string[] {
    if (!result) return [];
    const sel = selected[section];
    if (section === "both") {
      return result.both.filter((p) => sel.has(trackKey(p.a))).map((p) => (side === "a" ? p.a.uri : p.b.uri));
    }
    return tracksOf(section).filter((t) => sel.has(trackKey(t))).map((t) => t.uri);
  }

  function toggle(section: Section, key: string) {
    setSelected((prev) => {
      const next = new Set(prev[section]);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return { ...prev, [section]: next };
    });
  }

  function setAll(section: Section, on: boolean) {
    setSelected((prev) => ({ ...prev, [section]: on ? new Set(tracksOf(section).map(trackKey)) : new Set() }));
  }

  async function run(label: string, fn: () => Promise<string>) {
    setBusy(label);
    try {
      const msg = await fn();
      toast("success", msg);
      await refreshPlaylists(true);
      await compare();
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  const addTo = (section: Section, target: "a" | "b") =>
    run(`${section}-add-${target}`, async () => {
      if (!result) return "";
      const uris = selectedUris(section);
      const dest = target === "a" ? result.a : result.b;
      const n = await api.addTracksToPlaylist(dest.id, uris);
      return `Added ${n} track${n === 1 ? "" : "s"} to "${dest.name}".`;
    });

  const removeFrom = (section: Section, target: "a" | "b") =>
    run(`${section}-remove-${target}`, async () => {
      if (!result) return "";
      const uris = selectedUris(section, target);
      const dest = target === "a" ? result.a : result.b;
      const n = await api.removeTracksFromPlaylist(dest.id, uris);
      return `Removed ${n} track${n === 1 ? "" : "s"} from "${dest.name}".`;
    });

  const createNew = (section: Section, name: string) =>
    run(`${section}-create`, async () => {
      const uris = selectedUris(section);
      const p = await api.createPlaylistFromTracks(name, uris, false);
      setNaming(null);
      return `Created "${p.name}" with ${p.trackCount} tracks.`;
    });

  return (
    <div className="flex h-full flex-col">
      <PageHeader title="Playlist Diff" />
      <div className="flex-1 space-y-5 overflow-auto p-6">
        <section className="rounded-lg border border-line bg-panel p-5">
          <div className="flex flex-wrap items-center gap-3">
            <PlaylistSelect value={aId} onChange={setAId} placeholder="Playlist A…" exclude={bId} disabled={comparing} />
            <span className="text-muted">vs</span>
            <PlaylistSelect value={bId} onChange={setBId} placeholder="Playlist B…" exclude={aId} disabled={comparing} />
            <label className="flex items-center gap-2 text-sm text-zinc-300">
              <input type="checkbox" checked={matchByName} onChange={(e) => setMatchByName(e.target.checked)} />
              Match by title + artist
            </label>
            <div className="ml-auto">
              <Button onClick={compare} disabled={!aId || !bId || comparing || busy !== null}>
                {comparing ? <Spinner /> : "Compare"}
              </Button>
            </div>
          </div>
          <p className="mt-3 text-xs text-muted">
            Matching by Spotify track is exact. Title + artist matching also pairs re-releases and remasters that
            carry different URIs; those pairs are marked in the "in both" list.
          </p>
        </section>

        {result && (
          <>
            <DiffSection
              title={`Only in ${result.a.name}`}
              count={result.onlyA.length}
              tracks={result.onlyA}
              selected={selected.onlyA}
              onToggle={(k) => toggle("onlyA", k)}
              onAll={(on) => setAll("onlyA", on)}
              busy={busy}
              actions={[
                { id: "onlyA-add-b", label: `Add to ${result.b.name}`, onClick: () => addTo("onlyA", "b") },
                { id: "onlyA-remove-a", label: `Remove from ${result.a.name}`, onClick: () => removeFrom("onlyA", "a"), danger: true },
              ]}
              naming={naming?.section === "onlyA" ? naming.name : null}
              onNaming={(name) => setNaming(name === null ? null : { section: "onlyA", name })}
              onCreate={(name) => createNew("onlyA", name)}
            />
            <DiffSection
              title={`Only in ${result.b.name}`}
              count={result.onlyB.length}
              tracks={result.onlyB}
              selected={selected.onlyB}
              onToggle={(k) => toggle("onlyB", k)}
              onAll={(on) => setAll("onlyB", on)}
              busy={busy}
              actions={[
                { id: "onlyB-add-a", label: `Add to ${result.a.name}`, onClick: () => addTo("onlyB", "a") },
                { id: "onlyB-remove-b", label: `Remove from ${result.b.name}`, onClick: () => removeFrom("onlyB", "b"), danger: true },
              ]}
              naming={naming?.section === "onlyB" ? naming.name : null}
              onNaming={(name) => setNaming(name === null ? null : { section: "onlyB", name })}
              onCreate={(name) => createNew("onlyB", name)}
            />
            <DiffSection
              title="In both"
              count={result.both.length}
              tracks={result.both.map((p) => p.a)}
              note={(t) => {
                const pair = result.both.find((p) => trackKey(p.a) === trackKey(t));
                return pair?.differentUri ? "different version in B" : null;
              }}
              selected={selected.both}
              onToggle={(k) => toggle("both", k)}
              onAll={(on) => setAll("both", on)}
              busy={busy}
              actions={[
                { id: "both-remove-a", label: `Remove from ${result.a.name}`, onClick: () => removeFrom("both", "a"), danger: true },
                { id: "both-remove-b", label: `Remove from ${result.b.name}`, onClick: () => removeFrom("both", "b"), danger: true },
              ]}
              naming={naming?.section === "both" ? naming.name : null}
              onNaming={(name) => setNaming(name === null ? null : { section: "both", name })}
              onCreate={(name) => createNew("both", name)}
            />
          </>
        )}
      </div>
    </div>
  );
}

function DiffSection({
  title,
  count,
  tracks,
  note,
  selected,
  onToggle,
  onAll,
  busy,
  actions,
  naming,
  onNaming,
  onCreate,
}: {
  title: string;
  count: number;
  tracks: TrackRow[];
  note?: (t: TrackRow) => string | null;
  selected: Set<string>;
  onToggle: (key: string) => void;
  onAll: (on: boolean) => void;
  busy: string | null;
  actions: { id: string; label: string; onClick: () => void; danger?: boolean }[];
  naming: string | null;
  onNaming: (name: string | null) => void;
  onCreate: (name: string) => void;
}) {
  const n = selected.size;
  return (
    <section className="rounded-lg border border-line bg-panel">
      <div className="flex flex-wrap items-center gap-2 border-b border-line px-4 py-3">
        <h2 className="font-semibold">{title}</h2>
        <span className="text-sm text-muted">
          {count} · {n} selected
        </span>
        <button className="text-xs text-zinc-300 hover:text-white" onClick={() => onAll(true)}>
          All
        </button>
        <button className="text-xs text-zinc-300 hover:text-white" onClick={() => onAll(false)}>
          None
        </button>
        <div className="ml-auto flex flex-wrap items-center gap-2">
          {actions.map((a) => (
            <Button
              key={a.id}
              variant={a.danger ? "danger" : "secondary"}
              onClick={a.onClick}
              disabled={n === 0 || busy !== null}
            >
              {busy === a.id ? <Spinner /> : a.label}
            </Button>
          ))}
          {naming === null ? (
            <Button local variant="secondary" onClick={() => onNaming("")} disabled={n === 0 || busy !== null}>
              New playlist…
            </Button>
          ) : (
            <span className="flex items-center gap-2">
              <span className="text-xs text-muted">Utilify:</span>
              <input
                autoFocus
                value={naming}
                onChange={(e) => onNaming(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && naming.trim()) onCreate(naming);
                  if (e.key === "Escape") onNaming(null);
                }}
                placeholder="name"
                className="w-40 rounded-md border border-line bg-ink px-2 py-1 text-sm outline-none focus:border-spotify"
              />
              <Button onClick={() => onCreate(naming)} disabled={!naming.trim() || busy !== null}>
                Create
              </Button>
              <Button local variant="ghost" onClick={() => onNaming(null)}>
                ✕
              </Button>
            </span>
          )}
        </div>
      </div>
      <TrackList tracks={tracks} selected={selected} onToggle={onToggle} note={note} emptyText="Nothing here." />
    </section>
  );
}
