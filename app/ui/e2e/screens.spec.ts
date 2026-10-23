// Every screen in he and ar, plus a few in en for reference, compared with the
// baselines in e2e/__screenshots__. Update them with `npm run e2e:update`
// after an intended UI change, and look at the new images before committing.

import { expect, test, type Page } from '@playwright/test';

type Lang = 'en' | 'he' | 'ar';
const LANGS: Lang[] = ['en', 'he', 'ar'];
const LOCALES: Record<Lang, string> = { en: 'en-US', he: 'he-IL', ar: 'ar-SA' };
// English is the source layout; a handful of screens keep the baselines small.
const EN_REFERENCE = new Set(['unlock', 'home', 'movie', 'show-episodes', 'player-subs', 'settings-security', 'add', 'wizard-recovery']);

// The player draws over mpv's video; stand in for it with a gradient.
const FAKE_VIDEO = 'html.player-active { background: radial-gradient(circle at 30% 40%, #3b5966, #101a20 70%) !important; }';

interface Screen {
  name: string;
  query?: string;
  hash: string;
  act?: (p: Page) => Promise<void>;
}

const SCREENS: Screen[] = [
  { name: 'unlock', query: 'locked=1', hash: '/' },
  { name: 'unlock-recovery', query: 'locked=1', hash: '/', act: (p) => p.locator('.link-btn').click() },
  {
    name: 'unlock-error',
    query: 'locked=1',
    hash: '/',
    act: async (p) => {
      await p.locator('#pw').fill('nope');
      await p.locator('.unlock-card .btn-primary').click();
      await p.waitForSelector('.field-error');
    },
  },
  { name: 'home', hash: '/' },
  { name: 'home-empty', query: 'empty=1', hash: '/' },
  { name: 'movies', hash: '/movies' },
  { name: 'shows', hash: '/shows' },
  { name: 'movie', hash: '/movie/1' },
  { name: 'movie-file', hash: '/movie/1', act: (p) => p.mouse.wheel(0, 800) },
  { name: 'show', hash: '/show/1' },
  { name: 'show-episodes', hash: '/show/1', act: (p) => p.mouse.wheel(0, 500) },
  { name: 'search', hash: '/search?q=fa' },
  { name: 'add', hash: '/add' },
  { name: 'review', hash: '/add', act: (p) => p.locator('.queue-actions .btn-outline').first().click() },
  { name: 'settings-playback', hash: '/settings/playback' },
  { name: 'settings-subtitles', hash: '/settings/subtitles' },
  { name: 'settings-interface', hash: '/settings/interface' },
  { name: 'settings-security', hash: '/settings/security' },
  { name: 'settings-vault', hash: '/settings/vault' },
  { name: 'settings-about', hash: '/settings/about' },
  { name: 'player', hash: '/play/118', act: (p) => p.mouse.move(400, 400) },
  { name: 'player-subs', hash: '/play/118', act: (p) => p.locator('.menu-anchor .ctl-btn').nth(1).click() },
  { name: 'player-next', query: 'pos=2675&slow=1', hash: '/play/118', act: (p) => p.mouse.move(400, 400) },
  { name: 'wizard-drive', query: 'wizard=1', hash: '/', act: (p) => p.locator('.radio-card').first().click() },
  {
    name: 'wizard-prepare',
    query: 'wizard=1',
    hash: '/',
    act: async (p) => {
      await p.locator('.radio-card').nth(1).click();
      await p.locator('.wizard-actions .btn-primary').click();
      await p.locator('.form-grid input').first().fill('KINGSTON');
    },
  },
  {
    name: 'wizard-password',
    query: 'wizard=1',
    hash: '/',
    act: async (p) => {
      await p.locator('.radio-card').first().click();
      await p.locator('.wizard-actions .btn-primary').click();
      await p.locator('.wizard-actions .btn-primary').click();
      await p.locator('input[type=password]').nth(0).fill('correct horse battery');
      await p.locator('input[type=password]').nth(1).fill('correct horse battery');
    },
  },
  {
    name: 'wizard-recovery',
    query: 'wizard=1',
    hash: '/',
    act: async (p) => {
      await p.locator('.radio-card').first().click();
      await p.locator('.wizard-actions .btn-primary').click();
      await p.locator('.wizard-actions .btn-primary').click();
      await p.locator('input[type=password]').nth(0).fill('correct horse battery');
      await p.locator('input[type=password]').nth(1).fill('correct horse battery');
      await p.locator('.wizard-actions .btn-primary').click();
      await p.waitForSelector('.word-grid', { timeout: 8000 });
    },
  },
];

for (const lang of LANGS) {
  test.describe(lang, () => {
    test.use({ locale: LOCALES[lang] });

    for (const s of SCREENS.filter((x) => lang !== 'en' || EN_REFERENCE.has(x.name))) {
      test(s.name, async ({ page }) => {
        const errors: string[] = [];
        page.on('pageerror', (e) => errors.push(String(e)));
        page.on('console', (m) => {
          if (m.type() === 'error') errors.push(m.text());
        });
        // Dates ("added 3 days ago", "created …") come out the same on every run.
        await page.clock.setFixedTime(new Date('2026-06-01T12:00:00Z'));
        const query = ['freeze=1', `lang=${lang}`, s.query].filter(Boolean).join('&');
        await page.goto(`./?${query}#${s.hash}`);
        await page.addStyleTag({ content: FAKE_VIDEO });
        await page.waitForLoadState('networkidle');
        await expect(page.locator('html')).toHaveAttribute('lang', lang);
        await expect(page.locator('html')).toHaveAttribute('dir', lang === 'en' ? 'ltr' : 'rtl');
        // Web fonts must be in before the picture is taken.
        await page.evaluate(() => document.fonts.ready.then(() => undefined));
        if (s.act) await s.act(page);
        await page.waitForTimeout(300);
        await expect(page).toHaveScreenshot(`${s.name}-${lang}.png`);
        expect(errors).toEqual([]);
      });
    }
  });
}
