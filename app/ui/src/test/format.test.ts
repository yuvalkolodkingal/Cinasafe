import { beforeAll, describe, expect, it } from 'vitest';

import i18n from '../i18n';
import type { TrackInfo } from '../api/types';
import { formatBytes, formatClock, formatEta, formatRuntime, highlight, langCode, passwordStrength, trackLabel, videoSummary } from '../lib/format';

const t = i18n.t.bind(i18n);

const track = (p: Partial<TrackInfo>): TrackInfo => ({
  id: 1,
  kind: 'sub',
  lang: null,
  title: null,
  codec: null,
  external: false,
  default: false,
  forced: false,
  subtitleFileId: null,
  ...p,
});

describe('formatting', () => {
  beforeAll(async () => {
    await i18n.changeLanguage('en');
  });

  it('sizes use decimal units', () => {
    const fb = (n: number) => formatBytes(n, t).replace(/[\u2068\u2069]/g, '');
    expect(fb(999)).toBe('999 B');
    expect(fb(1500)).toBe('1.5 KB');
    expect(fb(61.2e9)).toBe('61.2 GB');
    expect(fb(312e9)).toBe('312 GB');
    expect(fb(2.5e12)).toBe('2.5 TB');
    expect(formatBytes(1500, t)).toBe('\u20681.5 KB\u2069');
  });

  it('runtimes read like the mockups', () => {
    expect(formatRuntime(118, t)).toBe('1 h 58 min');
    expect(formatRuntime(45, t)).toBe('45 min');
    expect(formatRuntime(120, t)).toBe('2 h');
    expect(formatRuntime(null, t)).toBeNull();
  });

  it('runtimes in Hebrew', async () => {
    await i18n.changeLanguage('he');
    expect(formatRuntime(118, t)).toBe('1 שע׳ 58 דק׳');
    await i18n.changeLanguage('en');
  });

  it('player clock', () => {
    expect(formatClock(0)).toBe('0:00');
    expect(formatClock(65)).toBe('1:05');
    expect(formatClock(3723.9)).toBe('1:02:03');
    expect(formatClock(NaN)).toBe('0:00');
  });

  it('import ETA', () => {
    expect(formatEta(30, t)).toBe('less than a minute left');
    expect(formatEta(70, t)).toBe('about 1 min left');
    expect(formatEta(null, t)).toBeNull();
  });

  it('language codes normalise', () => {
    expect(langCode('heb')).toBe('he');
    expect(langCode('he-IL')).toBe('he');
    expect(langCode('ENG')).toBe('en');
    expect(langCode('')).toBeNull();
    expect(langCode('und')).toBeNull();
  });

  it('track labels match the mockups', () => {
    expect(trackLabel(track({ lang: 'he', codec: 'srt', external: true }), t)).toBe('עברית · Hebrew · SRT');
    expect(trackLabel(track({ lang: 'eng' }), t)).toBe('English · embedded');
    expect(trackLabel(track({ lang: 'eng', title: 'SDH' }), t)).toBe('English SDH · embedded');
    expect(trackLabel(track({ id: 3 }), t)).toBe('Track 3 · embedded');
    expect(trackLabel(track({ kind: 'audio', lang: 'ara', codec: 'eac3', channels: 6 }), t)).toBe('العربية · Arabic · E-AC-3 5.1');
  });

  it('video summary', () => {
    expect(videoSummary([track({ kind: 'video', codec: 'hevc', width: 3840, height: 2160, hdr: true })])).toBe('4K HDR · HEVC');
    expect(videoSummary([track({ kind: 'video', codec: 'h264', width: 1920, height: 800 })])).toBe('1080p · H.264');
    expect(videoSummary([])).toBeNull();
  });

  it('highlights search terms, Hebrew included', () => {
    expect(highlight('Breaking Bad', 'bad')).toEqual([
      { text: 'Breaking ', hit: false },
      { text: 'Bad', hit: true },
    ]);
    expect(highlight('פאודה', 'פאו')).toEqual([
      { text: 'פאו', hit: true },
      { text: 'דה', hit: false },
    ]);
    expect(highlight('a (b)', '(b')).toEqual([
      { text: 'a ', hit: false },
      { text: '(b', hit: true },
      { text: ')', hit: false },
    ]);
  });

  it('password strength', () => {
    expect(passwordStrength('short')).toBe(0);
    expect(passwordStrength('abcdefgh')).toBe(1);
    expect(passwordStrength('aaaaaaaaaaaaaaaaaaaaaaa')).toBe(1);
    expect(passwordStrength('correct horse battery')).toBeGreaterThanOrEqual(3);
    expect(passwordStrength('Tr0ub4dor&3xyz')).toBe(4);
  });
});
