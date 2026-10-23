import { invoke } from "@tauri-apps/api/core";
import { mount } from "svelte";
invoke("timing_mark", { name: "page-start" });
import App from "./App.svelte";
import "./app.css";

export default mount(App, { target: document.getElementById("app")! });
