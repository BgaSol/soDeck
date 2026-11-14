import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';

const files = new Map([
  ['/', ['index.html', 'text/html']], ['/index.html', ['index.html', 'text/html']],
  ['/style.css', ['style.css', 'text/css']], ['/app.js', ['app.js', 'text/javascript']],
  ['/data.js', ['data.js', 'text/javascript']],
  ['/connection-tree.js', ['connection-tree.js', 'text/javascript']],
]);
export function createPrototypeServer() {
  return createServer(async (request, response) => {
    const file = files.get(new URL(request.url, 'http://localhost').pathname);
    if (!file || request.method !== 'GET') { response.writeHead(404).end(); return; }
    try {
      const body = await readFile(new URL(file[0], import.meta.url));
      response.writeHead(200, { 'Content-Type': file[1] + '; charset=utf-8', 'Cache-Control': 'no-store' });
      response.end(body);
    } catch { response.writeHead(500).end('Unable to read prototype asset'); }
  });
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const port = Number(process.argv[2] || 1460);
  if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Invalid port');
  const server = createPrototypeServer();
  server.on('error', error => { console.error(error.message); process.exitCode = 1; });
  server.listen(port, '127.0.0.1', () => console.log('SoDeck prototype: http://127.0.0.1:' + port));
}
