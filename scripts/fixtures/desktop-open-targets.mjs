import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';

export function assertDelivered(lines, url, directory) {
  assert.equal(lines.length, 2, 'Deliver one URL and one directory');
  assert(lines.every(line => /^(?:[0-9a-f]{2})+$/.test(line)), 'Native handler must record complete bytes');
  const encodedUrl = Buffer.from(url).toString('hex');
  assert.equal(lines.filter(line => line === encodedUrl).length, 1, 'Deliver the exact URL once');
  const received = Buffer.from(lines.find(line => line !== encodedUrl), 'hex').toString('utf8');
  const expectedInfo = fs.statSync(directory, {bigint:true});
  const receivedInfo = fs.statSync(received, {bigint:true});
  assert(receivedInfo.isDirectory());
  // Windows Shell expands 8.3 prefixes; compare the actual directory and its native filename.
  assert.equal(receivedInfo.dev, expectedInfo.dev);
  assert.equal(receivedInfo.ino, expectedInfo.ino, 'Deliver the selected directory');
  assert.equal(path.basename(received), path.basename(directory), 'Preserve the native directory name');
}
