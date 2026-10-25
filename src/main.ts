import { invoke } from "@tauri-apps/api/core";
import { mount } from "svelte";
invoke("timing_mark", { name: "page-start" });
import App from "./App.svelte";
import { applyTheme } from "./lib/theme";
// the last colours chosen, before the first paint (Options are read from disk a moment later)
applyTheme((localStorage.getItem("theme") as "system" | "light" | "dark" | null) ?? "system");
import "./app.css";

export default mount(App, { target: document.getElementById("app")! });
