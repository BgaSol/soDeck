import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

export function checkMarkdown(file, source, root = process.cwd()) {
  const errors = [];
  if (!source.endsWith('\n')) errors.push(`${file}: missing final newline`);
  if (/^(<{7}|={7}|>{7})( |$)/m.test(source)) errors.push(`${file}: unresolved merge marker`);
  // Check file destinations, not remote URLs or heading fragments; no network access needed.
  const prose = source.replace(/^([ \t]*)(`{3,}|~{3,})[^\n]*\n[\s\S]*?^\1\2[^\n]*(?:\n|$)/gm, '').replace(/`[^`\n]+`/g, '');
  const links = [...prose.matchAll(/!?\[[^\]\n]*\]\((<[^>\n]+>|[^\s)\n]+)(?:\s+["'][^\n]*?["'])?\)/g),
    ...prose.matchAll(/^\s*\[[^\]\n]+\]:\s*(<[^>\n]+>|[^\s\n]+)/gm)];
  for (const match of links) {
    const href = match[1].replace(/^<|>$/g, '');
    if (/^(?:[a-z][a-z\d+.-]*:|\/\/|#)/i.test(href)) continue;
    let destination;
    try { destination = decodeURIComponent(href.split(/[?#]/)[0]); }
    catch { errors.push(`${file}: invalid link encoding: ${href}`); continue; }
    if (!destination) continue;
    const target = destination.startsWith('/') ? path.resolve(root, `.${destination}`) : path.resolve(root, path.dirname(file), destination);
    if (!fs.existsSync(target)) errors.push(`${file}: missing local link target: ${href}`);
  }
  return errors;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  // Checking all tracked Markdown also catches links left behind by deletions/renames.
  const files = execFileSync('git', ['ls-files', '-z'], { encoding: 'utf8', windowsHide: true }).split('\0').filter(f => f.endsWith('.md'));
  const errors = files.flatMap(file => checkMarkdown(file, fs.readFileSync(file, 'utf8')));
  if (errors.length) throw new Error(errors.join('\n'));
  console.log(`Checked ${files.length} tracked Markdown files: final newlines, merge markers, local file links (heading fragments and remote URLs excluded).`);
}
