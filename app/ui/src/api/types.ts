// The IPC contract between the Rust core (app/src-tauri) and the UI.
// Brief section 11. Field names are camelCase on both sides (serde
// rename_all = "camelCase"); Tauri maps camelCase invoke arguments to the
// snake_case Rust parameters.

export type Id = number;

// ---------- Errors ----------

/** Every command rejects with this. `code` is also the i18n key `errors.<code>`. */
export interface AppError {
  code: ErrorCode;
  /** English fallback / technical detail. Never contains titles or paths from the vault. */
  message: string;
}

export type ErrorCode =
  | 'wrongPassword'
  | 'wrongRecoveryKey'
  | 'badRecoveryWords'
  | 'noRecoveryKey'
  | 'vaultTooNew'
  | 'notAVault'
  | 'vaultExists'
  | 'locked'
  | 'driveRemoved'
  | 'notFound'
  | 'cancelled'
  | 'verifyFailed'
  | 'libraryUnreadable'
  | 'lookupsOff'
  | 'network'
  | 'unauthorized'
  | 'player'
  | 'playerUnavailable'
  | 'formatFailed'
  | 'formatCancelled'
  | 'badLabel'
  | 'weakPassword'
  /** Update player apps from an installed (non-portable) copy. */
  | 'notPortable'
  | 'io'
  | 'internal';

// ---------- Vault ----------

/** Where the app is running from, when that's a vault (`vault_detect`). */
export interface VaultSummary {
  root: string;
  /** Volume label, if any. */
  label: string | null;
  sizeBytes: number;
  usedBytes: number;
  freeBytes: number | null;
  version: number;
  /** False: made by a newer Cinasafe ("Update Cinasafe"). */
  supported: boolean;
  hasRecovery: boolean;
  createdAt: string;
}

export interface UnlockResult {
  /** The library file was damaged and its backup copy was opened. */
  restoredFromBackup: boolean;
}

export interface VaultInfo {
  createdAt: string;
  hasRecovery: boolean;
  sizeBytes: number;
  usedBytes: number;
  freeBytes: number | null;
  movieCount: number;
  movieBytes: number;
  showCount: number;
  showBytes: number;
}

export interface IntegrityProgress {
  done: number;
  total: number;
  bytesDone: number;
  bytesTotal: number;
}

export interface IntegrityReport {
  checked: number;
  bytes: number;
  corruptCount: number;
  missingCount: number;
  orphanedCount: number;
  libraryRestoredFromBackup: boolean;
}

export type DriveStatus = 'ready' | 'needsFormatting' | 'notMounted';

export interface Drive {
  id: string;
  name: string;
  label: string | null;
  sizeBytes: number;
  freeBytes: number | null;
  filesystem: string | null;
  mountPath: string | null;
  hasVault: boolean;
  status: DriveStatus;
}

export type CreateMode = 'useAsIs' | 'format';

export interface CreateResult {
  root: string;
  /** Shown once, then gone. Null when created without a recovery key. */
  recoveryWords: string[] | null;
}

export type CreateStep = 'formatting' | 'deriving' | 'writing' | 'copyingApps' | 'done';

// ---------- Library ----------

export interface Progress {
  positionMs: number;
  durationMs: number | null;
  watched: boolean;
  lastWatchedAt: string | null;
}

export interface Movie {
  id: Id;
  title: string;
  originalTitle: string | null;
  year: number | null;
  runtimeMin: number | null;
  overview: string | null;
  genres: string[];
  tmdbId: number | null;
  posterId: Id | null;
  backdropId: Id | null;
  mediaFileId: Id;
  audioLangOverride: string | null;
  subtitleLangOverride: string | null;
  addedAt: string;
  progress: Progress;
}

export interface Show {
  id: Id;
  title: string;
  originalTitle: string | null;
  firstYear: number | null;
  lastYear: number | null;
  overview: string | null;
  genres: string[];
  tmdbId: number | null;
  tvmazeId: number | null;
  posterId: Id | null;
  backdropId: Id | null;
  audioLangOverride: string | null;
  subtitleLangOverride: string | null;
  addedAt: string;
  seasonCount: number;
  episodeCount: number;
  watchedCount: number;
}

export interface Season {
  id: Id;
  showId: Id;
  /** 0 = specials. */
  number: number;
  title: string | null;
  posterId: Id | null;
  episodeCount: number;
  watchedCount: number;
}

export interface Episode {
  id: Id;
  seasonId: Id;
  showId: Id;
  seasonNumber: number;
  number: number;
  title: string | null;
  overview: string | null;
  airDate: string | null;
  runtimeMin: number | null;
  thumbId: Id | null;
  mediaFileId: Id;
  progress: Progress;
}

export type WatchItem =
  | { kind: 'movie'; movie: Movie }
  | {
      kind: 'episode';
      episode: Episode;
      showId: Id;
      showTitle: string;
      showBackdropId: Id | null;
      showPosterId: Id | null;
    };

export type RecentItem = { kind: 'movie'; movie: Movie } | { kind: 'show'; show: Show };

export interface Home {
  /** The first continue-watching item, else the first up-next item. */
  hero: WatchItem | null;
  continueWatching: WatchItem[];
  upNext: WatchItem[];
  recentlyAdded: RecentItem[];
  movies: Movie[];
  shows: Show[];
  /** Nothing in the vault yet: show "Add your first movie or show". */
  empty: boolean;
}

export type WatchFilter = 'all' | 'unwatched' | 'inProgress' | 'watched';
export type SortBy = 'dateAdded' | 'title' | 'year' | 'lastWatched';

export interface Grid<T> {
  items: T[];
  /** All items of this kind, before filtering. */
  totalCount: number;
  totalBytes: number;
  /** Genres to offer in the genre select. */
  genres: string[];
}

export interface TrackInfo {
  /** mpv track id. */
  id: number;
  kind: 'video' | 'audio' | 'sub';
  lang: string | null;
  title: string | null;
  codec: string | null;
  external: boolean;
  default: boolean;
  forced: boolean;
  /** For imported subtitle files: the library id (RTL fix, delay). */
  subtitleFileId: Id | null;
  /** Video only: "3840×2160", "HDR". */
  width?: number | null;
  height?: number | null;
  hdr?: boolean;
  /** Audio only. */
  channels?: number | null;
}

export interface SubtitleFile {
  id: Id;
  mediaFileId: Id;
  hasRtlCopy: boolean;
  language: string | null;
  format: string;
  rtlFix: boolean;
  delayMs: number;
}

export interface MediaFileInfo {
  id: Id;
  originalName: string;
  sizeBytes: number;
  /** Size on the stick, padding and authentication tags included. */
  encryptedBytes: number;
  durationMs: number | null;
  container: string | null;
  /** From the probe at import; empty when unknown. */
  tracks: TrackInfo[];
  /** Imported external subtitle files. */
  subtitles: SubtitleFile[];
  addedAt: string;
}

export interface MovieDetail {
  movie: Movie;
  file: MediaFileInfo;
}

export interface ShowDetail {
  show: Show;
  seasons: Season[];
  /** "Resume S2 E5": the episode in progress, else the next unwatched one. */
  resume: Episode | null;
}

export interface SeasonDetail {
  season: Season;
  episodes: Episode[];
  /** The episode the show page would resume (for "Up next" status lines). */
  upNextEpisodeId: Id | null;
}

export interface EpisodeDetail {
  episode: Episode;
  showTitle: string;
  file: MediaFileInfo;
}

export type MetadataKind = 'movie' | 'show' | 'episode';

export interface MetadataPatch {
  title?: string;
  originalTitle?: string | null;
  year?: number | null;
  /** Shows. */
  lastYear?: number | null;
  overview?: string | null;
  genres?: string[];
  runtimeMin?: number | null;
  /** Episodes. */
  number?: number;
  airDate?: string | null;
}

export interface SearchResults {
  shows: Show[];
  movies: Movie[];
  episodes: Episode[];
  /** Show titles for the episode rows, by show id. */
  showTitles: Record<string, string>;
}

// ---------- Import ----------

export interface ArtworkRef {
  kind: 'poster' | 'backdrop' | 'thumb';
  url: string;
  width: number | null;
  height: number | null;
}

export interface Candidate {
  provider: string;
  id: string;
  title: string;
  originalTitle: string | null;
  year: number | null;
  overview: string | null;
  popularity: number;
  /** Remote URL. The UI never loads it (no network from the webview). */
  poster: ArtworkRef | null;
}

export interface Scored {
  candidate: Candidate;
  /** 0..1 */
  confidence: number;
}

export type ItemState =
  | { state: 'waiting' }
  | { state: 'wontFit' }
  | { state: 'alreadyInVault' }
  | { state: 'noVideo' }
  | { state: 'lookingUp' }
  | { state: 'encrypting'; fraction: number }
  | { state: 'verifying'; fraction: number }
  | { state: 'finishing' }
  | { state: 'done' }
  | { state: 'failed'; message: string }
  | { state: 'cancelled' };

export type MatchState = 'offline' | 'matched' | 'needsReview' | 'noMatch';

export interface SubtitleNote {
  fileName: string;
  language: string | null;
  /** e.g. "Converted from windows-1255 to UTF-8" (the UI localises: see `encoding`). */
  note: string | null;
  rtlFixOffered: boolean;
}

export type QueueItem = ItemState & {
  id: number;
  kind: 'movie' | 'episode' | 'subtitle';
  fileName: string;
  sizeBytes: number;
  /** "Inception (2010)" or "Fauda · S01E02". */
  detected: string;
  matchState: MatchState;
  candidates: Scored[];
  subtitles: SubtitleNote[];
  mediaFileId: Id | null;
  etaSeconds: number | null;
  messages: string[];
};

export interface ImportStatus {
  items: QueueItem[];
  paused: boolean;
}

export type Resolution =
  | { kind: 'candidate'; id: string }
  | { kind: 'manual'; title: string; year: number | null };

export interface Storage {
  sizeBytes: number;
  usedBytes: number;
  freeBytes: number | null;
  /** Bytes the waiting and running imports will add. */
  queuedBytes: number;
}

// ---------- Player ----------

export interface Chapter {
  title: string | null;
  /** Seconds. */
  time: number;
}

export interface NextUp {
  mediaFileId: Id;
  title: string;
  /** "S2 · E6 · Title" */
  line: string;
  thumbId: Id | null;
}

export interface PlayerState {
  mediaFileId: Id;
  title: string;
  /** Episode line ("Season 2 · Episode 5 · The Crossing"), null for movies. */
  line: string | null;
  /** Seconds. */
  position: number;
  duration: number | null;
  paused: boolean;
  buffering: boolean;
  /** 0..100 */
  volume: number;
  muted: boolean;
  speed: number;
  fullscreen: boolean;
  audioTracks: TrackInfo[];
  subTracks: TrackInfo[];
  audioId: number | null;
  subId: number | null;
  subDelayMs: number;
  /** 1.0 = default size. */
  subScale: number;
  /** 0..100, 100 = bottom. */
  subPos: number;
  /** For the selected imported subtitle file; null for embedded tracks or Off. */
  rtlFix: boolean | null;
  chapters: Chapter[];
  next: NextUp | null;
  /** Read speed from the stick is below the file's bitrate. */
  slowStick: boolean;
  /** Playback error (message key under errors.*), if any. */
  error: ErrorCode | null;
}

export type PlayerCommand =
  | { type: 'play' }
  | { type: 'pause' }
  | { type: 'togglePause' }
  /** Absolute position in seconds, or relative when `relative` is true. */
  | { type: 'seek'; seconds: number; relative?: boolean }
  | { type: 'volume'; value: number }
  | { type: 'mute'; on: boolean }
  | { type: 'setAudio'; id: number | null }
  | { type: 'setSub'; id: number | null }
  | { type: 'subDelay'; ms: number }
  | { type: 'subScale'; value: number }
  | { type: 'subPos'; value: number }
  | { type: 'speed'; value: number }
  | { type: 'rtlFix'; on: boolean }
  | { type: 'fullscreen'; on: boolean }
  | { type: 'next' };

// ---------- Settings ----------

export type Language = 'en' | 'he' | 'ar';

export interface Settings {
  /** Null: follow the system language. */
  language: Language | null;
  theme: 'dark' | 'light';
  /** Off by default (docs/OPEN_QUESTIONS.md). */
  lookupOnline: boolean;
  /** User-supplied TMDB key; without one shows use TVmaze and movies aren't looked up. */
  tmdbKey: string | null;
  /** TMDB language, e.g. "he-IL". */
  metadataLanguage: string;
  /** ISO 639-1 codes in order of preference. */
  audioLanguages: string[];
  subtitleLanguages: string[];
  hwdec: boolean;
  autoplayNext: boolean;
  /** 0 = never. */
  idleLockMinutes: number;
  lockOnSleep: boolean;
  subtitleScale: number;
  subtitlePos: number;
  subtitleColor: string;
  subtitleBackground: boolean;
}

export interface AppInfo {
  version: string;
  /** "linux-x86_64" etc. */
  platform: string;
  mpvVersion: string | null;
  /** Player apps present on the stick, by platform. */
  playerApps: { platform: string; version: string | null }[];
}

// ---------- Events ----------

export type LockReason = 'user' | 'idle' | 'removed' | 'sleep';

export interface EventMap {
  'vault-locked': { reason: LockReason };
  'drive-removed': Record<string, never>;
  'import-progress': QueueItem;
  'import-queue': { paused: boolean };
  'integrity-progress': IntegrityProgress;
  'create-progress': { step: CreateStep };
  'player-state': PlayerState;
  'player-ended': { nextMediaFileId: Id | null };
  'read-speed-warning': Record<string, never>;
}
