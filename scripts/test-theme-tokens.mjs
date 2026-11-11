import fs from 'node:fs';
import { themeTokens } from '../src/theme.ts';

export async function readTestThemeTokens(browser) {
  const page = await browser.newPage();
  try {
    await page.addStyleTag({ content: '@layer base, components, themes;' });
    for (const file of ['ui.css', 'themes.css']) {
      await page.addStyleTag({ content: fs.readFileSync(new URL(`../src/${file}`, import.meta.url), 'utf8') });
    }
    const snapshots = {};
    for (const theme of ['light', 'dark', 'dopamine']) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      snapshots[theme] = await page.evaluate(themeTokens);
    }
    return snapshots;
  } finally { await page.close(); }
}
