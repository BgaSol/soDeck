import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { cargoPackageVersion } from './cargo-package.mjs';
import { pluginConfig } from './release-metadata.mjs';

function fixture(t, manifest, workspace = '[workspace]\nmembers = ["plugins/sample", "a-dependency"]\nresolver = "2"\n') {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-cargo-test-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, 'Cargo.toml'), workspace);
  for (const [directory, source] of [['plugins/sample', manifest], ['a-dependency', '[package]\nname = "fixture-dependency"\nversion = "9.8.7"\nedition = "2021"\n']]) {
    fs.mkdirSync(path.join(root, directory, 'src'), { recursive: true });
    fs.writeFileSync(path.join(root, directory, 'Cargo.toml'), source);
    fs.writeFileSync(path.join(root, directory, 'src/lib.rs'), '');
  }
  const directory = path.join(root, 'plugins/sample');
  fs.writeFileSync(path.join(directory, 'release.json'), JSON.stringify({ id: 'test.sample', binary: 'different-binary-name', tagPrefix: 'plugin-sample-v' }));
  fs.writeFileSync(path.join(directory, 'CHANGELOG.md'), 'fixture notes');
  return root;
}

for (const version of ['version="1.2.3"', "version = '1.2.3'"]) {
  test(`plugin metadata accepts legal TOML: ${version}`, t => {
    const root = fixture(t, `[package]\nname = "z-plugin"\n${version}\nedition = "2021"\n`);
    const config = pluginConfig('sample', root);
    assert.equal(config.version, '1.2.3');
    assert.equal(config.tag, 'plugin-sample-v1.2.3');
  });
}

test('plugin version belongs to the requested manifest, regardless of table or workspace package order', t => {
  const root = fixture(t, '[dependencies.fixture-dependency]\nversion = "9.8.7"\npath = "../../a-dependency"\n[package]\nname = "z-plugin"\nversion = "1.2.3"\nedition = "2021"\n');
  assert.equal(pluginConfig('sample', root).version, '1.2.3');
});

test('Cargo resolves an inherited package version from its workspace', t => {
  const root = fixture(t, '[package]\nname = "z-plugin"\nversion.workspace = true\nedition = "2021"\n', '[workspace]\nmembers = ["plugins/sample", "a-dependency"]\nresolver = "2"\n[workspace.package]\nversion = "1.2.3"\n');
  assert.equal(pluginConfig('sample', root).version, '1.2.3');
});

test('invalid Cargo manifests fail instead of falling back to a version-looking line', t => {
  const root = fixture(t, '[package]\nname = "z-plugin"\nversion = "1.2.3"\ninvalid TOML\n');
  assert.throws(() => pluginConfig('sample', root), /Cargo metadata failed/);
});

test('a virtual workspace manifest cannot select the first member package', t => {
  const root = fixture(t, '[package]\nname = "z-plugin"\nversion = "1.2.3"\nedition = "2021"\n');
  assert.throws(() => cargoPackageVersion(path.join(root, 'Cargo.toml')), /exactly one package/);
});

test('missing Cargo reports the tool failure instead of guessing a version', t => {
  const root = fixture(t, '[package]\nname = "z-plugin"\nversion = "1.2.3"\nedition = "2021"\n');
  const source = `import { pluginConfig } from ${JSON.stringify(new URL('./release-metadata.mjs', import.meta.url).href)}; pluginConfig('sample', process.argv[1]);`;
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'));
  env.PATH = root;
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', source, root], { encoding: 'utf8', env, windowsHide: true });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /Cargo metadata failed.*spawnSync cargo ENOENT/);
});
