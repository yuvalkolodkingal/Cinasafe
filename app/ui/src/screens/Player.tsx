// Player controls drawn over mpv's video (brief 6.4, 7.3, 9.3 #6). The page is
// transparent here: the video is rendered by mpv underneath the webview.

import { useCallback, useEffect, useRef, useState, type CSSProperties } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { api, asAppError, on, type PlayerCommand, type PlayerState, type TrackInfo } from '../api';
import { Icon } from '../components/Icon';
import { Artwork, Button, IconButton } from '../components/ui';
import { formatClock, languageName, trackLabel } from '../lib/format';
import { useSession } from '../lib/session';

const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 2];
type Menu = 'subs' | 'audio' | 'speed' | null;

/** The error text. A video-output failure also shows the driver's own words (no vault data in it). */
function playerErrorText(t: (k: string) => string, e: unknown): string {
  const ae = asAppError(e);
  const text = t(`errors.${ae.code}`);
  return ae.code === 'videoUnavailable' && ae.message ? `${text} (${ae.message})` : text;
}

export function Player() {
  const { t } = useTranslation();
  const { mediaFileId } = useParams();
  const [search] = useSearchParams();
  const navigate = useNavigate();
  const { settings } = useSession();
  const [st, setSt] = useState<PlayerState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [visible, setVisible] = useState(true);
  const [menu, setMenu] = useState<Menu>(null);
  const [scrub, setScrub] = useState<number | null>(null);
  const [nextCancelled, setNextCancelled] = useState(false);
  const [countdown, setCountdown] = useState<number | null>(null);
  const [slowDismissed, setSlowDismissed] = useState(false);
  const hideTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rootRef = useRef<HTMLDivElement>(null);
  const openedId = useRef<number | null>(null);
  const nextCancelledRef = useRef(false);
  nextCancelledRef.current = nextCancelled;

  const cmd = useCallback((c: PlayerCommand) => {
    api.playerCommand(c).catch((e) => setError(playerErrorText(t, e)));
  }, [t]);

  // Transparent page while the player is up.
  useEffect(() => {
    document.documentElement.classList.add('player-active');
    return () => document.documentElement.classList.remove('player-active');
  }, []);

  // Open on mount (and when the route changes to another file); close on leave.
  useEffect(() => {
    const id = Number(mediaFileId);
    if (openedId.current === id) return;
    openedId.current = id;
    setNextCancelled(false);
    setCountdown(null);
    setSlowDismissed(false);
    api.playerOpen(id, search.get('start') === '1').then(setSt, (e) => setError(playerErrorText(t, e)));
  }, [mediaFileId, search, t]);

  useEffect(() => {
    return () => {
      void api.playerClose();
    };
  }, []);

  useEffect(() => {
    const offs = [
      on('player-state', (s) => {
        setSt(s);
        // "Next" switched files inside the core: follow it in the URL.
        if (openedId.current !== s.mediaFileId) {
          openedId.current = s.mediaFileId;
          navigate(`/play/${s.mediaFileId}`, { replace: true });
        }
      }),
      on('player-ended', (e) => {
        if (e.nextMediaFileId && settings.autoplayNext && !nextCancelledRef.current) cmd({ type: 'next' });
        else navigate(-1);
      }),
      on('read-speed-warning', () => setSt((s) => (s ? { ...s, slowStick: true } : s))),
    ];
    return () => offs.forEach((o) => o());
  }, [navigate, settings.autoplayNext, cmd]);

  // Controls fade after 3 s without input (unless a menu is open).
  const poke = useCallback(() => {
    setVisible(true);
    if (hideTimer.current) clearTimeout(hideTimer.current);
    hideTimer.current = setTimeout(() => setVisible(false), 3000);
  }, []);
  useEffect(() => {
    poke();
    return () => {
      if (hideTimer.current) clearTimeout(hideTimer.current);
    };
  }, [poke]);
  const showControls = visible || menu !== null || !!st?.paused;

  const back = useCallback(() => {
    if (st?.fullscreen) cmd({ type: 'fullscreen', on: false });
    else navigate(-1);
  }, [st?.fullscreen, cmd, navigate]);

  const cycle = useCallback(
    (kind: 'audio' | 'sub') => {
      if (!st) return;
      const list = kind === 'audio' ? st.audioTracks : st.subTracks;
      const ids: (number | null)[] = kind === 'sub' ? [null, ...list.map((x) => x.id)] : list.map((x) => x.id);
      if (ids.length === 0) return;
      const cur = kind === 'audio' ? st.audioId : st.subId;
      const next = ids[(ids.indexOf(cur) + 1) % ids.length];
      cmd(kind === 'audio' ? { type: 'setAudio', id: next } : { type: 'setSub', id: next });
    },
    [st, cmd],
  );

  // Keyboard shortcuts (brief 6.4).
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      if (target.closest('input[type="text"], textarea')) return;
      poke();
      if (!st) return;
      const k = e.key;
      const inMenu = target.closest('.player-menu');
      if (k === ' ' && !target.closest('button, [role="radio"]')) {
        e.preventDefault();
        cmd({ type: 'togglePause' });
      } else if ((k === 'ArrowLeft' || k === 'ArrowRight') && !inMenu && !target.closest('input[type="range"]')) {
        e.preventDefault();
        cmd({ type: 'seek', seconds: k === 'ArrowLeft' ? -10 : 10, relative: true });
      } else if ((k === 'ArrowUp' || k === 'ArrowDown') && !inMenu && !target.closest('input[type="range"]')) {
        e.preventDefault();
        cmd({ type: 'volume', value: Math.max(0, Math.min(100, st.volume + (k === 'ArrowUp' ? 5 : -5))) });
      } else if (k === 'f' || k === 'F') cmd({ type: 'fullscreen', on: !st.fullscreen });
      else if (k === 'm' || k === 'M') cmd({ type: 'mute', on: !st.muted });
      else if (k === 's' || k === 'S') cycle('sub');
      else if (k === 'a' || k === 'A') cycle('audio');
      else if (k === 'Escape') {
        if (menu) setMenu(null);
        else back();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [st, menu, poke, cmd, cycle, back]);

  // Next-episode card: last 30 s, or from a "credits" chapter.
  const creditsAt = st?.chapters.find((c) => /credit/i.test(c.title ?? ''))?.time;
  const nextWindowStart = st?.duration ? Math.min(st.duration - 30, creditsAt ?? Infinity) : Infinity;
  const showNext = !!st?.next && !nextCancelled && st.position >= nextWindowStart;
  useEffect(() => {
    if (!showNext || !settings.autoplayNext) {
      setCountdown(null);
      return;
    }
    setCountdown((c) => c ?? 10);
    const timer = setInterval(() => setCountdown((c) => (c == null ? c : Math.max(0, c - 1))), 1000);
    return () => clearInterval(timer);
  }, [showNext, settings.autoplayNext]);
  useEffect(() => {
    if (countdown === 0) {
      setCountdown(null);
      cmd({ type: 'next' });
    }
  }, [countdown, cmd]);

  const position = scrub ?? st?.position ?? 0;
  const duration = st?.duration ?? 0;
  const subTrack = st?.subTracks.find((x) => x.id === st.subId) ?? null;

  return (
    <div
      ref={rootRef}
      className={`player ${showControls ? 'controls-on' : 'controls-off'}`}
      onMouseMove={poke}
      onPointerDown={poke}
      onDoubleClick={(e) => {
        if ((e.target as HTMLElement).classList.contains('player-surface') && st) cmd({ type: 'fullscreen', on: !st.fullscreen });
      }}
    >
      <div
        className="player-surface"
        onClick={() => {
          if (menu) setMenu(null);
          else if (st) cmd({ type: 'togglePause' });
        }}
      />

      <header className="player-top">
        <IconButton icon="back" round label={t('player.back')} onClick={back} />
        <div className="player-titles">
          <bdi className="player-title">{st?.title}</bdi>
          {st?.line && (
            <span className="player-line" dir="auto">
              {st.line}
            </span>
          )}
        </div>
      </header>

      {(st?.buffering || (!st && !error)) && (
        <div className="player-center" role="status">
          <span className="spinner large" aria-hidden="true" />
          <span className="sr-only">{t('player.buffering')}</span>
        </div>
      )}
      {(error || st?.error) && (
        <div className="player-center player-error" role="alert">
          <Icon name="alert" size={28} />
          <p>{t('player.error')}</p>
          <p className="muted">{error ?? t(`errors.${st!.error}`)}</p>
          <Button onClick={() => navigate(-1)}>{t('player.back')}</Button>
        </div>
      )}

      {st?.slowStick && !slowDismissed && (
        <div className="player-notice" role="status">
          <Icon name="alert" size={20} />
          <span>{t('player.slowStick')}</span>
          <IconButton icon="x" label={t('common.close')} onClick={() => setSlowDismissed(true)} />
        </div>
      )}

      {showNext && st?.next && (
        <div className="next-card" role="dialog" aria-label={t('player.nextEpisode')}>
          <Artwork id={st.next.thumbId} kind="thumb" className="next-thumb" />
          <div className="next-text">
            <span className="eyebrow">{countdown != null ? t('player.nextUpIn', { s: countdown }) : t('player.nextEpisode')}</span>
            <bdi className="next-title">{st.next.line}</bdi>
            <div className="button-row">
              <Button variant="primary" icon="play" onClick={() => cmd({ type: 'next' })}>
                {t('player.playNow')}
              </Button>
              <Button onClick={() => setNextCancelled(true)}>{t('common.cancel')}</Button>
            </div>
          </div>
        </div>
      )}

      {st && (
        <footer className="player-bottom">
          <div className="seek-row" dir="ltr">
            <span className="time" aria-label={t('player.elapsed')}>
              {formatClock(position)}
            </span>
            <div className="seek-wrap">
              <div className="seek-track">
                <div className="seek-fill" style={{ inlineSize: `${duration ? (position / duration) * 100 : 0}%` }} />
                {st.chapters
                  .filter((c) => c.time > 0 && duration > 0)
                  .map((c) => (
                    <span key={c.time} className="chapter-tick" style={{ insetInlineStart: `${(c.time / duration) * 100}%` }} title={c.title ?? t('player.chapter')} />
                  ))}
              </div>
              <input
                type="range"
                className="seek-input"
                min={0}
                max={Math.max(1, duration)}
                step={1}
                value={Math.min(position, duration || 0)}
                aria-label={t('player.seek')}
                aria-valuetext={`${formatClock(position)} / ${formatClock(duration)}`}
                onChange={(e) => setScrub(Number(e.target.value))}
                onPointerUp={() => {
                  if (scrub != null) cmd({ type: 'seek', seconds: scrub });
                  setScrub(null);
                }}
                onKeyUp={(e) => {
                  if (scrub != null && (e.key.startsWith('Arrow') || e.key === 'Home' || e.key === 'End' || e.key.startsWith('Page'))) {
                    cmd({ type: 'seek', seconds: scrub });
                    setScrub(null);
                  }
                }}
                style={{ '--pos': `${duration ? (position / duration) * 100 : 0}%` } as CSSProperties}
              />
            </div>
            <span className="time" aria-label={t('player.remaining')}>
              -{formatClock(Math.max(0, duration - position))}
            </span>
          </div>

          <div className="control-row">
            <div className="control-group" dir="ltr">
              <IconButton icon="replay10" label={t('player.back10')} onClick={() => cmd({ type: 'seek', seconds: -10, relative: true })} size={26} />
              <button
                type="button"
                className="play-btn"
                aria-label={st.paused ? t('player.play') : t('player.pause')}
                onClick={() => cmd({ type: 'togglePause' })}
              >
                <Icon name={st.paused ? 'play' : 'pause'} size={30} />
              </button>
              <IconButton icon="forward10" label={t('player.forward10')} onClick={() => cmd({ type: 'seek', seconds: 10, relative: true })} size={26} />
              <div className="volume">
                <IconButton
                  icon={st.muted || st.volume === 0 ? 'volumeMute' : 'volume'}
                  label={st.muted ? t('player.unmute') : t('player.mute')}
                  onClick={() => cmd({ type: 'mute', on: !st.muted })}
                />
                <input
                  type="range"
                  className="volume-input"
                  min={0}
                  max={100}
                  value={st.muted ? 0 : st.volume}
                  aria-label={t('player.volume')}
                  onChange={(e) => cmd({ type: 'volume', value: Number(e.target.value) })}
                  style={{ '--pos': `${st.muted ? 0 : st.volume}%` } as CSSProperties}
                />
              </div>
            </div>

            <div className="control-group end">
              <div className="menu-anchor">
                <button
                  type="button"
                  className={`ctl-btn ${menu === 'audio' ? 'open' : ''}`}
                  aria-haspopup="true"
                  aria-expanded={menu === 'audio'}
                  onClick={() => setMenu(menu === 'audio' ? null : 'audio')}
                  disabled={st.audioTracks.length === 0}
                >
                  <Icon name="audio" size={22} />
                  <span>{t('player.audio')}</span>
                </button>
                {menu === 'audio' && (
                  <TrackMenu
                    title={t('player.audio')}
                    tracks={st.audioTracks}
                    selected={st.audioId}
                    onSelect={(id) => cmd({ type: 'setAudio', id })}
                    onClose={() => setMenu(null)}
                  />
                )}
              </div>
              <div className="menu-anchor">
                <button
                  type="button"
                  className={`ctl-btn ${menu === 'subs' ? 'open' : ''}`}
                  aria-haspopup="true"
                  aria-expanded={menu === 'subs'}
                  onClick={() => setMenu(menu === 'subs' ? null : 'subs')}
                >
                  <Icon name="subtitles" size={22} />
                  <span>{subTrack ? (languageName(subTrack.lang)?.split(' · ')[0] ?? t('player.subtitles')) : t('player.subtitlesOff')}</span>
                </button>
                {menu === 'subs' && <SubtitleMenu st={st} cmd={cmd} onClose={() => setMenu(null)} />}
              </div>
              <div className="menu-anchor">
                <button
                  type="button"
                  className={`ctl-btn ${menu === 'speed' ? 'open' : ''}`}
                  aria-haspopup="true"
                  aria-expanded={menu === 'speed'}
                  aria-label={t('player.speed')}
                  onClick={() => setMenu(menu === 'speed' ? null : 'speed')}
                >
                  <Icon name="speed" size={22} />
                  <span dir="ltr">{st.speed}×</span>
                </button>
                {menu === 'speed' && (
                  <div className="player-menu" role="radiogroup" aria-label={t('player.speed')}>
                    <h3 className="menu-title">{t('player.speed')}</h3>
                    {SPEEDS.map((s) => (
                      <MenuRadio key={s} checked={st.speed === s} onSelect={() => cmd({ type: 'speed', value: s })} onClose={() => setMenu(null)}>
                        {s === 1 ? t('player.normalSpeed') : <span dir="ltr">{s}×</span>}
                      </MenuRadio>
                    ))}
                  </div>
                )}
              </div>
              {st.next && <IconButton icon="next" label={t('player.nextEpisode')} onClick={() => cmd({ type: 'next' })} />}
              <IconButton
                icon={st.fullscreen ? 'fullscreenExit' : 'fullscreen'}
                label={st.fullscreen ? t('player.exitFullscreen') : t('player.fullscreen')}
                onClick={() => cmd({ type: 'fullscreen', on: !st.fullscreen })}
              />
            </div>
          </div>
        </footer>
      )}
    </div>
  );
}

function MenuRadio({ checked, onSelect, children, onClose }: { checked: boolean; onSelect: () => void; children: React.ReactNode; onClose?: () => void }) {
  return (
    <button
      type="button"
      role="radio"
      aria-checked={checked}
      className={`menu-radio ${checked ? 'checked' : ''}`}
      onClick={onSelect}
      onKeyDown={(e) => {
        const items = [...(e.currentTarget.closest('.player-menu')?.querySelectorAll<HTMLElement>('[role="radio"]') ?? [])];
        const i = items.indexOf(e.currentTarget);
        if (e.key === 'ArrowDown') {
          e.preventDefault();
          items[(i + 1) % items.length]?.focus();
        } else if (e.key === 'ArrowUp') {
          e.preventDefault();
          items[(i - 1 + items.length) % items.length]?.focus();
        } else if (e.key === 'Escape') {
          e.stopPropagation();
          onClose?.();
        }
      }}
    >
      <span className="radio-dot" aria-hidden="true" />
      <span className="menu-radio-label">{children}</span>
    </button>
  );
}

function TrackMenu({ title, tracks, selected, onSelect, onClose }: { title: string; tracks: TrackInfo[]; selected: number | null; onSelect: (id: number) => void; onClose: () => void }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('[aria-checked="true"], [role="radio"]')?.focus();
  }, []);
  return (
    <div className="player-menu" role="radiogroup" aria-label={title} ref={ref}>
      <h3 className="menu-title">{title}</h3>
      {tracks.map((tr) => (
        <MenuRadio key={tr.id} checked={tr.id === selected} onSelect={() => onSelect(tr.id)} onClose={onClose}>
          <bdi>{trackLabel(tr, t)}</bdi>
        </MenuRadio>
      ))}
    </div>
  );
}

function SubtitleMenu({ st, cmd, onClose }: { st: PlayerState; cmd: (c: PlayerCommand) => void; onClose: () => void }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('[aria-checked="true"]')?.focus();
  }, []);
  const delay = st.subDelayMs / 1000;
  return (
    <div className="player-menu subs-menu" role="dialog" aria-label={t('player.subtitles')} ref={ref}>
      <h3 className="menu-title">{t('player.subtitles')}</h3>
      <div role="radiogroup" aria-label={t('player.subtitles')}>
      <MenuRadio checked={st.subId == null} onSelect={() => cmd({ type: 'setSub', id: null })} onClose={onClose}>
        {t('player.subtitlesOff')}
      </MenuRadio>
      {st.subTracks.map((tr) => (
        <MenuRadio key={tr.id} checked={tr.id === st.subId} onSelect={() => cmd({ type: 'setSub', id: tr.id })} onClose={onClose}>
          <bdi>{trackLabel(tr, t)}</bdi>
        </MenuRadio>
      ))}
      </div>
      <div className="menu-sep" />
      <div className="menu-row">
        <span>{t('player.timing')}</span>
        <div className="stepper">
          <IconButton icon="minus" label={t('player.earlier')} onClick={() => cmd({ type: 'subDelay', ms: st.subDelayMs - 100 })} disabled={st.subId == null} />
          <output dir="ltr" aria-live="polite">
            {t('player.seconds', { s: `${delay > 0 ? '+' : ''}${delay.toFixed(1)}` })}
          </output>
          <IconButton icon="plus" label={t('player.later')} onClick={() => cmd({ type: 'subDelay', ms: st.subDelayMs + 100 })} disabled={st.subId == null} />
        </div>
      </div>
      <label className="menu-row">
        <span>{t('player.size')}</span>
        <input
          type="range"
          min={50}
          max={200}
          step={10}
          value={Math.round(st.subScale * 100)}
          onChange={(e) => cmd({ type: 'subScale', value: Number(e.target.value) / 100 })}
          style={{ '--pos': `${((st.subScale * 100 - 50) / 150) * 100}%` } as CSSProperties}
        />
      </label>
      {st.rtlFix !== null && (
        <label className="menu-check" title={t('player.fixRtlHint')}>
          <input type="checkbox" checked={st.rtlFix} onChange={(e) => cmd({ type: 'rtlFix', on: e.target.checked })} />
          <span className="checkbox-box" aria-hidden="true">
            <Icon name="check" size={16} />
          </span>
          <span>{t('player.fixRtl')}</span>
        </label>
      )}
    </div>
  );
}
