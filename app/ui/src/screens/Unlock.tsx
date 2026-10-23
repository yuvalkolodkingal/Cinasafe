import { useState, type FormEvent } from 'react';
import { useTranslation } from 'react-i18next';

import { api, asAppError, type Language, type LockReason, type UnlockResult, type VaultSummary } from '../api';
import { Icon, Logo } from '../components/Icon';
import { Button, Notice } from '../components/ui';
import { LANGUAGES } from '../i18n';
import { formatBytes } from '../lib/format';

export function Unlock({
  vault,
  lockReason,
  onUnlocked,
  onLanguage,
}: {
  vault: VaultSummary | null;
  lockReason: LockReason | null;
  onUnlocked: (r: UnlockResult) => void;
  onLanguage: (l: Language) => void;
}) {
  const { t, i18n } = useTranslation();
  const [mode, setMode] = useState<'password' | 'recovery'>('password');
  const [password, setPassword] = useState('');
  const [words, setWords] = useState('');
  const [show, setShow] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const tooNew = vault != null && !vault.supported;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      const r = mode === 'password' ? await api.vaultUnlock(password) : await api.vaultUnlockRecovery(words);
      setPassword('');
      setWords('');
      onUnlocked(r);
    } catch (err) {
      const ae = asAppError(err);
      setError(t(`errors.${ae.code}`, { defaultValue: ae.message }));
      setBusy(false);
    }
  };

  const reasonText = lockReason
    ? t(
        {
          user: 'unlock.lockedUser',
          idle: 'unlock.lockedIdle',
          removed: 'unlock.lockedRemoved',
          sleep: 'unlock.lockedSleep',
        }[lockReason],
      )
    : null;

  return (
    <div className="unlock-screen">
      <div className="unlock-column">
        <Logo size="large" withName={false} />
        <h1 className="unlock-title">{mode === 'password' ? t('unlock.title') : t('unlock.recoveryTitle')}</h1>
        {vault && (
          <div className="pill drive-pill">
            <Icon name="drive" size={18} />
            <bdi>
              {t('unlock.drivePill', {
                label: vault.label ?? t('unlock.unnamedDrive'),
                size: formatBytes(vault.sizeBytes, t),
                used: formatBytes(vault.usedBytes, t),
              })}
            </bdi>
          </div>
        )}
        {reasonText && lockReason !== 'user' && <Notice kind="info">{reasonText}</Notice>}

        {tooNew ? (
          <div className="card-panel">
            <h2 className="section-title">{t('unlock.tooNewTitle')}</h2>
            <p className="muted">{t('unlock.tooNewBody')}</p>
          </div>
        ) : (
          <form className="card-panel unlock-card" onSubmit={submit} noValidate>
            {mode === 'password' ? (
              <div className="field">
                <label htmlFor="pw">{t('unlock.password')}</label>
                <div className="input-with-button">
                  <input
                    id="pw"
                    type={show ? 'text' : 'password'}
                    autoComplete="off"
                    autoCorrect="off"
                    autoCapitalize="off"
                    spellCheck={false}
                    autoFocus
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    aria-invalid={!!error}
                    aria-describedby={error ? 'unlock-error' : undefined}
                  />
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label={show ? t('unlock.hidePassword') : t('unlock.showPassword')}
                    aria-pressed={show}
                    onClick={() => setShow((s) => !s)}
                  >
                    <Icon name={show ? 'eyeOff' : 'eye'} size={20} />
                  </button>
                </div>
              </div>
            ) : (
              <div className="field">
                <label htmlFor="words">{t('unlock.recoveryLabel')}</label>
                <textarea
                  id="words"
                  rows={4}
                  autoFocus
                  spellCheck={false}
                  autoComplete="off"
                  autoCapitalize="off"
                  value={words}
                  onChange={(e) => setWords(e.target.value)}
                  aria-invalid={!!error}
                  dir="ltr"
                />
                <p className="field-hint">{t('unlock.recoveryHint')}</p>
              </div>
            )}
            {error && (
              <p className="field-error" id="unlock-error" role="alert">
                {error}
              </p>
            )}
            <Button variant="primary" type="submit" className="block" busy={busy} disabled={mode === 'password' ? !password : !words.trim()}>
              {busy ? t('unlock.unlocking') : t('unlock.unlock')}
            </Button>
            <p className="muted small">{t('unlock.hint')}</p>
          </form>
        )}

        <div className="unlock-links">
          {!tooNew && (vault?.hasRecovery ?? true) && (
            <button
              type="button"
              className="link-btn"
              onClick={() => {
                setMode(mode === 'password' ? 'recovery' : 'password');
                setError(null);
              }}
            >
              <Icon name="key" size={18} />
              {mode === 'password' ? t('unlock.useRecovery') : t('unlock.usePassword')}
            </button>
          )}
          <label className="inline-select">
            <span>{t('unlock.language')}</span>
            <select value={i18n.language} onChange={(e) => onLanguage(e.target.value as Language)}>
              {LANGUAGES.map((l) => (
                <option key={l.code} value={l.code} lang={l.code}>
                  {l.native}
                </option>
              ))}
            </select>
          </label>
        </div>
        <p className="unlock-footer">
          <Icon name="shield" size={16} />
          {/* One sentence per line: the wrap doesn't depend on exact text widths. */}
          <span>
            <span className="unlock-footer-line">{t('unlock.footerDisk')}</span>
            <span className="unlock-footer-line">{t('unlock.footerRemove')}</span>
          </span>
        </p>
      </div>
    </div>
  );
}
