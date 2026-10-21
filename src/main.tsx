import { installDiagnostics } from "./diagnostics";
import { createRoot } from "react-dom/client";
import { Root } from "./App";
import "./styles.css";
installDiagnostics();
createRoot(document.getElementById("root")!).render(<Root />);
