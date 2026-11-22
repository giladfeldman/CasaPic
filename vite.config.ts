import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// Tauri expects a fixed port and must see Rust errors, so keep the screen.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1", watch: { ignored: ["**/src-tauri/**", "**/tmp/**", "**/target/**"] } },
  // two pages: the library (index.html) and the quick photo viewer (viewer.html), which loads
  // none of the library's code so it opens fast
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    rollupOptions: { input: { main: "index.html", viewer: "viewer.html" } },
  },
});
