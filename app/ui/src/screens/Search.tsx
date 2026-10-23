import { useEffect, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { api, type SearchResults } from '../api';
import { playHref } from '../components/Cards';
import { Icon } from '../components/Icon';
import { Artwork } from '../components/ui';
import { formatRuntime, highlight, yearRange } from '../lib/format';

function Hl({ text, q }: { text: string; q: string }) {
  return (
    <bdi>
      {highlight(text, q).map((c, i) =>
        c.hit ? (
          <mark key={i} className="hit">
            {c.text}
          </mark>
        ) : (
          <span key={i}>{c.text}</span>
        ),
      )}
    </bdi>
  );
}

export function Search() {
  const { t } = useTranslation();
  const [params, setParams] = useSearchParams();
  const [q, setQ] = useState(params.get('q') ?? '');
  const [results, setResults] = useState<SearchResults | null>(null);

  useEffect(() => {
    const query = q.trim();
    const timer = setTimeout(() => {
      setParams(query ? { q: query } : {}, { replace: true });
      if (!query) {
        setResults(null);
        return;
      }
      api.search(query).then(setResults, () => setResults(null));
    }, 150);
    return () => clearTimeout(timer);
  }, [q, setParams]);

  const total = results ? results.shows.length + results.movies.length + results.episodes.length : 0;

  return (
    <div className="search-page">
      <h1 className="sr-only">{t('search.title')}</h1>
      <div className="search-box">
        <Icon name="search" size={26} />
        <input
          type="search"
          autoFocus
          value={q}
          aria-label={t('search.label')}
          placeholder={t('search.placeholder')}
          onChange={(e) => setQ(e.target.value)}
          dir="auto"
          spellCheck={false}
          autoComplete="off"
        />
      </div>
      {!q.trim() && <p className="muted empty-line">{t('search.prompt')}</p>}
      {results && total === 0 && <p className="muted empty-line">{t('search.noResults', { q: q.trim() })}</p>}
      {results && results.shows.length > 0 && (
        <section className="result-group">
          <h2 className="section-title">{t('search.shows', { n: results.shows.length })}</h2>
          <ul className="result-list">
            {results.shows.map((s) => (
              <li key={s.id}>
                <Link to={`/show/${s.id}`} className="result-row">
                  <Artwork id={s.posterId} kind="poster" className="result-poster" />
                  <div className="result-text">
                    <span className="result-title">
                      <Hl text={s.title} q={q} />
                    </span>
                    <span className="card-meta">
                      {[yearRange(s.firstYear, s.lastYear, t), t('grid.seasons', { count: s.seasonCount })].filter(Boolean).join(' · ')}
                    </span>
                  </div>
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}
      {results && results.movies.length > 0 && (
        <section className="result-group">
          <h2 className="section-title">{t('search.movies', { n: results.movies.length })}</h2>
          <ul className="result-list">
            {results.movies.map((m) => (
              <li key={m.id}>
                <Link to={`/movie/${m.id}`} className="result-row">
                  <Artwork id={m.posterId} kind="poster" className="result-poster" />
                  <div className="result-text">
                    <span className="result-title">
                      <Hl text={m.title} q={q} />
                    </span>
                    <span className="card-meta">{[m.year, formatRuntime(m.runtimeMin, t), m.genres.join(', ')].filter(Boolean).join(' · ')}</span>
                  </div>
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}
      {results && results.episodes.length > 0 && (
        <section className="result-group">
          <h2 className="section-title">{t('search.episodes', { n: results.episodes.length })}</h2>
          <ul className="result-list">
            {results.episodes.map((e) => (
              <li key={e.id}>
                <Link to={playHref(e.mediaFileId)} className="result-row">
                  <Artwork id={e.thumbId} kind="thumb" className="result-thumb" />
                  <div className="result-text">
                    <span className="result-title">
                      <Hl text={e.title ?? ''} q={q} />
                    </span>
                    <span className="card-meta">
                      <bdi>{results.showTitles[String(e.showId)]}</bdi> · S{e.seasonNumber} · E{e.number}
                    </span>
                  </div>
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}
