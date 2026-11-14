import fs from 'node:fs';
import {tokenNames} from '../packages/ui/token-names.ts';
import { themeTokens } from '../src/theme.ts';

export async function readTestThemeTokens(browser) {
  const page = await browser.newPage();
  try {
    await page.addStyleTag({ content: '@layer base, components, themes;' });
    for (const file of ['shared.css', 'themes.css']) {
      await page.addStyleTag({ content: fs.readFileSync(new URL(`../packages/ui/${file}`, import.meta.url), 'utf8') });
    }
    const snapshots = {};
    for (const theme of ['light', 'dark', 'dopamine']) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      snapshots[theme] = await page.evaluate(themeTokens, tokenNames);
    }
    return snapshots;
  } finally { await page.close(); }
}
