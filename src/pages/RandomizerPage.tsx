import { useState } from "react";
import { api, errorMessage, type RandomizerSession } from "../lib/api";
import { useApp } from "../stores/app";
import { Button } from "../components/Button";
import { Spinner } from "../components/Spinner";
import { formatRelative } from "../lib/format";

export function RandomizerPage() {
  const sessions = useApp((s) => s.sessions);
  const playback = useApp((s) => s.playback);
  const refreshSessions = useApp((s) => s.refreshSessions);
  const refreshPlayback = useApp((s) => s.refreshPlayback);
  const setPage = useApp((s) => s.setPage);
  const toast = useApp((s) => s.toast);
  const [busy, setBusy] = useState<string | null>(null);

  const playingContext = playback?.context?.uri ?? null;

  async function reshuffle(s: RandomizerSession) {
    setBusy(s.shadowPlaylistId);
    try {
      const updated = await api.reshuffleSession(s.shadowPlaylistId);
      await refreshSessions();
      toast("success", `Re-shuffled ${updated.trackCount} tracks in "${updated.shadowName}".`);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function play(s: RandomizerSession) {
    setBusy(s.shadowPlaylistId);
    try {
      const result = await api.randomizePlaylist(s.sourcePlaylistId, true);
      await refreshSessions();
      toast(
        result.playbackStarted ? "success" : "info",
        result.playbackStarted
          ? `Fresh shuffle of "${s.sourceName}" is playing.`
          : (result.warning ?? "Shuffled, but playback did not start."),
      );
      setTimeout(() => refreshPlayback(), 1500);
    } catch (e) {
      toast("error", errorMessage(e));
      await refreshSessions();
    } finally {
      setBusy(null);
    }
  }

  async function stop(s: RandomizerSession) {
    setBusy(s.shadowPlaylistId);
    try {
      await api.stopRandomizerSession(s.shadowPlaylistId);
      await refreshSessions();
      toast("info", `Stopped tracking "${s.shadowName}". The playlist stays in your Spotify library.`);
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  return (
    <div className="flex h-full flex-col">
      <header className="flex items-center gap-3 border-b border-line px-6 py-4">
        <h1 className="text-xl font-bold">Randomizer</h1>
        <span className="text-sm text-muted">{sessions.length} active</span>
      </header>

      <div className="flex-1 overflow-auto p-6">
        <p className="mb-5 max-w-3xl text-sm text-muted">
          Each randomized playlist is a shadow copy named <span className="text-zinc-300">name-Utilify</span>, written
          in a true Fisher-Yates order and played with Spotify shuffle turned off. While Utilify runs (or sits in the
          tray), it watches playback and re-shuffles the shadow playlist as you near its end, keeping the playing track
          in place and pulling in any tracks added to the source since.
        </p>

        {sessions.length === 0 ? (
          <div className="rounded-lg border border-dashed border-line p-8 text-center text-muted">
            <p className="mb-3">No randomized playlists yet.</p>
            <Button local variant="secondary" onClick={() => setPage("playlists")}>
              Pick a playlist
            </Button>
          </div>
        ) : (
          <ul className="flex flex-col gap-3">
            {sessions.map((s) => {
              const isPlaying = playingContext === `spotify:playlist:${s.shadowPlaylistId}`;
              const isBusy = busy === s.shadowPlaylistId;
              return (
                <li key={s.shadowPlaylistId} className="rounded-lg border border-line bg-panel p-4">
                  <div className="flex items-start gap-4">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="truncate font-semibold">{s.shadowName}</span>
                        {isPlaying && (
                          <span className="rounded bg-spotify/20 px-1.5 py-0.5 text-[10px] font-semibold uppercase text-spotify">
                            Now playing
                          </span>
                        )}
                      </div>
                      <div className="mt-1 text-xs text-muted">
                        Source: <span className="text-zinc-300">{s.sourceName}</span> · {s.trackCount} tracks · shuffled{" "}
                        {formatRelative(s.lastShuffledAt)} · {s.reshuffleCount} automatic re-shuffle
                        {s.reshuffleCount === 1 ? "" : "s"}
                      </div>
                      {s.missingTracks.length > 0 && (
                        <details className="mt-2 text-xs">
                          <summary className="cursor-pointer text-amber-400 hover:text-amber-300">
                            {s.missingTracks.length} track{s.missingTracks.length === 1 ? "" : "s"} not included
                          </summary>
                          <ul className="mt-2 max-h-48 space-y-1.5 overflow-auto rounded border border-line bg-ink p-3">
                            {s.missingTracks.map((m, i) => (
                              <li key={`${m.uri ?? "unknown"}-${i}`} className="select-text">
                                <span className="text-zinc-200">{m.name}</span>
                                {m.artists && <span className="text-muted"> — {m.artists}</span>}
                                <div className="text-muted">{m.reason}</div>
                              </li>
                            ))}
                          </ul>
                        </details>
                      )}
                    </div>
                    <div className="flex shrink-0 gap-2">
                      <Button onClick={() => play(s)} disabled={busy !== null} title="Re-shuffle from source and start playing">
                        {isBusy ? <Spinner /> : "▶"} Play fresh
                      </Button>
                      <Button variant="secondary" onClick={() => reshuffle(s)} disabled={busy !== null} title="Re-shuffle without changing playback">
                        Re-shuffle
                      </Button>
                      <Button local variant="ghost" onClick={() => stop(s)} disabled={busy !== null}>
                        Stop
                      </Button>
                    </div>
                  </div>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
