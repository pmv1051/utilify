import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useApp } from "./stores/app";
import type { BenchRow, PlaybackState, RandomizerSession } from "./lib/api";
import { SetupPage } from "./pages/SetupPage";
import { PlaylistsPage } from "./pages/PlaylistsPage";
import { RandomizerPage } from "./pages/RandomizerPage";
import { BenchPage } from "./pages/BenchPage";
import { DuplicatesPage } from "./pages/DuplicatesPage";
import { DiffPage } from "./pages/DiffPage";
import { MergePage } from "./pages/MergePage";
import { DiscographyPage } from "./pages/DiscographyPage";
import { EditorPage } from "./pages/EditorPage";
import { ExportImportPage } from "./pages/ExportImportPage";
import { StatsPage } from "./pages/StatsPage";
import { SettingsPage } from "./pages/SettingsPage";
import { Sidebar } from "./components/Sidebar";
import { NowPlaying } from "./components/NowPlaying";
import { Toasts } from "./components/Toasts";
import { Spinner } from "./components/Spinner";

export default function App() {
  const ready = useApp((s) => s.ready);
  const setup = useApp((s) => s.setup);
  const page = useApp((s) => s.page);
  const init = useApp((s) => s.init);
  const setPlayback = useApp((s) => s.setPlayback);
  const refreshSessions = useApp((s) => s.refreshSessions);
  const refreshBenches = useApp((s) => s.refreshBenches);
  const toast = useApp((s) => s.toast);

  useEffect(() => {
    init();
    const unlisteners = [
      listen<PlaybackState | null>("playback-state", (e) => setPlayback(e.payload)),
      listen<RandomizerSession>("randomizer-reshuffled", (e) => {
        refreshSessions();
        toast("info", `Nearing the end of "${e.payload.shadowName}". Re-shuffled ${e.payload.trackCount} tracks.`);
      }),
      listen<string>("randomizer-error", (e) => toast("error", e.payload)),
      listen<BenchRow>("bench-restored", (e) => {
        refreshBenches();
        toast("info", `"${e.payload.trackName ?? e.payload.trackUri}" is back in "${e.payload.playlistName ?? "its playlist"}".`);
      }),
      listen<string>("bench-error", (e) => toast("error", e.payload)),
      listen<void>("auth-expired", () => {
        toast("error", "Spotify session expired. Please reconnect.");
        init();
      }),
    ];
    return () => {
      unlisteners.forEach((p) => p.then((un) => un()));
    };
  }, [init, setPlayback, refreshSessions, refreshBenches, toast]);

  if (!ready || !setup) {
    return (
      <div className="flex h-full items-center justify-center text-muted">
        <Spinner className="mr-3" /> Starting Utilify…
      </div>
    );
  }

  if (!setup.hasClientId || !setup.isAuthenticated) {
    return (
      <>
        <SetupPage />
        <Toasts />
      </>
    );
  }

  return (
    <div className="flex h-full">
      <Sidebar />
      <div className="flex min-w-0 flex-1 flex-col">
        <main className="min-h-0 flex-1">
          {page === "playlists" && <PlaylistsPage />}
          {page === "randomizer" && <RandomizerPage />}
          {page === "bench" && <BenchPage />}
          {page === "duplicates" && <DuplicatesPage />}
          {page === "diff" && <DiffPage />}
          {page === "merge" && <MergePage />}
          {page === "discography" && <DiscographyPage />}
          {page === "editor" && <EditorPage />}
          {page === "exportimport" && <ExportImportPage />}
          {page === "stats" && <StatsPage />}
          {["genre", "discovery"].includes(page) && (
            <div className="p-6 text-sm text-muted">This page is coming next in Phase 4.</div>
          )}
          {page === "settings" && <SettingsPage />}
        </main>
        <NowPlaying />
      </div>
      <Toasts />
    </div>
  );
}
