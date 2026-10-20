import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { checkVersion } from "./release-version.mjs";
import { writeUpdateManifest } from "./update-assets.mjs";
import { writeChecksums } from "./release-assets.mjs";

export async function publishRelease({
  github,
  context,
  core,
  directory = ".qa/release-assets",
  publicKey = JSON.parse(readFileSync("src-tauri/tauri.conf.json", "utf8"))
    .plugins.updater.pubkey,
}) {
  if (!context.ref.startsWith("refs/tags/"))
    throw new Error("Release requires a tag ref.");
  const tag = context.ref.slice("refs/tags/".length);
  const version = checkVersion(process.cwd(), tag);
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
  try {
    const { data: latest } = await github.rest.repos.getLatestRelease(repo);
    const previous = /^v(\d+)\.(\d+)\.(\d+)$/.exec(latest.tag_name ?? "");
    if (previous) {
      const a = version.split(".").map(Number),
        b = previous.slice(1).map(Number);
      const index = a.findIndex((v, i) => v !== b[i]);
      if (index < 0 || a[index] < b[index])
        throw new Error(
          "Stable release must be newer than the published latest version.",
        );
    }
  } catch (e) {
    if (e.status !== 404) throw e;
  }
  if (!release) {
    const { data: notes } = await github.rest.repos.generateReleaseNotes({
      ...repo,
      tag_name: tag,
      target_commitish: context.sha,
    });
    const body = [
      "SoDeck " + tag,
      "",
      "下载对应系统的安装包：Windows x64（EXE / MSI）、macOS（Apple Silicon / Intel）、Linux x64（DEB / AppImage）。",
      "",
      "本版本未配置 Windows 商业代码签名及 Apple Developer ID 签名/公证。macOS 包使用 ad-hoc 签名，首次安装可能需要在系统设置中允许打开。",
      "Linux 包在 Ubuntu 24.04 构建；建议在 Ubuntu 24.04 或更新的兼容系统运行。AppImage 下载后需添加执行权限。",
      "附件 SHA256SUMS.txt 可用于核验文件完整性。",
      "",
      "构建提交：" + context.sha,
      "",
      notes.body,
    ].join("\n");
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

  writeUpdateManifest(
    directory,
    version,
    repo,
    release.body ?? "",
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
  await core.summary
    .addHeading("SoDeck " + tag)
    .addLink("Download release", published.html_url)
    .addList(names)
    .write();
  core.info("Published " + published.html_url);
}
