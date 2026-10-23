// Typed wrappers for every Tauri command and event (brief section 11).
// When the UI runs outside Tauri (vite dev in a browser, screenshot tests),
// calls go to the in-memory mock in ./mock.ts instead.

import type {
  AppInfo,
  CreateMode,
  CreateResult,
  Drive,
  EpisodeDetail,
  EventMap,
  Grid,
  Home,
  Id,
  ImportStatus,
  IntegrityReport,
  MetadataKind,
  MetadataPatch,
  Movie,
  MovieDetail,
  PlayerCommand,
  PlayerState,
  QueueItem,
  Resolution,
  Scored,
  SearchResults,
  SeasonDetail,
  Settings,
  Show,
  ShowDetail,
  SortBy,
  Storage,
  UnlockResult,
  VaultInfo,
  VaultSummary,
  WatchFilter,
} from './types';

export * from './types';

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;
type Listen = <K extends keyof EventMap>(event: K, cb: (payload: EventMap[K]) => void) => Promise<() => void>;

interface Backend {
  invoke: Invoke;
  listen: Listen;
  /** URL for artwork served by the cinasafe-img protocol. */
  imageUrl: (id: Id) => string;
}

export const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

let backendPromise: Promise<Backend> | null = null;

function backend(): Promise<Backend> {
  if (!backendPromise) {
    backendPromise = inTauri
      ? Promise.all([import('@tauri-apps/api/core'), import('@tauri-apps/api/event')]).then(([core, event]) => ({
          invoke: core.invoke as Invoke,
          listen: (async (name, cb) => event.listen(name, (e) => cb(e.payload as never))) as Listen,
          imageUrl: (id: Id) => core.convertFileSrc(String(id), 'cinasafe-img'),
        }))
      : import('./mock').then((m) => m.mockBackend);
  }
  return backendPromise;
}

const call = <T>(cmd: string, args?: Record<string, unknown>) => backend().then((b) => b.invoke<T>(cmd, args));

export function on<K extends keyof EventMap>(event: K, cb: (payload: EventMap[K]) => void): () => void {
  let unlisten: (() => void) | null = null;
  let cancelled = false;
  backend()
    .then((b) => b.listen(event, cb))
    .then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
  return () => {
    cancelled = true;
    unlisten?.();
  };
}

/** Synchronous artwork URL (the backend is resolved by the time anything renders artwork). */
let imageUrlImpl: (id: Id) => string = (id) => `cinasafe-img://localhost/${id}`;
backend().then((b) => {
  imageUrlImpl = b.imageUrl;
});
export const imageUrl = (id: Id | null | undefined): string | undefined =>
  id == null ? undefined : imageUrlImpl(id);

export const api = {
  // Vault
  vaultDetect: () => call<VaultSummary | null>('vault_detect'),
  vaultStatus: () => call<{ unlocked: boolean }>('vault_status'),
  drivesList: () => call<Drive[]>('drives_list'),
  vaultCreate: (a: { driveId: string; mode: CreateMode; label: string | null; password: string; withRecovery: boolean }) =>
    call<CreateResult>('vault_create', a),
  vaultUnlock: (password: string) => call<UnlockResult>('vault_unlock', { password }),
  vaultUnlockRecovery: (words: string) => call<UnlockResult>('vault_unlock_recovery', { words }),
  vaultLock: () => call<void>('vault_lock'),
  vaultInfo: () => call<VaultInfo>('vault_info'),
  vaultChangePassword: (oldPassword: string, newPassword: string) =>
    call<void>('vault_change_password', { oldPassword, newPassword }),
  vaultReplaceRecovery: (password: string) => call<string[]>('vault_replace_recovery', { password }),
  /** Removes the recovery key (wizard "Skip", right after creation; needs an unlocked vault). */
  vaultRemoveRecovery: () => call<void>('vault_remove_recovery'),
  vaultIntegrityCheck: () => call<IntegrityReport>('vault_integrity_check'),
  ejectDrive: () => call<void>('eject_drive'),

  // Library
  libraryHome: () => call<Home>('library_home'),
  libraryMovies: (filter: WatchFilter, genre: string | null, sort: SortBy) =>
    call<Grid<Movie>>('library_movies', { filter, genre, sort }),
  libraryShows: (filter: WatchFilter, genre: string | null, sort: SortBy) =>
    call<Grid<Show>>('library_shows', { filter, genre, sort }),
  libraryMovie: (id: Id) => call<MovieDetail>('library_movie', { id }),
  libraryShow: (id: Id) => call<ShowDetail>('library_show', { id }),
  librarySeason: (id: Id) => call<SeasonDetail>('library_season', { id }),
  libraryEpisode: (id: Id) => call<EpisodeDetail>('library_episode', { id }),
  libraryUpdateMetadata: (kind: MetadataKind, id: Id, patch: MetadataPatch) =>
    call<void>('library_update_metadata', { kind, id, patch }),
  librarySetOverrides: (kind: 'movie' | 'show', id: Id, audio: string | null, subtitles: string | null) =>
    call<void>('library_set_overrides', { kind, id, audio, subtitles }),
  libraryMarkWatched: (mediaFileId: Id, watched: boolean) =>
    call<void>('library_mark_watched', { mediaFileId, watched }),
  libraryMarkSeasonWatched: (seasonId: Id, watched: boolean) =>
    call<void>('library_mark_season_watched', { seasonId, watched }),
  libraryRemove: (kind: 'movie' | 'show' | 'episode', id: Id) => call<void>('library_remove', { kind, id }),
  /** Opens a save dialog; resolves to the written path, or null if cancelled. */
  libraryExport: (mediaFileId: Id) => call<string | null>('library_export', { mediaFileId }),
  search: (query: string) => call<SearchResults>('search', { query }),

  // Import
  importAdd: (paths: string[]) => call<QueueItem[]>('import_add', { paths }),
  /** Opens a native file or folder picker and queues what was picked. */
  importBrowse: (folders: boolean) => call<QueueItem[]>('import_browse', { folders }),
  importStatus: () => call<ImportStatus>('import_status'),
  importPause: () => call<void>('import_pause'),
  importResume: () => call<void>('import_resume'),
  importCancel: (id: number | null) => call<void>('import_cancel', { id }),
  importClearFinished: () => call<void>('import_clear_finished'),
  importResolve: (itemId: number, resolution: Resolution) => call<void>('import_resolve', { itemId, resolution }),
  metadataSearch: (kind: 'movie' | 'show', query: string, year: number | null) =>
    call<Scored[]>('tmdb_search', { kind, query, year }),
  storage: () => call<Storage>('storage'),

  // Player
  playerOpen: (mediaFileId: Id, fromStart = false) => call<PlayerState>('player_open', { mediaFileId, fromStart }),
  playerCommand: (cmd: PlayerCommand) => call<void>('player_command', { cmd }),
  playerClose: () => call<void>('player_close'),
  playerState: () => call<PlayerState | null>('player_state'),

  // Settings and app
  settingsGet: () => call<Settings>('settings_get'),
  settingsSet: (patch: Partial<Settings>) => call<Settings>('settings_set', { patch }),
  appInfo: () => call<AppInfo>('app_info'),
  updatePlayerApps: () => call<void>('update_player_apps'),
  /** Tells the core the user is active (idle lock). Throttle on the UI side. */
  activity: () => call<void>('activity'),
};

export type Api = typeof api;

/** Narrows an unknown rejection to AppError. */
export function asAppError(e: unknown): import('./types').AppError {
  if (e && typeof e === 'object' && 'code' in e) return e as import('./types').AppError;
  return { code: 'internal', message: String(e) };
}
