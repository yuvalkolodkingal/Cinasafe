import { useCallback, useEffect, useRef, useState, type DragEvent } from 'react';
import { useTranslation } from 'react-i18next';

import { api, asAppError, inTauri, on, type QueueItem, type Storage } from '../api';
import { Icon } from '../components/Icon';
import { ReviewMatchDialog } from '../components/ReviewMatchDialog';
import { Button, Checkbox, IconButton, ProgressBar } from '../components/ui';
import { formatBytes, formatEta } from '../lib/format';
import { useSession } from '../lib/session';

const ACTIVE = new Set(['waiting', 'lookingUp', 'encrypting', 'verifying', 'finishing']);

export function AddMedia() {
  const { t } = useTranslation();
  const { settings, updateSettings, toast } = useSession();
  const [items, setItems] = useState<QueueItem[]>([]);
  const [paused, setPaused] = useState(false);
  const [storage, setStorage] = useState<Storage | null>(null);
  const [dragging, setDragging] = useState(false);
  const [reviewing, setReviewing] = useState<QueueItem | null>(null);
  const storageTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const refreshStorage = useCallback(() => {
    if (storageTimer.current) return;
    storageTimer.current = setTimeout(() => {
      storageTimer.current = null;
      api.storage().then(setStorage, () => {});
    }, 1000);
  }, []);

  const load = useCallback(() => {
    api.importStatus().then((s) => {
      setItems(s.items);
      setPaused(s.paused);
    });
    api.storage().then(setStorage, () => {});
  }, []);

  useEffect(() => {
    load();
    const offs = [
      on('import-progress', (item) => {
        setItems((list) => {
          const i = list.findIndex((x) => x.id === item.id);
          if (i < 0) return [...list, item];
          const next = [...list];
          next[i] = item;
          return next;
        });
        refreshStorage();
      }),
      on('import-queue', (q) => setPaused(q.paused)),
    ];
    return () => {
      offs.forEach((o) => o());
      if (storageTimer.current) clearTimeout(storageTimer.current);
    };
  }, [load, refreshStorage]);

  const add = useCallback(
    async (f: () => Promise<QueueItem[]>) => {
      try {
        await f();
        load();
      } catch (e) {
        toast(t(`errors.${asAppError(e).code}`));
      }
    },
    [load, toast, t],
  );

  // Files dropped on the window: Tauri gives real paths through its own event.
  useEffect(() => {
    if (!inTauri) return;
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    import('@tauri-apps/api/webview').then(async ({ getCurrentWebview }) => {
      const u = await getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === 'enter' || p.type === 'over') setDragging(true);
        else if (p.type === 'leave') setDragging(false);
        else if (p.type === 'drop') {
          setDragging(false);
          if (p.paths.length) void add(() => api.importAdd(p.paths));
        }
      });
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [add]);

  // In a plain browser (mock), HTML drag and drop gives names only.
  const htmlDrop = inTauri
    ? {}
    : {
        onDragOver: (e: DragEvent) => {
          e.preventDefault();
          setDragging(true);
        },
        onDragLeave: () => setDragging(false),
        onDrop: (e: DragEvent) => {
          e.preventDefault();
          setDragging(false);
          const names = [...e.dataTransfer.files].map((f) => f.name);
          if (names.length) void add(() => api.importAdd(names));
        },
      };

  const active = items.filter((i) => ACTIVE.has(i.state));
  const finished = items.filter((i) => !ACTIVE.has(i.state));
  const used = storage?.usedBytes ?? 0;
  const size = storage?.sizeBytes ?? 1;
  const queued = storage?.queuedBytes ?? 0;
  const leftAfter = Math.max(0, (storage?.freeBytes ?? size - used) - queued);

  return (
    <div className="add-page">
      <header className="page-head add-head">
        <h1 className="page-title">{t('add.title')}</h1>
        {storage && (
          <div className="storage">
            <div className="storage-text">
              <span className="storage-used">{t('add.storage', { used: formatBytes(used, t), size: formatBytes(size, t) })}</span>
              {queued > 0 && <span className="muted small">{t('add.queueAdds', { queued: formatBytes(queued, t), after: formatBytes(leftAfter, t) })}</span>}
            </div>
            <div className="storage-bar" role="img" aria-label={`${t('add.used')} ${formatBytes(used, t)}, ${t('add.queued')} ${formatBytes(queued, t)}`}>
              <div className="storage-used-fill" style={{ inlineSize: `${(used / size) * 100}%` }} />
              <div className="storage-queued-fill" style={{ inlineSize: `${(Math.min(queued, size - used) / size) * 100}%` }} />
            </div>
          </div>
        )}
      </header>

      <div className={`drop-zone ${dragging ? 'dragging' : ''}`} {...htmlDrop}>
        <div className="drop-icon">
          <Icon name="upload" size={34} />
        </div>
        <p className="drop-title">{dragging ? t('add.dropping') : t('add.dropTitle')}</p>
        <p className="muted">{t('add.dropBody')}</p>
        <div className="button-row center">
          <Button icon="file" onClick={() => add(() => api.importBrowse(false))}>
            {t('add.browseFiles')}
          </Button>
          <Button icon="folder" onClick={() => add(() => api.importBrowse(true))}>
            {t('add.browseFolder')}
          </Button>
        </div>
        <div className="drop-option">
          <Checkbox checked={settings.lookupOnline} onChange={(v) => void updateSettings({ lookupOnline: v })} label={t('add.lookupOnline')} />
          <p className="field-hint">{t('add.lookupOnlineHint')}</p>
        </div>
      </div>

      <section className="queue">
        <div className="queue-head">
          <h2 className="section-title">{t('add.queue')}</h2>
          <div className="button-row">
            {active.length > 0 && (
              <>
                <Button icon={paused ? 'play' : 'pause'} onClick={() => (paused ? api.importResume() : api.importPause()).then(() => setPaused(!paused))}>
                  {paused ? t('add.resume') : t('add.pause')}
                </Button>
                <Button variant="outline" icon="x" onClick={() => api.importCancel(null).then(load)}>
                  {t('add.cancelAll')}
                </Button>
              </>
            )}
            {finished.length > 0 && (
              <Button variant="ghost" onClick={() => api.importClearFinished().then(load)}>
                {t('add.clearFinished')}
              </Button>
            )}
          </div>
        </div>
        {items.length === 0 ? (
          <p className="muted empty-line">{t('add.emptyQueue')}</p>
        ) : (
          <ul className="queue-list">
            {items.map((i) => (
              <QueueRow key={i.id} item={i} paused={paused} onReview={() => setReviewing(i)} onCancel={() => api.importCancel(i.id)} />
            ))}
          </ul>
        )}
      </section>
      {reviewing && <ReviewMatchDialog item={reviewing} onClose={() => setReviewing(null)} onResolved={load} />}
    </div>
  );
}

function QueueRow({ item, paused, onReview, onCancel }: { item: QueueItem; paused: boolean; onReview: () => void; onCancel: () => void }) {
  const { t } = useTranslation();
  const running = item.state === 'encrypting' || item.state === 'verifying';
  const fraction = item.state === 'encrypting' ? item.fraction * 0.5 : item.state === 'verifying' ? 0.5 + item.fraction * 0.5 : 0;

  let status: { tone: 'good' | 'warn' | 'muted' | 'bad' | 'accent'; text: string };
  switch (item.state) {
    case 'encrypting':
    case 'verifying': {
      const pct = Math.round(item.fraction * 100);
      const eta = paused ? null : formatEta(item.etaSeconds, t);
      status = { tone: 'accent', text: [t(`add.state.${item.state}`, { pct }), eta].filter(Boolean).join(' · ') };
      break;
    }
    case 'failed':
      status = { tone: 'bad', text: t('add.state.failed', { message: item.message }) };
      break;
    case 'done':
      status =
        item.matchState === 'needsReview'
          ? { tone: 'warn', text: t('add.match.needsReview') }
          : item.matchState === 'matched'
            ? { tone: 'good', text: t('add.match.matched') }
            : item.matchState === 'noMatch'
              ? { tone: 'warn', text: t('add.match.noMatch') }
              : { tone: 'good', text: t('add.state.done') };
      break;
    case 'wontFit':
    case 'noVideo':
      status = { tone: 'bad', text: t(`add.state.${item.state}`) };
      break;
    default:
      status = { tone: 'muted', text: t(`add.state.${item.state}`) };
  }
  const canCancel = ACTIVE.has(item.state);
  return (
    <li className="queue-row">
      <div className="queue-icon">
        <Icon name={item.kind === 'subtitle' ? 'subtitles' : item.kind === 'movie' ? 'film' : 'tv'} size={22} />
      </div>
      <div className="queue-main">
        <div className="queue-line">
          <bdi className="queue-file">{item.fileName}</bdi>
          {item.sizeBytes > 0 && <span className="muted small">{formatBytes(item.sizeBytes, t)}</span>}
        </div>
        <div className="queue-line">
          <bdi className="queue-detected">{item.detected}</bdi>
        </div>
        <div className="queue-line">
          <span className={`status-dot tone-${status.tone}`} aria-hidden="true" />
          <span className={`status-text tone-${status.tone}`}>{status.text}</span>
        </div>
        {running && <ProgressBar fraction={fraction} className="queue-progress" />}
        {item.subtitles.map((s) => {
          const from = s.note?.match(/from (\S+) to/i)?.[1];
          return (
            <div key={s.fileName} className="queue-line sub-note">
              <Icon name="subtitles" size={16} />
              <bdi>{s.fileName}</bdi>
              <span className="tone-good small">{t('add.match.attached')}</span>
              {from && <span className="muted small">{t('add.converted', { from })}</span>}
              {s.rtlFixOffered && <span className="muted small">{t('add.rtlOffered')}</span>}
            </div>
          );
        })}
        {item.messages.map((m) => (
          <div key={m} className="queue-line muted small">
            {m}
          </div>
        ))}
      </div>
      <div className="queue-actions">
        {item.matchState === 'needsReview' && item.state === 'done' && (
          <Button variant="outline" onClick={onReview}>
            {t('add.reviewMatch')}
          </Button>
        )}
        {item.matchState === 'noMatch' && item.state === 'done' && (
          <Button variant="ghost" onClick={onReview}>
            {t('add.reviewMatch')}
          </Button>
        )}
        {canCancel && <IconButton icon="x" label={t('add.cancelItem')} onClick={onCancel} />}
      </div>
    </li>
  );
}
