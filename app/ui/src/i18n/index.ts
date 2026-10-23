import i18n from 'i18next';
import { initReactI18next } from 'react-i18next';

import ar from './ar.json';
import en from './en.json';
import he from './he.json';
import type { Language } from '../api/types';

export const LANGUAGES: { code: Language; native: string }[] = [
  { code: 'en', native: 'English' },
  { code: 'he', native: 'עברית' },
  { code: 'ar', native: 'العربية' },
];

const RTL = new Set<Language>(['he', 'ar']);

export function isRtl(lang: string): boolean {
  return RTL.has(lang as Language);
}

/** The UI language to use when settings don't pick one. */
export function systemLanguage(): Language {
  const fromQuery = new URLSearchParams(window.location.search).get('lang');
  if (fromQuery === 'en' || fromQuery === 'he' || fromQuery === 'ar') return fromQuery;
  for (const l of navigator.languages ?? [navigator.language]) {
    const base = l.toLowerCase().split('-')[0];
    if (base === 'he' || base === 'iw') return 'he';
    if (base === 'ar') return 'ar';
    if (base === 'en') return 'en';
  }
  return 'en';
}

/** Switches the UI language and the document's `lang` and `dir`. */
export function applyLanguage(lang: Language): void {
  void i18n.changeLanguage(lang);
  document.documentElement.lang = lang;
  document.documentElement.dir = isRtl(lang) ? 'rtl' : 'ltr';
}

void i18n.use(initReactI18next).init({
  resources: { en: { translation: en }, he: { translation: he }, ar: { translation: ar } },
  lng: 'en',
  fallbackLng: 'en',
  interpolation: { escapeValue: false },
  returnNull: false,
});

export default i18n;
