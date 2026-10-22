import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { checkVersion, parseVersion } from "./release-version.mjs";
import { writeUpdateManifest } from "./update-assets.mjs";
import { writeChecksums } from "./release-assets.mjs";
import { readReleaseNotes } from "./release-notes.mjs";

export async function publishRelease({
  github,
  context,
  core,
  directory = ".qa/release-assets",
  releaseNotesText,
  macosNotarized = process.env.SODECK_MACOS_NOTARIZED === "true",
  probeManifest = fetchStableManifest,
  publicKey = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"))
    .plugins.updater.pubkey,
}) {
  if (!context.ref.startsWith("refs/tags/"))
    throw new Error("Release requires a tag ref.");
  const tag = context.ref.slice("refs/tags/".length);
  const version = checkVersion(process.cwd(), tag);
  const notes = (releaseNotesText ?? readReleaseNotes(version)).trim();
  if (!notes)
    throw new Error("Missing user-facing release notes for " + version + ".");
  let names;
  const repo = context.repo;
  const { data: commit } = await github.rest.repos.getCommit({
    ...repo,
    ref: tag,
  });
  if (commit.sha !== context.sha)
    throw new Error(
      "The tag moved after this workflow started. Refusing to publish.",
    );

  let release;
  try {
    ({ data: release } = await github.rest.repos.getReleaseByTag({
      ...repo,
      tag,
    }));
  } catch (error) {
    if (error.status !== 404) throw error;
  }
  if (release && (!release.draft || release.target_commitish !== context.sha)) {
    throw new Error(
      "An existing published release or a draft for another commit cannot be overwritten. Use a new version.",
    );
  }
  const releases = await github.paginate(github.rest.repos.listReleases, {
    ...repo,
    per_page: 100,
  });
  const publishedStable = releases
    .filter((item) => !item.draft && !item.prerelease)
    .map((item) => /^v(\d+\.\d+\.\d+)$/.exec(item.tag_name ?? "")?.[1])
    .filter(Boolean);
  const highest = publishedStable.sort((a, b) => compareVersions(b, a))[0];
  if (highest && compareVersions(version, highest) <= 0)
    throw new Error(
      "Stable release must be newer than every published stable release (highest: v" +
        highest +
        ").",
    );
  const body = [
    notes,
    "",
    "下载对应系统的安装包：Windows x64（EXE / MSI）、macOS Apple Silicon、Linux x64（DEB / AppImage）。",
    "",
    "本版本未配置 Windows 商业代码签名。",
    macosNotarized
      ? "macOS 包使用 Apple Developer ID 签名并完成公证。"
      : "macOS 包使用 ad-hoc 签名，尚未配置 Developer ID 签名及公证；请手动安装。",
    "Linux 包在 Ubuntu 24.04 构建；建议在 Ubuntu 24.04 或更新的兼容系统运行。AppImage 下载后需添加执行权限。",
    "附件 SHA256SUMS.txt 可用于核验文件完整性。",
    "",
    "构建提交：" + context.sha,
  ].join("\n");
  if (!release) {
    ({ data: release } = await github.rest.repos.createRelease({
      ...repo,
      tag_name: tag,
      target_commitish: context.sha,
      name: "SoDeck " + tag,
      body,
      draft: true,
      prerelease: false,
    }));
  }

  if (release.body !== body) {
    ({ data: release } = await github.rest.repos.updateRelease({
      ...repo,
      release_id: release.id,
      name: "SoDeck " + tag,
      body,
      draft: true,
    }));
  }

  writeUpdateManifest(
    directory,
    version,
    repo,
    notes,
    new Date().toISOString(),
    publicKey,
  );
  names = writeChecksums(directory, version, true);

  const existing = await github.paginate(github.rest.repos.listReleaseAssets, {
    ...repo,
    release_id: release.id,
    per_page: 100,
  });
  if (existing.some((asset) => !names.includes(asset.name))) {
    throw new Error(
      "The draft contains unmanaged assets; refusing to change it.",
    );
  }
  const expected = new Map();
  for (const name of names) {
    const data = readFileSync(join(directory, name));
    const digest = "sha256:" + createHash("sha256").update(data).digest("hex");
    expected.set(name, { size: data.length, digest });
    const previous = existing.find((asset) => asset.name === name);
    if (previous)
      await github.rest.repos.deleteReleaseAsset({
        ...repo,
        asset_id: previous.id,
      });
    await github.rest.repos.uploadReleaseAsset({
      ...repo,
      release_id: release.id,
      name,
      data,
      headers: {
        "content-type": "application/octet-stream",
        "content-length": data.length,
      },
    });
    core.info("Uploaded " + name);
  }
  const uploaded = await github.paginate(github.rest.repos.listReleaseAssets, {
    ...repo,
    release_id: release.id,
    per_page: 100,
  });
  if (
    uploaded.length !== names.length ||
    uploaded.some((asset) => {
      const file = expected.get(asset.name);
      return (
        !file ||
        asset.state !== "uploaded" ||
        asset.size !== file.size ||
        (asset.digest && asset.digest !== file.digest)
      );
    })
  )
    throw new Error(
      "Release assets failed verification. The release remains a draft.",
    );

  const { data: published } = await github.rest.repos.updateRelease({
    ...repo,
    release_id: release.id,
    draft: false,
    make_latest: "true",
  });
  try {
    const expectedManifest = JSON.parse(
      readFileSync(join(directory, "latest.json"), "utf8"),
    );
    const remoteManifest = await probeManifest(expectedManifest, version);
    assertStableManifest(remoteManifest, version, expectedManifest);
  } catch (error) {
    throw new Error(
      "Release v" +
        version +
        " is public, but the stable updater manifest probe failed and needs remediation: " +
        error.message,
    );
  }
  await core.summary
    .addHeading("SoDeck " + tag)
    .addLink("Download release", published.html_url)
    .addList(names)
    .write();
  core.info("Published " + published.html_url);
}

function compareVersions(a, b) {
  const left = parseVersion(a).split(".").map(Number);
  const right = parseVersion(b).split(".").map(Number);
  for (let index = 0; index < 3; index++) {
    if (left[index] !== right[index])
      return left[index] < right[index] ? -1 : 1;
  }
  return 0;
}

async function fetchStableManifest() {
  const response = await fetch(
    "https://github.com/BgaSol/soDeck/releases/latest/download/latest.json",
    {
      signal: AbortSignal.timeout(30_000),
      headers: { accept: "application/json" },
    },
  );
  if (!response.ok) throw new Error("HTTP " + response.status);
  return response.json();
}

function assertStableManifest(actual, version, expected) {
  if (actual?.version !== version || actual?.sodeck?.schemaVersion !== 1)
    throw new Error(
      "latest.json does not point to the published version or supported schema",
    );
  if (
    JSON.stringify(actual.sodeck.manualAssets) !==
    JSON.stringify(expected.sodeck.manualAssets)
  )
    throw new Error("latest.json manual assets do not match the release");
  for (const [target, artifact] of Object.entries(expected.platforms)) {
    if (
      actual.platforms?.[target]?.url !== artifact.url ||
      actual.platforms?.[target]?.signature !== artifact.signature
    )
      throw new Error("latest.json updater target does not match " + target);
  }
}
