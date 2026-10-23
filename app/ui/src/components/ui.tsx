// Small shared building blocks: buttons, dialogs, segmented controls, posters.

import {
  useEffect,
  useId,
  useRef,
  type ButtonHTMLAttributes,
  type KeyboardEvent,
  type ReactNode,
} from 'react';
import { useTranslation } from 'react-i18next';

import { imageUrl, type Id } from '../api';
import { Icon, type IconName } from './Icon';

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'outline';

export function Button({
  variant = 'secondary',
  icon,
  children,
  busy,
  className,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; icon?: IconName; busy?: boolean }) {
  return (
    <button
      type="button"
      className={`btn btn-${variant} ${className ?? ''}`}
      aria-busy={busy || undefined}
      disabled={rest.disabled || busy}
      {...rest}
    >
      {busy ? <span className="spinner" aria-hidden="true" /> : icon && <Icon name={icon} size={20} />}
      {children && <span>{children}</span>}
    </button>
  );
}

export function IconButton({
  icon,
  label,
  className,
  round,
  size = 22,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { icon: IconName; label: string; round?: boolean; size?: number }) {
  return (
    <button
      type="button"
      className={`icon-btn ${round ? 'round' : ''} ${className ?? ''}`}
      aria-label={label}
      title={label}
      {...rest}
    >
      <Icon name={icon} size={size} />
    </button>
  );
}

export function Spinner({ label }: { label?: string }) {
  return (
    <div className="center-block" role="status">
      <span className="spinner large" aria-hidden="true" />
      {label && <span className="muted">{label}</span>}
    </div>
  );
}

export function Segmented<T extends string | number>({
  options,
  value,
  onChange,
  label,
}: {
  options: { value: T; label: string }[];
  value: T;
  onChange: (v: T) => void;
  label: string;
}) {
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const onKey = (e: KeyboardEvent, i: number) => {
    const rtl = document.documentElement.dir === 'rtl';
    const fwd = rtl ? 'ArrowLeft' : 'ArrowRight';
    const bwd = rtl ? 'ArrowRight' : 'ArrowLeft';
    let n = -1;
    if (e.key === fwd) n = (i + 1) % options.length;
    if (e.key === bwd) n = (i - 1 + options.length) % options.length;
    if (n >= 0) {
      e.preventDefault();
      onChange(options[n].value);
      refs.current[n]?.focus();
    }
  };
  return (
    <div className="segmented" role="tablist" aria-label={label}>
      {options.map((o, i) => (
        <button
          key={String(o.value)}
          ref={(el) => {
            refs.current[i] = el;
          }}
          type="button"
          role="tab"
          aria-selected={o.value === value}
          tabIndex={o.value === value ? 0 : -1}
          className={o.value === value ? 'active' : ''}
          onClick={() => onChange(o.value)}
          onKeyDown={(e) => onKey(e, i)}
        >
          {o.label}
        </button>
      ))}
    </div>
  );
}

export function Field({ label, hint, children, error }: { label: string; hint?: string; children: (id: string) => ReactNode; error?: string | null }) {
  const id = useId();
  return (
    <div className="field">
      <label htmlFor={id}>{label}</label>
      {children(id)}
      {hint && <p className="field-hint">{hint}</p>}
      {error && (
        <p className="field-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}

export function Toggle({ checked, onChange, label, hint, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: string; hint?: string; disabled?: boolean }) {
  const id = useId();
  return (
    <div className="toggle-row">
      <div className="toggle-text">
        <label htmlFor={id}>{label}</label>
        {hint && <p className="field-hint">{hint}</p>}
      </div>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        className={`switch ${checked ? 'on' : ''}`}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="switch-knob" />
      </button>
    </div>
  );
}

export function Checkbox({ checked, onChange, label, disabled }: { checked: boolean; onChange: (v: boolean) => void; label: ReactNode; disabled?: boolean }) {
  return (
    <label className={`checkbox ${disabled ? 'disabled' : ''}`}>
      <input type="checkbox" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      <span className="checkbox-box" aria-hidden="true">
        <Icon name="check" size={16} />
      </span>
      <span>{label}</span>
    </label>
  );
}

/** Modal dialog with focus trap and Esc to close. */
export function Dialog({
  title,
  children,
  onClose,
  actions,
  wide,
}: {
  title: string;
  children: ReactNode;
  onClose: () => void;
  actions?: ReactNode;
  wide?: boolean;
}) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const titleId = useId();
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null;
    const el = ref.current;
    const first = el?.querySelector<HTMLElement>('input, select, textarea, button:not(.dialog-close)');
    (first ?? el)?.focus();
    return () => prev?.focus?.();
  }, []);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.stopPropagation();
      onClose();
    }
    if (e.key === 'Tab' && ref.current) {
      const f = [...ref.current.querySelectorAll<HTMLElement>('a[href], button:not([disabled]), input:not([disabled]), select, textarea, [tabindex]:not([tabindex="-1"])')];
      if (f.length === 0) return;
      const i = f.indexOf(document.activeElement as HTMLElement);
      if (e.shiftKey && i <= 0) {
        e.preventDefault();
        f[f.length - 1].focus();
      } else if (!e.shiftKey && i === f.length - 1) {
        e.preventDefault();
        f[0].focus();
      }
    }
  };
  return (
    <div className="dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div
        ref={ref}
        className={`dialog ${wide ? 'wide' : ''}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        tabIndex={-1}
        onKeyDown={onKey}
      >
        <header className="dialog-head">
          <h2 id={titleId}>{title}</h2>
          <IconButton icon="x" label={t('common.close')} className="dialog-close" onClick={onClose} />
        </header>
        <div className="dialog-body">{children}</div>
        {actions && <footer className="dialog-actions">{actions}</footer>}
      </div>
    </div>
  );
}

export function ProgressBar({ fraction, label, className }: { fraction: number; label?: string; className?: string }) {
  const pct = Math.round(Math.max(0, Math.min(1, fraction)) * 100);
  return (
    <div className={`progress ${className ?? ''}`} role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct} aria-label={label}>
      <div className="progress-fill" style={{ inlineSize: `${pct}%` }} />
    </div>
  );
}

/** Artwork from the vault, or a tinted placeholder with the title. */
export function Artwork({ id, kind, title, className }: { id: Id | null | undefined; kind: 'poster' | 'backdrop' | 'thumb'; title?: string; className?: string }) {
  const src = imageUrl(id);
  return (
    <div className={`art art-${kind} ${className ?? ''}`}>
      {src ? (
        <img src={src} alt="" draggable={false} loading="lazy" />
      ) : (
        <div className="art-placeholder" aria-hidden="true">
          <Icon name={kind === 'poster' ? 'film' : 'tv'} size={kind === 'poster' ? 34 : 28} />
          {title && kind === 'poster' && (
            <span className="art-placeholder-title" dir="auto">
              {title}
            </span>
          )}
        </div>
      )}
    </div>
  );
}

export function WatchedBadge() {
  const { t } = useTranslation();
  return (
    <span className="watched-badge" title={t('detail.watchedBadge')}>
      <Icon name="check" size={16} />
      <span className="sr-only">{t('detail.watchedBadge')}</span>
    </span>
  );
}

export function Notice({ kind = 'warning', children }: { kind?: 'warning' | 'info' | 'error'; children: ReactNode }) {
  return (
    <div className={`notice notice-${kind}`} role={kind === 'error' ? 'alert' : 'status'}>
      <Icon name={kind === 'info' ? 'info' : 'alert'} size={20} />
      <div>{children}</div>
    </div>
  );
}

/** Activates on Enter and Space, for clickable cards. */
export function activate(cb: () => void) {
  return {
    role: 'link' as const,
    tabIndex: 0,
    onClick: cb,
    onKeyDown: (e: KeyboardEvent) => {
      if (e.key === 'Enter' || e.key === ' ') {
        e.preventDefault();
        cb();
      }
    },
  };
}
