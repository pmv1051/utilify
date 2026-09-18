import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, errorMessage, type PlayerAction } from "../lib/api";
import { useApp } from "../stores/app";
import { artistNames, formatDuration, formatUntil } from "../lib/format";
import { BENCH_PRESETS, playlistIdFromContext } from "../lib/bench";
import { useQuotaCooldown } from "../lib/quota";
import { Spinner } from "./Spinner";

const MIN_HEIGHT = 80;
const MAX_HEIGHT = 360;
const TIER1 = 130; // transport + actions
const TIER2 = 200; // big art, album, shuffle/repeat, context line
const STORAGE_KEY = "utilify.nowPlayingHeight";

function loadHeight(): number {
  try {
    const v = Number(localStorage.getItem(STORAGE_KEY));
    if (Number.isFinite(v) && v >= MIN_HEIGHT && v <= MAX_HEIGHT) return v;
  } catch {
    // storage unavailable
  }
  return MIN_HEIGHT;
}

export function NowPlaying() {
  const playback = useApp((s) => s.playback);
  const receivedAt = useApp((s) => s.playbackReceivedAt);
  const setPlayback = useApp((s) => s.setPlayback);
  const sessions = useApp((s) => s.sessions);
  const playlists = useApp((s) => s.playlists);
  const refreshPlayback = useApp((s) => s.refreshPlayback);
  const refreshSessions = useApp((s) => s.refreshSessions);
  const refreshPlaylists = useApp((s) => s.refreshPlaylists);
  const refreshBenches = useApp((s) => s.refreshBenches);
  const setPage = useApp((s) => s.setPage);
  const toast = useApp((s) => s.toast);
  const setup = useApp((s) => s.setup);
  const cooldown = useQuotaCooldown();
  const premium = setup?.userProduct === "premium" || !setup?.userProduct;
  const premiumTitle = cooldown.active
    ? cooldown.reason
    : premium
      ? undefined
      : "Spotify Premium is required to control playback.";
  const controlsBlocked = !premium || cooldown.active;

  // ---- resizable height ----
  const [height, setHeight] = useState(loadHeight);
  const drag = useRef<{ startY: number; startH: number } | null>(null);
  const onDragStart = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      drag.current = { startY: e.clientY, startH: height };
      const onMove = (ev: MouseEvent) => {
        if (!drag.current) return;
        const h = Math.max(MIN_HEIGHT, Math.min(MAX_HEIGHT, drag.current.startH + (drag.current.startY - ev.clientY)));
        setHeight(h);
      };
      const onUp = () => {
        drag.current = null;
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
        setHeight((h) => {
          try {
            localStorage.setItem(STORAGE_KEY, String(h));
          } catch {
            // ignore
          }
          return h;
        });
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },
    [height],
  );
  const tier = height >= TIER2 ? 2 : height >= TIER1 ? 1 : 0;

  // ---- live progress between polls ----
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!playback?.isPlaying) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [playback?.isPlaying, receivedAt]);

  const item = playback?.item ?? null;
  const duration = item?.durationMs ?? null;
  const progressMs = useMemo(() => {
    if (!playback || playback.progressMs == null) return null;
    const drift = playback.isPlaying ? Math.max(0, now - receivedAt) : 0;
    const p = playback.progressMs + drift;
    return duration != null ? Math.min(duration, p) : p;
  }, [playback, now, receivedAt, duration]);
  const progressPct = duration && progressMs != null ? (progressMs / duration) * 100 : 0;

  // ---- context ----
  const contextPlaylistId = playlistIdFromContext(playback?.context?.uri);
  const session = contextPlaylistId ? sessions.find((s) => s.shadowPlaylistId === contextPlaylistId) : undefined;
  const contextPlaylist = contextPlaylistId ? playlists.find((p) => p.id === contextPlaylistId) : undefined;
  const contextLabel = session?.shadowName ?? contextPlaylist?.name ?? (contextPlaylistId ? "a playlist" : playback?.context?.type ?? null);

  // ---- actions ----
  const [busy, setBusy] = useState<string | null>(null);
  const [benchOpen, setBenchOpen] = useState(false);

  async function cmd(action: PlayerAction, value?: string) {
    if (controlsBlocked) {
      toast("error", premiumTitle ?? "Playback control is unavailable right now.");
      return;
    }
    setBusy(action);
    try {
      setPlayback(await api.playerCommand(action, value));
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  function seekFromClick(e: React.MouseEvent<HTMLDivElement>) {
    if (tier < 1 || !duration) return;
    const rect = e.currentTarget.getBoundingClientRect();
    const frac = Math.max(0, Math.min(1, (e.clientX - rect.left) / rect.width));
    cmd("seek", String(Math.round(frac * duration)));
  }

  async function benchNow(secs: number) {
    if (!item?.uri || !contextPlaylistId) return;
    setBenchOpen(false);
    setBusy("bench");
    try {
      const row = await api.benchTrack({
        playlistId: contextPlaylistId,
        trackUri: item.uri,
        trackName: item.name,
        artistName: artistNames(item.artists),
        position: null,
        durationSecs: secs,
      });
      toast("success", `Benched "${item.name}". It returns ${formatUntil(row.restoreAt)}.`);
      await Promise.all([refreshBenches(), refreshSessions()]);
      // The song keeps playing after removal; move on, that is the point.
      setPlayback(await api.playerCommand("next"));
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  async function randomizeCurrent() {
    if (!contextPlaylistId) return;
    setBusy("randomize");
    try {
      if (session) {
        const updated = await api.reshuffleSession(session.shadowPlaylistId);
        toast("success", `Re-shuffled "${updated.shadowName}" around the current track.`);
        await refreshSessions();
      } else if (contextPlaylist?.isShadow) {
        toast("info", `"${contextPlaylist.name}" is a Utilify playlist that is not tracked. Randomize its source from the Playlists tab.`);
        setPage("playlists");
      } else {
        const result = await api.randomizePlaylist(contextPlaylistId, true);
        toast(
          result.playbackStarted ? "success" : "info",
          result.playbackStarted
            ? `Now playing "${result.session.shadowName}" (${result.session.trackCount} tracks).`
            : (result.warning ?? `Created "${result.session.shadowName}".`),
        );
        await Promise.all([refreshSessions(), refreshPlaylists(true)]);
        setTimeout(() => refreshPlayback(), 1500);
      }
    } catch (e) {
      toast("error", errorMessage(e));
    } finally {
      setBusy(null);
    }
  }

  const images = item?.album?.images ?? [];
  const cover = images.length > 0 ? images[tier >= 2 ? 0 : images.length - 1].url : null;
  const artSize = tier >= 2 ? "h-24 w-24" : "h-12 w-12";

  return (
    <footer
      style={{ height }}
      className="relative flex shrink-0 flex-col border-t border-line bg-panel"
    >
      <div
        onMouseDown={onDragStart}
        title="Drag to resize"
        className="absolute inset-x-0 -top-1 z-10 h-2 cursor-ns-resize hover:bg-spotify/40"
      />

      {!item ? (
        <div className="flex flex-1 items-center justify-between px-5 text-sm text-muted">
          <span>Nothing playing. Start Spotify on any device. Playback state refreshes every 30 seconds.</span>
          <button className="text-xs text-zinc-400 hover:text-white" onClick={() => refreshPlayback()}>
            Refresh
          </button>
        </div>
      ) : (
        <>
          {/* Row 1: track + progress */}
          <div className="flex min-h-0 flex-1 items-center gap-4 px-5">
            {cover ? (
              <img src={cover} alt="" className={`${artSize} shrink-0 rounded object-cover`} />
            ) : (
              <div className={`${artSize} shrink-0 rounded bg-panel-2`} />
            )}
            <div className="min-w-0 flex-1">
              <div className="truncate text-sm font-medium">{item.name}</div>
              <div className="truncate text-xs text-muted">
                {artistNames(item.artists)}
                {tier >= 2 && item.album?.name ? ` · ${item.album.name}` : ""}
              </div>
              <div
                onClick={seekFromClick}
                className={`mt-1.5 h-1 w-full overflow-hidden rounded bg-line ${tier >= 1 ? "cursor-pointer hover:h-1.5" : ""}`}
                title={tier >= 1 ? "Click to seek" : undefined}
              >
                <div className="h-full bg-spotify" style={{ width: `${Math.min(100, progressPct)}%` }} />
              </div>
            </div>
            <div className="shrink-0 text-right text-xs text-muted">
              <div>
                {formatDuration(progressMs)} / {formatDuration(duration)}
              </div>
              <div className="mt-1 flex items-center justify-end gap-2">
                {playback?.isPlaying ? <span className="text-spotify">Playing</span> : <span>Paused</span>}
                {playback?.shuffleState && <span className="text-amber-400">shuffle on</span>}
                {session && <span className="rounded bg-spotify/20 px-1.5 text-spotify">Randomized</span>}
                {tier < 2 && playback?.device && <span>· {playback.device.name}</span>}
              </div>
            </div>
          </div>

          {/* Row 2: transport + actions */}
          {tier >= 1 && (
            <div className="flex items-center gap-2 px-5 pb-3">
              <IconButton title={premiumTitle ?? "Previous"} onClick={() => cmd("previous")} busy={busy === "previous"} disabled={controlsBlocked}>
                ⏮
              </IconButton>
              <IconButton
                title={premiumTitle ?? (playback?.isPlaying ? "Pause" : "Play")}
                onClick={() => cmd(playback?.isPlaying ? "pause" : "play")}
                busy={busy === "play" || busy === "pause"}
                disabled={controlsBlocked}
                primary
              >
                {playback?.isPlaying ? "⏸" : "▶"}
              </IconButton>
              <IconButton title={premiumTitle ?? "Next"} onClick={() => cmd("next")} busy={busy === "next"} disabled={controlsBlocked}>
                ⏭
              </IconButton>

              {tier >= 2 && (
                <>
                  <span className="mx-1 h-5 w-px bg-line" />
                  <Toggle
                    active={!!playback?.repeatState && playback.repeatState !== "off"}
                    title={`Repeat: ${playback?.repeatState ?? "off"}`}
                    onClick={() =>
                      cmd("repeat", playback?.repeatState === "off" || !playback?.repeatState ? "context" : playback.repeatState === "context" ? "track" : "off")
                    }
                    busy={busy === "repeat"}
                  >
                    {playback?.repeatState === "track" ? "↻ Repeat 1" : "↻ Repeat"}
                  </Toggle>
                </>
              )}

              <div className="relative ml-auto flex items-center gap-2">
                <button
                  disabled={!contextPlaylistId || busy !== null || cooldown.active}
                  onClick={() => setBenchOpen((o) => !o)}
                  title={cooldown.active ? cooldown.reason : contextPlaylistId ? "Bench this track from the playlist it is playing from" : "Play from a playlist to bench"}
                  className="rounded-md border border-line bg-panel-2 px-3 py-1.5 text-xs text-zinc-100 hover:bg-line disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {busy === "bench" ? <Spinner /> : "⏸ Bench ▾"}
                </button>
                <button
                  disabled={!contextPlaylistId || busy !== null || cooldown.active}
                  onClick={randomizeCurrent}
                  title={cooldown.active ? cooldown.reason : session ? "Re-shuffle this randomized playlist now" : "Randomize the playlist that is playing"}
                  className="rounded-md bg-spotify px-3 py-1.5 text-xs font-semibold text-black hover:bg-spotify-dark disabled:cursor-not-allowed disabled:opacity-50"
                >
                  {busy === "randomize" ? <Spinner /> : session ? "⇄ Re-shuffle" : "⇄ Randomize playlist"}
                </button>

                {benchOpen && contextPlaylistId && (
                  <>
                    <div className="fixed inset-0 z-20" onClick={() => setBenchOpen(false)} />
                    <div className="absolute bottom-full right-0 z-30 mb-2 w-72 rounded-md border border-line bg-panel-2 p-3 shadow-xl">
                      <div className="mb-2 text-xs text-muted">
                        Bench <span className="text-zinc-200">{item.name}</span> from{" "}
                        <span className="text-zinc-200">{contextLabel}</span> for
                      </div>
                      <div className="flex flex-wrap gap-1.5">
                        {BENCH_PRESETS.map((p) => (
                          <button
                            key={p.secs}
                            onClick={() => benchNow(p.secs)}
                            className="rounded-full border border-line px-2.5 py-1 text-xs text-zinc-200 hover:border-spotify hover:text-spotify"
                          >
                            {p.label}
                          </button>
                        ))}
                      </div>
                    </div>
                  </>
                )}
              </div>
            </div>
          )}

          {/* Row 3: context details */}
          {tier >= 2 && (
            <div className="flex items-center gap-4 border-t border-line px-5 py-2 text-xs text-muted">
              <span>
                Playing from <span className="text-zinc-300">{contextLabel ?? "—"}</span>
                {session && <span className="ml-1 text-spotify">(tracked, {session.trackCount} tracks)</span>}
              </span>
              {playback?.device && (
                <span>
                  Device <span className="text-zinc-300">{playback.device.name}</span>
                </span>
              )}
              <span>
                Repeat <span className="text-zinc-300">{playback?.repeatState ?? "off"}</span>
              </span>
              <button className="ml-auto hover:text-white" onClick={() => refreshPlayback()}>
                Refresh
              </button>
            </div>
          )}
        </>
      )}
    </footer>
  );
}

function IconButton({
  children,
  onClick,
  title,
  busy,
  primary,
  disabled,
}: {
  children: string;
  onClick: () => void;
  title: string;
  busy?: boolean;
  primary?: boolean;
  disabled?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      disabled={busy || disabled}
      className={`flex h-8 w-8 items-center justify-center rounded-full text-sm transition disabled:opacity-60 ${
        primary ? "bg-white text-black hover:bg-zinc-200" : "text-zinc-300 hover:bg-panel-2 hover:text-white"
      }`}
    >
      {busy ? <Spinner /> : children}
    </button>
  );
}

function Toggle({
  children,
  active,
  onClick,
  title,
  busy,
}: {
  children: string;
  active: boolean;
  onClick: () => void;
  title: string;
  busy?: boolean;
}) {
  return (
    <button
      onClick={onClick}
      title={title}
      disabled={busy}
      className={`rounded-full border px-2.5 py-1 text-xs transition disabled:opacity-60 ${
        active ? "border-spotify bg-spotify/20 text-spotify" : "border-line text-zinc-400 hover:text-white"
      }`}
    >
      {busy ? <Spinner /> : children}
    </button>
  );
}
