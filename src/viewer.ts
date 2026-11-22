// The quick photo viewer (Picasa's Photo Viewer): one photo over a dimmed screen, the folder's
// other photos one key away, zoom and pan, slideshow, rotate, star, delete, and "Edit in CasaPic".
// Deliberately plain TypeScript with no library code, so the window opens fast.
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import "./viewer.css";

type State = { folder: string; files: string[]; index: number };
type Info = { name: string; path: string; width: number; height: number; bytes: number; taken: number | null; file_date: number; star: boolean; edited: boolean };

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const img = $<HTMLImageElement>("photo");
const stage = $("stage");
const note = $("note");
const zoomEl = $<HTMLInputElement>("zoom");

/** margin kept around a fitted photo (the bars sit over it) */
const MARGIN = 48;
/** the largest zoom, as a share of the photo's own pixels */
const MAX_SCALE = 8;
const SLIDE_MS = 3000;
/** photos are decoded to fit the screen; zooming in past that loads the full photo */
const EDGE = Math.min(4096, Math.ceil(Math.max(screen.width, screen.height) * devicePixelRatio));

let files: string[] = [];
let folder = "";
let index = 0;
let info: Info | null = null;
/** bumped when the shown photo changes on disk (rotate), so its picture reloads */
let rev = 0;
/** CSS pixels per photo pixel; null = fitted to the screen */
let scale: number | null = null;
let pan = { x: 0, y: 0 };
let slideshow: ReturnType<typeof setInterval> | null = null;
let loadToken = 0;

const src = (i: number, edge: number) => `http://casapic.localhost/v/${i}/${edge}?r=${rev}`;

/** the bottom strip and the name line above it, which the photo keeps clear of */
const BOTTOM = 84;

function view() {
  return { w: window.innerWidth, h: Math.max(100, window.innerHeight - BOTTOM) };
}

function fitScale(): number {
  if (!info || !info.width) return 1;
  const v = view();
  return Math.min((v.w - 2 * MARGIN) / info.width, (v.h - 2 * MARGIN) / info.height, 1);
}

function currentScale(): number {
  return scale ?? fitScale();
}

/** Places the picture: centred, then moved by the pan (kept so the photo never leaves a gap). */
function layout() {
  if (!info) return;
  const s = currentScale();
  const v = view();
  const w = info.width * s, h = info.height * s;
  const maxX = Math.max(0, (w - v.w) / 2), maxY = Math.max(0, (h - v.h) / 2);
  pan.x = Math.max(-maxX, Math.min(maxX, pan.x));
  pan.y = Math.max(-maxY, Math.min(maxY, pan.y));
  img.style.width = `${w}px`;
  img.style.height = `${h}px`;
  img.style.left = `${(v.w - w) / 2 + pan.x}px`;
  img.style.top = `${(v.h - h) / 2 + pan.y}px`;
  img.classList.toggle("grab", w > v.w || h > v.h);
  // the slider runs from "fitted" (0) to MAX_SCALE (100) on a log scale
  const f = fitScale();
  zoomEl.value = String(f >= MAX_SCALE ? 0 : Math.round((100 * Math.log(s / f)) / Math.log(MAX_SCALE / f)));
  // past the screen-size picture, show the photo's own pixels
  const needFull = w * devicePixelRatio > EDGE * 1.05 && Math.max(info.width, info.height) > EDGE;
  const want = src(index, needFull ? 0 : EDGE);
  if (needFull && img.dataset.src !== want) swap(want);
}

/** Loads a picture, keeping the old one on screen until the new one is ready. */
function swap(url: string, onShown?: () => void) {
  const token = ++loadToken;
  const pre = new Image();
  pre.onload = () => {
    if (token !== loadToken) return;
    img.src = url;
    img.dataset.src = url;
    img.classList.add("shown");
    note.hidden = true;
    onShown?.();
  };
  pre.onerror = () => {
    if (token !== loadToken) return;
    img.classList.remove("shown");
    note.textContent = `CasaPic cannot show ${files[index] ?? "this file"}.`;
    note.hidden = false;
  };
  pre.src = url;
}

async function show(i: number) {
  if (!files.length) return;
  index = (i + files.length) % files.length;
  scale = null;
  pan = { x: 0, y: 0 };
  $("name").textContent = files[index];
  $("pos").textContent = `${index + 1} of ${files.length}`;
  document.title = `${files[index]} - CasaPic`;
  try {
    info = await invoke<Info>("viewer_info", { index });
  } catch (e) {
    info = null;
    note.textContent = String(e);
    note.hidden = false;
    return;
  }
  $("size").textContent = info.width ? `(${info.width} × ${info.height})` : "";
  layout();
  swap(src(index, EDGE), layout);
  $("star").textContent = info.star ? "★" : "☆";
  $("star").classList.toggle("on", info.star);
  ($("prev") as HTMLButtonElement).disabled = files.length < 2;
  ($("next") as HTMLButtonElement).disabled = files.length < 2;
  ($("play") as HTMLButtonElement).disabled = files.length < 2;
  if (!$("info").hidden) fillInfo();
  // the next photo is decoded by the viewer meanwhile; fetch it so it is ready to show
  if (files.length > 1) new Image().src = src((index + 1) % files.length, EDGE);
}

function setScale(s: number, at?: { x: number; y: number }) {
  if (!info) return;
  const f = fitScale();
  const old = currentScale();
  const next = Math.max(Math.min(f, 1), Math.min(MAX_SCALE, s));
  // keep the point under the mouse where it is
  if (at) {
    const v = view();
    const cx = at.x - v.w / 2 - pan.x, cy = at.y - v.h / 2 - pan.y;
    pan.x -= cx * (next / old - 1);
    pan.y -= cy * (next / old - 1);
  }
  scale = Math.abs(next - f) < 1e-4 ? null : next;
  if (scale === null) pan = { x: 0, y: 0 };
  layout();
}

function fillInfo() {
  if (!info) return;
  const mb = info.bytes / 1048576;
  // dates are camera-local wall-clock seconds (file dates too), so they are read back as UTC
  const fmt = (s: number) => new Date(s * 1000).toLocaleString(undefined, { timeZone: "UTC", dateStyle: "medium", timeStyle: "short" });
  const taken = info.taken ? fmt(info.taken) : info.file_date > 0 ? `${fmt(info.file_date)} (file date: no camera date)` : "unknown";
  const rows: [string, string][] = [
    ["Name", info.name],
    ["Size", `${info.width} × ${info.height} pixels`],
    ["File", mb >= 1 ? `${mb.toFixed(1)} MB` : `${Math.max(1, Math.round(info.bytes / 1024))} KB`],
    ["Taken", taken],
    ["Folder", folder],
    ["Edits", info.edited ? "yes (in CasaPic)" : "none"],
  ];
  const box = $("info");
  box.replaceChildren(
    ...rows.flatMap(([k, v]) => {
      const dt = document.createElement("dt");
      dt.textContent = k;
      const dd = document.createElement("dd");
      dd.textContent = v;
      return [dt, dd];
    }),
  );
}

function toggleInfo() {
  const box = $("info");
  box.hidden = !box.hidden;
  if (!box.hidden) fillInfo();
}

function stopSlideshow() {
  if (slideshow) clearInterval(slideshow);
  slideshow = null;
  $("play").textContent = "▶";
}

function toggleSlideshow() {
  if (slideshow) return stopSlideshow();
  if (files.length < 2) return;
  slideshow = setInterval(() => show(index + 1), SLIDE_MS);
  $("play").textContent = "❚❚";
  idleSoon();
}

async function rotate(turns: number) {
  stopSlideshow();
  await invoke("viewer_rotate", { index, turns }).catch(alertError);
  rev++;
  await show(index);
}

async function toggleStar() {
  if (!info) return;
  const star = !info.star;
  await invoke("viewer_star", { index, star }).catch(alertError);
  info.star = star;
  $("star").textContent = star ? "★" : "☆";
  $("star").classList.toggle("on", star);
}

function askDelete() {
  stopSlideshow();
  $("askText").textContent = `Move "${files[index]}" to the Recycle Bin?`;
  $("ask").hidden = false;
  $("askYes").focus();
}

async function doDelete() {
  $("ask").hidden = true;
  try {
    const left = await invoke<number>("viewer_delete", { index });
    files.splice(index, 1);
    if (!left || !files.length) return close();
    rev++;
    await show(Math.min(index, files.length - 1));
  } catch (e) {
    alertError(e);
  }
}

function alertError(e: unknown) {
  note.textContent = String(e);
  note.hidden = false;
  setTimeout(() => (note.hidden = true), 4000);
}

const close = () => invoke("viewer_close");
const editInCasaPic = () => invoke("viewer_edit_in_casapic", { index });

// ---------- mouse ----------

let drag: { x: number; y: number; px: number; py: number; moved: boolean } | null = null;

stage.addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  drag = { x: e.clientX, y: e.clientY, px: pan.x, py: pan.y, moved: false };
  if (img.classList.contains("grab")) img.classList.add("grabbing");
});
window.addEventListener("mousemove", (e) => {
  wake();
  if (!drag) return;
  const dx = e.clientX - drag.x, dy = e.clientY - drag.y;
  if (Math.abs(dx) + Math.abs(dy) > 3) drag.moved = true;
  if (drag.moved && img.classList.contains("grab")) {
    pan = { x: drag.px + dx, y: drag.py + dy };
    layout();
  }
});
window.addEventListener("mouseup", (e) => {
  const d = drag;
  drag = null;
  img.classList.remove("grabbing");
  // a plain click on the dimmed screen around the photo closes the viewer
  if (d && !d.moved && e.target === stage) close();
});
img.addEventListener("dblclick", (e) => (scale === null ? setScale(1, { x: e.clientX, y: e.clientY }) : setScale(0)));
stage.addEventListener("wheel", (e) => {
  e.preventDefault();
  setScale(currentScale() * (e.deltaY < 0 ? 1.2 : 1 / 1.2), { x: e.clientX, y: e.clientY });
}, { passive: false });
zoomEl.addEventListener("input", () => {
  const f = fitScale();
  setScale(f * Math.pow(MAX_SCALE / f, Number(zoomEl.value) / 100));
});

// the bars fade out while the mouse rests
let idleTimer: ReturnType<typeof setTimeout> | undefined;
function wake() {
  document.body.classList.remove("idle");
  idleSoon();
}
function idleSoon() {
  clearTimeout(idleTimer);
  idleTimer = setTimeout(() => {
    if ($("ask").hidden && !document.querySelector("#bar:hover")) document.body.classList.add("idle");
  }, 2500);
}

const on = (id: string, f: () => unknown) => $(id).addEventListener("click", () => void f());
on("edit", editInCasaPic);
on("close", close);
on("prev", () => (stopSlideshow(), show(index - 1)));
on("next", () => (stopSlideshow(), show(index + 1)));
on("play", toggleSlideshow);
on("left", () => rotate(-1));
on("right", () => rotate(1));
on("star", toggleStar);
on("delete", askDelete);
on("infobtn", toggleInfo);
on("fit", () => setScale(0));
on("actual", () => setScale(1));
on("askYes", doDelete);
on("askNo", () => ($("ask").hidden = true));

// ---------- keys ----------

window.addEventListener("keydown", (e) => {
  wake();
  const k = e.key;
  if (!$("ask").hidden) {
    if (k === "Escape") $("ask").hidden = true;
    return;
  }
  const ctrl = e.ctrlKey || e.metaKey;
  if (ctrl && k.toLowerCase() === "r") rotate(e.shiftKey ? -1 : 1);
  else if (ctrl && k === "8") toggleStar();
  else if (k === "ArrowLeft" || k === "PageUp" || k === "Backspace") (stopSlideshow(), show(index - 1));
  else if (k === "ArrowRight" || k === "PageDown") (stopSlideshow(), show(index + 1));
  else if (k === "Home") (stopSlideshow(), show(0));
  else if (k === "End") (stopSlideshow(), show(files.length - 1));
  else if (k === " ") toggleSlideshow();
  else if (k === "Escape") {
    if (slideshow) stopSlideshow();
    else if (!$("info").hidden) $("info").hidden = true;
    else close();
  } else if (k === "Enter") editInCasaPic();
  else if (k === "Delete") askDelete();
  else if (k === "+" || k === "=") setScale(currentScale() * 1.25);
  else if (k === "-") setScale(currentScale() / 1.25);
  else if (k === "0") setScale(0);
  else if (k === "1") setScale(1);
  else if (k.toLowerCase() === "i") toggleInfo();
  else if (k === "F11") {
    const w = getCurrentWindow();
    w.isFullscreen().then((f) => w.setFullscreen(!f));
  } else return;
  e.preventDefault();
});

window.addEventListener("resize", layout);

invoke<State>("viewer_state").then((s) => {
  files = s.files;
  folder = s.folder;
  show(s.index);
  idleSoon();
});
