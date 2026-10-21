import { describe, expect, it } from "vitest";
import { Compartment, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { history, redo, undo } from "@codemirror/commands";
import {
  createEditorAppearance,
  editorSyntaxHighlighting,
} from "./editorTheme";

describe("editor appearance", () => {
  it("sets CodeMirror's actual light/dark facet", () => {
    for (const theme of ["light", "dark"] as const) {
      const state = EditorState.create({
        extensions: [createEditorAppearance(theme), editorSyntaxHighlighting],
      });
      expect(state.facet(EditorView.darkTheme)).toBe(theme === "dark");
    }
  });
  it("reconfigures appearance without replacing the document, selection or history", () => {
    const appearance = new Compartment();
    let state = EditorState.create({
      doc: "original",
      extensions: [history(), appearance.of(createEditorAppearance("light"))],
    });
    state = state.update({
      changes: { from: 8, insert: " changed" },
      selection: { anchor: 4, head: 12 },
      userEvent: "input.type",
    }).state;
    const document = state.doc,
      selection = state.selection;
    state = state.update({
      effects: appearance.reconfigure(createEditorAppearance("dark")),
    }).state;
    expect(state.doc).toBe(document);
    expect(state.selection).toBe(selection);
    expect(state.facet(EditorView.darkTheme)).toBe(true);
    expect(
      undo({
        state,
        dispatch: (tr) => {
          state = tr.state;
        },
      }),
    ).toBe(true);
    expect(state.doc.toString()).toBe("original");
    expect(
      redo({
        state,
        dispatch: (tr) => {
          state = tr.state;
        },
      }),
    ).toBe(true);
    expect(state.doc.toString()).toBe("original changed");
  });
});
