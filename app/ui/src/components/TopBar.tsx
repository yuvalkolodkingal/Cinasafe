import { NavLink, useNavigate } from 'react-router-dom';
import { useTranslation } from 'react-i18next';

import { useSession } from '../lib/session';
import { Icon, Logo } from './Icon';
import { IconButton } from './ui';

export function TopBar() {
  const { t } = useTranslation();
  const { lock } = useSession();
  const navigate = useNavigate();
  const link = ({ isActive }: { isActive: boolean }) => `nav-link ${isActive ? 'active' : ''}`;
  return (
    <header className="topbar">
      <NavLink to="/" className="topbar-logo" aria-label={t('nav.home')}>
        <Logo />
      </NavLink>
      <nav className="topbar-nav" aria-label={t('nav.mainNav')}>
        <NavLink to="/" end className={link}>
          {t('nav.home')}
        </NavLink>
        <NavLink to="/movies" className={link}>
          {t('nav.movies')}
        </NavLink>
        <NavLink to="/shows" className={link}>
          {t('nav.shows')}
        </NavLink>
        <NavLink to="/search" className={link}>
          <Icon name="search" size={18} />
          <span>{t('nav.search')}</span>
        </NavLink>
      </nav>
      <div className="topbar-end">
        <button type="button" className="btn btn-primary" onClick={() => navigate('/add')}>
          <Icon name="plus" size={20} />
          <span>{t('nav.add')}</span>
        </button>
        <IconButton icon="settings" label={t('nav.settings')} onClick={() => navigate('/settings')} />
        <IconButton icon="lock" label={t('nav.lock')} onClick={() => void lock()} />
      </div>
    </header>
  );
}
