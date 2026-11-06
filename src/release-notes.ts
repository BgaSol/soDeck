import MarkdownIt from "markdown-it";
import { escape as e } from "./ui";

const markdown = new MarkdownIt({ html: false, linkify: true });

// Release-note links go through the core's web-page adapter.
markdown.validateLink = (url) => {
  if (!/^https?:\/\//i.test(url)) return false;
  try { return !!new URL(url).hostname; } catch { return false; }
};
markdown.renderer.rules.link_open = (tokens, index, options, _env, renderer) => {
  tokens[index].attrSet("data-update-link", "");
  return renderer.renderToken(tokens, index, options);
};
// Keep release notes self-contained; display image descriptions without fetching assets.
markdown.renderer.rules.image = (tokens, index, options, env, renderer) =>
  e(renderer.renderInlineAsText(tokens[index].children ?? [], options, env));
markdown.renderer.rules.table_open = (tokens, index, options, _env, renderer) =>
  `<div class="release-notes-table">${renderer.renderToken(tokens, index, options)}`;
markdown.renderer.rules.table_close = (tokens, index, options, _env, renderer) =>
  `${renderer.renderToken(tokens, index, options)}</div>`;

export function renderReleaseNotes(notes: string): string {
  return markdown.render(notes);
}
