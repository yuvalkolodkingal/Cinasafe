import { useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { api, asAppError, type MediaFileInfo, type TrackInfo } from '../api';
import { playHref } from '../components/Cards';
import { EditDetailsDialog, RemoveDialog } from '../components/LibraryDialogs';
import { Artwork, Button, Notice, ProgressBar, Spinner } from '../components/ui';
import { formatBytes, formatRuntime, isolate, KNOWN_LANGUAGES, langCode, languageName, minutesLeft, trackLabel, videoSummary } from '../lib/format';
import { useAsync, useSession } from '../lib/session';

export function MovieDetail() {
  const { t } = useTranslation();
  const { id } = useParams();
  const navigate = useNavigate();
  const { toast } = useSession();
  const d = useAsync(() => api.libraryMovie(Number(id)), [id]);
  const [editing, setEditing] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [exporting, setExporting] = useState(false);

  if (d.loading && !d.data) return <Spinner />;
  if (d.error) return <Notice kind="error">{d.error.code === 'notFound' ? t('detail.notFound') : t(`errors.${d.error.code}`)}</Notice>;
  const { movie, file } = d.data!;
  const p = movie.progress;
  const started = p.positionMs > 0 && !p.watched;
  const video = videoSummary(file.tracks);
  const meta = [movie.year, formatRuntime(movie.runtimeMin, t), movie.genres.join(', ') || null, video?.split(' · ')[0]].filter(Boolean).join(' · ');

  return (
    <div className="detail">
      <section className="detail-band">
        <Artwork id={movie.backdropId} kind="backdrop" className="band-art" />
        <div className="band-shade" />
        <div className="band-content">
          <Artwork id={movie.posterId} kind="poster" title={movie.title} className="detail-poster" />
          <div className="band-text">
            <h1 className="detail-title">
              <bdi>{movie.title}</bdi>
            </h1>
            {movie.originalTitle && (
              <p className="muted" dir="auto">
                {movie.originalTitle}
              </p>
            )}
            <p className="detail-meta">{meta}</p>
            <p className="detail-overview" dir="auto">
              {movie.overview ?? t('detail.noOverview')}
            </p>
            {started && p.durationMs && (
              <div className="hero-progress">
                <ProgressBar fraction={p.positionMs / p.durationMs} />
                <span className="muted small">{minutesLeft(p.positionMs, p.durationMs, t)}</span>
              </div>
            )}
            <div className="button-row">
              <Button variant="primary" icon="play" onClick={() => navigate(playHref(movie.mediaFileId))}>
                {started ? t('detail.resume') : t('detail.play')}
              </Button>
              {started && (
                <Button icon="refresh" onClick={() => navigate(playHref(movie.mediaFileId, true))}>
                  {t('detail.playFromStart')}
                </Button>
              )}
              <Button
                icon="check"
                onClick={async () => {
                  await api.libraryMarkWatched(movie.mediaFileId, !p.watched);
                  d.reload();
                }}
              >
                {p.watched ? t('detail.markUnwatched') : t('detail.markWatched')}
              </Button>
            </div>
          </div>
        </div>
      </section>

      <div className="detail-cards">
        <DefaultsCard
          title={t('detail.defaultsMovie')}
          audio={movie.audioLangOverride}
          subtitles={movie.subtitleLangOverride}
          tracks={file}
          onChange={async (audio, subtitles) => {
            await api.librarySetOverrides('movie', movie.id, audio, subtitles);
            d.reload();
          }}
        />
        <FileCard
          file={file}
          actions={
            <>
              <Button icon="edit" onClick={() => setEditing(true)}>
                {t('detail.editDetails')}
              </Button>
              <Button
                icon="export"
                busy={exporting}
                title={t('detail.exportWarning')}
                onClick={async () => {
                  setExporting(true);
                  try {
                    const path = await api.libraryExport(file.id);
                    if (path) toast(t('detail.exported', { path: isolate(path) }));
                  } catch (e) {
                    toast(t(`errors.${asAppError(e).code}`));
                  }
                  setExporting(false);
                }}
              >
                {t('detail.exportCopy')}
              </Button>
              <Button variant="danger" icon="trash" onClick={() => setRemoving(true)}>
                {t('detail.removeFromVault')}
              </Button>
            </>
          }
        />
      </div>

      {editing && <EditDetailsDialog kind="movie" id={movie.id} initial={movie} onClose={() => setEditing(false)} onSaved={d.reload} />}
      {removing && (
        <RemoveDialog
          kind="movie"
          id={movie.id}
          title={movie.title}
          onClose={() => setRemoving(false)}
          onRemoved={() => navigate('/movies', { replace: true })}
        />
      )}
    </div>
  );
}

/** Audio and subtitle language overrides for a title. */
export function DefaultsCard({
  title,
  audio,
  subtitles,
  tracks,
  onChange,
}: {
  title: string;
  audio: string | null;
  subtitles: string | null;
  tracks?: MediaFileInfo;
  onChange: (audio: string | null, subtitles: string | null) => void;
}) {
  const { t } = useTranslation();
  const langs = (kind: 'audio' | 'sub') => {
    const fromFile = new Set<string>();
    tracks?.tracks.filter((x) => x.kind === kind).forEach((x) => {
      const c = langCode(x.lang);
      if (c) fromFile.add(c);
    });
    if (kind === 'sub') tracks?.subtitles.forEach((s) => s.language && fromFile.add(s.language));
    const list = fromFile.size ? [...fromFile] : KNOWN_LANGUAGES.slice(0, 8);
    return list;
  };
  return (
    <section className="card-panel">
      <h2 className="section-title">{title}</h2>
      <div className="form-grid">
        <label className="field">
          <span className="label">{t('detail.audio')}</span>
          <select value={audio ?? ''} onChange={(e) => onChange(e.target.value || null, subtitles)}>
            <option value="">{t('detail.useSettings')}</option>
            {langs('audio').map((l) => (
              <option key={l} value={l}>
                {languageName(l)}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          <span className="label">{t('detail.subtitles')}</span>
          <select value={subtitles ?? ''} onChange={(e) => onChange(audio, e.target.value || null)}>
            <option value="">{t('detail.useSettings')}</option>
            <option value="off">{t('detail.subtitlesOff')}</option>
            {langs('sub').map((l) => (
              <option key={l} value={l}>
                {languageName(l)}
              </option>
            ))}
          </select>
        </label>
      </div>
    </section>
  );
}

/** Comma-separated labels, each isolated so mixed Hebrew/English labels don't interleave. */
function BidiList({ items }: { items: string[] }) {
  return (
    <span className="bidi-list">
      {items.map((x, i) => (
        <span key={i}>
          <bdi>{x}</bdi>
          {i < items.length - 1 && ', '}
        </span>
      ))}
    </span>
  );
}

export function FileCard({ file, actions }: { file: MediaFileInfo; actions: React.ReactNode }) {
  const { t } = useTranslation();
  const audio = file.tracks.filter((x) => x.kind === 'audio');
  const subs: TrackInfo[] = [
    ...file.subtitles.map(
      (s, i): TrackInfo => ({ id: -1 - i, kind: 'sub', lang: s.language, title: null, codec: s.format, external: true, default: false, forced: false, subtitleFileId: s.id }),
    ),
    ...file.tracks.filter((x) => x.kind === 'sub'),
  ];
  const rows: [string, React.ReactNode][] = [
    [t('detail.originalName'), <bdi key="n" className="mono-ish">{file.originalName}</bdi>],
    [t('detail.video'), videoSummary(file.tracks) ?? t('common.unknown')],
    [t('detail.audio'), audio.length ? <BidiList items={audio.map((a) => trackLabel(a, t))} /> : t('common.unknown')],
    [t('detail.subtitles'), subs.length ? <BidiList items={subs.map((s) => trackLabel(s, t))} /> : t('common.none')],
    [t('detail.encryptedSize'), formatBytes(file.encryptedBytes, t)],
  ];
  return (
    <section className="card-panel">
      <h2 className="section-title">{t('detail.file')}</h2>
      <dl className="kv">
        {rows.map(([k, v]) => (
          <div key={k} className="kv-row">
            <dt>{k}</dt>
            <dd>{v}</dd>
          </div>
        ))}
      </dl>
      <div className="button-row wrap">{actions}</div>
    </section>
  );
}
