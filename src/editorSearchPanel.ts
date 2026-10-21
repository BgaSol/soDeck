import { EditorState } from "@codemirror/state";
import {
  closeSearchPanel,
  findNext,
  findPrevious,
  getSearchQuery,
  replaceAll,
  replaceNext,
  search,
  SearchQuery,
  selectMatches,
  setSearchQuery,
} from "@codemirror/search";
import {
  EditorView,
  runScopeHandlers,
  type Command,
  type Panel,
  type ViewUpdate,
} from "@codemirror/view";
import {
  createMatchScan,
  MATCH_LIMIT,
  matchSummaryText,
  sameMatchQuery,
  type MatchSummary,
} from "./editorSearchStats";

let panelSerial = 0;

function icon(
  doc: Document,
  kind: "disclosure" | "up" | "down" | "close" | "search",
) {
  const svg = doc.createElementNS("http://www.w3.org/2000/svg", "svg");
  for (const [name, value] of Object.entries({
    width: "16",
    height: "16",
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "1.7",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
    "aria-hidden": "true",
    focusable: "false",
  }))
    svg.setAttribute(name, value);
  const path = doc.createElementNS(svg.namespaceURI, "path");
  path.setAttribute(
    "d",
    {
      disclosure: "m9 6 6 6-6 6",
      up: "m6 15 6-6 6 6",
      down: "m6 9 6 6 6-6",
      close: "m6 6 12 12M6 18 18 6",
      search: "m21 21-4.3-4.3M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
    }[kind],
  );
  svg.append(path);
  return svg;
}

class EditorSearchPanel implements Panel {
  readonly dom: HTMLDivElement;
  readonly top = true;
  private readonly searchField: HTMLInputElement;
  private readonly replaceField: HTMLInputElement;
  private readonly caseField: HTMLInputElement;
  private readonly regexpField: HTMLInputElement;
  private readonly wordField: HTMLInputElement;
  private readonly replaceToggle: HTMLButtonElement;
  private readonly optionsToggle: HTMLButtonElement;
  private readonly replaceRow: HTMLDivElement;
  private readonly optionsRow: HTMLDivElement;
  private readonly previous: HTMLButtonElement;
  private readonly next: HTMLButtonElement;
  private readonly replace: HTMLButtonElement;
  private readonly replaceAll: HTMLButtonElement;
  private readonly selectAll: HTMLButtonElement;
  private readonly result: HTMLSpanElement;
  private readonly hint: HTMLParagraphElement;
  private readonly announcement: HTMLSpanElement;
  private readonly events = new AbortController();
  private query: SearchQuery;
  private summary: MatchSummary = { kind: "empty" };
  private generation = 0;
  private scanTimer: ReturnType<typeof setTimeout> | undefined;
  private announceTimer: ReturnType<typeof setTimeout> | undefined;
  private composing: HTMLInputElement | null = null;
  private destroyed = false;

  constructor(private readonly view: EditorView) {
    const doc = view.dom.ownerDocument;
    const id = `editor-search-${++panelSerial}`;
    this.query = getSearchQuery(view.state);
    const element = <K extends keyof HTMLElementTagNameMap>(
      tag: K,
      className = "",
    ) => {
      const el = doc.createElement(tag);
      el.className = className;
      return el;
    };
    const button = (
      name: string,
      action: () => void,
      glyph?: Parameters<typeof icon>[1],
    ) => {
      const el = element("button", "editor-search-button");
      el.type = "button";
      el.title = name;
      el.setAttribute("aria-label", name);
      if (glyph) {
        el.classList.add("editor-search-icon");
        el.append(icon(doc, glyph));
      } else el.textContent = name;
      el.addEventListener("click", action, { signal: this.events.signal });
      return el;
    };
    const input = (name: string) => {
      const el = element("input", "editor-search-input");
      el.type = "text";
      el.placeholder = name;
      el.setAttribute("aria-label", name);
      el.setAttribute("autocapitalize", "off");
      el.autocomplete = "off";
      el.spellcheck = false;
      el.addEventListener(
        "input",
        () => {
          if (this.composing !== el) this.commit();
        },
        { signal: this.events.signal },
      );
      el.addEventListener(
        "compositionstart",
        () => {
          this.composing = el;
          this.refresh();
        },
        { signal: this.events.signal },
      );
      el.addEventListener(
        "compositionend",
        () => {
          this.composing = null;
          if (!this.destroyed) {
            this.commit();
            this.refresh();
          }
        },
        { signal: this.events.signal },
      );
      return el;
    };
    const checkbox = (name: string) => {
      const label = element("label", "editor-search-option");
      const input = element("input");
      input.type = "checkbox";
      input.addEventListener("change", () => this.commit(), {
        signal: this.events.signal,
      });
      label.append(input, doc.createTextNode(name));
      this.optionsRow.append(label);
      return input;
    };

    this.dom = element("div", "cm-search file-editor-search");
    this.dom.setAttribute("role", "search");
    this.dom.setAttribute("aria-label", "文件内查找与替换");
    const main = element("div", "editor-search-main");
    this.replaceRow = element("div", "editor-search-replace");
    this.replaceRow.id = `${id}-replace`;
    this.replaceRow.hidden = true;
    this.optionsRow = element("div", "editor-search-options");
    this.optionsRow.id = `${id}-options`;
    this.optionsRow.hidden = true;
    this.optionsRow.setAttribute("role", "group");
    this.optionsRow.setAttribute("aria-label", "匹配选项");
    this.searchField = input("查找内容");
    this.searchField.setAttribute("main-field", "true");
    this.replaceField = input("替换为");
    const field = element("div", "editor-search-field");
    field.append(icon(doc, "search"), this.searchField);
    this.replaceToggle = button(
      "展开替换",
      () => this.toggleReplace(),
      "disclosure",
    );
    this.replaceToggle.classList.add("editor-search-disclosure");
    this.replaceToggle.setAttribute("aria-expanded", "false");
    this.replaceToggle.setAttribute("aria-controls", this.replaceRow.id);
    this.result = element("span", "editor-search-result");
    this.result.setAttribute("aria-label", "匹配数量");
    this.previous = button("上一处匹配", () => this.run(findPrevious), "up");
    this.next = button("下一处匹配", () => this.run(findNext), "down");
    const navigation = element("div", "editor-search-navigation");
    navigation.append(this.previous, this.next);
    this.optionsToggle = button("匹配选项", () => this.toggleOptions());
    this.optionsToggle.classList.add("editor-search-options-toggle");
    this.optionsToggle.setAttribute("aria-expanded", "false");
    this.optionsToggle.setAttribute("aria-controls", this.optionsRow.id);
    const close = button(
      "关闭查找",
      () => {
        closeSearchPanel(view);
        // WebKit doesn't necessarily focus a clicked button. Restore explicitly
        // after removing the panel rather than relying on its activeElement check.
        view.focus();
      },
      "close",
    );
    close.classList.add("editor-search-close");
    main.append(
      this.replaceToggle,
      field,
      this.result,
      navigation,
      this.optionsToggle,
      close,
    );

    this.replace = button("替换", () => this.run(replaceNext, true));
    this.replace.title = "替换当前匹配并查找下一处；未选中匹配时先定位";
    this.replaceAll = button("全部替换", () => this.run(replaceAll, true));
    const replaceActions = element("div", "editor-search-replace-actions");
    replaceActions.append(this.replace, this.replaceAll);
    this.replaceRow.append(this.replaceField, replaceActions);
    this.caseField = checkbox("区分大小写");
    this.regexpField = checkbox("正则表达式");
    this.wordField = checkbox("整词匹配");
    this.selectAll = button("选中全部匹配", () => this.run(selectMatches));
    this.optionsRow.append(this.selectAll);
    this.hint = element("p", "editor-search-hint");
    this.hint.id = `${id}-hint`;
    this.hint.hidden = true;
    this.announcement = element("span", "editor-search-announcement");
    this.announcement.setAttribute("role", "status");
    this.announcement.setAttribute("aria-label", "查找结果播报");
    this.announcement.setAttribute("aria-live", "polite");
    this.announcement.setAttribute("aria-atomic", "true");
    this.dom.append(
      main,
      this.replaceRow,
      this.optionsRow,
      this.hint,
      this.announcement,
    );
    this.dom.addEventListener("keydown", (event) => this.keydown(event), {
      signal: this.events.signal,
    });
    this.syncFields();
    this.restartScan();
  }

  mount() {
    this.searchField.focus();
    this.searchField.select();
  }

  update(update: ViewUpdate) {
    const query = getSearchQuery(update.state);
    const rescan = update.docChanged || !sameMatchQuery(query, this.query);
    this.query = query;
    this.syncFields();
    if (rescan) this.restartScan();
    else this.refresh();
  }

  destroy() {
    this.destroyed = true;
    this.generation++;
    this.events.abort();
    clearTimeout(this.scanTimer);
    clearTimeout(this.announceTimer);
  }

  private syncFields() {
    for (const [field, value] of [
      [this.searchField, this.query.search],
      [this.replaceField, this.query.replace],
    ] as const)
      if (this.composing !== field && field.value !== value)
        field.value = value;
    this.caseField.checked = this.query.caseSensitive;
    this.regexpField.checked = this.query.regexp;
    this.wordField.checked = this.query.wholeWord;
  }

  private commit() {
    if (this.destroyed || this.composing) return;
    const query = new SearchQuery({
      search: this.searchField.value,
      replace: this.replaceField.value,
      caseSensitive: this.caseField.checked,
      regexp: this.regexpField.checked,
      wholeWord: this.wordField.checked,
      literal: this.query.literal,
      test: this.query.test,
    });
    if (!query.eq(this.query))
      this.view.dispatch({ effects: setSearchQuery.of(query) });
  }

  private run(command: Command, replacing = false) {
    if (
      this.destroyed ||
      this.composing ||
      !this.query.valid ||
      (replacing && this.view.state.readOnly)
    )
      return;
    command(this.view);
  }

  private toggleReplace() {
    const expanded = this.replaceRow.hidden;
    if (!expanded && this.replaceRow.contains(this.view.root.activeElement))
      this.replaceToggle.focus();
    this.replaceRow.hidden = !expanded;
    this.replaceToggle.setAttribute("aria-expanded", String(expanded));
    const label = expanded ? "收起替换" : "展开替换";
    this.replaceToggle.setAttribute("aria-label", label);
    this.replaceToggle.title = label;
  }

  private toggleOptions() {
    const expanded = this.optionsRow.hidden;
    if (!expanded && this.optionsRow.contains(this.view.root.activeElement))
      this.optionsToggle.focus();
    this.optionsRow.hidden = !expanded;
    this.optionsToggle.setAttribute("aria-expanded", String(expanded));
    this.refresh();
  }

  private keydown(event: KeyboardEvent) {
    if (this.composing || event.isComposing || event.keyCode === 229) {
      event.stopPropagation();
      return;
    }
    if (event.key === "Escape" && !this.optionsRow.hidden) {
      this.toggleOptions();
      this.optionsToggle.focus();
    } else if (event.key === "Enter" && event.target === this.searchField) {
      this.run(event.shiftKey ? findPrevious : findNext);
    } else if (event.key === "Enter" && event.target === this.replaceField) {
      this.run(replaceNext, true);
    } else if (!runScopeHandlers(this.view, event, "search-panel")) return;
    event.preventDefault();
    event.stopPropagation();
  }

  private restartScan() {
    const generation = ++this.generation;
    clearTimeout(this.scanTimer);
    clearTimeout(this.announceTimer);
    this.announcement.textContent = "";
    this.summary = !this.query.search
      ? { kind: "empty" }
      : !this.query.valid
        ? { kind: "invalid" }
        : { kind: "pending", found: 0 };
    this.refresh();
    if (this.summary.kind !== "pending") {
      this.announce();
      return;
    }
    const scan = createMatchScan(this.view.state, this.query);
    const step = () => {
      if (this.destroyed || generation !== this.generation) return;
      this.summary = scan.step();
      this.refresh();
      if (this.summary.kind === "pending") this.scanTimer = setTimeout(step, 0);
      else this.announce();
    };
    this.scanTimer = setTimeout(step, 0);
  }

  private announce() {
    const text = matchSummaryText(this.summary);
    this.announceTimer = setTimeout(() => {
      if (!this.destroyed) this.announcement.textContent = text;
    }, 200);
  }

  private refresh() {
    this.result.textContent = matchSummaryText(
      this.summary,
      this.view.state.selection.main,
    );
    const invalid = this.summary.kind === "invalid";
    this.searchField.setAttribute("aria-invalid", String(invalid));
    if (invalid)
      this.searchField.setAttribute("aria-describedby", this.hint.id);
    else this.searchField.removeAttribute("aria-describedby");
    const empty =
      this.summary.kind === "complete" && this.summary.ranges.length === 0;
    const disabled = !this.query.valid || empty || !!this.composing;
    this.previous.disabled = this.next.disabled = disabled;
    this.replace.disabled = this.replaceAll.disabled =
      disabled || this.view.state.readOnly;
    this.selectAll.disabled = disabled || this.summary.kind !== "complete";
    const optionCount =
      Number(this.query.caseSensitive) +
      Number(this.query.regexp) +
      Number(this.query.wholeWord);
    this.optionsToggle.textContent = optionCount
      ? `匹配选项 · ${optionCount}`
      : "匹配选项";
    this.hint.textContent = invalid
      ? "请检查正则表达式的括号、转义或量词。"
      : this.summary.kind === "capped" && !this.optionsRow.hidden
        ? `匹配超过 ${MATCH_LIMIT} 项，无法一次选中全部；仍可逐项查找或全部替换。`
        : "";
    this.hint.hidden = !this.hint.textContent;
    this.hint.dataset.invalid = String(invalid);
  }
}

export function createEditorSearchPanel(view: EditorView): Panel {
  return new EditorSearchPanel(view);
}

export const editorSearchExtension = [
  search({ top: true, createPanel: createEditorSearchPanel }),
  EditorState.phrases.of({
    "current match": "当前匹配",
    "on line": "位于行",
    "No results": "无匹配结果",
    "replaced match on line $": "已替换第 $ 行的匹配",
    "replaced $ matches": "已替换 $ 个匹配",
    "Go to line": "跳转到行",
    go: "跳转",
    close: "关闭",
  }),
];
