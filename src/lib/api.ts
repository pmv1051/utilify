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

export interface GeneratedPlaylist {
  id: string;
  name: string;
  trackCount: number;
  requested: number;
}

export interface PlaylistRef {
  id: string;
  name: string;
}

export interface DiffPair {
  a: TrackRow;
  b: TrackRow;
  differentUri: boolean;
}

export interface DiffResult {
  a: PlaylistRef;
  b: PlaylistRef;
  matchByName: boolean;
  onlyA: TrackRow[];
  onlyB: TrackRow[];
  both: DiffPair[];
}

export interface MergeResult {
  playlist: GeneratedPlaylist;
  sources: number;
  tracksSeen: number;
  duplicatesSkipped: number;
}

export interface ArtistHit {
  id: string;
  name: string;
  imageUrl: string | null;
  genres: string[];
  followers: number | null;
}

export interface AlbumInfo {
  id: string;
  name: string;
  group: "album" | "single" | "compilation" | "appears_on" | string;
  releaseDate: string | null;
  totalTracks: number;
  imageUrl: string | null;
  artists: string;
}

export interface EditorTrack extends TrackRow {
  liked: boolean | null;
}

export interface EditorProgress {
  playlistId: string;
  done: number;
  total: number;
}

export interface ApplyResult {
  moves: number;
}

export interface ExportContent {
  fileName: string;
  content: string;
  tracks: number;
}

export interface ImportCandidate {
  uri: string;
  name: string;
  artists: string;
  album: string | null;
  durationMs: number | null;
  score: number;
}

export interface ImportMatch {
  line: string;
  queryArtist: string | null;
  queryTitle: string;
  candidates: ImportCandidate[];
  best: number | null;
  status: "exact" | "fuzzy" | "unsure" | "none";
}

export interface ImportProgress {
  done: number;
  total: number;
}

export interface DiscographyResult {
  playlist: GeneratedPlaylist;
  albums: number;
  tracksSeen: number;
  duplicatesSkipped: number;
  otherArtistSkipped: number;
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

  createPlaylistFromTracks: (name: string, uris: string[], randomize: boolean) =>
    invoke<GeneratedPlaylist>("create_playlist_from_tracks", { name, uris, randomize }),
  addTracksToPlaylist: (playlistId: string, uris: string[]) =>
    invoke<number>("add_tracks_to_playlist", { playlistId, uris }),
  removeTracksFromPlaylist: (playlistId: string, uris: string[]) =>
    invoke<number>("remove_tracks_from_playlist", { playlistId, uris }),

  diffPlaylists: (a: string, b: string, matchByName: boolean) =>
    invoke<DiffResult>("diff_playlists", { a, b, matchByName }),
  mergePlaylists: (playlistIds: string[], name: string, dedupeByName: boolean, randomize: boolean) =>
    invoke<MergeResult>("merge_playlists", { playlistIds, name, dedupeByName, randomize }),

  searchArtists: (query: string) => invoke<ArtistHit[]>("search_artists", { query }),
  getArtistAlbums: (artistId: string, includeCompilations: boolean, includeAppearsOn: boolean) =>
    invoke<AlbumInfo[]>("get_artist_albums", { artistId, includeCompilations, includeAppearsOn }),
  createDiscography: (args: {
    artistId: string;
    artistName: string;
    albumIds: string[];
    name: string;
    onlyThisArtist: boolean;
    dedupeByName: boolean;
    randomize: boolean;
  }) => invoke<DiscographyResult>("create_discography", args),

  loadPlaylistForEditor: (playlistId: string) => invoke<EditorTrack[]>("load_playlist_for_editor", { playlistId }),
  applyPlaylistOrder: (playlistId: string, order: number[]) =>
    invoke<ApplyResult>("apply_playlist_order", { playlistId, order }),

  exportPlaylist: (playlistId: string, format: "csv" | "txt") =>
    invoke<ExportContent>("export_playlist", { playlistId, format }),
  saveTextFile: (fileName: string, content: string) =>
    invoke<string | null>("save_text_file", { fileName, content }),
  importSearch: (lines: string[]) => invoke<ImportMatch[]>("import_search", { lines }),
};
