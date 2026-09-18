import { invoke } from "@tauri-apps/api/core";

// ---- Types mirrored from the Rust side (serde camelCase) ----

export interface AppError {
  kind: string;
  message: string;
}

export interface SetupState {
  hasClientId: boolean;
  isAuthenticated: boolean;
  userDisplayName: string | null;
  userProduct: string | null;
  redirectUri: string;
}

export interface Settings {
  clientId: string | null;
  minimizeToTray: boolean;
  redirectUri: string;
  userDisplayName: string | null;
  userId: string | null;
  dbPath: string;
}

export interface Playlist {
  id: string;
  name: string;
  ownerId: string | null;
  ownerName: string | null;
  trackCount: number;
  snapshotId: string | null;
  imageUrl: string | null;
  uri: string | null;
  isShadow: boolean;
  isOwn: boolean;
  position: number;
}

export interface RandomizerSession {
  shadowPlaylistId: string;
  sourcePlaylistId: string;
  sourceName: string;
  shadowName: string;
  trackCount: number;
  lastTrackUri: string | null;
  lastShuffledAt: number;
  reshuffleCount: number;
  active: boolean;
  pendingPin: string | null;
  missingTracks: MissingTrack[];
}

export interface MissingTrack {
  uri: string | null;
  name: string;
  artists: string;
  reason: string;
}

export interface RandomizeResult {
  session: RandomizerSession;
  playbackStarted: boolean;
  warning: string | null;
}

export interface Artist {
  id: string | null;
  name: string;
}

export interface Track {
  id: string | null;
  uri: string | null;
  name: string;
  artists: Artist[];
  album: { name: string; images: { url: string }[] } | null;
  durationMs: number | null;
}

export interface PlaybackState {
  device: { id: string | null; name: string; isActive: boolean } | null;
  isPlaying: boolean;
  progressMs: number | null;
  shuffleState: boolean;
  repeatState: string | null;
  context: { uri: string; type: string | null } | null;
  item: Track | null;
}

export type PlayerAction = "play" | "pause" | "next" | "previous" | "seek" | "shuffle" | "repeat";

export interface TrackRow {
  uri: string;
  id: string | null;
  name: string;
  artists: string;
  artistIds: string[];
  album: string | null;
  durationMs: number | null;
  addedAt: string | null;
  /** Index among all playlist items, as the Spotify API counts positions. */
  position: number;
  isLocal: boolean;
  playable: boolean;
}

export interface DuplicateOccurrence {
  playlistId: string;
  playlistName: string;
  track: TrackRow;
}

export interface DuplicateGroup {
  key: string;
  matchKind: "uri" | "name";
  name: string;
  artists: string;
  occurrences: DuplicateOccurrence[];
}

export interface DuplicateReport {
  mode: "single" | "cross";
  playlistsScanned: number;
  tracksScanned: number;
  groups: DuplicateGroup[];
}

export interface RemovalRequest {
  playlistId: string;
  uri: string;
  positions: number[];
}

export interface RemovalSummary {
  removed: number;
  reAdded: number;
  playlists: number;
}

export interface BenchRow {
  id: number;
  trackUri: string;
  trackName: string | null;
  artistName: string | null;
  playlistId: string;
  playlistName: string | null;
  originalPosition: number | null;
  benchedAt: number;
  restoreAt: number;
  restoredAt: number | null;
}

export function errorMessage(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as AppError).message);
  return String(e);
}

// ---- Commands ----

export const api = {
  getSetupState: () => invoke<SetupState>("get_setup_state"),
  saveClientId: (clientId: string) => invoke<void>("save_client_id", { clientId }),
  startAuth: () => invoke<SetupState>("start_auth"),
  disconnect: () => invoke<void>("disconnect"),
  openExternal: (url: string) => invoke<void>("open_external", { url }),

  getSettings: () => invoke<Settings>("get_settings"),
  setMinimizeToTray: (enabled: boolean) => invoke<void>("set_minimize_to_tray", { enabled }),

  getPlaylists: (refresh: boolean) => invoke<Playlist[]>("get_playlists", { refresh }),

  randomizePlaylist: (playlistId: string, startPlayback: boolean) =>
    invoke<RandomizeResult>("randomize_playlist", { playlistId, startPlayback }),
  reshuffleSession: (shadowPlaylistId: string) =>
    invoke<RandomizerSession>("reshuffle_session", { shadowPlaylistId }),
  getRandomizerSessions: () => invoke<RandomizerSession[]>("get_randomizer_sessions"),
  stopRandomizerSession: (shadowPlaylistId: string) =>
    invoke<void>("stop_randomizer_session", { shadowPlaylistId }),

  getPlaybackState: (refresh: boolean) => invoke<PlaybackState | null>("get_playback_state", { refresh }),
  playerCommand: (action: PlayerAction, value?: string) =>
    invoke<PlaybackState | null>("player_command", { action, value: value ?? null }),

  getPlaylistTracks: (playlistId: string) => invoke<TrackRow[]>("get_playlist_tracks", { playlistId }),
  benchTrack: (args: {
    playlistId: string;
    trackUri: string;
    trackName: string | null;
    artistName: string | null;
    position: number | null;
    durationSecs: number;
  }) => invoke<BenchRow>("bench_track", args),
  getBenchedTracks: () => invoke<BenchRow[]>("get_benched_tracks"),
  unbenchTrack: (id: number) => invoke<BenchRow>("unbench_track", { id }),

  scanDuplicates: (playlistIds: string[], matchByName: boolean) =>
    invoke<DuplicateReport>("scan_duplicates", { playlistIds, matchByName }),
  removeDuplicates: (removals: RemovalRequest[]) => invoke<RemovalSummary>("remove_duplicates", { removals }),
};
