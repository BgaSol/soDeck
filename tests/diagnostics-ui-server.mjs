import { build, preview } from "vite";
import react from "@vitejs/plugin-react";
// Exercise the production component. This avoids dev-server dependency scans
// and keeps test assets out of the application's dist directory.
const buildOptions = {
  outDir: ".qa/diagnostics-ui",
  emptyOutDir: true,
  rollupOptions: { input: "tests/diagnostics.html" },
};
export default async function setup() {
  await build({ configFile: false, plugins: [react()], build: buildOptions });
  const server = await preview({
    configFile: false,
    build: buildOptions,
    preview: { host: "127.0.0.1", port: 1471, strictPort: true },
  });
  return async () => {
    server.httpServer.closeAllConnections();
    await new Promise((resolve) => server.httpServer.close(resolve));
  };
}
