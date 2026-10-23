import '@fontsource/rubik/400.css';
import '@fontsource/rubik/500.css';
import '@fontsource/rubik/600.css';
import '@fontsource/rubik/700.css';
import '@fontsource/noto-sans-hebrew/400.css';
import '@fontsource/noto-sans-hebrew/600.css';
import '@fontsource/noto-sans-arabic/400.css';
import '@fontsource/noto-sans-arabic/500.css';
import '@fontsource/noto-sans-arabic/600.css';
import '@fontsource/noto-sans-arabic/700.css';

import './styles/tokens.css';
import './styles/base.css';
import './styles/components.css';
import './styles/screens.css';
import './styles/player.css';

import './i18n';

import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';

import { App } from './App';

// No browser context menu (Inspect, Save image...) outside text fields.
window.addEventListener('contextmenu', (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest('input, textarea')) e.preventDefault();
});

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
