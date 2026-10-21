import { createRoot } from "react-dom/client";
import { DiagnosticsSection } from "../src/DiagnosticsSection";
import "../src/styles.css";
createRoot(document.getElementById("root")!).render(
  <main
    style={{ maxWidth: 640, margin: "24px auto", fontFamily: "sans-serif" }}
  >
    <DiagnosticsSection />
  </main>,
);
