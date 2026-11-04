import fs from "node:fs";
import path from "node:path";
const target = process.argv[2];
if (!/^(x86_64|aarch64)-(pc-windows-msvc|unknown-linux-gnu|apple-darwin)$/.test(target ?? "")) throw new Error("Invalid target");
const root = `src-tauri/target/${target}/release/bundle`;
const output = `artifacts/core/${target}`;
fs.mkdirSync(output, { recursive: true });
let count = 0;
for (const kind of ["nsis", "deb", "dmg", "macos", "appimage"]) {
  const directory = path.join(root, kind);
  if (!fs.existsSync(directory)) continue;
  for (const file of fs.readdirSync(directory)) {
    if (!/\.(exe|deb|dmg|AppImage|app\.tar\.gz|sig)$/.test(file)) continue;
    fs.copyFileSync(path.join(directory, file), path.join(output, file)); count++;
  }
}
if (!count) throw new Error("No core packages found");
