import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { HashRouter, Navigate, Route, Routes, useLocation, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { api, on, type LockReason, type Settings, type VaultSummary } from './api';
import { TopBar } from './components/TopBar';
import { Spinner } from './components/ui';
import { applyLanguage, systemLanguage } from './i18n';
import { SessionContext, throttle, type Session } from './lib/session';
import { AddMedia } from './screens/AddMedia';
import { Grid } from './screens/Grid';
import { Home } from './screens/Home';
import { MovieDetail } from './screens/MovieDetail';
import { Player } from './screens/Player';
import { Search } from './screens/Search';
import { SettingsScreen } from './screens/Settings';
import { ShowDetail } from './screens/ShowDetail';
import { Unlock } from './screens/Unlock';
import { Wizard } from './screens/Wizard';

type Phase = 'loading' | 'wizard' | 'locked' | 'unlocked';

function applyTheme(theme: Settings['theme']) {
  document.documentElement.dataset.theme = theme;
}

export function App() {
  return (
    <HashRouter>
      <Root />
    </HashRouter>
  );
}

function Root() {
  const [phase, setPhase] = useState<Phase>('loading');
  const [vault, setVault] = useState<VaultSummary | null>(null);
  const [lockReason, setLockReason] = useState<LockReason | null>(null);
  const [restored, setRestored] = useState(false);
  /** Bumped on every unlock so all unlocked-state components start fresh. */
  const [sessionId, setSessionId] = useState(0);
  const navigate = useNavigate();
  const uiLanguage = useRef(systemLanguage());

  useEffect(() => {
    applyLanguage(uiLanguage.current);
    (async () => {
      const [status, detected] = await Promise.all([api.vaultStatus(), api.vaultDetect()]);
      setVault(detected);
      if (status.unlocked) setPhase('unlocked');
      else setPhase(detected ? 'locked' : 'wizard');
    })().catch(() => setPhase('wizard'));
  }, []);

  const goLocked = useCallback(
    (reason: LockReason) => {
      setLockReason(reason);
      setRestored(false);
      setPhase((p) => (p === 'wizard' ? p : 'locked'));
      navigate('/', { replace: true });
      api.vaultDetect().then(setVault, () => setVault(null));
    },
    [navigate],
  );

  useEffect(() => {
    const offs = [on('vault-locked', (e) => goLocked(e.reason)), on('drive-removed', () => goLocked('removed'))];
    return () => offs.forEach((o) => o());
  }, [goLocked]);

  // Idle lock: tell the core about user activity, at most every 30 s.
  useEffect(() => {
    if (phase !== 'unlocked') return;
    const ping = throttle(() => void api.activity().catch(() => {}), 30000);
    const events = ['keydown', 'pointerdown', 'pointermove', 'wheel'] as const;
    events.forEach((e) => window.addEventListener(e, ping, { passive: true }));
    return () => events.forEach((e) => window.removeEventListener(e, ping));
  }, [phase]);

  const onUnlocked = (r: { restoredFromBackup: boolean }) => {
    setRestored(r.restoredFromBackup);
    setLockReason(null);
    setSessionId((s) => s + 1);
    setPhase('unlocked');
    navigate('/', { replace: true });
  };

  if (phase === 'loading') return <Spinner />;
  if (phase === 'wizard')
    return (
      <Wizard
        onDone={(addNow) => {
          setSessionId((s) => s + 1);
          setPhase('unlocked');
          navigate(addNow ? '/add' : '/', { replace: true });
        }}
      />
    );
  if (phase === 'locked')
    return (
      <Unlock
        vault={vault}
        lockReason={lockReason}
        onUnlocked={onUnlocked}
        onLanguage={(l) => {
          uiLanguage.current = l;
          applyLanguage(l);
        }}
      />
    );
  return <Unlocked key={sessionId} vault={vault} restored={restored} uiLanguage={uiLanguage.current} onLock={goLocked} />;
}

function Unlocked({ vault, restored, uiLanguage, onLock }: { vault: VaultSummary | null; restored: boolean; uiLanguage: ReturnType<typeof systemLanguage>; onLock: (r: LockReason) => void }) {
  const { t } = useTranslation();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [toasts, setToasts] = useState<{ id: number; text: string }[]>([]);
  const location = useLocation();
  const inPlayer = location.pathname.startsWith('/play/');

  const toast = useCallback((text: string) => {
    const id = Date.now() + Math.random();
    setToasts((ts) => [...ts, { id, text }]);
    setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), 4000);
  }, []);

  useEffect(() => {
    api.settingsGet().then((s) => {
      setSettings(s);
      applyLanguage(s.language ?? uiLanguage);
      applyTheme(s.theme);
    });
  }, [uiLanguage]);

  useEffect(() => {
    if (restored) toast(t('unlock.restoredFromBackup'));
  }, [restored, toast, t]);

  const session = useMemo<Session | null>(
    () =>
      settings && {
        settings,
        vault,
        toast,
        updateSettings: async (patch) => {
          const s = await api.settingsSet(patch);
          setSettings(s);
          if ('language' in patch) applyLanguage(s.language ?? uiLanguage);
          if ('theme' in patch) applyTheme(s.theme);
        },
        lock: async () => {
          await api.vaultLock().catch(() => {});
          onLock('user');
        },
      },
    [settings, vault, toast, uiLanguage, onLock],
  );

  if (!session) return <Spinner />;
  return (
    <SessionContext.Provider value={session}>
      {!inPlayer && <TopBar />}
      <main className={inPlayer ? 'player-main' : 'page'} id="main">
        <Routes>
          <Route path="/" element={<Home />} />
          <Route path="/movies" element={<Grid kind="movies" />} />
          <Route path="/shows" element={<Grid kind="shows" />} />
          <Route path="/movie/:id" element={<MovieDetail />} />
          <Route path="/show/:id" element={<ShowDetail />} />
          <Route path="/search" element={<Search />} />
          <Route path="/add" element={<AddMedia />} />
          <Route path="/settings" element={<Navigate to="/settings/playback" replace />} />
          <Route path="/settings/:page" element={<SettingsScreen />} />
          <Route path="/play/:mediaFileId" element={<Player />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </main>
      <div className="toasts" aria-live="polite">
        {toasts.map((x) => (
          <div key={x.id} className="toast">
            {x.text}
          </div>
        ))}
      </div>
    </SessionContext.Provider>
  );
}
