import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, asAppError, type QueueItem, type Scored } from '../api';
import { Button, Dialog, Field, Notice } from './ui';

/** Pick one of the candidates, search again, or type the details (brief 9.3 #8). */
export function ReviewMatchDialog({ item, onClose, onResolved }: { item: QueueItem; onClose: () => void; onResolved: () => void }) {
  const { t } = useTranslation();
  const kind = item.kind === 'movie' ? 'movie' : 'show';
  const guess = item.detected.replace(/\s*\(\d{4}\)\s*$/, '').replace(/\s*·.*$/, '');
  const guessYear = item.detected.match(/\((\d{4})\)/)?.[1] ?? '';
  const [candidates, setCandidates] = useState<Scored[]>(item.candidates);
  const [query, setQuery] = useState(guess);
  const [title, setTitle] = useState(guess);
  const [year, setYear] = useState(guessYear);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lookupsOff, setLookupsOff] = useState(false);

  const run = async (key: string, f: () => Promise<void>) => {
    setBusy(key);
    setError(null);
    try {
      await f();
    } catch (e) {
      const ae = asAppError(e);
      if (ae.code === 'lookupsOff') setLookupsOff(true);
      else setError(t(`errors.${ae.code}`));
    }
    setBusy(null);
  };

  return (
    <Dialog title={t('review.title')} onClose={onClose} wide>
      <p className="muted">
        {t('review.file')}: <bdi className="mono-ish">{item.fileName}</bdi>
      </p>

      <h3 className="sub-title">{t('review.candidates')}</h3>
      {candidates.length === 0 && <p className="muted">{t('review.noCandidates')}</p>}
      <ul className="candidate-list">
        {candidates.map((c) => (
          <li key={`${c.candidate.provider}:${c.candidate.id}`} className="candidate">
            <div className="candidate-text">
              <span className="candidate-title">
                <bdi>{c.candidate.title}</bdi>
                {c.candidate.year && <span className="muted"> ({c.candidate.year})</span>}
              </span>
              {c.candidate.originalTitle && c.candidate.originalTitle !== c.candidate.title && (
                <span className="muted small" dir="auto">
                  {c.candidate.originalTitle}
                </span>
              )}
              {c.candidate.overview && (
                <span className="muted small clamp-2" dir="auto">
                  {c.candidate.overview}
                </span>
              )}
              <span className="small accent-text">{t('review.confidence', { pct: Math.round(c.confidence * 100) })}</span>
            </div>
            <Button
              variant="primary"
              busy={busy === c.candidate.id}
              disabled={busy != null}
              onClick={() =>
                run(c.candidate.id, async () => {
                  await api.importResolve(item.id, { kind: 'candidate', id: c.candidate.id });
                  onResolved();
                  onClose();
                })
              }
            >
              {t('review.useThis')}
            </Button>
          </li>
        ))}
      </ul>

      <form
        className="inline-form"
        onSubmit={(e) => {
          e.preventDefault();
          void run('search', async () => setCandidates(await api.metadataSearch(kind, query.trim(), guessYear ? Number(guessYear) : null)));
        }}
      >
        <Field label={t('review.searchLabel')}>{(id) => <input spellCheck={false} autoComplete="off" id={id} value={query} dir="auto" onChange={(e) => setQuery(e.target.value)} />}</Field>
        <Button type="submit" icon="search" busy={busy === 'search'} disabled={!query.trim() || busy != null}>
          {t('review.searchAgain')}
        </Button>
      </form>
      {lookupsOff && <Notice kind="info">{t('review.lookupsOff')}</Notice>}

      <h3 className="sub-title">{t('review.manual')}</h3>
      <form
        className="inline-form"
        onSubmit={(e) => {
          e.preventDefault();
          void run('manual', async () => {
            await api.importResolve(item.id, { kind: 'manual', title: title.trim(), year: year ? Number(year) : null });
            onResolved();
            onClose();
          });
        }}
      >
        <Field label={t('edit.titleField')}>{(id) => <input spellCheck={false} autoComplete="off" id={id} value={title} dir="auto" onChange={(e) => setTitle(e.target.value)} />}</Field>
        <Field label={t('edit.year')}>
          {(id) => <input spellCheck={false} autoComplete="off" id={id} className="year-input" inputMode="numeric" dir="ltr" value={year} onChange={(e) => setYear(e.target.value.replace(/\D/g, '').slice(0, 4))} />}
        </Field>
        <Button type="submit" busy={busy === 'manual'} disabled={!title.trim() || busy != null}>
          {t('review.useManual')}
        </Button>
      </form>
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </Dialog>
  );
}
