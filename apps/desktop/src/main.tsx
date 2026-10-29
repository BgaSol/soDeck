import '@sodeck/ui/styles.css';
import { createRoot } from "react-dom/client";
import {ApplicationMenuPopup} from "./application-menu";
import { App } from "./app";
import {CommunicationMonitor} from './communication-monitor/root';
import "./styles.css";
import {SettingsWindow} from './settings/root';
import { initializeTheme } from "@sodeck/sdk";
if(window.__SODECK_APPLICATION_MENU__){createRoot(document.getElementById("root")!).render(<ApplicationMenuPopup/>);}else void initializeTheme().then(()=>createRoot(document.getElementById("root")!).render(window.__SODECK_SETTINGS_CONTEXT__?<SettingsWindow/>:window.__SODECK_MONITOR_CONTEXT__?<CommunicationMonitor/>:<App />)).catch(e=>{document.getElementById("root")!.textContent=String(e);});
