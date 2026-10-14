import { StreamLanguage } from "@codemirror/language";
import type { Extension } from "@codemirror/state";

export const languages = [
  ["text", "纯文本"],
  ["java", "Java"],
  ["javascript", "JavaScript"],
  ["jsx", "JavaScript JSX"],
  ["typescript", "TypeScript"],
  ["tsx", "TypeScript TSX"],
  ["python", "Python"],
  ["go", "Go"],
  ["rust", "Rust"],
  ["cpp", "C / C++"],
  ["json", "JSON"],
  ["jsonc", "JSONC"],
  ["yaml", "YAML"],
  ["toml", "TOML"],
  ["ini", "INI / Properties / .env"],
  ["shell", "Shell"],
  ["powershell", "PowerShell"],
  ["html", "HTML"],
  ["css", "CSS"],
  ["xml", "XML"],
  ["sql", "SQL"],
  ["markdown", "Markdown"],
  ["dockerfile", "Dockerfile"],
  ["nginx", "Nginx"],
] as const;
export type LanguageId = (typeof languages)[number][0];
export type LanguageMode = "auto" | LanguageId;
export const languageName = (id: LanguageId) =>
  languages.find(([key]) => key === id)![1];

export function detectLanguage(path: string, content: string): LanguageId {
  const normalized = path.replaceAll("\\", "/").toLowerCase();
  const name = normalized.split("/").pop() || "";
  if (/^(dockerfile|containerfile)(\..*)?$/.test(name)) return "dockerfile";
  if (
    name === "nginx.conf" ||
    (/\/nginx\//.test(normalized) && /\.conf$/.test(name))
  )
    return "nginx";
  if (
    /^\.(bashrc|bash_profile|bash_login|profile|zshrc|zprofile|kshrc)$/.test(
      name,
    )
  )
    return "shell";
  if (
    /^\.env(?:\.|$)/.test(name) ||
    name === ".gitconfig" ||
    name === ".editorconfig"
  )
    return "ini";
  const extension = name.split(".").pop() || "";
  const extensions: Record<string, LanguageId> = {
    java: "java",
    js: "javascript",
    mjs: "javascript",
    cjs: "javascript",
    jsx: "jsx",
    ts: "typescript",
    mts: "typescript",
    cts: "typescript",
    tsx: "tsx",
    py: "python",
    pyw: "python",
    go: "go",
    rs: "rust",
    c: "cpp",
    h: "cpp",
    cpp: "cpp",
    hpp: "cpp",
    cc: "cpp",
    cxx: "cpp",
    json: "json",
    jsonc: "jsonc",
    yaml: "yaml",
    yml: "yaml",
    toml: "toml",
    ini: "ini",
    properties: "ini",
    env: "ini",
    sh: "shell",
    bash: "shell",
    zsh: "shell",
    ksh: "shell",
    ps1: "powershell",
    psm1: "powershell",
    psd1: "powershell",
    html: "html",
    htm: "html",
    css: "css",
    xml: "xml",
    svg: "xml",
    sql: "sql",
    md: "markdown",
    markdown: "markdown",
  };
  if (name.includes(".") && extensions[extension]) return extensions[extension];
  const first = content
    .replace(/^\uFEFF/, "")
    .split(/[\r\n]/, 1)[0]
    .slice(0, 512);
  if (first.startsWith("#!")) {
    if (/(?:\/|\s)(?:ba|da|k|z)?sh(?:\s|$)/.test(first)) return "shell";
    if (/(?:\/|\s)python[\d.]*(?:\s|$)/.test(first)) return "python";
    if (/(?:\/|\s)(?:node|nodejs)(?:\s|$)/.test(first)) return "javascript";
    if (/(?:\/|\s)(?:pwsh|powershell)(?:\s|$)/.test(first)) return "powershell";
  }
  return "text";
}

const cache = new Map<LanguageId, Promise<Extension>>();
export function loadLanguage(id: LanguageId): Promise<Extension> {
  const existing = cache.get(id);
  if (existing) return existing;
  const pending = (async (): Promise<Extension> => {
    switch (id) {
      case "java":
        return (await import("@codemirror/lang-java")).java();
      case "javascript":
      case "jsx":
      case "typescript":
      case "tsx":
        return (await import("@codemirror/lang-javascript")).javascript({
          typescript: id === "typescript" || id === "tsx",
          jsx: id === "jsx" || id === "tsx",
        });
      case "python":
        return (await import("@codemirror/lang-python")).python();
      case "go":
        return (await import("@codemirror/lang-go")).go();
      case "rust":
        return (await import("@codemirror/lang-rust")).rust();
      case "cpp":
        return (await import("@codemirror/lang-cpp")).cpp();
      case "json":
        return (await import("@codemirror/lang-json")).json();
      case "jsonc":
        return StreamLanguage.define(
          (await import("@codemirror/legacy-modes/mode/javascript")).json,
        );
      case "yaml":
        return (await import("@codemirror/lang-yaml")).yaml();
      case "toml":
        return StreamLanguage.define(
          (await import("@codemirror/legacy-modes/mode/toml")).toml,
        );
      case "ini":
        return StreamLanguage.define(
          (await import("@codemirror/legacy-modes/mode/properties")).properties,
        );
      case "shell":
        return StreamLanguage.define(
          (await import("@codemirror/legacy-modes/mode/shell")).shell,
        );
      case "powershell":
        return StreamLanguage.define(
          (await import("@codemirror/legacy-modes/mode/powershell")).powerShell,
        );
      case "html":
        return (await import("@codemirror/lang-html")).html();
      case "css":
        return (await import("@codemirror/lang-css")).css();
      case "xml":
        return (await import("@codemirror/lang-xml")).xml();
      case "sql":
        return (await import("@codemirror/lang-sql")).sql();
      case "markdown":
        return (await import("@codemirror/lang-markdown")).markdown();
      case "dockerfile":
        return StreamLanguage.define(
          (await import("@codemirror/legacy-modes/mode/dockerfile")).dockerFile,
        );
      case "nginx":
        return StreamLanguage.define(
          (await import("@codemirror/legacy-modes/mode/nginx")).nginx,
        );
      default:
        return [];
    }
  })();
  cache.set(id, pending);
  void pending.catch(() => cache.delete(id));
  return pending;
}
