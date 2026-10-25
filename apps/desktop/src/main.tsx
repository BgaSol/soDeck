import { createRoot } from "react-dom/client";
import { App } from "./app";
import "./styles.css";
import { initializeTheme } from "@sodeck/sdk";
void initializeTheme().then(()=>createRoot(document.getElementById("root")!).render(<App />)).catch(e=>{document.getElementById("root")!.textContent=String(e);});
