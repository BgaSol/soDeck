import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { registerHooks } from 'node:module';

// Node strips TypeScript; resolve the frontend's extensionless local imports.
const sourceRoot = new URL('../src/', import.meta.url).href;
registerHooks({ resolve(specifier, context, nextResolve) {
  if (context.parentURL?.startsWith(sourceRoot) && specifier.startsWith('.') && !path.extname(specifier))
    return nextResolve(`${specifier}.ts`, context);
  return nextResolve(specifier, context);
} });
// The HTML helpers also register chrome listeners; these tests only render strings.
globalThis.document = { addEventListener() {} };
globalThis.window = { addEventListener() {} };
const { updateNotes } = await import('../src/updates.ts');
const render = notes => updateNotes('sodeck.core', { notes });

test('release notes render the published Markdown as headings, paragraphs, code and tables', () => {
  const markdown = fs.readFileSync(new URL('../docs/releases/v0.9.0.md', import.meta.url), 'utf8');
  const html = render(markdown);
  assert.match(html, /<h1>SoDeck v0\.9\.0<\/h1>/);
  assert.match(html, /<h2>桌面交互<\/h2>/);
  assert.match(html, /<code>245823731ed6c194f56e7d6a37e5a9b412733e3c<\/code>/);
  assert.match(html, /<table>/);
  assert.match(html, /<th>验收<\/th>/);
  assert.match(html, /<a [^>]*href="https:\/\/github\.com\/BgaSol\/soDeck\/actions\/runs\/42206361218"/);
  assert.doesNotMatch(html, /<p># SoDeck/);
});

test('release notes preserve list nesting, ordered starts, quotes and inline formatting', () => {
  const html = render('# 更新\n\n## 功能\n\n- **加粗**和*斜体*、~~旧功能~~\n  - 子项 `node_name`\n- 第二项\n\n3. 下载\n4. 重启\n\n> 引用\n>\n> **提示**');
  assert.match(html, /<strong>加粗<\/strong>和<em>斜体<\/em>、<s>旧功能<\/s>/);
  assert.match(html, /<ul>\s*<li>[\s\S]*<ul>\s*<li>子项 <code>node_name<\/code>/);
  assert.match(html, /<ol start="3">\s*<li>下载/);
  assert.match(html, /<blockquote>\s*<p>引用<\/p>\s*<p><strong>提示/);
});

test('release notes keep code literal, normalize line endings and honor hard breaks and escaped markers', () => {
  const html = render('标题\r\n====\r\n\r\n```sh\r\n<xml> **literal**\r\n  keep_indent\r\n```\r\n\r\n`**raw**` and `` `tick` ``\r\n\r\n第一行  \r\n第二行\\\r\n第三行\r\n\r\n\\# 普通文字\r\n\r\nserver_name\r\n\r\n---');
  assert.match(html, /<h1>标题<\/h1>/);
  assert.match(html, /<pre><code[^>]*>&lt;xml&gt; \*\*literal\*\*\n  keep_indent\n<\/code><\/pre>/);
  assert.match(html, /<code>\*\*raw\*\*<\/code> and <code>`tick`<\/code>/);
  assert.match(html, /第一行<br>\s*第二行<br>\s*第三行/);
  assert.match(html, /<p># 普通文字<\/p>/);
  assert.match(html, /<p>server_name<\/p>\s*<hr>/);
  assert.equal(render(''), '');
  assert.equal(render(' \r\n\t'), '');
});

test('release notes render table alignment and escaped pipes without splitting cells', () => {
  const html = render('| 左 | 中 | 右 |\n| :--- | :---: | ---: |\n| `a\\|b` | c\\|d | **结果** |');
  assert.match(html, /<th style="text-align:left">左<\/th>\s*<th style="text-align:center">中<\/th>\s*<th style="text-align:right">右<\/th>/);
  assert.match(html, /<td style="text-align:left"><code>a\|b<\/code><\/td>\s*<td style="text-align:center">c\|d<\/td>/);
});

test('release-note links support parentheses, titles, autolinks and plain web URLs', () => {
  const html = render('[**说明**](https://example.com/a_(b)?x=1&y=2 "查看说明")\n\n<https://example.com/auto>\n\nhttps://example.com/plain.');
  assert.match(html, /href="https:\/\/example.com\/a_\(b\)\?x=1&amp;y=2"[^>]*title="查看说明"[^>]*><strong>说明<\/strong><\/a>/);
  assert.match(html, /href="https:\/\/example.com\/auto"/);
  assert.match(html, /href="https:\/\/example.com\/plain"[^>]*>https:\/\/example.com\/plain<\/a>\./);
});

test('remote release notes cannot inject HTML, active URLs or attributes', () => {
  const html = render('<script>alert(1)</script>\n\n<img src=x onerror=alert(1)>\n\n[脚本](javascript:alert(1)) [文件](file:///etc/passwd) [数据](data:text/html,unsafe)\n\n[链接](https://example.com/ "&#34; onmouseover=&#34;alert(1)")\n\n![图片](https://example.com/image.png)');
  assert.doesNotMatch(html, /<(?:script|img|iframe|style)\b/);
  assert.doesNotMatch(html, /href="(?:javascript|file|data):/);
  assert.doesNotMatch(html, /" onmouseover="/);
  assert.match(html, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/);
  assert.match(html, /title="&quot; onmouseover=&quot;alert\(1\)"/);
});

test('release notes use CommonMark for references, lazy list continuations and nested emphasis', () => {
  const html = render('- 列表第一行\n继续同一项\n\n[发行说明][release]\n\n[release]: https://example.com/releases\n\n**加粗包含 *斜体***\n\n    <code>indented</code>');
  assert.match(html, /<li>列表第一行\n继续同一项<\/li>/);
  assert.match(html, /href="https:\/\/example.com\/releases"[^>]*>发行说明<\/a>/);
  assert.match(html, /<strong>加粗包含 <em>斜体<\/em><\/strong>/);
  assert.match(html, /<pre><code>&lt;code&gt;indented&lt;\/code&gt;\n<\/code><\/pre>/);
});

test('headings only remove closing markers separated by whitespace', () => {
  const html = render('# C#\n\n## 带关闭标记 ##\n\n普通描述\n下一行');
  assert.match(html, /<h1>C#<\/h1>/);
  assert.match(html, /<h2>带关闭标记<\/h2>/);
  assert.match(html, /<p>普通描述\n下一行<\/p>/);
});
