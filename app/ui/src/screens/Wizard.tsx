// Create-vault wizard (brief 9.3 #10). The vault is created when the
// password step completes, because the recovery words come from creation;
// skipping the recovery key then removes it again.

import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, asAppError, on, type CreateMode, type CreateStep, type Drive } from '../api';
import { Icon, Logo } from '../components/Icon';
import { RecoveryWords, StrengthMeter } from '../components/RecoveryWords';
import { Button, Checkbox, Dialog, Field, Notice, Spinner } from '../components/ui';
import { LANGUAGES, applyLanguage } from '../i18n';
import { formatBytes, isolate, passwordStrength } from '../lib/format';

type Step = 'drive' | 'prepare' | 'password' | 'recovery' | 'finish';
const STEPS: Step[] = ['drive', 'prepare', 'password', 'recovery', 'finish'];

export function Wizard({ onDone }: { onDone: (addNow: boolean) => void }) {
  const { t, i18n } = useTranslation();
  const [step, setStep] = useState<Step>('drive');
  const [drives, setDrives] = useState<Drive[] | null>(null);
  const [driveId, setDriveId] = useState<string | null>(null);
  const [mode, setMode] = useState<CreateMode>('useAsIs');
  const [confirmName, setConfirmName] = useState('');
  const [label, setLabel] = useState('CINASAFE');
  const [pw, setPw] = useState('');
  const [pw2, setPw2] = useState('');
  const [creating, setCreating] = useState<CreateStep | null>(null);
  const [words, setWords] = useState<string[] | null>(null);
  const [saved, setSaved] = useState(false);
  const [skipAsk, setSkipAsk] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ejected, setEjected] = useState(false);

  const loadDrives = () => {
    setDrives(null);
    api.drivesList().then(
      (d) => {
        setDrives(d);
        if (driveId && !d.some((x) => x.id === driveId)) setDriveId(null);
      },
      (e) => {
        setDrives([]);
        setError(t(`errors.${asAppError(e).code}`));
      },
    );
  };
  useEffect(loadDrives, []); // eslint-disable-line react-hooks/exhaustive-deps
  useEffect(() => on('create-progress', (p) => setCreating(p.step)), []);

  const drive = drives?.find((d) => d.id === driveId) ?? null;
  const mustFormat = drive?.status === 'needsFormatting';
  const effectiveMode: CreateMode = mustFormat ? 'format' : mode;
  const driveName = drive?.label ?? drive?.name ?? '';
  const strength = passwordStrength(pw);
  const idx = STEPS.indexOf(step);

  const create = async () => {
    if (!drive) return;
    setError(null);
    setCreating(effectiveMode === 'format' ? 'formatting' : 'deriving');
    try {
      const r = await api.vaultCreate({ driveId: drive.id, mode: effectiveMode, label: effectiveMode === 'format' ? label : null, password: pw, withRecovery: true });
      setPw('');
      setPw2('');
      setWords(r.recoveryWords);
      setStep(r.recoveryWords ? 'recovery' : 'finish');
    } catch (e) {
      setError(t(`errors.${asAppError(e).code}`, { defaultValue: asAppError(e).message }));
    }
    setCreating(null);
  };

  return (
    <div className="wizard">
      <header className="wizard-head">
        <Logo />
        <label className="inline-select">
          <span className="sr-only">{t('unlock.language')}</span>
          <select value={i18n.language} onChange={(e) => applyLanguage(e.target.value as 'en' | 'he' | 'ar')}>
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code} lang={l.code}>
                {l.native}
              </option>
            ))}
          </select>
        </label>
      </header>
      <div className="wizard-body">
        <h1 className="page-title">{t('wizard.title')}</h1>
        <ol className="steps" aria-label={t('wizard.title')}>
          {STEPS.map((s, i) => (
            <li key={s} className={`step ${i < idx ? 'done' : ''} ${i === idx ? 'current' : ''}`} aria-current={i === idx ? 'step' : undefined}>
              <span className="step-dot" aria-hidden="true">
                {i < idx ? <Icon name="check" size={14} /> : i + 1}
              </span>
              <span>{t(`wizard.steps.${s}`)}</span>
              {i < idx && <span className="sr-only">({t('wizard.stepDone')})</span>}
            </li>
          ))}
        </ol>

        <section className="card-panel wizard-card">
          {step === 'drive' && (
            <>
              <div className="wizard-card-head">
                <div>
                  <h2 className="section-title">{t('wizard.driveTitle')}</h2>
                  <p className="muted">{t('wizard.driveBody')}</p>
                </div>
                <Button variant="ghost" icon="refresh" onClick={loadDrives}>
                  {t('common.refresh')}
                </Button>
              </div>
              {!drives && <Spinner />}
              {drives && drives.length === 0 && <Notice kind="info">{t('wizard.noDrives')}</Notice>}
              <div className="radio-cards" role="radiogroup" aria-label={t('wizard.driveTitle')}>
                {drives?.map((d) => (
                  <label key={d.id} className={`radio-card ${driveId === d.id ? 'selected' : ''}`}>
                    <input type="radio" name="drive" checked={driveId === d.id} onChange={() => setDriveId(d.id)} />
                    <Icon name="drive" size={28} />
                    <div className="radio-card-text">
                      <span className="radio-card-title">
                        <bdi>{d.name}</bdi> · {formatBytes(d.sizeBytes, t)}
                      </span>
                      <span className="muted small">
                        <bdi>
                          {t('wizard.driveMeta', {
                            fs: d.filesystem?.toUpperCase() ?? '—',
                            free: d.freeBytes != null ? formatBytes(d.freeBytes, t) : '—',
                            mount: isolate(d.mountPath ?? d.id),
                          })}
                        </bdi>
                      </span>
                    </div>
                    <span className={`status-pill ${d.status === 'ready' ? 'good' : 'warn'}`}>
                      {d.hasVault ? t('wizard.hasVault') : t(`wizard.status${d.status[0].toUpperCase()}${d.status.slice(1)}`)}
                    </span>
                  </label>
                ))}
              </div>
              <div className="wizard-actions">
                <span />
                <Button variant="primary" disabled={!drive || drive.hasVault || drive.status === 'notMounted'} onClick={() => setStep('prepare')}>
                  {t('common.next')}
                </Button>
              </div>
            </>
          )}

          {step === 'prepare' && drive && (
            <>
              <h2 className="section-title">{t('wizard.prepareTitle')}</h2>
              {mustFormat && <Notice kind="warning">{t('wizard.formatOnly', { fs: drive.filesystem?.toUpperCase() ?? '—' })}</Notice>}
              <div className="radio-cards" role="radiogroup" aria-label={t('wizard.prepareTitle')}>
                {!mustFormat && (
                  <label className={`radio-card ${mode === 'useAsIs' ? 'selected' : ''}`}>
                    <input type="radio" name="mode" checked={mode === 'useAsIs'} onChange={() => setMode('useAsIs')} />
                    <Icon name="folder" size={26} />
                    <div className="radio-card-text">
                      <span className="radio-card-title">
                        {t('wizard.useAsIs')} · <span className="accent-text">{t('wizard.recommended')}</span>
                      </span>
                      <span className="muted small">{t('wizard.useAsIsBody')}</span>
                    </div>
                  </label>
                )}
                <label className={`radio-card danger ${effectiveMode === 'format' ? 'selected' : ''}`}>
                  <input type="radio" name="mode" checked={effectiveMode === 'format'} onChange={() => setMode('format')} />
                  <Icon name="trash" size={26} />
                  <div className="radio-card-text">
                    <span className="radio-card-title danger-text">{t('wizard.format')}</span>
                    <span className="muted small">{t('wizard.formatBody')}</span>
                  </div>
                </label>
              </div>
              {effectiveMode === 'format' && (
                <div className="form-grid">
                  <Field label={t('wizard.typeToConfirm', { name: driveName })}>
                    {(id) => <input id={id} value={confirmName} dir="auto" autoComplete="off" spellCheck={false} onChange={(e) => setConfirmName(e.target.value)} />}
                  </Field>
                  <Field label={t('wizard.volumeLabel')} error={/^[A-Za-z0-9_-]{1,11}$/.test(label) ? null : t('errors.badLabel')}>
                    {(id) => <input id={id} value={label} dir="ltr" maxLength={11} onChange={(e) => setLabel(e.target.value.toUpperCase())} />}
                  </Field>
                </div>
              )}
              <div className="wizard-actions">
                <Button onClick={() => setStep('drive')}>{t('common.back')}</Button>
                {effectiveMode === 'format' ? (
                  <Button
                    variant="danger"
                    disabled={confirmName.trim() !== driveName || !/^[A-Za-z0-9_-]{1,11}$/.test(label)}
                    onClick={() => setStep('password')}
                  >
                    {t('wizard.eraseAndContinue')}
                  </Button>
                ) : (
                  <Button variant="primary" onClick={() => setStep('password')}>
                    {t('common.next')}
                  </Button>
                )}
              </div>
            </>
          )}

          {step === 'password' && (
            <>
              <h2 className="section-title">{t('wizard.passwordTitle')}</h2>
              <p className="muted">{t('wizard.passwordBody')}</p>
              {creating ? (
                <div className="center-block" role="status">
                  <span className="spinner large" aria-hidden="true" />
                  <span>{t(`wizard.progress.${creating}`)}</span>
                </div>
              ) : (
                <form
                  className="stack"
                  onSubmit={(e) => {
                    e.preventDefault();
                    if (pw.length >= 8 && pw === pw2) void create();
                  }}
                >
                  <Field label={t('wizard.password')} hint={pw.length > 0 && pw.length < 8 ? t('wizard.minLength') : undefined}>
                    {(id) => <input id={id} type="password" dir="ltr" autoComplete="new-password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} />}
                  </Field>
                  <StrengthMeter score={strength} />
                  <Field label={t('wizard.confirm')}>
                    {(id) => <input id={id} type="password" dir="ltr" autoComplete="new-password" value={pw2} onChange={(e) => setPw2(e.target.value)} />}
                  </Field>
                  {pw2 && (
                    <p className={`match ${pw === pw2 ? 'tone-good' : 'tone-bad'}`}>
                      <Icon name={pw === pw2 ? 'check' : 'x'} size={16} /> {pw === pw2 ? t('wizard.match') : t('wizard.noMatch')}
                    </p>
                  )}
                  <div className="warning-note">
                    <Icon name="alert" size={20} />
                    <span>{t('wizard.noForgot')}</span>
                  </div>
                  {error && (
                    <p className="field-error" role="alert">
                      {error}
                    </p>
                  )}
                  <div className="wizard-actions">
                    <Button onClick={() => setStep('prepare')}>{t('common.back')}</Button>
                    <Button type="submit" variant="primary" disabled={pw.length < 8 || pw !== pw2}>
                      {t('common.next')}
                    </Button>
                  </div>
                </form>
              )}
            </>
          )}

          {step === 'recovery' && words && (
            <>
              <h2 className="section-title">{t('wizard.recoveryTitle')}</h2>
              <p className="muted">{t('wizard.recoveryBody')}</p>
              <RecoveryWords words={words} />
              <Checkbox checked={saved} onChange={setSaved} label={t('wizard.savedIt')} />
              <div className="wizard-actions">
                <Button variant="ghost" onClick={() => setSkipAsk(true)}>
                  {t('wizard.skipRecovery')}
                </Button>
                <Button
                  variant="primary"
                  disabled={!saved}
                  onClick={() => {
                    setWords(null);
                    setStep('finish');
                  }}
                >
                  {t('common.next')}
                </Button>
              </div>
              {skipAsk && (
                <Dialog
                  title={t('wizard.skipTitle')}
                  onClose={() => setSkipAsk(false)}
                  actions={
                    <>
                      <Button onClick={() => setSkipAsk(false)}>{t('common.cancel')}</Button>
                      <Button
                        variant="danger"
                        onClick={async () => {
                          try {
                            await api.vaultRemoveRecovery();
                          } catch (e) {
                            setError(t(`errors.${asAppError(e).code}`));
                          }
                          setSkipAsk(false);
                          setWords(null);
                          setStep('finish');
                        }}
                      >
                        {t('wizard.skipConfirm')}
                      </Button>
                    </>
                  }
                >
                  <p>{t('wizard.skipBody')}</p>
                </Dialog>
              )}
            </>
          )}

          {step === 'finish' && (
            <div className="finish">
              <div className="finish-icon">
                <Icon name="check" size={36} />
              </div>
              <h2 className="section-title">{t('wizard.finishTitle')}</h2>
              <p className="muted">{ejected ? t('wizard.ejected') : t('wizard.finishBody')}</p>
              {error && <Notice kind="error">{error}</Notice>}
              {!ejected && (
                <div className="button-row center">
                  <Button variant="primary" icon="plus" onClick={() => onDone(true)}>
                    {t('wizard.addNow')}
                  </Button>
                  <Button
                    icon="eject"
                    onClick={async () => {
                      try {
                        await api.ejectDrive();
                        setEjected(true);
                      } catch (e) {
                        setError(t(`errors.${asAppError(e).code}`));
                      }
                    }}
                  >
                    {t('wizard.eject')}
                  </Button>
                </div>
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}
