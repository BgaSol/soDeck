import '@sodeck/ui/styles.css';
import { createRoot } from "react-dom/client";
import { initializeTheme } from "@sodeck/sdk";
import { FilesView } from "./view";
import "./files.css";
void initializeTheme()
  .then(() =>
    createRoot(document.getElementById("root")!).render(<FilesView />),
  )
  .catch((e) => {
    document.getElementById("root")!.textContent = String(e);
  });
