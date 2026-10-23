import { Link } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import type { Movie, RecentItem, Show, WatchItem } from '../api';
import { formatRuntime, minutesLeft, yearRange } from '../lib/format';
import { Artwork, ProgressBar, WatchedBadge } from './ui';

/** Where a watch item leads: movie page, or the show page at the episode's season. */
export function watchItemHref(w: WatchItem): string {
  return w.kind === 'movie' ? `/movie/${w.movie.id}` : `/show/${w.showId}?season=${w.episode.seasonId}`;
}

export function playHref(mediaFileId: number, fromStart = false): string {
  return `/play/${mediaFileId}${fromStart ? '?start=1' : ''}`;
}

/** 16:9 card with a progress bar (Continue watching, Up next). */
export function WatchCard({ item }: { item: WatchItem }) {
  const { t } = useTranslation();
  const p = item.kind === 'movie' ? item.movie.progress : item.episode.progress;
  const mediaFileId = item.kind === 'movie' ? item.movie.mediaFileId : item.episode.mediaFileId;
  const art = item.kind === 'movie' ? (item.movie.backdropId ?? item.movie.posterId) : (item.episode.thumbId ?? item.showBackdropId);
  const title = item.kind === 'movie' ? item.movie.title : item.showTitle;
  const sub =
    item.kind === 'movie'
      ? [item.movie.year, minutesLeft(p.positionMs, p.durationMs, t)].filter(Boolean).join(' · ')
      : `S${item.episode.seasonNumber} · E${item.episode.number}${item.episode.title ? ` · ${item.episode.title}` : ''}`;
  return (
    <Link to={playHref(mediaFileId)} className="card watch-card">
      <div className="watch-card-art">
        <Artwork id={art} kind="thumb" title={title} />
        {p.positionMs > 0 && !p.watched && <ProgressBar fraction={p.durationMs ? p.positionMs / p.durationMs : 0} className="card-progress" />}
      </div>
      <div className="card-text">
        <bdi className="card-title">{title}</bdi>
        <span className="card-meta" dir="auto">
          {sub}
        </span>
      </div>
    </Link>
  );
}

export function MoviePoster({ movie }: { movie: Movie }) {
  const { t } = useTranslation();
  const p = movie.progress;
  const meta = [movie.year, formatRuntime(movie.runtimeMin, t)].filter(Boolean).join(' · ');
  return (
    <Link to={`/movie/${movie.id}`} className="card poster-card">
      <div className="poster-wrap">
        <Artwork id={movie.posterId} kind="poster" title={movie.title} />
        {p.watched && <WatchedBadge />}
        {!p.watched && p.positionMs > 0 && <ProgressBar fraction={p.durationMs ? p.positionMs / p.durationMs : 0} className="card-progress" />}
      </div>
      <div className="card-text">
        <bdi className="card-title">{movie.title}</bdi>
        {meta && <span className="card-meta">{meta}</span>}
      </div>
    </Link>
  );
}

export function ShowPoster({ show }: { show: Show }) {
  const { t } = useTranslation();
  const watched = show.episodeCount > 0 && show.watchedCount === show.episodeCount;
  const meta = [yearRange(show.firstYear, show.lastYear, t), t('grid.seasons', { count: show.seasonCount })].filter(Boolean).join(' · ');
  return (
    <Link to={`/show/${show.id}`} className="card poster-card">
      <div className="poster-wrap">
        <Artwork id={show.posterId} kind="poster" title={show.title} />
        {watched && <WatchedBadge />}
        {!watched && show.watchedCount > 0 && <ProgressBar fraction={show.watchedCount / Math.max(1, show.episodeCount)} className="card-progress" />}
      </div>
      <div className="card-text">
        <bdi className="card-title">{show.title}</bdi>
        <span className="card-meta">{meta}</span>
      </div>
    </Link>
  );
}

export function RecentPoster({ item }: { item: RecentItem }) {
  return item.kind === 'movie' ? <MoviePoster movie={item.movie} /> : <ShowPoster show={item.show} />;
}

/** Horizontally scrolling row with a heading. */
export function Row({ title, children, href, seeAll }: { title: string; children: React.ReactNode; href?: string; seeAll?: string }) {
  return (
    <section className="row">
      <div className="row-head">
        <h2 className="section-title">{title}</h2>
        {href && seeAll && (
          <Link to={href} className="row-more">
            {seeAll}
          </Link>
        )}
      </div>
      <div className="row-scroller">{children}</div>
    </section>
  );
}
