import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, asAppError, type Id, type MetadataKind, type MetadataPatch } from '../api';
import { Button, Dialog, Field } from './ui';

export interface EditableDetails {
  title: string;
  originalTitle: string | null;
  year: number | null;
  lastYear?: number | null;
  overview: string | null;
  genres: string[];
  runtimeMin?: number | null;
}

const num = (s: string): number | null => {
  const n = parseInt(s, 10);
  return Number.isFinite(n) ? n : null;
};

export function EditDetailsDialog({
  kind,
  id,
  initial,
  onClose,
  onSaved,
}: {
  kind: MetadataKind;
  id: Id;
  initial: EditableDetails;
  onClose: () => void;
  onSaved: () => void;
}) {
  const { t } = useTranslation();
  const [title, setTitle] = useState(initial.title);
  const [original, setOriginal] = useState(initial.originalTitle ?? '');
  const [year, setYear] = useState(initial.year?.toString() ?? '');
  const [lastYear, setLastYear] = useState(initial.lastYear?.toString() ?? '');
  const [overview, setOverview] = useState(initial.overview ?? '');
  const [genres, setGenres] = useState(initial.genres.join(', '));
  const [runtime, setRuntime] = useState(initial.runtimeMin?.toString() ?? '');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    setBusy(true);
    const patch: MetadataPatch = {
      title: title.trim(),
      originalTitle: original.trim() || null,
      year: num(year),
      overview: overview.trim() || null,
      genres: genres
        .split(/[,،]/)
        .map((g) => g.trim())
        .filter(Boolean),
    };
    if (kind === 'show') patch.lastYear = num(lastYear);
    if (kind === 'movie') patch.runtimeMin = num(runtime);
    try {
      await api.libraryUpdateMetadata(kind, id, patch);
      onSaved();
      onClose();
    } catch (e) {
      setError(t(`errors.${asAppError(e).code}`));
      setBusy(false);
    }
  };

  return (
    <Dialog
      title={t('edit.title')}
      onClose={onClose}
      wide
      actions={
        <>
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button variant="primary" busy={busy} disabled={!title.trim()} onClick={save}>
            {t('common.save')}
          </Button>
        </>
      }
    >
      <div className="form-grid">
        <Field label={t('edit.titleField')}>{(fid) => <input id={fid} value={title} dir="auto" onChange={(e) => setTitle(e.target.value)} />}</Field>
        <Field label={t('edit.originalTitle')}>{(fid) => <input id={fid} value={original} dir="auto" onChange={(e) => setOriginal(e.target.value)} />}</Field>
        <Field label={kind === 'show' ? t('edit.firstYear') : t('edit.year')}>
          {(fid) => <input id={fid} inputMode="numeric" value={year} dir="ltr" onChange={(e) => setYear(e.target.value.replace(/\D/g, '').slice(0, 4))} />}
        </Field>
        {kind === 'show' && (
          <Field label={t('edit.lastYear')}>
            {(fid) => <input id={fid} inputMode="numeric" value={lastYear} dir="ltr" onChange={(e) => setLastYear(e.target.value.replace(/\D/g, '').slice(0, 4))} />}
          </Field>
        )}
        {kind === 'movie' && (
          <Field label={t('edit.runtime')}>
            {(fid) => <input id={fid} inputMode="numeric" value={runtime} dir="ltr" onChange={(e) => setRuntime(e.target.value.replace(/\D/g, '').slice(0, 4))} />}
          </Field>
        )}
        <Field label={t('edit.genres')} hint={t('edit.genresHint')}>
          {(fid) => <input id={fid} value={genres} dir="auto" onChange={(e) => setGenres(e.target.value)} />}
        </Field>
      </div>
      <Field label={t('edit.overview')}>{(fid) => <textarea id={fid} rows={5} value={overview} dir="auto" onChange={(e) => setOverview(e.target.value)} />}</Field>
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </Dialog>
  );
}

export function RemoveDialog({
  kind,
  id,
  title,
  onClose,
  onRemoved,
}: {
  kind: 'movie' | 'show' | 'episode';
  id: Id;
  title: string;
  onClose: () => void;
  onRemoved: () => void;
}) {
  const { t } = useTranslation();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const heading = { movie: t('remove.titleMovie'), show: t('remove.titleShow'), episode: t('remove.titleEpisode') }[kind];
  return (
    <Dialog
      title={heading}
      onClose={onClose}
      actions={
        <>
          <Button onClick={onClose}>{t('common.cancel')}</Button>
          <Button
            variant="danger"
            icon="trash"
            busy={busy}
            onClick={async () => {
              setBusy(true);
              try {
                await api.libraryRemove(kind, id);
                onRemoved();
              } catch (e) {
                setError(t(`errors.${asAppError(e).code}`));
                setBusy(false);
              }
            }}
          >
            {t('remove.confirm')}
          </Button>
        </>
      }
    >
      <p>
        {t('remove.body', { title: '⁨' + title + '⁩' })}
      </p>
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </Dialog>
  );
}
