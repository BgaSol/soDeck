import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export function configureUbuntuMirror(directory = "/etc/apt") {
  if (!path.isAbsolute(directory)) throw new Error("Use an absolute APT configuration directory");
  const sources = path.join(directory, "sources.list.d");
  const files = [path.join(directory, "sources.list"), ...(fs.existsSync(sources)
    ? fs.readdirSync(sources).filter(name => /\.(?:list|sources)$/.test(name)).map(name => path.join(sources, name)) : [])];
  const changed = [];
  for (const file of files) {
    if (!fs.existsSync(file) || !fs.statSync(file).isFile()) continue;
    const original = fs.readFileSync(file, "utf8");
    const configured = original.replace(/https?:\/\/azure\.archive\.ubuntu\.com\/ubuntu/g, "https://archive.ubuntu.com/ubuntu");
    if (configured !== original) { fs.writeFileSync(file, configured); changed.push(file); }
  }
  return changed;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  console.log(`Configured official Ubuntu mirror in ${configureUbuntuMirror().length} APT source files.`);
}
