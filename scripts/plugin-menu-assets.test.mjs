import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import vm from 'node:vm';
import { test } from 'node:test';
import { packageMenuAssets } from './plugin-menu-assets.mjs';

test('standalone plugin menu asset loads without npm or CDN and preserves plugin UI', async () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-menu-assets-'));
  try {
    fs.mkdirSync(path.join(directory, 'ui'));
    const entry = path.join(directory, 'ui', 'index.html'); fs.writeFileSync(entry, '<main>plugin</main>');
    await packageMenuAssets(directory);
    assert.equal(fs.readFileSync(entry, 'utf8'), '<main>plugin</main>');
    const window = {};
    vm.runInNewContext(fs.readFileSync(path.join(directory, 'ui', 'text-menu.js'), 'utf8'), { window });
    assert.equal(typeof window.installTextMenu, 'function');
    assert.equal(typeof window.SoDeckMenuPosition.attachMenuPosition, 'function');
    for (const name of ['dom', 'core', 'utils']) assert.match(fs.readFileSync(path.join(directory, 'ui', 'licenses', `floating-ui-${name}.LICENSE`), 'utf8'), /MIT License/);
  } finally {
    const target = path.resolve(directory);
    assert.equal(path.dirname(target), path.resolve(os.tmpdir()));
    assert.ok(path.basename(target).startsWith('sodeck-menu-assets-'));
    fs.rmSync(target, { recursive: true, force: true });
  }
});
