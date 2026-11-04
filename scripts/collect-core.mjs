import fs from "node:fs";
import path from "node:path";
const target = process.argv[2];
if (!/^(x86_64|aarch64)-(pc-windows-msvc|unknown-linux-gnu|apple-darwin)$/.test(target ?? "")) throw new Error("Invalid target");
const { version } = JSON.parse(fs.readFileSync("src-tauri/tauri.conf.json", "utf8"));
const root = `src-tauri/target/${target}/release/bundle`;
const output = `artifacts/core/${target}`;
fs.mkdirSync(output, { recursive: true });
let count = 0;
for (const kind of ["nsis", "deb", "dmg", "macos", "appimage"]) {
  const directory = path.join(root, kind);
  if (!fs.existsSync(directory)) continue;
  for (const file of fs.readdirSync(directory)) {
    if (!/\.(exe|deb|dmg|AppImage|app\.tar\.gz)(\.sig)?$/.test(file)) continue;
    // tauri-action versions macOS updater asset names when uploading to Releases.
    const name = kind === "macos" ? file.replace(/\.app\.tar\.gz(\.sig)?$/, `_${version}_${target.split("-")[0]}.app.tar.gz$1`) : file;
    fs.copyFileSync(path.join(directory, file), path.join(output, name)); count++;
  }
}
if (!count) throw new Error("No core packages found");
