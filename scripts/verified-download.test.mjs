import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { Writable } from 'node:stream';
import { spawnSync } from 'node:child_process';
import { writeVerifiedDownload, nativeGithub } from './native-github.mjs';
import { download } from './test-release-upgrade.mjs';

const bytes = Buffer.from('complete 中文 package');
const expected = { size: bytes.length, sha256: createHash('sha256').update(bytes).digest('hex') };
function directory(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-verified-download-'));
  t.after(() => { assert.equal(path.dirname(root), os.tmpdir()); fs.rmSync(root, { recursive: true, force: true }); });
  return root;
}

test('verified writer streams multiple chunks before replacing an existing final file', async t => {
  const file = path.join(directory(t), 'package.bin');
  fs.writeFileSync(file, 'previous package');
  const chunks = [bytes.subarray(0, 3), bytes.subarray(3, 9), bytes.subarray(9)];
  const response = new Response(new ReadableStream({ pull(controller) {
    chunks.length ? controller.enqueue(chunks.shift()) : controller.close();
  } }));
  response.arrayBuffer = () => { throw new Error('Whole-body buffering is forbidden'); };
  await writeVerifiedDownload(response, file, expected);
  assert.deepEqual(fs.readFileSync(file), bytes);
  assert.equal(fs.existsSync(`${file}.part`), false);
});

test('verified writer rejects truncated, corrupt and excess bytes without changing the final file', async t => {
  const file = path.join(directory(t), 'package.bin');
  fs.writeFileSync(file, 'previous package');
  for (const body of [bytes.subarray(0, 4), Buffer.alloc(bytes.length), Buffer.concat([bytes, bytes])]) {
    await assert.rejects(writeVerifiedDownload(new Response(body), file, expected), /mismatch|exceeds/);
    assert.equal(fs.readFileSync(file, 'utf8'), 'previous package');
    assert.equal(fs.existsSync(`${file}.part`), false);
  }
});

test('verified writer cancels invalid metadata and a response whose destination cannot be created', async t => {
  const root = directory(t), parent = path.join(root, 'not-a-directory');
  fs.writeFileSync(parent, 'previous package');
  for (const [file, metadata] of [
    [path.join(root, 'package.bin'), { ...expected, size: 0 }],
    [path.join(root, 'package.bin'), { ...expected, size: Number.MAX_SAFE_INTEGER + 1 }],
    [path.join(root, 'package.bin'), { ...expected, sha256: expected.sha256.toUpperCase() }],
    [path.join(parent, 'package.bin'), expected],
  ]) {
    let cancelled = false;
    const response = new Response(new ReadableStream({ cancel() { cancelled = true; } }));
    await assert.rejects(writeVerifiedDownload(response, file, metadata));
    assert(cancelled, 'An unused response must release its network body');
    assert.equal(fs.existsSync(`${file}.part`), false);
  }
  assert.equal(fs.readFileSync(parent, 'utf8'), 'previous package');
});

test('verified writer cleans partial files after a source or output stream error', async t => {
  const file = path.join(directory(t), 'package.bin');
  fs.writeFileSync(file, 'previous package');
  let sent = false;
  const broken = new Response(new ReadableStream({ pull(controller) {
    if (!sent) { sent = true; controller.enqueue(bytes.subarray(0, 4)); }
    else controller.error(new Error('source failed'));
  } }));
  await assert.rejects(writeVerifiedDownload(broken, file, expected), /source failed/);
  assert.equal(fs.existsSync(`${file}.part`), false);
  t.mock.method(fs, 'createWriteStream', () => new Writable({ write(_chunk, _encoding, callback) { callback(new Error('output failed')); } }));
  await assert.rejects(writeVerifiedDownload(new Response(bytes), file, expected), /output failed/);
  assert.equal(fs.readFileSync(file, 'utf8'), 'previous package');
  assert.equal(fs.existsSync(`${file}.part`), false);
});

test('verified writer aborts a stalled body and reports partial cleanup failures', async t => {
  const file = path.join(directory(t), 'package.bin');
  fs.writeFileSync(file, 'previous package');
  let cancelled = false;
  const controller = new AbortController();
  const stalled = new Response(new ReadableStream({ start(stream) { stream.enqueue(bytes.subarray(0, 4)); }, cancel() { cancelled = true; } }));
  const timer = setTimeout(() => controller.abort(), 20);
  try { await assert.rejects(writeVerifiedDownload(stalled, file, expected, { signal: controller.signal }), { name: 'AbortError' }); }
  finally { clearTimeout(timer); }
  assert(cancelled);
  assert.equal(fs.readFileSync(file, 'utf8'), 'previous package');
  assert.equal(fs.existsSync(`${file}.part`), false);
  const remove = fs.rmSync;
  t.mock.method(fs, 'rmSync', (target, options) => {
    if (target === `${file}.part`) throw new Error('partial cleanup failed');
    return remove(target, options);
  });
  await assert.rejects(writeVerifiedDownload(new Response(bytes.subarray(0, 4)), file, expected), /partial cleanup failed/);
  assert.equal(fs.readFileSync(file, 'utf8'), 'previous package');
  t.mock.restoreAll();
  fs.rmSync(`${file}.part`, { force: true });
});

test('published upgrade downloads remain anonymous and use one deadline for fetch and body', async t => {
  const root = directory(t), asset = { name: '包 with spaces.bin', size: expected.size, digest: `sha256:${expected.sha256}`, browser_download_url: 'https://example.test/exact-package' };
  let received;
  const file = await download(asset, root, async (url, options) => {
    received = { url, options };
    assert.equal(new Headers(options.headers).has('Authorization'), false);
    assert(options.signal instanceof AbortSignal);
    return new Response(bytes);
  });
  assert.equal(received.url, asset.browser_download_url);
  assert.deepEqual(fs.readFileSync(file), bytes);
  fs.rmSync(file);
  const controller = new AbortController();
  t.mock.method(AbortSignal, 'timeout', milliseconds => { assert.equal(milliseconds, 120_000); return controller.signal; });
  let cancelled = false;
  const timer = setTimeout(() => controller.abort(), 20);
  try {
    await assert.rejects(download(asset, root, async (_url, options) => {
      assert.equal(options.signal, controller.signal);
      return new Response(new ReadableStream({ cancel() { cancelled = true; } }));
    }), { name: 'AbortError' });
  } finally { clearTimeout(timer); }
  assert(cancelled, 'The fetch deadline must also stop a stalled response body');
  assert.equal(fs.existsSync(file), false);
  assert.equal(fs.existsSync(`${file}.part`), false);
});

test('published upgrade HTTP failures release the response without creating a file', async t => {
  const root = directory(t), asset = { name: 'package.bin', size: expected.size, digest: `sha256:${expected.sha256}`, browser_download_url: 'https://example.test/package' };
  let cancelled = false;
  await assert.rejects(download(asset, root, async () => new Response(new ReadableStream({ cancel() { cancelled = true; } }), { status: 503 })), /503/);
  assert(cancelled);
  assert.deepEqual(fs.readdirSync(root), []);
});

test('importing published upgrade helpers performs no environment parsing or preparation', t => {
  const root = directory(t);
  const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => key.toLowerCase() !== 'path'));
  env.PATH = root; env.TMP = root; env.TEMP = root; env.SODECK_UPGRADE_TARGET = 'invalid';
  const result = spawnSync(process.execPath, ['--input-type=module', '-e', `await import(${JSON.stringify(new URL('./test-release-upgrade.mjs', import.meta.url).href)});`], {
    cwd: root, env, windowsHide: true, encoding: 'utf8',
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout, '');
  assert.deepEqual(fs.readdirSync(root), []);
  assert.throws(() => nativeGithub('test/repo', ''), /token/);
});
