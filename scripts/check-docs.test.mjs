import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { checkMarkdown } from './check-docs.mjs';

test('Markdown validates relative, encoded, root and image destinations without fetching remote links', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'sodeck-docs-'));
  try {
    fs.mkdirSync(path.join(root, 'docs'));
    fs.writeFileSync(path.join(root, 'docs', '中文 file.md'), 'ok');
    fs.writeFileSync(path.join(root, 'image.png'), 'image');
    const source = '[file](<中文 file.md>) [encoded](%E4%B8%AD%E6%96%87%20file.md#heading) ![image](/image.png) [url](https://invalid.test) [fragment](#heading)\n';
    assert.deepEqual(checkMarkdown('docs/README.md', source, root), []);
    fs.unlinkSync(path.join(root, 'image.png'));
    assert.match(checkMarkdown('docs/README.md', source, root).join('\n'), /missing local link target: \/image.png/);
    assert.match(checkMarkdown('docs/README.md', '[bad](%zz.md)\n', root).join('\n'), /invalid link encoding/);
    assert.match(checkMarkdown('docs/README.md', '[ref]: absent.md\n', root).join('\n'), /missing local link target/);
    assert.match(checkMarkdown('docs/README.md', '[outer [inner]](absent.md)\n', root).join('\n'), /missing local link target: absent\.md/);
    fs.writeFileSync(path.join(root, 'docs', 'file(one).md'), 'ok');
    assert.deepEqual(checkMarkdown('docs/README.md', '[nested [label]](file\\(one\\).md)\n', root), []);
    assert.deepEqual(checkMarkdown('docs/README.md', '[balanced](file(one).md)\n', root), []);
    fs.writeFileSync(path.join(root, 'image.png'), 'image');
    assert.deepEqual(checkMarkdown('docs/README.md', '![image][asset]\n\n[asset]: /image.png\n', root), []);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('Markdown checks merge markers/newlines and ignores links in code examples', () => {
  assert.equal(checkMarkdown('README.md', 'no newline').length, 1);
  assert.match(checkMarkdown('README.md', '<<<<<<< conflict\n').join('\n'), /merge marker/);
  assert.deepEqual(checkMarkdown('README.md', '`[example](absent.md)`\n\n```md\n[example](absent.md)\n```\n\n~~~md\n[example](absent.md)\n~~~\n'), []);
  assert.deepEqual(checkMarkdown('README.md', '    [example](absent.md)\n\n``[example](absent.md)``\n', ''), []);
});
