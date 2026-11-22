// Fails when any front-end file other than src/lib/safe-storage.ts touches localStorage or
// sessionStorage directly (blocked storage throws and would stop the window from opening).
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, sep } from "node:path";

const root = new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const allowed = join("src", "lib", "safe-storage.ts");
const bad = [];
function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(ts|js|svelte)$/.test(name) && relative(root, p) !== allowed) {
      readFileSync(p, "utf8").split(/\r?\n/).forEach((line, i) => {
        if (/\b(localStorage|sessionStorage)\b/.test(line)) bad.push(`${relative(root, p).split(sep).join("/")}:${i + 1}: ${line.trim()}`);
      });
    }
  }
}
walk(join(root, process.argv[2] ?? "src"));
if (bad.length) {
  console.error(`Raw browser storage outside ${allowed} (use storage.get / storage.set):\n${bad.join("\n")}`);
  process.exit(1);
}
console.log("check-storage: no raw localStorage/sessionStorage outside safe-storage.ts");
