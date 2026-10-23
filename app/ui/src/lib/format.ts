import type { TFunction } from 'i18next';

import type { TrackInfo } from '../api/types';

/** Wraps text in a first-strong isolate so numbers and units keep their order in RTL text. */
export const isolate = (s: string) => `\u2068${s}\u2069`;

/** Decimal units, like the operating system's file managers. */
export function formatBytes(n: number, t: TFunction): string {
  return isolate(bytesText(n, t));
}

function bytesText(n: number, t: TFunction): string {
  if (n < 1000) return t('units.bytes', { n });
  const units = ['kb', 'mb', 'gb', 'tb'] as const;
  let v = n;
  let u = -1;
  while (v >= 1000 && u < units.length - 1) {
    v /= 1000;
    u++;
  }
  const digits = v >= 100 ? 0 : 1;
  return t(`units.${units[u]}`, { n: v.toFixed(digits) });
}

/** "1 h 58 min", "45 min", "2 h". */
export function formatRuntime(minutes: number | null | undefined, t: TFunction): string | null {
  if (minutes == null || minutes <= 0) return null;
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  if (h === 0) return t('units.minutes', { m });
  if (m === 0) return t('units.hours', { h });
  return t('units.hoursMinutes', { h, m });
}

/** Player clock: "4:05", "1:02:03". */
export function formatClock(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds) || seconds < 0) return '0:00';
  const s = Math.floor(seconds);
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  const mm = h > 0 ? String(m).padStart(2, '0') : String(m);
  return `${h > 0 ? `${h}:` : ''}${mm}:${String(sec).padStart(2, '0')}`;
}

/** "24 min left" from a position and duration in milliseconds. */
export function minutesLeft(positionMs: number, durationMs: number | null, t: TFunction): string | null {
  if (!durationMs || durationMs <= 0) return null;
  const left = Math.max(0, durationMs - positionMs) / 60000;
  if (left < 1) return t('units.lessThanMinuteLeft');
  return t('units.minutesLeft', { m: Math.round(left) });
}

/** ETA for the import queue. */
export function formatEta(seconds: number | null | undefined, t: TFunction): string | null {
  if (seconds == null) return null;
  if (seconds < 60) return t('units.lessThanMinuteLeft');
  const count = Math.round(seconds / 60);
  return t('units.aboutMinutesLeft', { count });
}

const ISO2: Record<string, string> = {
  heb: 'he', eng: 'en', ara: 'ar', fre: 'fr', fra: 'fr', spa: 'es', ger: 'de', deu: 'de', rus: 'ru', ita: 'it',
  jpn: 'ja', kor: 'ko', chi: 'zh', zho: 'zh', por: 'pt', tur: 'tr', dut: 'nl', nld: 'nl', pol: 'pl', swe: 'sv',
  hin: 'hi', per: 'fa', fas: 'fa', ukr: 'uk', gre: 'el', ell: 'el', iw: 'he',
};

/** Normalises "heb", "he-IL", "HE" to "he". */
export function langCode(lang: string | null | undefined): string | null {
  if (!lang) return null;
  const l = lang.toLowerCase().split(/[-_]/)[0];
  return ISO2[l] ?? (l.length === 2 ? l : null);
}

const NATIVE: Record<string, string> = {
  he: 'עברית', en: 'English', ar: 'العربية', fr: 'Français', es: 'Español', de: 'Deutsch', ru: 'Русский',
  it: 'Italiano', ja: '日本語', ko: '한국어', zh: '中文', pt: 'Português', tr: 'Türkçe', nl: 'Nederlands',
  pl: 'Polski', sv: 'Svenska', hi: 'हिन्दी', fa: 'فارسی', uk: 'Українська', el: 'Ελληνικά',
};

const ENGLISH: Record<string, string> = {
  he: 'Hebrew', en: 'English', ar: 'Arabic', fr: 'French', es: 'Spanish', de: 'German', ru: 'Russian',
  it: 'Italian', ja: 'Japanese', ko: 'Korean', zh: 'Chinese', pt: 'Portuguese', tr: 'Turkish', nl: 'Dutch',
  pl: 'Polish', sv: 'Swedish', hi: 'Hindi', fa: 'Persian', uk: 'Ukrainian', el: 'Greek',
};

export const KNOWN_LANGUAGES = Object.keys(NATIVE);

/** "עברית · Hebrew" for a language code, or the code itself. */
export function languageName(lang: string | null | undefined): string | null {
  const c = langCode(lang);
  if (!c) return null;
  const native = NATIVE[c];
  const english = ENGLISH[c];
  if (!native) return c;
  return native === english ? native : `${native} · ${english}`;
}

const CODEC_NAMES: Record<string, string> = {
  subrip: 'SRT', srt: 'SRT', ass: 'ASS', ssa: 'SSA', webvtt: 'WebVTT', vtt: 'WebVTT', mov_text: 'Text',
  hdmv_pgs_subtitle: 'PGS', pgs: 'PGS', dvd_subtitle: 'VobSub', vobsub: 'VobSub',
  aac: 'AAC', ac3: 'AC-3', eac3: 'E-AC-3', dts: 'DTS', truehd: 'TrueHD', opus: 'Opus', flac: 'FLAC', mp3: 'MP3',
  h264: 'H.264', hevc: 'HEVC', av1: 'AV1', vp9: 'VP9', mpeg4: 'MPEG-4',
};

export function codecName(codec: string | null | undefined): string | null {
  if (!codec) return null;
  return CODEC_NAMES[codec.toLowerCase()] ?? codec.toUpperCase();
}

/**
 * Track menu label, as in the mockups: "עברית · Hebrew · SRT",
 * "English · embedded", "English SDH · embedded".
 */
export function trackLabel(track: TrackInfo, t: TFunction): string {
  const parts: string[] = [];
  const lang = languageName(track.lang);
  const sdh = /\b(sdh|hi|cc)\b/i.test(track.title ?? '');
  let first = lang ?? track.title ?? t('player.track', { n: track.id });
  if (sdh) first += ` ${t('player.sdh')}`;
  parts.push(first);
  if (track.forced) parts.push(t('player.forced'));
  if (track.kind === 'sub') {
    parts.push(track.external ? (codecName(track.codec) ?? t('player.external')) : t('player.embedded'));
  } else if (track.kind === 'audio') {
    const c = codecName(track.codec);
    if (c) parts.push(c + (track.channels ? ` ${channelLayout(track.channels)}` : ''));
  }
  return parts.join(' · ');
}

function channelLayout(n: number): string {
  if (n === 1) return 'mono';
  if (n === 2) return 'stereo';
  if (n === 6) return '5.1';
  if (n === 8) return '7.1';
  return `${n}ch`;
}

/** "4K HDR · HEVC" style summary for the video track. */
export function videoSummary(tracks: TrackInfo[]): string | null {
  const v = tracks.find((x) => x.kind === 'video');
  if (!v) return null;
  const parts: string[] = [];
  const h = v.height ?? 0;
  const w = v.width ?? 0;
  if (w >= 3200 || h >= 2000) parts.push('4K');
  else if (h >= 1000 || w >= 1900) parts.push('1080p');
  else if (h >= 700) parts.push('720p');
  else if (h > 0) parts.push(`${h}p`);
  if (v.hdr) parts.push('HDR');
  const c = codecName(v.codec);
  const res = parts.join(' ');
  return [res || null, c].filter(Boolean).join(' · ') || null;
}

/** Splits text into [before, match, after...] chunks for highlighting search hits. */
export function highlight(text: string, query: string): { text: string; hit: boolean }[] {
  const terms = query
    .trim()
    .split(/\s+/)
    .filter(Boolean)
    .map((s) => s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&'));
  if (terms.length === 0) return [{ text, hit: false }];
  const re = new RegExp(`(${terms.join('|')})`, 'giu');
  return text
    .split(re)
    .filter((s) => s !== '')
    .map((s) => ({ text: s, hit: terms.some((term) => new RegExp(`^${term}$`, 'iu').test(s)) }));
}

/** 0..4 password strength for the wizard meter. Length and variety; no dictionary. */
export function passwordStrength(pw: string): 0 | 1 | 2 | 3 | 4 {
  if (pw.length < 8) return 0;
  let classes = 0;
  if (/[a-z]/.test(pw)) classes++;
  if (/[A-Z]/.test(pw)) classes++;
  if (/[0-9]/.test(pw)) classes++;
  if (/[^A-Za-z0-9]/.test(pw)) classes++;
  const unique = new Set(pw).size;
  let score = 1;
  if (pw.length >= 12) score++;
  if (pw.length >= 16 || (pw.length >= 12 && classes >= 3)) score++;
  if (pw.length >= 20 || (pw.length >= 14 && classes >= 3)) score++;
  if (unique < 5) score = Math.min(score, 1);
  return Math.min(4, score) as 0 | 1 | 2 | 3 | 4;
}

export function yearRange(first: number | null, last: number | null, t: TFunction): string | null {
  if (!first) return null;
  if (!last) return t('detail.yearsOngoing', { from: first });
  if (last === first) return String(first);
  return t('detail.years', { from: first, to: last });
}
