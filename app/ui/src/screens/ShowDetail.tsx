import { useState } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { api, type Episode, type Id } from '../api';
import { playHref } from '../components/Cards';
import { EditDetailsDialog, RemoveDialog } from '../components/LibraryDialogs';
import { Artwork, Button, IconButton, Notice, ProgressBar, Segmented, Spinner, WatchedBadge } from '../components/ui';
import { formatRuntime, minutesLeft, yearRange } from '../lib/format';
import { useAsync } from '../lib/session';
import { DefaultsCard } from './MovieDetail';

export function ShowDetail() {
  const { t } = useTranslation();
  const { id } = useParams();
  const [search, setSearch] = useSearchParams();
  const navigate = useNavigate();
  const d = useAsync(() => api.libraryShow(Number(id)), [id]);
  const [editing, setEditing] = useState(false);
  const [removing, setRemoving] = useState(false);

  const seasonParam = Number(search.get('season')) || null;
  const data = d.data;
  const seasonId: Id | null = data
    ? (data.seasons.find((s) => s.id === seasonParam)?.id ?? data.resume?.seasonId ?? data.seasons[0]?.id ?? null)
    : null;
  const season = useAsync(() => (seasonId ? api.librarySeason(seasonId) : Promise.resolve(null)), [seasonId]);

  if (d.loading && !data) return <Spinner />;
  if (d.error) return <Notice kind="error">{d.error.code === 'notFound' ? t('detail.notFound') : t(`errors.${d.error.code}`)}</Notice>;
  const { show, seasons, resume } = data!;
  const years = yearRange(show.firstYear, show.lastYear, t);
  const meta = [
    years,
    t('grid.seasons', { count: show.seasonCount }),
    t('grid.episodes', { count: show.episodeCount }),
    show.genres.join(', ') || null,
  ]
    .filter(Boolean)
    .join(' · ');
  const resumeStarted = resume && resume.progress.positionMs > 0 && !resume.progress.watched;
  const s = season.data;

  const reloadAll = () => {
    d.reload();
    season.reload();
  };

  return (
    <div className="detail">
      <section className="detail-band show-band">
        <Artwork id={show.backdropId} kind="backdrop" className="band-art" />
        <div className="band-shade" />
        <div className="band-content">
          <div className="band-text">
            <h1 className="detail-title">
              <bdi>{show.title}</bdi>
            </h1>
            {show.originalTitle && (
              <p className="muted" dir="auto">
                {show.originalTitle}
              </p>
            )}
            <p className="detail-meta">{meta}</p>
            <p className="detail-overview" dir="auto">
              {show.overview ?? t('detail.noOverview')}
            </p>
            <div className="button-row">
              {resume && (
                <Button variant="primary" icon="play" onClick={() => navigate(playHref(resume.mediaFileId))}>
                  {resumeStarted
                    ? t('detail.resumeEpisode', { season: resume.seasonNumber, episode: resume.number })
                    : t('detail.startEpisode', { season: resume.seasonNumber, episode: resume.number })}
                </Button>
              )}
              <Button icon="edit" onClick={() => setEditing(true)}>
                {t('detail.editDetails')}
              </Button>
              <Button variant="danger" icon="trash" onClick={() => setRemoving(true)}>
                {t('detail.removeFromVault')}
              </Button>
            </div>
          </div>
        </div>
      </section>

      <section className="seasons">
        <div className="seasons-head">
          {seasons.length > 1 ? (
            <Segmented<number>
              label={t('detail.seasons')}
              value={seasonId ?? 0}
              onChange={(v) => setSearch({ season: String(v) }, { replace: true })}
              options={seasons.map((x) => ({ value: x.id, label: x.number === 0 ? t('detail.specials') : t('detail.seasonTab', { n: x.number }) }))}
            />
          ) : (
            <h2 className="section-title">{seasons[0] && (seasons[0].number === 0 ? t('detail.specials') : t('detail.seasonTab', { n: seasons[0].number }))}</h2>
          )}
          {s && (
            <div className="seasons-meta">
              <span className="muted">{t('detail.watchedOf', { watched: s.season.watchedCount, total: s.season.episodeCount })}</span>
              <Button
                variant="ghost"
                icon="check"
                onClick={async () => {
                  const all = s.season.watchedCount === s.season.episodeCount;
                  await api.libraryMarkSeasonWatched(s.season.id, !all);
                  reloadAll();
                }}
              >
                {s.season.watchedCount === s.season.episodeCount ? t('detail.markSeasonUnwatched') : t('detail.markSeasonWatched')}
              </Button>
            </div>
          )}
        </div>
        {season.loading && !s && <Spinner />}
        {s && (
          <ol className="episode-list">
            {s.episodes.map((e) => (
              <EpisodeRow key={e.id} e={e} upNext={e.id === s.upNextEpisodeId} onChanged={reloadAll} />
            ))}
          </ol>
        )}
      </section>

      <div className="detail-cards single">
        <DefaultsCard
          title={t('detail.defaultsShow')}
          audio={show.audioLangOverride}
          subtitles={show.subtitleLangOverride}
          onChange={async (audio, subtitles) => {
            await api.librarySetOverrides('show', show.id, audio, subtitles);
            d.reload();
          }}
        />
      </div>

      {editing && (
        <EditDetailsDialog
          kind="show"
          id={show.id}
          initial={{ ...show, year: show.firstYear }}
          onClose={() => setEditing(false)}
          onSaved={d.reload}
        />
      )}
      {removing && (
        <RemoveDialog kind="show" id={show.id} title={show.title} onClose={() => setRemoving(false)} onRemoved={() => navigate('/shows', { replace: true })} />
      )}
    </div>
  );
}

function EpisodeRow({ e, upNext, onChanged }: { e: Episode; upNext: boolean; onChanged: () => void }) {
  const { t } = useTranslation();
  const p = e.progress;
  const inProgress = p.positionMs > 0 && !p.watched;
  const left = minutesLeft(p.positionMs, p.durationMs, t);
  let status: string;
  if (p.watched) status = t('detail.statusWatched');
  else if (upNext) status = [t('detail.statusUpNext'), inProgress ? left : null].filter(Boolean).join(' · ');
  else if (inProgress) status = [t('detail.statusInProgress'), left].filter(Boolean).join(' · ');
  else status = t('detail.statusNotStarted');
  return (
    <li className={`episode-row ${upNext ? 'up-next' : ''}`}>
      <Link to={playHref(e.mediaFileId)} className="episode-link">
        <div className="episode-thumb">
          <Artwork id={e.thumbId} kind="thumb" />
          {inProgress && <ProgressBar fraction={p.durationMs ? p.positionMs / p.durationMs : 0} className="card-progress" />}
          {p.watched && <WatchedBadge />}
        </div>
        <div className="episode-text">
          <span className="episode-title">
            {e.number}. <bdi>{e.title ?? t('home.episodeLine', { season: e.seasonNumber, episode: e.number })}</bdi>
          </span>
          <span className="card-meta">{formatRuntime(e.runtimeMin, t)}</span>
          <span className={`episode-status ${upNext ? 'accent' : ''}`}>{status}</span>
          {e.overview && (
            <p className="episode-overview" dir="auto">
              {e.overview}
            </p>
          )}
        </div>
      </Link>
      <IconButton
        icon="check"
        className={p.watched ? 'toggled' : ''}
        label={p.watched ? t('detail.markUnwatched') : t('detail.markWatched')}
        aria-pressed={p.watched}
        onClick={async () => {
          await api.libraryMarkWatched(e.mediaFileId, !p.watched);
          onChanged();
        }}
      />
    </li>
  );
}
