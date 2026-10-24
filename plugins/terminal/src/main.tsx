import { createRoot } from "react-dom/client";
import { context } from "@sodeck/sdk";
import { TerminalView } from "./terminal-view";
import { SessionList } from "./session-list";
import "@xterm/xterm/css/xterm.css";
import "./terminal.css";

createRoot(document.getElementById("root")!).render(context().viewType === "terminal.sessions" ? <SessionList /> : <TerminalView />);
