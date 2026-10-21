import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { EditorView } from "@codemirror/view";
import { tags } from "@lezer/highlight";
import type { Theme } from "./theme";

export const editorSyntaxHighlighting = syntaxHighlighting(
  HighlightStyle.define([
    { tag: tags.keyword, color: "var(--editor-keyword)" },
    { tag: [tags.string, tags.regexp], color: "var(--editor-string)" },
    { tag: [tags.number, tags.bool, tags.null], color: "var(--editor-number)" },
    { tag: tags.comment, color: "var(--muted)", fontStyle: "italic" },
    {
      tag: [tags.propertyName, tags.function(tags.variableName)],
      color: "var(--editor-property)",
    },
    { tag: [tags.typeName, tags.className], color: "var(--editor-type)" },
    {
      tag: [tags.variableName, tags.attributeName],
      color: "var(--editor-property)",
    },
    {
      tag: [tags.operator, tags.meta, tags.tagName],
      color: "var(--editor-keyword)",
    },
    { tag: tags.heading, color: "var(--editor-property)", fontWeight: "bold" },
    {
      tag: tags.link,
      color: "var(--editor-string)",
      textDecoration: "underline",
    },
    { tag: tags.strong, fontWeight: "bold" },
    { tag: tags.emphasis, fontStyle: "italic" },
    { tag: tags.invalid, textDecoration: "underline wavy var(--error)" },
  ]),
);

export function createEditorAppearance(theme: Theme) {
  return EditorView.theme(
    {
      "&": {
        height: "100%",
        backgroundColor: "var(--surface)",
        color: "var(--text)",
      },
      "&.cm-focused": { outline: "none" },
      ".cm-scroller": {
        overflow: "auto",
        flex: "1",
        minHeight: "0",
        height: "auto",
        fontFamily: "var(--mono)",
        fontSize: "var(--editor-font-size)",
        lineHeight: "1.65",
      },
      ".cm-content": { caretColor: "var(--text)", padding: "0.857rem 0" },
      ".cm-line": { padding: "0 0.857rem" },
      ".cm-gutters": {
        backgroundColor: "var(--surface)",
        color: "var(--muted)",
        borderRight: "none",
      },
      ".cm-activeLine, .cm-activeLineGutter": {
        backgroundColor: "var(--hover)",
      },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        {
          backgroundColor: "var(--selected)",
        },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--text)" },
      ".cm-panels": {
        backgroundColor: "var(--canvas)",
        color: "var(--text)",
        borderColor: "var(--line)",
      },
      ".cm-searchMatch": {
        backgroundColor: "var(--editor-match)",
        outline: "1px solid var(--editor-match-border)",
        color: "var(--text)",
      },
      ".cm-searchMatch *": { color: "inherit" },
      ".cm-searchMatch-selected": {
        backgroundColor: "var(--editor-match-current)",
      },
      ".cm-tooltip": {
        color: "var(--text)",
        backgroundColor: "var(--surface)",
        borderColor: "var(--strong-line)",
      },
      ".cm-tooltip-autocomplete > ul > li[aria-selected]": {
        backgroundColor: "var(--selected)",
        color: "var(--text)",
      },
      ".cm-textfield": {
        background: "var(--field)",
        color: "var(--text)",
        borderColor: "var(--strong-line)",
      },
      ".cm-button": {
        background: "var(--surface)",
        color: "var(--text)",
        borderColor: "var(--strong-line)",
      },
    },
    { dark: theme === "dark" },
  );
}
