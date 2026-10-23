import { createContext, useCallback, useContext, useEffect, useRef, useState, type DependencyList } from 'react';

import { asAppError, type AppError, type Settings, type VaultSummary } from '../api';

export interface Session {
  settings: Settings;
  updateSettings: (patch: Partial<Settings>) => Promise<void>;
  vault: VaultSummary | null;
  /** Locks the vault and returns to Unlock. */
  lock: () => Promise<void>;
  /** Shows a short non-blocking message. */
  toast: (message: string) => void;
}

export const SessionContext = createContext<Session | null>(null);

export function useSession(): Session {
  const s = useContext(SessionContext);
  if (!s) throw new Error('useSession outside the unlocked app');
  return s;
}

export interface AsyncState<T> {
  data: T | undefined;
  error: AppError | null;
  loading: boolean;
  reload: () => void;
  set: (v: T) => void;
}

/** Runs `fn` on mount and when `deps` change; ignores results from stale runs. */
export function useAsync<T>(fn: () => Promise<T>, deps: DependencyList): AsyncState<T> {
  const [data, setData] = useState<T>();
  const [error, setError] = useState<AppError | null>(null);
  const [loading, setLoading] = useState(true);
  const [tick, setTick] = useState(0);
  const run = useRef(0);
  useEffect(() => {
    const me = ++run.current;
    setLoading(true);
    fn().then(
      (v) => {
        if (me !== run.current) return;
        setData(v);
        setError(null);
        setLoading(false);
      },
      (e) => {
        if (me !== run.current) return;
        setError(asAppError(e));
        setLoading(false);
      },
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, tick]);
  const reload = useCallback(() => setTick((t) => t + 1), []);
  return { data, error, loading, reload, set: setData };
}

/** Calls `cb` at most once per `ms`. */
export function throttle(cb: () => void, ms: number): () => void {
  let last = 0;
  return () => {
    const now = Date.now();
    if (now - last >= ms) {
      last = now;
      cb();
    }
  };
}
