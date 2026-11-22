// Light and dark colours. "system" follows the Windows setting (Settings > Personalisation >
// Colours > app mode) and changes with it while CasaPic runs.
import { storage } from "./safe-storage";
import { getCurrentWindow } from "@tauri-apps/api/window";

export type Theme = "system" | "light" | "dark";

const dark = window.matchMedia("(prefers-color-scheme: dark)");
let chosen: Theme = "system";

function resolve(): "light" | "dark" {
  return chosen === "system" ? (dark.matches ? "dark" : "light") : chosen;
}

function paint() {
  const t = resolve();
  document.documentElement.dataset.theme = t;
  // the window's own title bar follows too (null = the Windows setting)
  getCurrentWindow()
    .setTheme(chosen === "system" ? null : t)
    .catch(() => {});
}

export function applyTheme(t: Theme) {
  chosen = t;
  storage.set("theme", t);
  paint();
}

dark.addEventListener("change", () => chosen === "system" && paint());
