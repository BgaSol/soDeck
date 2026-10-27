import '@sodeck/ui/styles.css';
import { createRoot } from "react-dom/client";
import {ApplicationMenuPopup} from "./application-menu";
import { App } from "./app";
import "./styles.css";
import { initializeTheme } from "@sodeck/sdk";
if(window.__SODECK_APPLICATION_MENU__){createRoot(document.getElementById("root")!).render(<ApplicationMenuPopup/>);}else void initializeTheme().then(()=>createRoot(document.getElementById("root")!).render(<App />)).catch(e=>{document.getElementById("root")!.textContent=String(e);});
