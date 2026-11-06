import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

// Cargo owns TOML parsing and workspace inheritance. Select the package by its
// manifest, since metadata may list other workspace members before this one.
export function cargoPackageVersion(manifestPath) {
  const manifest = fs.realpathSync(path.resolve(manifestPath));
  const result = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps', '--locked', '--manifest-path', manifest], {
    cwd: path.dirname(manifest), encoding: 'utf8', windowsHide: true, maxBuffer: 4 * 1024 * 1024,
  });
  if (result.error || result.status !== 0) {
    const reason = result.error?.message || result.stderr?.trim() || `exit ${result.status}`;
    throw new Error(`Cargo metadata failed for ${manifest}: ${reason}`);
  }
  let metadata;
  try { metadata = JSON.parse(result.stdout); }
  catch { throw new Error(`Invalid Cargo metadata for ${manifest}`); }
  if (!Array.isArray(metadata?.packages)) throw new Error(`Invalid Cargo metadata for ${manifest}`);
  const identity = value => {
    const canonical = fs.realpathSync(value);
    return process.platform === 'win32' ? canonical.toLowerCase() : canonical;
  };
  const requested = identity(manifest);
  const packages = metadata.packages.filter(pkg => typeof pkg?.manifest_path === 'string' && identity(pkg.manifest_path) === requested);
  if (packages.length !== 1) throw new Error(`Cargo metadata must contain exactly one package for ${manifest}`);
  const { version } = packages[0];
  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`Plugin releases require a stable package version: ${manifest}`);
  return version;
}
