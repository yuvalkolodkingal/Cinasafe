import { Link, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { api, type WatchItem } from '../api';
import { MoviePoster, playHref, RecentPoster, Row, ShowPoster, WatchCard, watchItemHref } from '../components/Cards';
import { Icon } from '../components/Icon';
import { Artwork, Button, Notice, ProgressBar, Spinner } from '../components/ui';
import { minutesLeft } from '../lib/format';
import { useAsync } from '../lib/session';

export function Home() {
  const { t } = useTranslation();
  const home = useAsync(() => api.libraryHome(), []);
  if (home.loading && !home.data) return <Spinner />;
  if (home.error) return <Notice kind="error">{t(`errors.${home.error.code}`)}</Notice>;
  const h = home.data!;
  if (h.empty) return <EmptyVault />;
  return (
    <div className="home">
      {h.hero && <Hero item={h.hero} />}
      {h.continueWatching.length > 0 && (
        <Row title={t('home.continueWatching')}>
          {h.continueWatching.map((w) => (
            <WatchCard key={key(w)} item={w} />
          ))}
        </Row>
      )}
      {h.upNext.length > 0 && (
        <Row title={t('home.upNext')}>
          {h.upNext.map((w) => (
            <WatchCard key={key(w)} item={w} />
          ))}
        </Row>
      )}
      {h.recentlyAdded.length > 0 && (
        <Row title={t('home.recentlyAdded')}>
          {h.recentlyAdded.map((r) => (
            <RecentPoster key={r.kind === 'movie' ? `m${r.movie.id}` : `s${r.show.id}`} item={r} />
          ))}
        </Row>
      )}
      {h.movies.length > 0 && (
        <Row title={t('home.movies')} href="/movies" seeAll={t('home.seeAll')}>
          {h.movies.map((m) => (
            <MoviePoster key={m.id} movie={m} />
          ))}
        </Row>
      )}
      {h.shows.length > 0 && (
        <Row title={t('home.shows')} href="/shows" seeAll={t('home.seeAll')}>
          {h.shows.map((s) => (
            <ShowPoster key={s.id} show={s} />
          ))}
        </Row>
      )}
    </div>
  );
}

const key = (w: WatchItem) => (w.kind === 'movie' ? `m${w.movie.id}` : `e${w.episode.id}`);

function Hero({ item }: { item: WatchItem }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const p = item.kind === 'movie' ? item.movie.progress : item.episode.progress;
  const started = p.positionMs > 0;
  const backdrop = item.kind === 'movie' ? item.movie.backdropId : (item.showBackdropId ?? item.episode.thumbId);
  const title = item.kind === 'movie' ? item.movie.title : item.showTitle;
  const line =
    item.kind === 'episode'
      ? item.episode.title
        ? t('home.episodeLineTitled', { season: item.episode.seasonNumber, episode: item.episode.number, title: item.episode.title })
        : t('home.episodeLine', { season: item.episode.seasonNumber, episode: item.episode.number })
      : [item.movie.year, item.movie.genres.slice(0, 2).join(', ')].filter(Boolean).join(' · ');
  const mediaFileId = item.kind === 'movie' ? item.movie.mediaFileId : item.episode.mediaFileId;
  const left = minutesLeft(p.positionMs, p.durationMs, t);
  return (
    <section className="hero" aria-label={title}>
      <Artwork id={backdrop} kind="backdrop" className="hero-art" />
      <div className="hero-shade" />
      <div className="hero-content">
        <span className="eyebrow">{started ? t('home.continueWatching') : t('home.upNext')}</span>
        <h1 className="hero-title">
          <bdi>{title}</bdi>
        </h1>
        <p className="hero-line" dir="auto">
          {line}
        </p>
        {started && p.durationMs && (
          <div className="hero-progress">
            <ProgressBar fraction={p.positionMs / p.durationMs} />
            {left && <span className="muted small">{left}</span>}
          </div>
        )}
        <div className="button-row">
          <Button variant="primary" icon="play" onClick={() => navigate(playHref(mediaFileId))}>
            {started ? t('home.resume') : t('home.play')}
          </Button>
          <Button variant="secondary" icon="info" onClick={() => navigate(watchItemHref(item))}>
            {t('home.moreInfo')}
          </Button>
        </div>
      </div>
    </section>
  );
}

function EmptyVault() {
  const { t } = useTranslation();
  return (
    <div className="empty-vault">
      <div className="empty-icon">
        <Icon name="upload" size={40} />
      </div>
      <h1 className="page-title">{t('home.emptyTitle')}</h1>
      <p className="muted">{t('home.emptyBody')}</p>
      <Link to="/add" className="btn btn-primary">
        <Icon name="plus" size={20} />
        <span>{t('home.emptyCta')}</span>
      </Link>
    </div>
  );
}
