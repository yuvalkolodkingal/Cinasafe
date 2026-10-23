import { useEffect, useState, type FormEvent } from 'react';
import { NavLink, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { api, asAppError, on, type IntegrityProgress, type IntegrityReport, type Language, type Settings } from '../api';
import { Icon, type IconName } from '../components/Icon';
import { RecoveryWords } from '../components/RecoveryWords';
import { Button, Dialog, Field, IconButton, Notice, ProgressBar, Spinner, Toggle } from '../components/ui';
import { LANGUAGES } from '../i18n';
import { formatBytes, KNOWN_LANGUAGES, languageName } from '../lib/format';
import { useAsync, useSession } from '../lib/session';

const PAGES: { id: string; icon: IconName }[] = [
  { id: 'playback', icon: 'play' },
  { id: 'subtitles', icon: 'subtitles' },
  { id: 'interface', icon: 'settings' },
  { id: 'security', icon: 'shield' },
  { id: 'vault', icon: 'drive' },
  { id: 'about', icon: 'info' },
];

export function SettingsScreen() {
  const { t } = useTranslation();
  const { page = 'playback' } = useParams();
  return (
    <div className="settings">
      <h1 className="page-title">{t('settings.title')}</h1>
      <div className="settings-layout">
        <nav className="settings-nav" aria-label={t('settings.title')}>
          {PAGES.map((p) => (
            <NavLink key={p.id} to={`/settings/${p.id}`} className={({ isActive }) => `settings-link ${isActive ? 'active' : ''}`}>
              <Icon name={p.icon} size={20} />
              <span>{t(`settings.${p.id}`)}</span>
            </NavLink>
          ))}
        </nav>
        <div className="settings-content">
          {page === 'playback' && <Playback />}
          {page === 'subtitles' && <Subtitles />}
          {page === 'interface' && <Interface />}
          {page === 'security' && <Security />}
          {page === 'vault' && <Vault />}
          {page === 'about' && <About />}
        </div>
      </div>
    </div>
  );
}

function Card({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section className="card-panel settings-card">
      <h2 className="section-title">{title}</h2>
      {children}
    </section>
  );
}

function LanguageOrder({ value, onChange, label }: { value: string[]; onChange: (v: string[]) => void; label: string }) {
  const { t } = useTranslation();
  const [adding, setAdding] = useState('');
  const move = (i: number, d: -1 | 1) => {
    const next = [...value];
    const j = i + d;
    if (j < 0 || j >= next.length) return;
    [next[i], next[j]] = [next[j], next[i]];
    onChange(next);
  };
  return (
    <div className="field">
      <span className="sr-only">{label}</span>
      <ol className="lang-order" aria-label={label}>
        {value.map((l, i) => (
          <li key={l}>
            <span className="lang-n">{i + 1}</span>
            <span className="lang-name">{languageName(l)}</span>
            <IconButton icon="arrowUp" label={t('settings.moveUp')} disabled={i === 0} onClick={() => move(i, -1)} />
            <IconButton icon="arrowDown" label={t('settings.moveDown')} disabled={i === value.length - 1} onClick={() => move(i, 1)} />
            <IconButton icon="x" label={t('common.remove')} onClick={() => onChange(value.filter((x) => x !== l))} />
          </li>
        ))}
      </ol>
      <div className="inline-form">
        <select aria-label={t('settings.addLanguage')} value={adding} onChange={(e) => setAdding(e.target.value)}>
          <option value="">{t('settings.addLanguage')}</option>
          {KNOWN_LANGUAGES.filter((l) => !value.includes(l)).map((l) => (
            <option key={l} value={l}>
              {languageName(l)}
            </option>
          ))}
        </select>
        <Button
          icon="plus"
          disabled={!adding}
          onClick={() => {
            onChange([...value, adding]);
            setAdding('');
          }}
        >
          {t('settings.addLanguage')}
        </Button>
      </div>
      <p className="field-hint">{t('settings.languageOrderHint')}</p>
    </div>
  );
}

function Playback() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSession();
  return (
    <>
      <Card title={t('settings.playback')}>
        <Toggle label={t('settings.hwdec')} hint={t('settings.hwdecHint')} checked={settings.hwdec} onChange={(v) => updateSettings({ hwdec: v })} />
        <Toggle label={t('settings.autoplayNext')} checked={settings.autoplayNext} onChange={(v) => updateSettings({ autoplayNext: v })} />
      </Card>
      <Card title={t('settings.audioLanguages')}>
        <LanguageOrder label={t('settings.audioLanguages')} value={settings.audioLanguages} onChange={(v) => updateSettings({ audioLanguages: v })} />
      </Card>
    </>
  );
}

function Subtitles() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSession();
  const colors = ['#ffffff', '#f2e35b', '#9be3ff', '#c8f7a6'];
  return (
    <>
      <Card title={t('settings.subtitleLanguages')}>
        <LanguageOrder label={t('settings.subtitleLanguages')} value={settings.subtitleLanguages} onChange={(v) => updateSettings({ subtitleLanguages: v })} />
      </Card>
      <Card title={t('settings.subtitles')}>
        <div className="sub-preview" aria-hidden="true">
          <span
            className={settings.subtitleBackground ? 'boxed' : ''}
            style={{ color: settings.subtitleColor, fontSize: `${20 * settings.subtitleScale}px`, marginBlockEnd: `${(100 - settings.subtitlePos) * 0.6}px` }}
          >
            {t('settings.subtitlePreview')}
          </span>
        </div>
        <label className="field">
          <span className="label">{t('settings.subtitleSize')}</span>
          <input type="range" min={50} max={200} step={10} value={Math.round(settings.subtitleScale * 100)} onChange={(e) => updateSettings({ subtitleScale: Number(e.target.value) / 100 })} />
        </label>
        <label className="field">
          <span className="label">{t('settings.subtitlePosition')}</span>
          <input type="range" min={50} max={100} step={1} value={settings.subtitlePos} onChange={(e) => updateSettings({ subtitlePos: Number(e.target.value) })} />
        </label>
        <div className="field">
          <span className="label">{t('settings.subtitleColor')}</span>
          <div className="swatches" role="radiogroup" aria-label={t('settings.subtitleColor')}>
            {colors.map((c) => (
              <button
                key={c}
                type="button"
                role="radio"
                aria-checked={settings.subtitleColor === c}
                aria-label={c}
                className={`swatch ${settings.subtitleColor === c ? 'on' : ''}`}
                style={{ background: c }}
                onClick={() => updateSettings({ subtitleColor: c })}
              />
            ))}
          </div>
        </div>
        <Toggle label={t('settings.subtitleBackground')} checked={settings.subtitleBackground} onChange={(v) => updateSettings({ subtitleBackground: v })} />
      </Card>
    </>
  );
}

function Interface() {
  const { t } = useTranslation();
  const { settings, updateSettings } = useSession();
  return (
    <Card title={t('settings.interface')}>
      <label className="field">
        <span className="label">{t('settings.language')}</span>
        <select value={settings.language ?? ''} onChange={(e) => updateSettings({ language: (e.target.value || null) as Language | null })}>
          <option value="">{t('settings.systemLanguage')}</option>
          {LANGUAGES.map((l) => (
            <option key={l.code} value={l.code} lang={l.code}>
              {l.native}
            </option>
          ))}
        </select>
      </label>
      <label className="field">
        <span className="label">{t('settings.theme')}</span>
        <select value={settings.theme} onChange={(e) => updateSettings({ theme: e.target.value as Settings['theme'] })}>
          <option value="dark">{t('settings.themeDark')}</option>
          <option value="light">{t('settings.themeLight')}</option>
        </select>
      </label>
    </Card>
  );
}

function Security() {
  const { t, i18n } = useTranslation();
  const { settings, updateSettings } = useSession();
  const info = useAsync(() => api.vaultInfo(), []);
  const [changing, setChanging] = useState(false);
  const [replacing, setReplacing] = useState(false);
  const [key, setKey] = useState(settings.tmdbKey ?? '');
  const created = info.data ? new Date(info.data.createdAt).toLocaleDateString(i18n.language) : '';
  return (
    <>
      <Card title={t('settings.password')}>
        <p className="muted">{t('settings.passwordBody')}</p>
        <Button icon="key" onClick={() => setChanging(true)}>
          {t('settings.changePassword')}
        </Button>
      </Card>
      <Card title={t('settings.recoveryKey')}>
        <p className="muted">{info.data?.hasRecovery ? t('settings.recoveryCreated', { date: created }) : t('settings.recoveryNone')}</p>
        <Button icon="refresh" onClick={() => setReplacing(true)}>
          {info.data?.hasRecovery === false ? t('settings.createRecovery') : t('settings.replaceRecovery')}
        </Button>
      </Card>
      <Card title={t('settings.locking')}>
        <label className="field">
          <span className="label">{t('settings.idleLock')}</span>
          <select value={settings.idleLockMinutes} onChange={(e) => updateSettings({ idleLockMinutes: Number(e.target.value) })}>
            <option value={5}>{t('settings.idle5')}</option>
            <option value={15}>{t('settings.idle15')}</option>
            <option value={60}>{t('settings.idle60')}</option>
            <option value={0}>{t('settings.idleNever')}</option>
          </select>
        </label>
        <Toggle label={t('settings.lockOnRemoval')} hint={t('settings.lockOnRemovalHint')} checked disabled onChange={() => {}} />
        <Toggle label={t('settings.lockOnSleep')} checked={settings.lockOnSleep} onChange={(v) => updateSettings({ lockOnSleep: v })} />
      </Card>
      <Card title={t('settings.privacy')}>
        <Toggle label={t('settings.lookupOnline')} checked={settings.lookupOnline} onChange={(v) => updateSettings({ lookupOnline: v })} />
        <p className="muted">{t('settings.lookupOnlineBody')}</p>
        <form
          className="inline-form"
          onSubmit={(e) => {
            e.preventDefault();
            void updateSettings({ tmdbKey: key.trim() || null });
          }}
        >
          <Field label={t('settings.tmdbKey')} hint={t('settings.tmdbKeyHint')}>
            {(id) => <input id={id} type="password" autoComplete="off" spellCheck={false} dir="ltr" value={key} onChange={(e) => setKey(e.target.value)} />}
          </Field>
          <Button type="submit" disabled={(settings.tmdbKey ?? '') === key.trim()}>
            {t('common.save')}
          </Button>
        </form>
        <label className="field">
          <span className="label">{t('settings.metadataLanguage')}</span>
          <select value={settings.metadataLanguage} onChange={(e) => updateSettings({ metadataLanguage: e.target.value })}>
            <option value="en-US">English</option>
            <option value="he-IL">עברית</option>
            <option value="ar-SA">العربية</option>
          </select>
        </label>
      </Card>
      {changing && <ChangePasswordDialog onClose={() => setChanging(false)} />}
      {replacing && (
        <ReplaceRecoveryDialog
          onClose={() => {
            setReplacing(false);
            info.reload();
          }}
        />
      )}
    </>
  );
}

function ChangePasswordDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const { toast } = useSession();
  const [old, setOld] = useState('');
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const valid = old && pw.length >= 8 && pw === pw2;
  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!valid) return;
    setBusy(true);
    setError(null);
    try {
      await api.vaultChangePassword(old, pw);
      toast(t('settings.passwordChanged'));
      onClose();
    } catch (err) {
      setError(t(`errors.${asAppError(err).code}`));
      setBusy(false);
    }
  };
  return (
    <Dialog title={t('settings.changePassword')} onClose={onClose}>
      <form onSubmit={submit} className="stack">
        <Field label={t('settings.currentPassword')}>{(id) => <input spellCheck={false} autoComplete="off" id={id} type="password" dir="ltr" value={old} onChange={(e) => setOld(e.target.value)} />}</Field>
        <Field label={t('settings.newPassword')} hint={t('wizard.minLength')}>
          {(id) => <input spellCheck={false} autoComplete="off" id={id} type="password" dir="ltr" value={pw} onChange={(e) => setPw(e.target.value)} />}
        </Field>
        <Field label={t('settings.confirmPassword')} error={pw2 && pw !== pw2 ? t('wizard.noMatch') : null}>
          {(id) => <input spellCheck={false} autoComplete="off" id={id} type="password" dir="ltr" value={pw2} onChange={(e) => setPw2(e.target.value)} />}
        </Field>
        {error && (
          <p className="field-error" role="alert">
            {error}
          </p>
        )}
        <div className="dialog-actions inline">
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button type="submit" variant="primary" busy={busy} disabled={!valid}>
            {t('settings.changePassword')}
          </Button>
        </div>
      </form>
    </Dialog>
  );
}

function ReplaceRecoveryDialog({ onClose }: { onClose: () => void }) {
  const { t } = useTranslation();
  const [pw, setPw] = useState('');
  const [words, setWords] = useState<string[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Dialog title={words ? t('settings.newRecoveryTitle') : t('settings.replaceRecovery')} onClose={onClose} wide={!!words}>
      {words ? (
        <>
          <p className="muted">{t('wizard.recoveryBody')}</p>
          <RecoveryWords words={words} />
          <div className="dialog-actions inline">
            <Button variant="primary" onClick={onClose}>
              {t('common.done')}
            </Button>
          </div>
        </>
      ) : (
        <form
          className="stack"
          onSubmit={async (e) => {
            e.preventDefault();
            setBusy(true);
            setError(null);
            try {
              setWords(await api.vaultReplaceRecovery(pw));
              setPw('');
            } catch (err) {
              setError(t(`errors.${asAppError(err).code}`));
            }
            setBusy(false);
          }}
        >
          <p className="muted">{t('settings.replaceRecoveryBody')}</p>
          <Field label={t('unlock.password')}>{(id) => <input spellCheck={false} autoComplete="off" id={id} type="password" dir="ltr" value={pw} onChange={(e) => setPw(e.target.value)} />}</Field>
          {error && (
            <p className="field-error" role="alert">
              {error}
            </p>
          )}
          <div className="dialog-actions inline">
            <Button onClick={onClose}>{t('common.cancel')}</Button>
            <Button type="submit" variant="primary" busy={busy} disabled={!pw}>
              {t('common.next')}
            </Button>
          </div>
        </form>
      )}
    </Dialog>
  );
}

function Vault() {
  const { t } = useTranslation();
  const { toast } = useSession();
  const info = useAsync(() => api.vaultInfo(), []);
  const [progress, setProgress] = useState<IntegrityProgress | null>(null);
  const [report, setReport] = useState<IntegrityReport | null>(null);
  const [checking, setChecking] = useState(false);
  const [updating, setUpdating] = useState(false);
  useEffect(() => on('integrity-progress', setProgress), []);
  const v = info.data;
  return (
    <>
      <Card title={t('settings.spaceUsed')}>
        {!v ? (
          <Spinner />
        ) : (
          <>
            <p>
              {t('settings.spaceSummary', {
                used: formatBytes(v.usedBytes, t),
                size: formatBytes(v.sizeBytes, t),
                free: formatBytes(v.freeBytes ?? v.sizeBytes - v.usedBytes, t),
              })}
            </p>
            <ProgressBar fraction={v.usedBytes / v.sizeBytes} />
            <p className="muted small">
              {t('settings.libraryCounts', {
                movies: `${t('grid.movieCount', { count: v.movieCount })} (${formatBytes(v.movieBytes, t)})`,
                shows: `${t('grid.showCount', { count: v.showCount })} (${formatBytes(v.showBytes, t)})`,
              })}
            </p>
          </>
        )}
      </Card>
      <Card title={t('settings.integrity')}>
        <p className="muted">{t('settings.integrityBody')}</p>
        {checking && progress && (
          <>
            <ProgressBar fraction={progress.total ? progress.done / progress.total : 0} />
            <p className="muted small">{t('settings.integrityRunning', { done: progress.done, total: progress.total })}</p>
          </>
        )}
        {report &&
          (report.corruptCount + report.missingCount + report.orphanedCount === 0 ? (
            <Notice kind="info">{t('settings.integrityOk', { n: report.checked })}</Notice>
          ) : (
            <Notice kind="error">
              {t('settings.integrityProblems', { corrupt: report.corruptCount, missing: report.missingCount, orphaned: report.orphanedCount })}
            </Notice>
          ))}
        <Button
          icon="shield"
          busy={checking}
          onClick={async () => {
            setChecking(true);
            setReport(null);
            setProgress(null);
            try {
              setReport(await api.vaultIntegrityCheck());
            } catch (e) {
              toast(t(`errors.${asAppError(e).code}`));
            }
            setChecking(false);
          }}
        >
          {t('settings.integrityRun')}
        </Button>
      </Card>
      <Card title={t('settings.playerApps')}>
        <p className="muted">{t('settings.playerAppsBody')}</p>
        <Button
          icon="refresh"
          busy={updating}
          onClick={async () => {
            setUpdating(true);
            try {
              await api.updatePlayerApps();
              toast(t('settings.playerAppsUpdated'));
            } catch (e) {
              toast(t(`errors.${asAppError(e).code}`));
            }
            setUpdating(false);
          }}
        >
          {t('settings.updatePlayerApps')}
        </Button>
      </Card>
    </>
  );
}

function About() {
  const { t } = useTranslation();
  const info = useAsync(() => api.appInfo(), []);
  const a = info.data;
  return (
    <>
      <Card title={t('settings.about')}>
        {a && (
          <dl className="kv">
            <div className="kv-row">
              <dt>Cinasafe</dt>
              <dd dir="ltr">{t('settings.version', { v: a.version })}</dd>
            </div>
            <div className="kv-row">
              <dt>{t('settings.platform')}</dt>
              <dd dir="ltr">{a.platform}</dd>
            </div>
            <div className="kv-row">
              <dt>{t('settings.mpv')}</dt>
              <dd dir="ltr">{a.mpvVersion ?? t('common.unknown')}</dd>
            </div>
            <div className="kv-row">
              <dt>{t('settings.playerApps')}</dt>
              <dd dir="ltr">{a.playerApps.map((p) => `${p.platform}${p.version ? ` ${p.version}` : ''}`).join(', ') || t('common.none')}</dd>
            </div>
          </dl>
        )}
      </Card>
      <Card title={t('settings.licenses')}>
        <p className="muted">{t('settings.licensesBody')}</p>
        <p className="muted small">{t('settings.tmdbAttribution')}</p>
      </Card>
    </>
  );
}
