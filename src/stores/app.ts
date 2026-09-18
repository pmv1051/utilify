import { create } from "zustand";
import {
  api,
  errorMessage,
  type BenchRow,
  type PlaybackState,
  type Playlist,
  type RandomizerSession,
  type Settings,
  type SetupState,
  type UpdateInfo,
} from "../lib/api";

export type Page =
  | "playlists"
  | "randomizer"
  | "bench"
  | "duplicates"
  | "diff"
  | "merge"
  | "discography"
  | "editor"
  | "exportimport"
  | "stats"
  | "settings";

export interface Toast {
  id: number;
  kind: "info" | "success" | "error";
  text: string;
}

interface AppStore {
  ready: boolean;
  setup: SetupState | null;
  settings: Settings | null;
  page: Page;
  playlists: Playlist[];
  playlistsLoading: boolean;
  sessions: RandomizerSession[];
  benches: BenchRow[];
  /** Playlist preselected on the Bench page (set from the Playlists page). */
  benchPlaylistId: string | null;
  playback: PlaybackState | null;
  /** Wall-clock ms when `playback` was received; lets the UI tick progress between polls. */
  playbackReceivedAt: number;
  /** Unix seconds until artist-family API calls are paused, or null. */
  quotaCooldownUntil: number | null;
  /** Unix seconds since Spotify became unreachable, or null when online. */
  offlineSince: number | null;
  /** Newer version found by the updater, if any. */
  availableUpdate: UpdateInfo | null;
  toasts: Toast[];

  init: () => Promise<void>;
  setQuotaCooldown: (until: number | null) => void;
  refreshQuota: () => Promise<void>;
  setOfflineSince: (since: number | null) => void;
  setAvailableUpdate: (u: UpdateInfo | null) => void;
  setPage: (page: Page) => void;
  openBenchFor: (playlistId: string) => void;
  setBenchPlaylist: (playlistId: string | null) => void;
  refreshBenches: () => Promise<void>;
  setSetup: (setup: SetupState) => void;
  loadAll: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  refreshPlaylists: (force: boolean) => Promise<void>;
  refreshSessions: () => Promise<void>;
  setPlayback: (state: PlaybackState | null) => void;
  refreshPlayback: () => Promise<void>;
  toast: (kind: Toast["kind"], text: string) => void;
  dismissToast: (id: number) => void;
}

let toastSeq = 1;

export const useApp = create<AppStore>((set, get) => ({
  ready: false,
  setup: null,
  settings: null,
  page: "playlists",
  playlists: [],
  playlistsLoading: false,
  sessions: [],
  benches: [],
  benchPlaylistId: null,
  playback: null,
  playbackReceivedAt: 0,
  quotaCooldownUntil: null,
  offlineSince: null,
  availableUpdate: null,
  toasts: [],

  init: async () => {
    try {
      const setup = await api.getSetupState();
      set({ setup, ready: true });
      if (setup.isAuthenticated) await get().loadAll();
    } catch (e) {
      set({ ready: true });
      get().toast("error", errorMessage(e));
    }
  },

  loadAll: async () => {
    await Promise.all([
      get().refreshPlaylists(false),
      get().refreshSessions(),
      get().refreshBenches(),
      get().refreshPlayback(),
      get().refreshSettings(),
      get().refreshQuota(),
    ]);
  },

  setQuotaCooldown: (until) => set({ quotaCooldownUntil: until }),
  setOfflineSince: (since) => set({ offlineSince: since }),
  setAvailableUpdate: (u) => set({ availableUpdate: u }),

  refreshQuota: async () => {
    try {
      const q = await api.getQuotaStatus();
      set({ quotaCooldownUntil: q.cooldownUntil });
    } catch {
      // non-critical
    }
  },

  setPage: (page) => set({ page }),
  openBenchFor: (playlistId) => set({ benchPlaylistId: playlistId, page: "bench" }),
  setBenchPlaylist: (playlistId) => set({ benchPlaylistId: playlistId }),

  refreshBenches: async () => {
    try {
      set({ benches: await api.getBenchedTracks() });
    } catch (e) {
      get().toast("error", errorMessage(e));
    }
  },
  setSetup: (setup) => set({ setup }),

  refreshSettings: async () => {
    try {
      set({ settings: await api.getSettings() });
    } catch (e) {
      get().toast("error", errorMessage(e));
    }
  },

  refreshPlaylists: async (force) => {
    set({ playlistsLoading: true });
    try {
      const playlists = await api.getPlaylists(force);
      set({ playlists });
    } catch (e) {
      get().toast("error", errorMessage(e));
    } finally {
      set({ playlistsLoading: false });
    }
  },

  refreshSessions: async () => {
    try {
      set({ sessions: await api.getRandomizerSessions() });
    } catch (e) {
      get().toast("error", errorMessage(e));
    }
  },

  setPlayback: (playback) => set({ playback, playbackReceivedAt: Date.now() }),

  refreshPlayback: async () => {
    try {
      set({ playback: await api.getPlaybackState(true), playbackReceivedAt: Date.now() });
    } catch {
      // Playback is best-effort; the polling loop will retry.
    }
  },

  toast: (kind, text) => {
    const id = toastSeq++;
    set((s) => ({ toasts: [...s.toasts, { id, kind, text }] }));
    setTimeout(() => get().dismissToast(id), kind === "error" ? 8000 : 4500);
  },

  dismissToast: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
}));
