import { describe, expect, it } from "vitest";
import { Compartment, EditorState } from "@codemirror/state";
import { ensureSyntaxTree } from "@codemirror/language";
import { classHighlighter, highlightTree } from "@lezer/highlight";
import { history, undoDepth } from "@codemirror/commands";
import {
  detectLanguage,
  loadLanguage,
  type LanguageId,
} from "./editorLanguages";

describe("language detection and local parsers", () => {
  it.each([
    ["/src/Main.java", "", "java"],
    ["/app/CONFIG.JSON", "", "json"],
    ["/app/tsconfig.jsonc", "", "jsonc"],
    ["/etc/nginx/sites-enabled/site.conf", "", "nginx"],
    ["/etc/unknown.conf", "value=1", "text"],
    ["/app/.env.production", "", "ini"],
    ["/app/Dockerfile.dev", "#!/bin/sh", "dockerfile"],
    ["/app/Containerfile", "", "dockerfile"],
    ["/home/ops/.bashrc", "", "shell"],
    ["/app/script", "#!/usr/bin/env -S python3 -u\nprint(1)", "python"],
    ["/app/script", "#!/bin/bash\necho hello", "shell"],
    ["/app/script", "#!/usr/bin/env pwsh", "powershell"],
    ["/app/main.java", "#!/bin/sh", "java"],
    ["/app/README", "hello", "text"],
  ])("detects %s", (path, content, expected) => {
    expect(detectLanguage(path, content)).toBe(expected);
  });

  const examples: [LanguageId, string][] = [
    ["java", 'public class Main { String name = "hi"; }'],
    ["javascript", 'const value = "hi";'],
    ["jsx", "const el = <div>Hi</div>;"],
    ["typescript", "const value: number = 1;"],
    ["tsx", "const el = <div>Hi</div>;"],
    ["python", 'def f():\n  return "hello"'],
    ["go", "package main\nfunc main() {}"],
    ["rust", "fn main() { let x = 1; }"],
    ["cpp", "int main() { return 0; }"],
    ["json", '{"port":8080,"enabled":true}'],
    ["jsonc", '// comment\n{"port":8080}'],
    ["yaml", "port: 8080\nenabled: true"],
    ["toml", "[server]\nport = 8080"],
    ["ini", "[server]\nport=8080"],
    ["shell", '#!/bin/sh\necho "$HOME"'],
    ["powershell", 'Write-Host "$env:USER"'],
    ["html", '<div class="a">hi</div>'],
    ["css", "body { color: red; }"],
    ["xml", '<root value="x"/>'],
    ["sql", "SELECT * FROM users;"],
    ["markdown", "# Title\n**bold**"],
    ["dockerfile", "FROM alpine\nRUN echo hello"],
    ["nginx", "server { listen 80; }"],
  ];
  it.each(examples)("%s produces syntax tokens", async (id, doc) => {
    const state = EditorState.create({
      doc,
      extensions: [await loadLanguage(id)],
    });
    const tree = ensureSyntaxTree(state, doc.length, 1000)!;
    const tokens: string[] = [];
    highlightTree(tree, classHighlighter, (from, to, classes) =>
      tokens.push(`${doc.slice(from, to)}:${classes}`),
    );
    expect(tokens.length).toBeGreaterThan(0);
    if (id === "jsonc")
      expect(tokens.some((token) => token.includes("tok-comment"))).toBe(true);
  });

  it("reconfiguring syntax keeps content, selection and undo history", async () => {
    const language = new Compartment();
    let state = EditorState.create({
      doc: '{"port":80}',
      extensions: [history(), language.of(await loadLanguage("json"))],
    });
    state = state.update({
      changes: { from: 8, to: 10, insert: "8080" },
      selection: { anchor: 12 },
    }).state;
    const snapshot = {
      doc: state.doc.toString(),
      selection: state.selection.toJSON(),
      undo: undoDepth(state),
    };
    state = state.update({
      effects: language.reconfigure(await loadLanguage("java")),
    }).state;
    expect({
      doc: state.doc.toString(),
      selection: state.selection.toJSON(),
      undo: undoDepth(state),
    }).toEqual(snapshot);
  });
});
