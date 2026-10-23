// In-memory stand-in for the Rust core, used when the UI runs in a plain
// browser (vite dev, screenshot tests). It implements every command in
// ./index.ts with sample data and emits the same events.
//
// URL switches: ?locked=1 (start at Unlock), ?wizard=1 (no vault: wizard),
// ?empty=1 (empty library), ?tooNew=1 (vault from a newer version),
// ?lang=he|ar|en (UI language), ?slow=1 (player shows the slow-stick notice).

import type {
  AppError,
  Drive,
  Episode,
  EventMap,
  Grid,
  Home,
  Id,
  ImportStatus,
  IntegrityReport,
  MediaFileInfo,
  MetadataPatch,
  Movie,
  PlayerCommand,
  PlayerState,
  Progress,
  QueueItem,
  RecentItem,
  Scored,
  SearchResults,
  Season,
  Settings,
  Show,
  SortBy,
  TrackInfo,
  WatchFilter,
  WatchItem,
} from './types';

const params = typeof window !== 'undefined' ? new URLSearchParams(window.location.search) : new URLSearchParams();
const flag = (k: string) => params.get(k) === '1';
/** `?freeze=1`: the player and import queue don't tick (screenshot tests). */
const frozen = flag('freeze');

// ---------- events ----------

type Listener = (payload: unknown) => void;
const listeners = new Map<string, Set<Listener>>();

function emit<K extends keyof EventMap>(event: K, payload: EventMap[K]) {
  listeners.get(event)?.forEach((l) => l(payload));
}

// ---------- artwork ----------

const ART: Record<number, [string, string]> = {};
let nextArt = 1;
function art(a: string, b: string): Id {
  const id = nextArt++;
  ART[id] = [a, b];
  return id;
}

function imageUrl(id: Id): string {
  const [a, b] = ART[id] ?? ['#2a2f38', '#14161a'];
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 160 160" preserveAspectRatio="none"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="${a}"/><stop offset="1" stop-color="${b}"/></linearGradient></defs><rect width="160" height="160" fill="url(#g)"/><circle cx="120" cy="40" r="46" fill="#ffffff" fill-opacity="0.06"/></svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

// ---------- library ----------

const now = Date.now();
const iso = (daysAgo: number) => new Date(now - daysAgo * 86400000).toISOString();

const progress = (positionMs: number, durationMs: number, watched = false, daysAgo?: number): Progress => ({
  positionMs,
  durationMs,
  watched,
  lastWatchedAt: daysAgo == null ? null : iso(daysAgo),
});

let nextMedia = 100;
const mediaInfo = new Map<Id, MediaFileInfo>();

function file(name: string, sizeGb: number, durationMin: number, extra: Partial<MediaFileInfo> = {}): Id {
  const id = nextMedia++;
  const sizeBytes = Math.round(sizeGb * 1e9);
  mediaInfo.set(id, {
    id,
    originalName: name,
    sizeBytes,
    encryptedBytes: Math.ceil((sizeBytes * 1.000016) / 1048576) * 1048576,
    durationMs: durationMin * 60000,
    container: 'mkv',
    tracks: [
      { id: 1, kind: 'video', lang: null, title: null, codec: 'hevc', external: false, default: true, forced: false, subtitleFileId: null, width: 1920, height: 1080, hdr: false },
      { id: 1, kind: 'audio', lang: 'eng', title: null, codec: 'eac3', external: false, default: true, forced: false, subtitleFileId: null, channels: 6 },
    ],
    subtitles: [],
    addedAt: iso(3),
    ...extra,
  });
  return id;
}

const empty = flag('empty');

const movies: Movie[] = empty
  ? []
  : [
      mkMovie(1, 'Dune', 2021, 155, ['Sci-fi', 'Adventure'], 'A noble family becomes embroiled in a war for control over the galaxy’s most valuable asset.', progress(0, 155 * 60000), 1, art('#c98b4a', '#3b2412'), art('#d8a35f', '#2a1a10'), file('Dune.2021.2160p.UHD.BluRay.x265.HDR.mkv', 18.4, 155, {
        tracks: [
          { id: 1, kind: 'video', lang: null, title: null, codec: 'hevc', external: false, default: true, forced: false, subtitleFileId: null, width: 3840, height: 2160, hdr: true },
          { id: 1, kind: 'audio', lang: 'eng', title: 'Atmos', codec: 'truehd', external: false, default: true, forced: false, subtitleFileId: null, channels: 8 },
          { id: 2, kind: 'audio', lang: 'heb', title: null, codec: 'ac3', external: false, default: false, forced: false, subtitleFileId: null, channels: 6 },
          { id: 1, kind: 'sub', lang: 'eng', title: null, codec: 'subrip', external: false, default: false, forced: false, subtitleFileId: null },
        ],
        subtitles: [{ id: 501, mediaFileId: 100, hasRtlCopy: false, language: 'he', format: 'srt', rtlFix: false, delayMs: 0 }],
      })),
      mkMovie(2, 'Arrival', 2016, 116, ['Sci-fi', 'Drama'], 'A linguist works with the military to communicate with alien lifeforms after twelve mysterious spacecraft appear around the world.', progress(41 * 60000, 116 * 60000, false, 1), 10, art('#6f8d9c', '#18242c'), art('#8fb0bf', '#1c2b33'), file('Arrival.2016.1080p.BluRay.x264.mkv', 9.2, 116)),
      mkMovie(3, 'ואלס עם באשיר', 2008, 90, ['Animation', 'Drama'], 'במאי יוצא למסע בעקבות זיכרונותיו ממלחמת לבנון הראשונה.', progress(0, 90 * 60000, true, 20), 30, art('#d9a441', '#3a2a0c'), null, file('Waltz.With.Bashir.2008.mkv', 4.4, 90)),
      mkMovie(4, 'Heat', 1995, 170, ['Crime', 'Thriller'], 'A group of high-end professional thieves start to feel the heat from the LAPD when they unknowingly leave a clue at their latest heist.', progress(0, 170 * 60000, true, 40), 60, art('#3d5a80', '#0f1a2a'), art('#4d6a90', '#131c28'), file('Heat.1995.1080p.mkv', 12.1, 170)),
      mkMovie(5, 'The Grand Budapest Hotel', 2014, 99, ['Comedy', 'Drama'], 'A writer encounters the owner of an aging high-class hotel, who tells him of his early years serving as a lobby boy.', progress(0, 99 * 60000), 5, art('#d97b93', '#4a1f2c'), art('#e39aae', '#3a1822'), file('The.Grand.Budapest.Hotel.2014.mkv', 6.8, 99)),
      mkMovie(6, 'Inception', 2010, 148, ['Sci-fi', 'Action'], 'A thief who steals corporate secrets through dream-sharing technology is given the inverse task of planting an idea.', progress(0, 148 * 60000), 2, art('#4b6e7d', '#101b21'), art('#5e8494', '#14232a'), file('Inception.2010.1080p.BluRay.x264.mkv', 10.5, 148)),
    ];

function mkMovie(id: Id, title: string, year: number, runtime: number, genres: string[], overview: string, p: Progress, addedDaysAgo: number, poster: Id | null, backdrop: Id | null, mediaFileId: Id): Movie {
  return {
    id,
    title,
    originalTitle: null,
    year,
    runtimeMin: runtime,
    overview,
    genres,
    tmdbId: null,
    posterId: poster,
    backdropId: backdrop,
    mediaFileId,
    audioLangOverride: null,
    subtitleLangOverride: null,
    addedAt: iso(addedDaysAgo),
    progress: p,
  };
}

const shows: Show[] = [];
const seasons: Season[] = [];
const episodes: Episode[] = [];

const EP_TITLES: Record<string, string[][]> = {
  Fauda: [
    ['The Wedding', 'Mourning', 'The Shoe', 'Doron', 'The Crossing', 'Escape', 'Abu Ahmad', 'Hostage'],
    ['Deceit', 'Funeral', 'The Return', 'Ambush', 'The Crossing', 'Nidal', 'Shirin', 'Gaby'],
  ],
  שטיסל: [['גשם', 'אקיבא', 'הדירה', 'הציור', 'החתונה', 'שלום', 'בית', 'ליבי']],
  'Breaking Bad': [['Pilot', "Cat's in the Bag...", "...And the Bag's in the River", 'Cancer Man', 'Gray Matter', "Crazy Handful of Nothin'", 'A No-Rough-Stuff-Type Deal']],
};

function mkShow(id: Id, title: string, original: string | null, first: number, last: number | null, genres: string[], overview: string, addedDaysAgo: number, poster: Id, backdrop: Id, watchedUpTo: [number, number], inProgress?: [number, number, number]) {
  shows.push({
    id,
    title,
    originalTitle: original,
    firstYear: first,
    lastYear: last,
    overview,
    genres,
    tmdbId: null,
    tvmazeId: null,
    posterId: poster,
    backdropId: backdrop,
    audioLangOverride: null,
    subtitleLangOverride: null,
    addedAt: iso(addedDaysAgo),
    seasonCount: 0,
    episodeCount: 0,
    watchedCount: 0,
  });
  const key = original ?? title;
  (EP_TITLES[key] ?? EP_TITLES[title]).forEach((titles, si) => {
    const sid = id * 100 + si + 1;
    seasons.push({ id: sid, showId: id, number: si + 1, title: null, posterId: null, episodeCount: titles.length, watchedCount: 0 });
    titles.forEach((et, ei) => {
      const s = si + 1;
      const e = ei + 1;
      const dur = 45;
      const watched = s < watchedUpTo[0] || (s === watchedUpTo[0] && e <= watchedUpTo[1]);
      const inProg = inProgress && inProgress[0] === s && inProgress[1] === e;
      const mf = file(`${title.replace(/\s/g, '.')}.S${String(s).padStart(2, '0')}E${String(e).padStart(2, '0')}.1080p.mkv`, 1.4, dur);
      if (title === 'Fauda') {
        const info = mediaInfo.get(mf)!;
        info.tracks.push({ id: 1, kind: 'sub', lang: 'eng', title: null, codec: 'subrip', external: false, default: false, forced: false, subtitleFileId: null });
        info.subtitles.push({ id: mf * 10, mediaFileId: mf, hasRtlCopy: false, language: 'he', format: 'srt', rtlFix: false, delayMs: 0 });
      }
      episodes.push({
        id: sid * 100 + e,
        seasonId: sid,
        showId: id,
        seasonNumber: s,
        number: e,
        title: et,
        overview: null,
        airDate: null,
        runtimeMin: dur,
        thumbId: art(['#2f4b57', '#4b3a2f', '#3a2f4b', '#2f4b3a'][(s + e) % 4], '#111317'),
        mediaFileId: mf,
        progress: progress(inProg ? inProgress![2] * 60000 : 0, dur * 60000, watched, watched || inProg ? 2 : undefined),
      });
    });
  });
}

if (!empty) {
  mkShow(1, 'Fauda', 'פאודה', 2015, null, ['Drama', 'Thriller'], 'Doron, a commander in an undercover unit, comes out of retirement to hunt down a militant he thought he had killed.', 4, art('#7a2e2e', '#1d0e0e'), art('#1c2b33', '#0b1014'), [2, 4], [2, 5, 21]);
  mkShow(2, 'שטיסל', null, 2013, 2021, ['Drama'], 'משפחה חרדית בשכונת גאולה בירושלים, בין מסורת לאהבה.', 15, art('#55606e', '#15181c'), art('#6b7889', '#1a1e23'), [1, 3]);
  mkShow(3, 'Breaking Bad', null, 2008, 2013, ['Crime', 'Drama'], 'A chemistry teacher diagnosed with cancer turns to manufacturing methamphetamine to secure his family’s future.', 50, art('#4c7a3a', '#10200b'), art('#5e8b4b', '#14240f'), [0, 0]);
}

function refreshCounts() {
  for (const s of seasons) {
    const eps = episodes.filter((e) => e.seasonId === s.id);
    s.episodeCount = eps.length;
    s.watchedCount = eps.filter((e) => e.progress.watched).length;
  }
  for (const sh of shows) {
    const ss = seasons.filter((s) => s.showId === sh.id);
    sh.seasonCount = ss.length;
    sh.episodeCount = ss.reduce((a, s) => a + s.episodeCount, 0);
    sh.watchedCount = ss.reduce((a, s) => a + s.watchedCount, 0);
  }
}
refreshCounts();

function progressOf(mediaFileId: Id): Progress | null {
  return movies.find((m) => m.mediaFileId === mediaFileId)?.progress ?? episodes.find((e) => e.mediaFileId === mediaFileId)?.progress ?? null;
}

const episodeItem = (e: Episode): WatchItem => {
  const sh = shows.find((s) => s.id === e.showId)!;
  return { kind: 'episode', episode: e, showId: sh.id, showTitle: sh.title, showBackdropId: sh.backdropId, showPosterId: sh.posterId };
};

const inProgress = (p: Progress) => !p.watched && p.positionMs > 0;

function upNextEpisode(showId: Id): Episode | null {
  const eps = episodes.filter((e) => e.showId === showId).sort((a, b) => a.seasonNumber - b.seasonNumber || a.number - b.number);
  const prog = eps.find((e) => inProgress(e.progress));
  if (prog) return prog;
  const lastWatched = [...eps].reverse().find((e) => e.progress.watched);
  if (!lastWatched) return null;
  return eps[eps.indexOf(lastWatched) + 1] ?? null;
}

function filterBy<T>(items: T[], f: WatchFilter, state: (x: T) => 'unwatched' | 'inProgress' | 'watched'): T[] {
  return f === 'all' ? items : items.filter((x) => state(x) === f);
}

function sortBy<T extends { title: string; addedAt: string }>(items: T[], s: SortBy, year: (x: T) => number, last: (x: T) => string): T[] {
  const c = [...items];
  if (s === 'title') c.sort((a, b) => a.title.localeCompare(b.title));
  if (s === 'year') c.sort((a, b) => year(b) - year(a));
  if (s === 'dateAdded') c.sort((a, b) => b.addedAt.localeCompare(a.addedAt));
  if (s === 'lastWatched') c.sort((a, b) => last(b).localeCompare(last(a)));
  return c;
}

const movieState = (m: Movie) => (m.progress.watched ? 'watched' : m.progress.positionMs > 0 ? 'inProgress' : 'unwatched');
const showState = (s: Show) => {
  const eps = episodes.filter((e) => e.showId === s.id);
  if (eps.length && eps.every((e) => e.progress.watched)) return 'watched' as const;
  if (eps.some((e) => e.progress.watched || e.progress.positionMs > 0)) return 'inProgress' as const;
  return 'unwatched' as const;
};

// ---------- settings, vault ----------

let settings: Settings = {
  language: null,
  theme: 'dark',
  lookupOnline: false,
  tmdbKey: null,
  metadataLanguage: 'en-US',
  audioLanguages: ['he', 'en'],
  subtitleLanguages: ['he', 'en'],
  hwdec: true,
  autoplayNext: true,
  idleLockMinutes: 15,
  lockOnSleep: true,
  subtitleScale: 1,
  subtitlePos: 100,
  subtitleColor: '#ffffff',
  subtitleBackground: true,
};

let unlocked = !flag('locked') && !flag('wizard') && !flag('tooNew');
let password = 'correct horse';
let hasRecovery = true;
const SIZE = 119e9;

const usedBytes = () => [...mediaInfo.values()].reduce((a, f) => a + f.encryptedBytes, 0) + 8e6;

const fail = (code: AppError['code'], message = code): never => {
  throw { code, message } satisfies AppError;
};
const needUnlocked = () => {
  if (!unlocked) fail('locked');
};

const WORDS = 'abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual'.split(' ');

// ---------- import queue ----------

let importPaused = false;
let nextItem = 1;
const queue: QueueItem[] = [];

function addItem(p: Omit<QueueItem, 'id' | 'candidates' | 'subtitles' | 'mediaFileId' | 'etaSeconds' | 'messages'> & Partial<QueueItem>) {
  queue.push({ id: nextItem++, candidates: [], subtitles: [], mediaFileId: null, etaSeconds: null, messages: [], ...p } as QueueItem);
}

if (!empty) {
  addItem({ kind: 'episode', fileName: 'Fauda.S03E01.1080p.WEB.mkv', sizeBytes: 1.6e9, detected: 'Fauda · S03E01', state: 'encrypting', fraction: 0.64, matchState: 'matched', etaSeconds: 70 } as never);
  addItem({
    kind: 'episode',
    fileName: 'The.Office.S01E01.720p.mkv',
    sizeBytes: 0.7e9,
    detected: 'The Office · S01E01',
    state: 'done',
    matchState: 'needsReview',
    mediaFileId: 999,
    candidates: [
      { candidate: { provider: 'tvmaze', id: '526', title: 'The Office', originalTitle: null, year: 2005, overview: 'A mockumentary on a group of typical office workers.', popularity: 1, poster: null }, confidence: 0.71 },
      { candidate: { provider: 'tvmaze', id: '1214', title: 'The Office', originalTitle: null, year: 2001, overview: 'The story of an office that faces closure.', popularity: 1, poster: null }, confidence: 0.69 },
    ],
  } as never);
  addItem({
    kind: 'movie',
    fileName: 'Waltz.With.Bashir.2008.mkv',
    sizeBytes: 4.4e9,
    detected: 'ואלס עם באשיר (2008)',
    state: 'done',
    matchState: 'matched',
    subtitles: [{ fileName: 'Waltz.With.Bashir.2008.he.srt', language: 'he', note: 'Converted from windows-1255 to UTF-8', rtlFixOffered: true }],
  } as never);
  addItem({ kind: 'episode', fileName: 'Fauda.S03E02.1080p.WEB.mkv', sizeBytes: 1.5e9, detected: 'Fauda · S03E02', state: 'waiting', matchState: 'offline' } as never);
  addItem({ kind: 'subtitle', fileName: 'random.ar.srt', sizeBytes: 0, detected: 'random.ar.srt', state: 'noVideo', matchState: 'offline' } as never);
}

function tickImport() {
  if (importPaused) return;
  const running = queue.find((q) => q.state === 'encrypting' || q.state === 'verifying');
  if (running && (running.state === 'encrypting' || running.state === 'verifying')) {
    running.fraction = Math.min(1, running.fraction + 0.01);
    running.etaSeconds = Math.max(0, (running.etaSeconds ?? 60) - 1);
    if (running.fraction >= 1) {
      if (running.state === 'encrypting') Object.assign(running, { state: 'verifying', fraction: 0 });
      else {
        Object.assign(running, { state: 'done', etaSeconds: null });
        delete (running as { fraction?: number }).fraction;
        const next = queue.find((q) => q.state === 'waiting');
        if (next) Object.assign(next, { state: 'encrypting', fraction: 0, etaSeconds: 90 });
        if (next) emit('import-progress', next);
      }
    }
    emit('import-progress', running);
  }
}

// ---------- player ----------

let player: PlayerState | null = null;
let playerTimer: ReturnType<typeof setInterval> | null = null;

function tracksFor(mediaFileId: Id): { audio: TrackInfo[]; subs: TrackInfo[] } {
  const info = mediaInfo.get(mediaFileId);
  const audio = info?.tracks.filter((t) => t.kind === 'audio') ?? [];
  const embeddedSubs = info?.tracks.filter((t) => t.kind === 'sub') ?? [];
  const external: TrackInfo[] = (info?.subtitles ?? []).map((s, i) => ({
    id: embeddedSubs.length + i + 1,
    kind: 'sub',
    lang: s.language,
    title: null,
    codec: s.format,
    external: true,
    default: false,
    forced: false,
    subtitleFileId: s.id,
  }));
  const subs = [...external, ...embeddedSubs];
  if (subs.length && !subs.some((s) => s.lang === 'eng' && /sdh/i.test(s.title ?? ''))) {
    subs.push({ id: subs.length + 1, kind: 'sub', lang: 'eng', title: 'SDH', codec: 'subrip', external: false, default: false, forced: false, subtitleFileId: null });
  }
  return { audio, subs };
}

function emitPlayer() {
  if (player) emit('player-state', { ...player });
}

function openPlayer(mediaFileId: Id, fromStart: boolean): PlayerState {
  const m = movies.find((x) => x.mediaFileId === mediaFileId);
  const e = episodes.find((x) => x.mediaFileId === mediaFileId);
  if (!m && !e) fail('notFound');
  const info = mediaInfo.get(mediaFileId)!;
  const { audio, subs } = tracksFor(mediaFileId);
  const p = (m ?? e)!.progress;
  let next: PlayerState['next'] = null;
  let title = m?.title ?? '';
  let line: string | null = null;
  if (e) {
    const sh = shows.find((s) => s.id === e.showId)!;
    title = sh.title;
    line = `S${e.seasonNumber} · E${e.number} · ${e.title ?? ''}`;
    const eps = episodes.filter((x) => x.showId === e.showId).sort((a, b) => a.seasonNumber - b.seasonNumber || a.number - b.number);
    const n = eps[eps.indexOf(e) + 1];
    if (n) next = { mediaFileId: n.mediaFileId, title: n.title ?? '', line: `S${n.seasonNumber} · E${n.number} · ${n.title ?? ''}`, thumbId: n.thumbId };
  }
  const duration = (info.durationMs ?? 0) / 1000;
  const heSub = subs.find((s) => s.lang === 'he');
  player = {
    mediaFileId,
    title,
    line,
    position: fromStart ? 0 : p.positionMs / 1000,
    duration,
    paused: false,
    buffering: false,
    volume: 80,
    muted: false,
    speed: 1,
    fullscreen: false,
    audioTracks: audio,
    subTracks: subs,
    audioId: audio[0]?.id ?? null,
    subId: heSub?.id ?? null,
    subDelayMs: 0,
    subScale: settings.subtitleScale,
    subPos: settings.subtitlePos,
    rtlFix: heSub?.external ? false : null,
    chapters: [
      { title: 'Opening', time: 0 },
      { title: null, time: duration * 0.22 },
      { title: null, time: duration * 0.5 },
      { title: 'Credits', time: duration * 0.95 },
    ],
    next,
    slowStick: flag('slow'),
    error: null,
  };
  const pos = params.get('pos');
  if (pos) player.position = Number(pos);
  if (playerTimer) clearInterval(playerTimer);
  if (frozen) return player;
  playerTimer = setInterval(() => {
    if (!player || player.paused) return;
    player.position = Math.min(player.duration ?? 0, player.position + 0.5 * player.speed);
    emitPlayer();
    if (player.duration && player.position >= player.duration) {
      emit('player-ended', { nextMediaFileId: player.next?.mediaFileId ?? null });
      player.paused = true;
    }
  }, 500);
  return player;
}

function closePlayer() {
  if (player) {
    const p = progressOf(player.mediaFileId);
    if (p && player.duration) {
      p.positionMs = player.position * 1000;
      p.lastWatchedAt = new Date().toISOString();
      if (player.position / player.duration >= 0.9) {
        p.watched = true;
        p.positionMs = 0;
      }
      refreshCounts();
    }
  }
  if (playerTimer) clearInterval(playerTimer);
  playerTimer = null;
  player = null;
}

function playerCommand(cmd: PlayerCommand) {
  if (!player) fail('player');
  const p = player!;
  switch (cmd.type) {
    case 'play':
      p.paused = false;
      break;
    case 'pause':
      p.paused = true;
      break;
    case 'togglePause':
      p.paused = !p.paused;
      break;
    case 'seek':
      p.position = Math.max(0, Math.min(p.duration ?? 0, cmd.relative ? p.position + cmd.seconds : cmd.seconds));
      break;
    case 'volume':
      p.volume = Math.max(0, Math.min(100, cmd.value));
      p.muted = false;
      break;
    case 'mute':
      p.muted = cmd.on;
      break;
    case 'setAudio':
      p.audioId = cmd.id;
      break;
    case 'setSub': {
      p.subId = cmd.id;
      const tr = p.subTracks.find((t) => t.id === cmd.id);
      const sf = tr?.subtitleFileId != null ? findSubtitle(tr.subtitleFileId) : null;
      p.rtlFix = sf ? sf.rtlFix : null;
      p.subDelayMs = sf?.delayMs ?? 0;
      break;
    }
    case 'subDelay':
      p.subDelayMs = cmd.ms;
      break;
    case 'subScale':
      p.subScale = cmd.value;
      break;
    case 'subPos':
      p.subPos = cmd.value;
      break;
    case 'speed':
      p.speed = cmd.value;
      break;
    case 'rtlFix': {
      p.rtlFix = cmd.on;
      const tr = p.subTracks.find((t) => t.id === p.subId);
      const sf = tr?.subtitleFileId != null ? findSubtitle(tr.subtitleFileId) : null;
      if (sf) sf.rtlFix = cmd.on;
      break;
    }
    case 'fullscreen':
      p.fullscreen = cmd.on;
      if (typeof document !== 'undefined') {
        if (cmd.on) void document.documentElement.requestFullscreen?.().catch(() => {});
        else if (document.fullscreenElement) void document.exitFullscreen?.().catch(() => {});
      }
      break;
    case 'next':
      if (p.next) {
        const n = p.next.mediaFileId;
        closePlayer();
        openPlayer(n, false);
      }
      break;
  }
  emitPlayer();
}

function findSubtitle(id: Id) {
  for (const f of mediaInfo.values()) {
    const s = f.subtitles.find((x) => x.id === id);
    if (s) return s;
  }
  return null;
}

// ---------- commands ----------

const delay = (ms: number) => new Promise((r) => setTimeout(r, ms));

const drives: Drive[] = [
  { id: '/dev/sdb', name: 'SanDisk Ultra', label: 'SANDISK', sizeBytes: 128e9, freeBytes: 119e9, filesystem: 'exfat', mountPath: '/media/user/SANDISK', hasVault: false, status: 'ready' },
  { id: '/dev/sdc', name: 'Kingston DataTraveler', label: 'KINGSTON', sizeBytes: 64e9, freeBytes: 30e9, filesystem: 'vfat', mountPath: '/media/user/KINGSTON', hasVault: false, status: 'needsFormatting' },
];

function removeMedia(mediaFileId: Id) {
  mediaInfo.delete(mediaFileId);
  const mi = movies.findIndex((m) => m.mediaFileId === mediaFileId);
  if (mi >= 0) movies.splice(mi, 1);
  for (let i = episodes.length - 1; i >= 0; i--) if (episodes[i].mediaFileId === mediaFileId) episodes.splice(i, 1);
  for (let i = seasons.length - 1; i >= 0; i--) if (!episodes.some((e) => e.seasonId === seasons[i].id)) seasons.splice(i, 1);
  for (let i = shows.length - 1; i >= 0; i--) if (!seasons.some((s) => s.showId === shows[i].id)) shows.splice(i, 1);
  refreshCounts();
}

const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  vault_detect: () =>
    flag('wizard')
      ? null
      : { root: '/media/user/CINASAFE', label: 'CINASAFE', sizeBytes: SIZE, usedBytes: usedBytes(), freeBytes: SIZE - usedBytes(), version: flag('tooNew') ? 2 : 1, supported: !flag('tooNew'), hasRecovery, createdAt: iso(90) },
  vault_status: () => ({ unlocked }),
  drives_list: async () => {
    await delay(300);
    return drives;
  },
  vault_create: async (a) => {
    if (String(a.password).length < 8) fail('weakPassword');
    if (a.mode === 'format') {
      emit('create-progress', { step: 'formatting' });
      await delay(1200);
    }
    emit('create-progress', { step: 'deriving' });
    await delay(900);
    emit('create-progress', { step: 'writing' });
    await delay(500);
    emit('create-progress', { step: 'copyingApps' });
    await delay(900);
    emit('create-progress', { step: 'done' });
    password = String(a.password);
    hasRecovery = Boolean(a.withRecovery);
    unlocked = true;
    return { root: '/media/user/SANDISK', recoveryWords: a.withRecovery ? WORDS : null };
  },
  vault_unlock: async (a) => {
    await delay(700);
    if (a.password !== password) fail('wrongPassword');
    unlocked = true;
    return { restoredFromBackup: false };
  },
  vault_unlock_recovery: async (a) => {
    await delay(700);
    const words = String(a.words).trim().split(/\s+/);
    if (words.length !== 24) fail('badRecoveryWords');
    if (words.join(' ') !== WORDS.join(' ')) fail('wrongRecoveryKey');
    unlocked = true;
    return { restoredFromBackup: false };
  },
  vault_lock: () => {
    closePlayer();
    unlocked = false;
    emit('vault-locked', { reason: 'user' });
  },
  vault_info: () => {
    needUnlocked();
    return {
      createdAt: iso(90),
      hasRecovery,
      sizeBytes: SIZE,
      usedBytes: usedBytes(),
      freeBytes: SIZE - usedBytes(),
      movieCount: movies.length,
      movieBytes: movies.reduce((a, m) => a + (mediaInfo.get(m.mediaFileId)?.sizeBytes ?? 0), 0),
      showCount: shows.length,
      showBytes: episodes.reduce((a, e) => a + (mediaInfo.get(e.mediaFileId)?.sizeBytes ?? 0), 0),
    };
  },
  vault_change_password: async (a) => {
    await delay(600);
    if (a.oldPassword !== password) fail('wrongPassword');
    if (String(a.newPassword).length < 8) fail('weakPassword');
    password = String(a.newPassword);
  },
  vault_replace_recovery: async (a) => {
    await delay(600);
    if (a.password !== password) fail('wrongPassword');
    hasRecovery = true;
    return [...WORDS].reverse();
  },
  vault_remove_recovery: () => {
    needUnlocked();
    hasRecovery = false;
  },
  vault_integrity_check: async (): Promise<IntegrityReport> => {
    const total = mediaInfo.size;
    for (let i = 0; i <= total; i += Math.max(1, Math.floor(total / 10))) {
      emit('integrity-progress', { done: i, total, bytesDone: i * 1e9, bytesTotal: total * 1e9 });
      await delay(150);
    }
    emit('integrity-progress', { done: total, total, bytesDone: total * 1e9, bytesTotal: total * 1e9 });
    return { checked: total, bytes: usedBytes(), corruptCount: 0, missingCount: 0, orphanedCount: 0, libraryRestoredFromBackup: false };
  },
  eject_drive: async () => {
    await delay(300);
    unlocked = false;
  },

  library_home: (): Home => {
    needUnlocked();
    const cw: WatchItem[] = [
      ...movies.filter((m) => inProgress(m.progress)).map((m) => ({ kind: 'movie', movie: m }) as WatchItem),
      ...episodes.filter((e) => inProgress(e.progress)).map(episodeItem),
    ].sort((a, b) => lastOf(b).localeCompare(lastOf(a)));
    const un: WatchItem[] = shows
      .map((s) => upNextEpisode(s.id))
      .filter((e): e is Episode => !!e && !inProgress(e.progress))
      .map(episodeItem);
    const recent: RecentItem[] = [
      ...movies.map((m) => ({ kind: 'movie', movie: m }) as RecentItem),
      ...shows.map((s) => ({ kind: 'show', show: s }) as RecentItem),
    ]
      .sort((a, b) => (b.kind === 'movie' ? b.movie.addedAt : b.show.addedAt).localeCompare(a.kind === 'movie' ? a.movie.addedAt : a.show.addedAt))
      .slice(0, 12);
    return {
      hero: cw[0] ?? un[0] ?? null,
      continueWatching: cw,
      upNext: un,
      recentlyAdded: recent,
      movies: sortBy(movies, 'dateAdded', (m) => m.year ?? 0, () => ''),
      shows: sortBy(shows, 'dateAdded', (s) => s.firstYear ?? 0, () => ''),
      empty: movies.length === 0 && shows.length === 0,
    };
  },
  library_movies: (a): Grid<Movie> => {
    needUnlocked();
    let items = filterBy(movies, a.filter as WatchFilter, movieState);
    if (a.genre) items = items.filter((m) => m.genres.includes(String(a.genre)));
    items = sortBy(items, a.sort as SortBy, (m) => m.year ?? 0, (m) => m.progress.lastWatchedAt ?? '');
    return {
      items,
      totalCount: movies.length,
      totalBytes: movies.reduce((s, m) => s + (mediaInfo.get(m.mediaFileId)?.sizeBytes ?? 0), 0),
      genres: [...new Set(movies.flatMap((m) => m.genres))].sort(),
    };
  },
  library_shows: (a): Grid<Show> => {
    needUnlocked();
    let items = filterBy(shows, a.filter as WatchFilter, showState);
    if (a.genre) items = items.filter((m) => m.genres.includes(String(a.genre)));
    items = sortBy(items, a.sort as SortBy, (s) => s.firstYear ?? 0, (s) =>
      episodes.filter((e) => e.showId === s.id).map((e) => e.progress.lastWatchedAt ?? '').sort().pop() ?? '',
    );
    return {
      items,
      totalCount: shows.length,
      totalBytes: episodes.reduce((s, e) => s + (mediaInfo.get(e.mediaFileId)?.sizeBytes ?? 0), 0),
      genres: [...new Set(shows.flatMap((m) => m.genres))].sort(),
    };
  },
  library_movie: (a) => {
    needUnlocked();
    const movie = movies.find((m) => m.id === a.id) ?? fail('notFound');
    return { movie, file: mediaInfo.get(movie.mediaFileId)! };
  },
  library_show: (a) => {
    needUnlocked();
    const show = shows.find((s) => s.id === a.id) ?? fail('notFound');
    const ss = seasons.filter((s) => s.showId === show.id);
    const resume = upNextEpisode(show.id) ?? episodes.find((e) => e.showId === show.id) ?? null;
    return { show, seasons: ss, resume };
  },
  library_season: (a) => {
    needUnlocked();
    const season = seasons.find((s) => s.id === a.id) ?? fail('notFound');
    const eps = episodes.filter((e) => e.seasonId === season.id).sort((x, y) => x.number - y.number);
    return { season, episodes: eps, upNextEpisodeId: upNextEpisode(season.showId)?.id ?? null };
  },
  library_episode: (a) => {
    needUnlocked();
    const episode = episodes.find((e) => e.id === a.id) ?? fail('notFound');
    return { episode, showTitle: shows.find((s) => s.id === episode.showId)?.title ?? '', file: mediaInfo.get(episode.mediaFileId)! };
  },
  library_update_metadata: (a) => {
    needUnlocked();
    const patch = a.patch as MetadataPatch;
    if (a.kind === 'movie') {
      const m = movies.find((x) => x.id === a.id) ?? fail('notFound');
      Object.assign(m, {
        ...(patch.title != null && { title: patch.title }),
        ...('originalTitle' in patch && { originalTitle: patch.originalTitle }),
        ...('year' in patch && { year: patch.year }),
        ...('overview' in patch && { overview: patch.overview }),
        ...(patch.genres && { genres: patch.genres }),
        ...('runtimeMin' in patch && { runtimeMin: patch.runtimeMin }),
      });
    } else if (a.kind === 'show') {
      const s = shows.find((x) => x.id === a.id) ?? fail('notFound');
      Object.assign(s, {
        ...(patch.title != null && { title: patch.title }),
        ...('originalTitle' in patch && { originalTitle: patch.originalTitle }),
        ...('year' in patch && { firstYear: patch.year }),
        ...('lastYear' in patch && { lastYear: patch.lastYear }),
        ...('overview' in patch && { overview: patch.overview }),
        ...(patch.genres && { genres: patch.genres }),
      });
    } else {
      const e = episodes.find((x) => x.id === a.id) ?? fail('notFound');
      Object.assign(e, { ...(patch.title != null && { title: patch.title }), ...('overview' in patch && { overview: patch.overview }) });
    }
  },
  library_set_overrides: (a) => {
    const target = a.kind === 'movie' ? movies.find((m) => m.id === a.id) : shows.find((s) => s.id === a.id);
    if (!target) fail('notFound');
    target!.audioLangOverride = (a.audio as string | null) ?? null;
    target!.subtitleLangOverride = (a.subtitles as string | null) ?? null;
  },
  library_mark_watched: (a) => {
    const p = progressOf(a.mediaFileId as Id) ?? fail('notFound');
    p.watched = Boolean(a.watched);
    p.positionMs = 0;
    p.lastWatchedAt = new Date().toISOString();
    refreshCounts();
  },
  library_mark_season_watched: (a) => {
    for (const e of episodes.filter((x) => x.seasonId === a.seasonId)) {
      e.progress.watched = Boolean(a.watched);
      e.progress.positionMs = 0;
    }
    refreshCounts();
  },
  library_remove: (a) => {
    if (a.kind === 'movie') removeMedia((movies.find((m) => m.id === a.id) ?? fail('notFound')).mediaFileId);
    else if (a.kind === 'episode') removeMedia((episodes.find((e) => e.id === a.id) ?? fail('notFound')).mediaFileId);
    else for (const e of episodes.filter((x) => x.showId === a.id)) removeMedia(e.mediaFileId);
  },
  library_export: async (a) => {
    await delay(400);
    const f = mediaInfo.get(a.mediaFileId as Id) ?? fail('notFound');
    return `/home/user/Videos/${f.originalName}`;
  },
  search: (a): SearchResults => {
    needUnlocked();
    const q = String(a.query).trim().toLowerCase();
    if (!q) return { shows: [], movies: [], episodes: [], showTitles: {} };
    const terms = q.split(/\s+/);
    const hit = (s: string | null) => !!s && terms.every((t) => s.toLowerCase().split(/[\s.,:'’-]+/).some((w) => w.startsWith(t)));
    const showTitles: Record<string, string> = {};
    for (const s of shows) showTitles[String(s.id)] = s.title;
    return {
      shows: shows.filter((s) => hit(s.title) || hit(s.originalTitle)),
      movies: movies.filter((m) => hit(m.title) || hit(m.originalTitle)),
      episodes: episodes.filter((e) => hit(e.title)).slice(0, 20),
      showTitles,
    };
  },

  import_add: (a) => {
    const added: QueueItem[] = [];
    for (const p of a.paths as string[]) {
      const name = p.split(/[\\/]/).pop() ?? p;
      addItem({ kind: 'movie', fileName: name, sizeBytes: 2e9, detected: name.replace(/\.[^.]+$/, '').replace(/\./g, ' '), state: 'waiting', matchState: 'offline' } as never);
      added.push(queue[queue.length - 1]);
      emit('import-progress', queue[queue.length - 1]);
    }
    if (!queue.some((q) => q.state === 'encrypting' || q.state === 'verifying')) {
      const next = queue.find((q) => q.state === 'waiting');
      if (next) Object.assign(next, { state: 'encrypting', fraction: 0, etaSeconds: 90 });
    }
    return added;
  },
  import_browse: (a) => handlers.import_add({ paths: a.folders ? ['/home/user/Videos/Shtisel S03'] : ['/home/user/Videos/Paterson.2016.mkv'] }),
  import_status: (): ImportStatus => ({ items: queue, paused: importPaused }),
  import_pause: () => {
    importPaused = true;
    emit('import-queue', { paused: true });
  },
  import_resume: () => {
    importPaused = false;
    emit('import-queue', { paused: false });
  },
  import_cancel: (a) => {
    for (const q of queue) {
      if ((a.id == null || q.id === a.id) && ['waiting', 'encrypting', 'verifying', 'lookingUp', 'finishing'].includes(q.state)) {
        Object.assign(q, { state: 'cancelled', etaSeconds: null });
        emit('import-progress', q);
      }
    }
  },
  import_clear_finished: () => {
    for (let i = queue.length - 1; i >= 0; i--) {
      const q = queue[i];
      if (['done', 'failed', 'cancelled', 'alreadyInVault', 'wontFit', 'noVideo'].includes(q.state) && q.matchState !== 'needsReview') queue.splice(i, 1);
    }
  },
  import_resolve: async (a) => {
    await delay(300);
    const q = queue.find((x) => x.id === a.itemId) ?? fail('notFound');
    const r = a.resolution as { kind: string; id?: string; title?: string; year?: number | null };
    if (r.kind === 'candidate') {
      const c = q.candidates.find((x) => x.candidate.id === r.id);
      if (c) q.detected = `${c.candidate.title}${c.candidate.year ? ` (${c.candidate.year})` : ''}`;
    } else q.detected = `${r.title}${r.year ? ` (${r.year})` : ''}`;
    q.matchState = 'matched';
    emit('import-progress', q);
  },
  tmdb_search: async (a): Promise<Scored[]> => {
    await delay(400);
    if (!settings.lookupOnline) fail('lookupsOff');
    const q = String(a.query);
    return [
      { candidate: { provider: 'tmdb', id: '1', title: q, originalTitle: null, year: (a.year as number | null) ?? 2010, overview: 'A search result from the mock backend.', popularity: 10, poster: null }, confidence: 0.92 },
      { candidate: { provider: 'tmdb', id: '2', title: `${q} II`, originalTitle: null, year: 2014, overview: null, popularity: 3, poster: null }, confidence: 0.61 },
    ];
  },
  storage: () => ({
    sizeBytes: SIZE,
    usedBytes: usedBytes(),
    freeBytes: SIZE - usedBytes(),
    queuedBytes: queue.filter((q) => ['waiting', 'encrypting', 'verifying', 'lookingUp'].includes(q.state)).reduce((a, q) => a + q.sizeBytes, 0),
  }),

  player_open: (a) => {
    needUnlocked();
    return openPlayer(a.mediaFileId as Id, Boolean(a.fromStart));
  },
  player_command: (a) => playerCommand(a.cmd as PlayerCommand),
  player_close: () => closePlayer(),
  player_state: () => player,

  settings_get: () => settings,
  settings_set: (a) => {
    settings = { ...settings, ...(a.patch as Partial<Settings>) };
    return settings;
  },
  app_info: () => ({
    version: '0.1.0',
    platform: 'linux-x86_64',
    mpvVersion: 'mpv 0.39.0 (mock)',
    playerApps: [
      { platform: 'windows-x64', version: '0.1.0' },
      { platform: 'linux-x86_64', version: '0.1.0' },
    ],
  }),
  update_player_apps: async () => {
    await delay(1200);
  },
  activity: () => undefined,
};

function lastOf(w: WatchItem): string {
  return (w.kind === 'movie' ? w.movie.progress.lastWatchedAt : w.episode.progress.lastWatchedAt) ?? '';
}

if (typeof window !== 'undefined' && !frozen) setInterval(tickImport, 500);

export const mockBackend = {
  invoke: async <T>(cmd: string, args: Record<string, unknown> = {}): Promise<T> => {
    const h = handlers[cmd];
    if (!h) throw { code: 'internal', message: `mock: no command ${cmd}` } satisfies AppError;
    await delay(20);
    // Hand out copies so the UI can't mutate mock state by accident.
    const r = await h(args);
    return (r === undefined ? undefined : JSON.parse(JSON.stringify(r))) as T;
  },
  listen: async <K extends keyof EventMap>(event: K, cb: (payload: EventMap[K]) => void) => {
    const set = listeners.get(event) ?? new Set();
    listeners.set(event, set);
    const l = cb as Listener;
    set.add(l);
    return () => set.delete(l);
  },
  imageUrl,
};

export const mockCommands = Object.keys(handlers);
