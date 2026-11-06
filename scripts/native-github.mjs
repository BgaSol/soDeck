import fs from "node:fs";
import path from "node:path";
import { createHash } from "node:crypto";
import { Readable, Transform } from "node:stream";
import { pipeline } from "node:stream/promises";
import { setTimeout as delay } from "node:timers/promises";

export function nativeGithub(repository, token, request = fetch, wait = delay) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository ?? "") || !token) throw new Error("Provide a GitHub repository and token");
  const cache = new Map();
  async function response(route, accept, optional = false) {
    if (!route.startsWith("/")) throw new Error("Use a repository GitHub API route");
    for (let attempt = 0; ; attempt++) {
      let result;
      try {
        result = await request(`https://api.github.com/repos/${repository}${route}`, {
          headers: { Authorization: `Bearer ${token}`, Accept: accept, "X-GitHub-Api-Version": "2022-11-28" },
          signal: AbortSignal.timeout(120_000),
        });
      } catch {
        if (attempt >= 2) throw new Error(`GitHub GET ${route.split("?")[0]}: network request failed`);
        await wait(1000 * (attempt + 1)); continue;
      }
      if (optional && result.status === 404) return null;
      if (result.ok) return result;
      if (attempt < 2 && (result.status === 429 || result.status >= 500)) {
        const seconds = Number(result.headers.get("retry-after"));
        await result.body?.cancel();
        await wait(Math.min(10_000, Math.max(1000 * (attempt + 1), Number.isFinite(seconds) ? seconds * 1000 : 0)));
        continue;
      }
      await result.body?.cancel();
      throw new Error(`GitHub GET ${route.split("?")[0]} failed: HTTP ${result.status}`);
    }
  }
  const api = {
    repository,
    async get(route, optional = false) {
      // Source configuration is immutable by commit; live run/release state is not cached.
      const immutable = route.startsWith("/contents/");
      if (immutable && cache.has(route)) return cache.get(route);
      const result = await response(route, "application/vnd.github+json", optional);
      const value = result ? await result.json() : null;
      if (immutable) cache.set(route, value);
      return value;
    },
    async list(route, field) {
      const items = [];
      for (let page = 1; page <= 10; page++) {
        const value = await api.get(`${route}${route.includes("?") ? "&" : "?"}per_page=100&page=${page}`);
        const batch = field ? value[field] : value;
        if (!Array.isArray(batch)) throw new Error(`Invalid GitHub list: ${route}`);
        items.push(...batch);
        if (batch.length < 100) return items;
      }
      throw new Error(`GitHub pagination limit reached; query incomplete: ${route}`);
    },
    async text(file, commit) {
      const item = await api.get(`/contents/${file}?ref=${commit}`);
      if (item.encoding !== "base64" || item.size > 1024 * 1024) throw new Error(`Invalid source configuration: ${file}`);
      return Buffer.from(item.content, "base64").toString("utf8");
    },
    async bytes(route, expected, limit = 128 * 1024) {
      // Artifact ZIP endpoints require the GitHub API media type; Release assets
      // require octet-stream. Both redirect to the actual binary download.
      const result = await response(route, route.startsWith("/actions/artifacts/") ? "application/vnd.github+json" : "application/octet-stream");
      const chunks = []; let size = 0;
      for await (const chunk of Readable.fromWeb(result.body)) {
        size += chunk.length;
        if (size > limit) throw new Error("GitHub metadata exceeds its size limit");
        chunks.push(chunk);
      }
      const bytes = Buffer.concat(chunks);
      checkBytes(bytes.length, createHash("sha256").update(bytes).digest("hex"), expected);
      return bytes;
    },
    async download(route, file, expected) {
      fs.mkdirSync(path.dirname(file), { recursive: true });
      const partial = `${file}.part`;
      const result = await response(route, "application/octet-stream");
      const hash = createHash("sha256"); let size = 0;
      const count = new Transform({ transform(chunk, _encoding, callback) {
        size += chunk.length; hash.update(chunk);
        callback(size > expected.size ? new Error("Download exceeds expected size") : null, chunk);
      } });
      try {
        await pipeline(Readable.fromWeb(result.body), count, fs.createWriteStream(partial));
        checkBytes(size, hash.digest("hex"), expected);
        fs.renameSync(partial, file);
      } finally { fs.rmSync(partial, { force: true }); }
    },
  };
  return api;
}

export function checkBytes(size, hash, expected) {
  if (!Number.isSafeInteger(expected?.size) || expected.size <= 0 || !/^[a-f0-9]{64}$/.test(expected.sha256 ?? "") || size !== expected.size || hash !== expected.sha256) throw new Error("Downloaded file size or SHA-256 mismatch");
}

export function assetRecord(asset, name) {
  if (!Number.isSafeInteger(asset?.id) || asset.id <= 0 || asset.name !== name || asset.state !== "uploaded" || !Number.isSafeInteger(asset.size) || asset.size <= 0 || !/^sha256:[a-f0-9]{64}$/.test(asset.digest ?? "")) throw new Error(`Missing or invalid Release asset: ${name}`);
  return { id: asset.id, name, size: asset.size, sha256: asset.digest.slice(7) };
}
