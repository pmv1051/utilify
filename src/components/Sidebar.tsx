import { useApp, type Page } from "../stores/app";

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
      { id: "genre", label: "Genres", icon: "◔" },
    ],
  },
  {
    title: "Listening",
    items: [
      { id: "stats", label: "Stats", icon: "▤" },
      { id: "discovery", label: "Discovery", icon: "✦" },
    ],
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
