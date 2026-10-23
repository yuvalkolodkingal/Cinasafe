import { useState } from 'react';
import { useTranslation } from 'react-i18next';

import { api, type Grid as GridData, type Movie, type Show, type SortBy, type WatchFilter } from '../api';
import { MoviePoster, ShowPoster } from '../components/Cards';
import { Notice, Segmented, Spinner } from '../components/ui';
import { formatBytes } from '../lib/format';
import { useAsync } from '../lib/session';

export function Grid({ kind }: { kind: 'movies' | 'shows' }) {
  const { t } = useTranslation();
  const [filter, setFilter] = useState<WatchFilter>('all');
  const [genre, setGenre] = useState<string>('');
  const [sort, setSort] = useState<SortBy>('dateAdded');
  const data = useAsync<GridData<Movie> | GridData<Show>>(
    () => (kind === 'movies' ? api.libraryMovies(filter, genre || null, sort) : api.libraryShows(filter, genre || null, sort)),
    [kind, filter, genre, sort],
  );
  const g = data.data;
  return (
    <div className="grid-page">
      <header className="page-head">
        <h1 className="page-title">{kind === 'movies' ? t('grid.moviesTitle') : t('grid.showsTitle')}</h1>
        {g && (
          <p className="muted">
            {kind === 'movies' ? t('grid.movieCount', { count: g.totalCount }) : t('grid.showCount', { count: g.totalCount })}
            {' · '}
            {formatBytes(g.totalBytes, t)}
          </p>
        )}
      </header>
      <div className="toolbar">
        <Segmented<WatchFilter>
          label={t('grid.filter')}
          value={filter}
          onChange={setFilter}
          options={[
            { value: 'all', label: t('grid.all') },
            { value: 'unwatched', label: t('grid.unwatched') },
            { value: 'inProgress', label: t('grid.inProgress') },
            { value: 'watched', label: t('grid.watched') },
          ]}
        />
        <div className="toolbar-end">
          <label className="inline-select">
            <span>{t('grid.genre')}</span>
            <select value={genre} onChange={(e) => setGenre(e.target.value)}>
              <option value="">{t('grid.allGenres')}</option>
              {g?.genres.map((x) => (
                <option key={x} value={x}>
                  {x}
                </option>
              ))}
            </select>
          </label>
          <label className="inline-select">
            <span>{t('grid.sort')}</span>
            <select value={sort} onChange={(e) => setSort(e.target.value as SortBy)}>
              <option value="dateAdded">{t('grid.sortDateAdded')}</option>
              <option value="title">{t('grid.sortTitle')}</option>
              <option value="year">{t('grid.sortYear')}</option>
              <option value="lastWatched">{t('grid.sortLastWatched')}</option>
            </select>
          </label>
        </div>
      </div>
      {data.error && <Notice kind="error">{t(`errors.${data.error.code}`)}</Notice>}
      {!g && data.loading && <Spinner />}
      {g && g.items.length === 0 && <p className="muted empty-line">{t('grid.nothingHere')}</p>}
      {g && (
        <div className="poster-grid">
          {kind === 'movies'
            ? (g.items as Movie[]).map((m) => <MoviePoster key={m.id} movie={m} />)
            : (g.items as Show[]).map((s) => <ShowPoster key={s.id} show={s} />)}
        </div>
      )}
    </div>
  );
}
