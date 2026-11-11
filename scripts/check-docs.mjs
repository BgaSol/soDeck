import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import MarkdownIt from 'markdown-it';

const markdown = new MarkdownIt({ html: false, linkify: false });
// Preserve malformed percent escapes for the existing path validation.
markdown.normalizeLink = href => href;

function markdownDestinations(source) {
  const env = {};
  const tokens = markdown.parse(source, env);
  const links = tokens.flatMap(token => token.children ?? []).flatMap(token => {
    if (token.type === 'link_open') return [token.attrGet('href')];
    if (token.type === 'image') return [token.attrGet('src')];
    return [];
  });
  return new Set([...links, ...Object.values(env.references ?? {}).map(ref => ref.href)]);
}

export function checkMarkdown(file, source, root = process.cwd()) {
  const errors = [];
  if (!source.endsWith('\n')) errors.push(`${file}: missing final newline`);
  if (/^(<{7}|={7}|>{7})( |$)/m.test(source)) errors.push(`${file}: unresolved merge marker`);
  // Check file destinations, not remote URLs or heading fragments; no network access needed.
  for (const href of markdownDestinations(source)) {
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
