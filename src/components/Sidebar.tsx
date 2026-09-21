import { useApp, type Page } from "../stores/app";
import { SCOPE_LABEL, usePausedScopes } from "../lib/quota";

interface Item {
  id: Page;
  label: string;
  icon: string;
}

const groups: { title: string | null; items: Item[] }[] = [
  {
    title: null,
    items: [
      { id: "playlists", label: "Playlists", icon: "♫" },
      { id: "randomizer", label: "Randomizer", icon: "⇄" },
      { id: "bench", label: "Bench", icon: "⏸" },
    ],
  },
  {
    title: "Tools",
    items: [
      { id: "duplicates", label: "Duplicates", icon: "⧉" },
      { id: "diff", label: "Diff", icon: "⇔" },
      { id: "merge", label: "Merge", icon: "⊕" },
      { id: "discography", label: "Discography", icon: "♪" },
      { id: "editor", label: "Editor", icon: "≡" },
      { id: "exportimport", label: "Export / Import", icon: "⇅" },
    ],
  },
  {
    title: "Listening",
    items: [{ id: "stats", label: "Stats", icon: "▤" }],
  },
  {
    title: null,
    items: [{ id: "settings", label: "Settings", icon: "⚙" }],
  },
];

export function Sidebar() {
  const page = useApp((s) => s.page);
  const setPage = useApp((s) => s.setPage);
  const setup = useApp((s) => s.setup);
  const sessions = useApp((s) => s.sessions);
  const benches = useApp((s) => s.benches);
  const paused = usePausedScopes();
  const offlineSince = useApp((s) => s.offlineSince);
  const availableUpdate = useApp((s) => s.availableUpdate);
  const badge: Partial<Record<Page, number>> = { randomizer: sessions.length, bench: benches.length };

  return (
    <aside className="flex w-56 shrink-0 flex-col border-r border-line bg-panel">
      <div className="flex items-center gap-2 px-5 py-5">
        <span className="flex h-8 w-8 items-center justify-center rounded-full bg-spotify font-black text-black">U</span>
        <span className="text-lg font-bold tracking-tight">Utilify</span>
      </div>
      <nav className="flex flex-1 flex-col gap-1 overflow-auto px-3">
        {groups.map((g, gi) => (
          <div key={gi} className={gi > 0 ? "mt-3" : ""}>
            {g.title && (
              <div className="mb-1 px-3 text-[10px] font-semibold uppercase tracking-wider text-muted">{g.title}</div>
            )}
            {g.items.map((it) => (
              <button
                key={it.id}
                onClick={() => setPage(it.id)}
                className={`flex w-full items-center justify-between rounded-md px-3 py-2 text-left text-sm transition ${
                  page === it.id ? "bg-panel-2 text-white" : "text-zinc-400 hover:bg-panel-2 hover:text-zinc-100"
                }`}
              >
                <span className="flex items-center gap-3">
                  <span className="w-4 text-center text-base">{it.icon}</span>
                  {it.label}
                </span>
                {(badge[it.id] ?? 0) > 0 && (
                  <span className="rounded-full bg-spotify/20 px-2 text-xs text-spotify">{badge[it.id]}</span>
                )}
              </button>
            ))}
          </div>
        ))}
      </nav>
      {offlineSince !== null && (
        <div
          className="mx-3 mb-2 rounded-md border border-red-500/40 bg-red-500/10 px-3 py-2 text-xs text-red-300"
          title="Utilify retries every 30 seconds. Re-shuffles and bench restores resume automatically once Spotify answers again."
        >
          <div className="font-semibold">Spotify unreachable</div>
          <div className="text-red-200/80">Since {new Date(offlineSince * 1000).toLocaleTimeString()} · retrying</div>
        </div>
      )}
      {availableUpdate && (
        <button
          onClick={() => setPage("settings")}
          className="mx-3 mb-2 rounded-md border border-spotify/40 bg-spotify/10 px-3 py-2 text-left text-xs text-spotify hover:bg-spotify/20"
          title="Open Settings → Updates to install"
        >
          <div className="font-semibold">Update available</div>
          <div className="text-spotify/80">Utilify {availableUpdate.version}</div>
        </button>
      )}
      {paused.length > 0 && (
        <div className="mx-3 mb-2 rounded-md border border-amber-400/40 bg-amber-400/10 px-3 py-2 text-xs text-amber-300">
          <div className="font-semibold">
            {paused.length === 1 ? "Part of Spotify's quota is gone" : "Parts of Spotify's quota are gone"}
          </div>
          {paused.map((p) => (
            <div key={p.scope} className="text-amber-200/80">
              {SCOPE_LABEL[p.scope]} · retry in {p.remaining}
            </div>
          ))}
        </div>
      )}
      <div className="px-5 py-4 text-xs text-muted">
        {setup?.userDisplayName ? (
          <>
            Signed in as <span className="text-zinc-300">{setup.userDisplayName}</span>
            {setup.userProduct && setup.userProduct !== "premium" && (
              <div className="mt-1 text-amber-400">Spotify Premium is required for playback control.</div>
            )}
          </>
        ) : (
          "Not connected"
        )}
      </div>
    </aside>
  );
}
