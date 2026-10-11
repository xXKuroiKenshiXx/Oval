"use strict";

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const appWindow = window.__TAURI__.window.getCurrentWindow();
const { LogicalSize } = window.__TAURI__.dpi;

const $ = (sel, root = document) => root.querySelector(sel);
const $$ = (sel, root = document) => [...root.querySelectorAll(sel)];

const store = {
  get(key, fallback) {
    try { const v = localStorage.getItem(key); return v === null ? fallback : v; } catch { return fallback; }
  },
  set(key, value) {
    try { localStorage.setItem(key, value); } catch { /* private mode */ }
  },
};

const ICON = {
  pause: '<svg viewBox="0 0 24 24" class="fill"><rect x="7" y="6" width="3.6" height="12" rx="1.3"/><rect x="13.4" y="6" width="3.6" height="12" rx="1.3"/></svg>',
  play: '<svg viewBox="0 0 24 24" class="fill"><path d="M8.5 6.6v10.8a1 1 0 0 0 1.5.86l8.6-5.4a1 1 0 0 0 0-1.72L10 5.74a1 1 0 0 0-1.5.86z"/></svg>',
  stop: '<svg viewBox="0 0 24 24" class="fill"><rect x="7" y="7" width="10" height="10" rx="2.2"/></svg>',
  retry: '<svg viewBox="0 0 24 24"><path d="M19.5 12a7.5 7.5 0 1 1-2.2-5.3M19.5 4.5v4h-4"/></svg>',
  check: '<svg viewBox="0 0 24 24"><path d="M5.5 12.5l4 4 9-9"/></svg>',
  bang: '<svg viewBox="0 0 24 24"><path d="M12 7v6M12 17h.01"/></svg>',
  x: '<svg viewBox="0 0 24 24"><path d="M7.5 7.5l9 9M16.5 7.5l-9 9"/></svg>',
};

const DESIGN = 672; // diameter the stage is laid out on
const DEFAULT_SIZE = 700;
let MOBILE = false; // phones get an upright, full-screen oval instead

/* ---------- Formatting ---------- */

const nf1 = new Intl.NumberFormat("es", { maximumFractionDigits: 1, minimumFractionDigits: 1 });
const nf0 = new Intl.NumberFormat("es", { maximumFractionDigits: 0 });

function bytes(n) {
  if (!n || n <= 0) return "";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
  return `${i >= 2 ? nf1.format(n) : nf0.format(n)} ${units[i]}`;
}

function clock(sec) {
  if (sec == null || !isFinite(sec) || sec < 0) return "";
  sec = Math.round(sec);
  if (sec < 60) return `${sec} s`;
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = String(sec % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

/** Always "m:ss" / "h:mm:ss", for durations and recording time. */
function stamp(sec) {
  sec = Math.max(0, Math.round(sec || 0));
  const h = Math.floor(sec / 3600);
  const m = Math.floor((sec % 3600) / 60);
  const s = String(sec % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${s}` : `${m}:${s}`;
}

const mbps = (bytesPerSec) => `${nf1.format((bytesPerSec * 8) / 1e6)} Mb/s`;

const esc = (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));

/* ---------- Toast ---------- */

let toastTimer;
function toast(text, bad = false) {
  const el = $("#toast");
  el.textContent = text;
  el.classList.toggle("bad", bad);
  el.classList.add("show");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => el.classList.remove("show"), 2600);
}

function shake(el) {
  el.classList.remove("shake");
  void el.offsetWidth;
  el.classList.add("shake");
}

/* ---------- Links ---------- */

function normalizeUrl(raw) {
  let s = raw.trim().replace(/^<|>$/g, "");
  if (!s) return null;
  if (!/^https?:\/\//i.test(s)) {
    if (!/^[\w-]+(\.[\w-]+)+(\/|$)/.test(s)) return null;
    s = "https://" + s;
  }
  try {
    const u = new URL(s);
    return u.hostname.includes(".") ? u.href : null;
  } catch {
    return null;
  }
}

const hostOf = (url) => {
  try { return new URL(url).hostname.replace(/^(www|m)\./, ""); } catch { return ""; }
};
const isYouTube = (url) => /(^|\.)youtube\.com$|^youtu\.be$/.test(hostOf(url));

/** Whole channels, profiles and playlists belong to the "Canal" tab. */
function isCollection(url) {
  const u = new URL(url);
  const host = hostOf(url);
  if (host === "youtube.com" || host === "music.youtube.com") {
    if (u.searchParams.has("list") && !u.searchParams.has("v")) return true;
    return /^\/(@[^/]+|channel\/|c\/|user\/)/.test(u.pathname) && !/\/(live|watch)\b/.test(u.pathname);
  }
  if (host === "tiktok.com") return /^\/@[^/]+\/?$/.test(u.pathname);
  return false;
}

/** Live channels belong to the "Directo" tab. */
function isLive(url) {
  const u = new URL(url);
  const host = hostOf(url);
  const path = u.pathname.replace(/\/+$/, "");
  if (host === "twitch.tv") return /^\/[\w]+$/.test(path) && !/^\/(videos|directory|downloads|settings)$/.test(path);
  if (host === "kick.com") return /^\/[\w-]+$/.test(path);
  if (host === "youtube.com") return /\/live(\/|$)/.test(path);
  if (host === "tiktok.com") return /^\/@[^/]+\/live$/.test(path);
  if (host === "instagram.com") return /\/live$/.test(path);
  return false;
}

/* ---------- Tabs & mode ---------- */

const INPUTS = { link: "#url", channel: "#channel-url", live: "#live-url" };
let tab = "link";

function setTab(next) {
  tab = INPUTS[next] ? next : "link";
  $(".tabs").dataset.on = tab;
  $("#stage").dataset.tab = tab;
  $$(".tabs button").forEach((b) => b.classList.toggle("on", b.dataset.tab === tab));
  $("#view-link").classList.toggle("on", tab === "link");
  $("#view-channel").classList.toggle("on", tab === "channel");
  $("#view-live").classList.toggle("on", tab === "live");
  closeMenu();
  store.set("tab", tab);
  setTimeout(() => $(INPUTS[tab]).focus({ preventScroll: true }), 60);
}

let mode = store.get("mode", "video") === "audio" ? "audio" : "video";
function setMode(m) {
  mode = m;
  $$(".mode button").forEach((b) => b.classList.toggle("on", b.dataset.mode === m));
  store.set("mode", m);
  renderQuality();
}

/* ---------- Quality ---------- */

// value "" = best available
const QUALITIES = {
  video: [["", "Máxima", "Máx"], ["2160", "4K", "4K"], ["1440", "1440p", "1440p"], ["1080", "1080p", "1080p"],
          ["720", "720p", "720p"], ["480", "480p", "480p"], ["360", "360p", "360p"]],
  audio: [["", "Máxima", "Máx"], ["320", "320 kbps", "320k"], ["192", "192 kbps", "192k"], ["128", "128 kbps", "128k"]],
  live: [["", "Máxima", "Máx"], ["1080", "1080p", "1080p"], ["720", "720p", "720p"], ["480", "480p", "480p"], ["360", "360p", "360p"]],
};
const quality = { video: store.get("q-video", ""), audio: store.get("q-audio", ""), live: store.get("q-live", "") };
/** Which quality list a chip controls: recordings have their own. */
const qualityKey = (ctx) => (ctx === "live" ? "live" : mode);
const currentQuality = (ctx) => quality[qualityKey(ctx)] || null;

function renderQuality() {
  for (const chip of $$(".qchip")) {
    const key = qualityKey(chip.dataset.ctx);
    const entry = QUALITIES[key].find(([v]) => v === quality[key]) || QUALITIES[key][0];
    $(".qlabel", chip).textContent = entry[2];
  }
}

let menuChip = null;
function openQualityMenu(chip) {
  if (menuChip === chip) return closeMenu();
  const menu = $("#qmenu");
  const key = qualityKey(chip.dataset.ctx);
  menu.dataset.key = key;
  menu.innerHTML =
    `<div class="menu-title">${key === "audio" ? "Calidad de audio" : "Resolución máxima"}</div>` +
    QUALITIES[key].map(([v, label]) =>
      `<button type="button" role="menuitemradio" data-q="${v}" class="${v === quality[key] ? "on" : ""}">` +
      `<span>${label}${v === "" ? " <small>la mejor disponible</small>" : ""}</span>` +
      `<svg class="tick" viewBox="0 0 24 24"><path d="M5.5 12.5l4 4 9-9"/></svg></button>`).join("");
  menu.hidden = false;
  // The stage is scaled, so convert screen pixels back to stage pixels.
  const stage = $("#stage");
  const st = stage.getBoundingClientRect();
  const s = parseFloat(getComputedStyle(stage).getPropertyValue("--s")) || 1;
  const width = st.width / s, height = st.height / s;
  const margin = MOBILE ? 16 : 110; // keep clear of the oval's curved edge
  const r = chip.getBoundingClientRect();
  const mw = menu.offsetWidth, mh = menu.offsetHeight;
  const cx = (r.left + r.width / 2 - st.left) / s;
  const left = Math.min(Math.max(cx - mw / 2, margin), width - mw - margin);
  const above = (r.top - st.top) / s > height / 2;
  menu.style.left = `${left}px`;
  menu.style.top = above ? `${(r.top - st.top) / s - mh - 8}px` : `${(r.bottom - st.top) / s + 8}px`;
  menu.style.setProperty("--origin", above ? "bottom center" : "top center");
  $$(".qchip").forEach((c) => c.classList.toggle("open", c === chip));
  menuChip = chip;
}

function closeMenu() {
  $("#qmenu").hidden = true;
  $$(".qchip").forEach((c) => c.classList.remove("open"));
  menuChip = null;
}

/* ---------- Folder ---------- */

function renderFolder(path) {
  const name = path.split(/[\\/]/).filter(Boolean).pop() || path;
  $("#folder-name").textContent = name;
  $("#folder").title = `Abrir ${path}`;
}

async function changeFolder() {
  const path = await invoke("choose_folder").catch(() => null);
  if (path) {
    renderFolder(path);
    toast(`Se guardará en ${path}`);
  }
}

/* ---------- Window size ---------- */

/** Scales the 672px design to whatever oval the window currently is. */
function fitStage() {
  if (MOBILE) return $("#stage").style.setProperty("--s", "1");
  const oval = $("#oval");
  const s = Math.min(oval.clientWidth, oval.clientHeight) / DESIGN;
  $("#stage").style.setProperty("--s", s.toFixed(4));
}

let saveSizeTimer;
function onResize() {
  fitStage();
  if (MOBILE) return;
  clearTimeout(saveSizeTimer);
  saveSizeTimer = setTimeout(() => store.set("size", `${innerWidth}x${innerHeight}`), 400);
}

async function restoreSize() {
  if (MOBILE) return;
  const m = /^(\d+)x(\d+)$/.exec(store.get("size", ""));
  if (m && (+m[1] !== innerWidth || +m[2] !== innerHeight)) {
    await appWindow.setSize(new LogicalSize(+m[1], +m[2])).catch(() => {});
  }
}

/* ---------- Engine ---------- */

let engine = { ready: false, message: "Preparando motor", progress: null };
function renderEngine() {
  for (const el of $$(".engine")) {
    el.hidden = engine.ready;
    el.classList.toggle("indeterminate", engine.progress == null);
    $(".engine-ring", el).style.setProperty("--p", engine.progress == null ? 0 : engine.progress);
    const pct = engine.progress != null ? ` ${Math.round(engine.progress * 100)}%` : "";
    $(".engine-text", el).textContent = engine.message + pct;
  }
  for (const job of jobs.values()) if (job.data.status === "starting") renderJob(job.data);
}

/* ---------- Queue ---------- */

const jobs = new Map(); // id -> { data, el, icon }
const tpl = $("#item-tpl");

function metaFor(j) {
  if (j.status === "starting" && !engine.ready) {
    const p = engine.progress != null ? ` · ${Math.round(engine.progress * 100)}%` : "";
    return `${esc(engine.message)}${p}`;
  }
  if (j.live) {
    switch (j.status) {
      case "queued":
      case "starting":
        return "Conectando al directo…";
      case "downloading":
        return [`<span class="rec">REC</span> <b>${stamp(j.elapsed)}</b>`, bytes(j.downloaded), j.speed ? mbps(j.speed) : ""]
          .filter(Boolean).join(" · ");
      case "processing":
        return "Guardando la grabación…";
      case "done":
        return [`<b>Guardado</b>`, j.elapsed ? stamp(j.elapsed) : "", bytes(j.total), "clic para mostrar"].filter(Boolean).join(" · ");
      case "error":
        return esc(j.error || "Error");
    }
  }
  const pct = `<b>${Math.floor((j.progress || 0) * 100)}%</b>`;
  const size = j.total ? `${bytes(j.downloaded)} de ${bytes(j.total)}` : bytes(j.downloaded);
  switch (j.status) {
    case "queued":
      return "En cola";
    case "starting":
      return "Conectando…";
    case "downloading": {
      const parts = [pct, size, j.speed ? `${bytes(j.speed)}/s` : ""];
      if (j.eta != null) parts.push(`<span class="eta">quedan ${clock(j.eta)}</span>`);
      return parts.filter(Boolean).join(" · ");
    }
    case "processing":
      return j.mode === "audio" ? "Convirtiendo a MP3…" : "Uniendo vídeo y audio…";
    case "paused":
      return ["En pausa", pct, size].filter(Boolean).join(" · ");
    case "done":
      return [`<b>Listo</b>`, bytes(j.total), "clic para mostrar"].filter(Boolean).join(" · ");
    case "error":
      return esc(j.error || "Error");
  }
  return "";
}

function toggleIcon(j) {
  if (j.status === "error") return "retry";
  if (j.live) return "stop";
  return j.status === "paused" ? "play" : "pause";
}

const TOGGLE_TITLE = { retry: "Reintentar", play: "Reanudar", pause: "Pausar", stop: "Detener y guardar" };

function renderJob(j) {
  let entry = jobs.get(j.id);
  if (!entry) {
    const el = tpl.content.firstElementChild.cloneNode(true);
    el.dataset.id = j.id;
    $(".act.toggle", el).addEventListener("click", (e) => {
      e.stopPropagation();
      const cur = jobs.get(j.id).data;
      const icon = toggleIcon(cur);
      const cmd = { retry: "resume", play: "resume", pause: "pause", stop: "stop_live" }[icon];
      invoke(cmd, { id: j.id });
    });
    $(".act.stop", el).addEventListener("click", (e) => {
      e.stopPropagation();
      el.classList.add("leaving");
      invoke("cancel", { id: j.id });
    });
    el.addEventListener("click", () => {
      const cur = jobs.get(j.id);
      if (!cur || cur.data.status !== "done") return;
      if (MOBILE) toast("Guardado en Descargas/Oval");
      else invoke("reveal", { id: j.id });
    });
    $(j.live ? "#live-queue" : "#queue").appendChild(el);
    entry = { data: j, el, icon: null };
    jobs.set(j.id, entry);
  }
  entry.data = j;
  const { el } = entry;

  el.className = "item " + j.status + (j.status === "queued" ? " waiting" : "") +
    (j.thumbnail ? "" : " noimg") + (j.live ? " live" : "");
  const full = j.status === "done" || j.status === "error";
  $(".ring", el).style.setProperty("--p", full ? 1 : j.progress || 0);
  const img = $("img", el);
  if (j.thumbnail && img.getAttribute("src") !== j.thumbnail) img.src = j.thumbnail;
  $(".title", el).textContent = j.title;
  $(".title", el).title = j.title;
  $(".meta", el).innerHTML = metaFor(j);
  $(".ring-badge", el).innerHTML = j.status === "error" ? ICON.bang : ICON.check;

  const icon = toggleIcon(j);
  if (entry.icon !== icon) {
    const t = $(".act.toggle", el);
    t.innerHTML = ICON[icon];
    t.title = TOGGLE_TITLE[icon];
    entry.icon = icon;
  }
  const busyLive = j.live && j.status !== "done";
  $(".act.stop", el).title = j.status === "done" ? "Quitar de la lista" : busyLive ? "Descartar grabación" : "Cancelar";
  syncQueueChrome();
}

function removeJob(id) {
  const entry = jobs.get(id);
  if (!entry) return;
  jobs.delete(id);
  entry.el.classList.add("leaving");
  setTimeout(() => { entry.el.remove(); syncQueueChrome(); }, 280);
  syncQueueChrome();
}

function syncQueueChrome() {
  const all = [...jobs.values()];
  $("#queue").classList.toggle("has-items", all.some((j) => !j.data.live));
  $("#live-queue").classList.toggle("has-items", all.some((j) => j.data.live));
  $("#clear").hidden = !all.some((j) => j.data.status === "done");
}

async function addLinks(text) {
  const urls = text.split(/\s+/).map(normalizeUrl).filter(Boolean);
  if (!urls.length) {
    shake($("#link-form"));
    toast("Eso no parece un enlace", true);
    return false;
  }
  if (urls.length === 1 && isCollection(urls[0])) {
    setTab("channel");
    $("#channel-url").value = urls[0];
    loadChannel();
    return true;
  }
  if (urls.length === 1 && isLive(urls[0])) {
    setTab("live");
    await startRecording(urls[0]);
    toast("Es un directo: grabando");
    return true;
  }
  for (const url of urls) await invoke("add_download", { url, mode, quality: currentQuality("link") });
  if (urls.length > 1) toast(`${urls.length} descargas en cola`);
  return true;
}

async function startRecording(url) {
  await invoke("add_live", { url, quality: currentQuality("live") });
}

/* ---------- Clipboard ---------- */

let lastClip = "";
async function suggestFromClipboard() {
  const input = $(INPUTS[tab]);
  if (input.value) return;
  const text = ((await invoke("clipboard_text").catch(() => null)) || "").trim();
  if (!text || text === lastClip || text.length > 2000) return;
  lastClip = text;
  const url = normalizeUrl(text);
  if (url && /^https?:/i.test(text)) {
    input.value = url;
    input.select();
    syncChannelTools();
  }
}

/* ---------- Channel ---------- */

const channel = { token: 0, entries: [], off: new Set(), have: new Set(), disk: null, diskAsked: false, name: null, last: null, loading: false, query: "" };

/** Lowercase without accents, so "camion" finds "Camión". */
const fold = (s) => String(s).normalize("NFD").replace(/[\u0300-\u036f]/g, "").toLowerCase();
const matches = (entry) => !channel.query || fold(entry.title).includes(channel.query);
/** Letters and digits only: file names lose characters like "/" or "?" on the way. */
const titleKey = (s) => fold(s).replace(/[^\p{L}\p{N}]+/gu, "");

function renderCount() {
  const total = channel.entries.length;
  const sel = total - channel.off.size;
  const shown = channel.query ? channel.entries.filter(matches).length : total;
  $("#ccount").innerHTML = !total
    ? channel.loading ? "…" : ""
    : channel.query ? `${shown} de ${total}` : `<b>${sel}</b>/${total}`;
  $("#csearch").placeholder = channel.loading ? "Buscando vídeos…" : "Buscar en el canal";
  $("#csel").textContent = sel;
  $("#cdownload").disabled = sel === 0;
}

/** The Vídeos / Shorts / Directos switch only applies to YouTube. */
function syncChannelTools() {
  const url = normalizeUrl($("#channel-url").value);
  $(".ctools").classList.toggle("other", !!url && !isYouTube(url));
}

function tileFor(entry, i) {
  const t = document.createElement("button");
  t.type = "button";
  t.className = "tile" + (entry.thumbnail ? "" : " noimg");
  t.dataset.i = i;
  t.title = entry.title;
  t.style.animationDelay = `${Math.min(i % 60, 24) * 12}ms`;
  if (!entry.thumbnail) t.style.setProperty("--h", (i * 47) % 360);
  t.innerHTML =
    (entry.thumbnail ? `<img loading="lazy" referrerpolicy="no-referrer" draggable="false" alt="" src="${esc(entry.thumbnail)}">` : "") +
    `<span class="shade"></span>` +
    (entry.duration ? `<span class="dur">${stamp(entry.duration)}</span>` : "") +
    `<span class="mark"><span class="v">${ICON.check}</span><span class="x">${ICON.x}</span></span>` +
    `<span class="have-tag">Ya descargado</span>` +
    `<span class="t">${esc(entry.title)}</span>`;
  if (!matches(entry)) t.hidden = true;
  // Some sites block hotlinked thumbnails: fall back to the placeholder.
  const img = $("img", t);
  if (img) img.addEventListener("error", () => {
    t.classList.add("noimg");
    t.style.setProperty("--h", (i * 47) % 360);
    $("img", t).remove();
  });
  return t;
}

function setOff(i, off) {
  if (off) channel.off.add(i); else channel.off.delete(i);
  const tile = $(`.tile[data-i="${i}"]`);
  if (tile) tile.classList.toggle("off", off);
}

async function loadChannel() {
  const url = normalizeUrl($("#channel-url").value);
  if (!url) {
    shake($("#channel-form"));
    return;
  }
  syncChannelTools();
  const token = ++channel.token;
  Object.assign(channel, { entries: [], off: new Set(), have: new Set(), disk: null, diskAsked: false, name: null, last: null, loading: true, query: "" });
  $("#csearch").value = "";
  $("#grid").innerHTML = "";
  $("#grid").classList.remove("settled");
  $(".grid-wrap").classList.remove("filled");
  $("#channel-go").classList.add("busy");
  renderCount();
  // A pasted ".../streams" or ".../shorts" link picks that section.
  const fromUrl = (new URL(url).pathname.match(/\/(videos|shorts|streams)\/?$/) || [])[1];
  if (fromUrl) $$("#ctabs button").forEach((x) => x.classList.toggle("on", x.dataset.ctab === fromUrl));
  const section = $("#ctabs .on").dataset.ctab;
  try {
    const summary = await invoke("list_channel", { url, tab: section, token });
    if (summary.token !== channel.token) return;
    channel.name = summary.channel || channel.name;
    if (!summary.count) toast("No se encontraron vídeos", true);
    else await markDownloaded(true);
  } catch (err) {
    if (token !== channel.token || err === "cancelled") return;
    toast(String(err), true);
  } finally {
    if (token === channel.token) {
      channel.loading = false;
      $("#channel-go").classList.remove("busy");
      renderCount();
    }
  }
}

/** What the download folder already holds, read fresh from disk. */
async function readDisk() {
  const d = await invoke("on_disk", { folder: channel.name }).catch(() => null);
  if (!d) return null;
  const names = d.names.map(titleKey).filter(Boolean);
  return { ids: new Set(d.ids), partial: new Set(d.partial), exact: new Set(names), long: names.filter((k) => k.length >= 24) };
}

/** "have" when the video is in the folder, "partial" when it was cut halfway. */
function diskState(entry) {
  const d = channel.disk;
  if (!d) return null;
  if (d.ids.has(entry.id)) return "have";
  const key = titleKey(entry.title);
  // Files named by title only (other apps, older downloads); long titles get cut in file names.
  if (key && (d.exact.has(key) || d.long.some((k) => key.startsWith(k)))) return "have";
  return d.partial.has(entry.id) ? "partial" : null;
}

/** Yellow tiles are already downloaded and left out; half-done ones stay selected and resume. */
function applyDisk(i) {
  const entry = channel.entries[i];
  const state = diskState(entry);
  const tile = $(`.tile[data-i="${i}"]`);
  if (state === "have" && !channel.have.has(i)) {
    channel.have.add(i);
    channel.off.add(i);
    if (tile) {
      tile.classList.remove("partial");
      tile.classList.add("have", "off");
    }
    if (tile) $(".have-tag", tile).textContent = "Ya descargado";
    return "have";
  }
  if (state === "partial" && !channel.have.has(i)) {
    if (tile) tile.classList.add("partial");
    if (tile) $(".have-tag", tile).textContent = "A medias · se continúa";
    return "partial";
  }
  return null;
}

async function markDownloaded(announce = false) {
  const token = channel.token;
  const disk = await readDisk();
  if (!disk || token !== channel.token) return;
  channel.disk = disk;
  let have = 0;
  let partial = 0;
  channel.entries.forEach((_, i) => {
    applyDisk(i);
    if (channel.have.has(i)) have++;
    else if (diskState(channel.entries[i]) === "partial") partial++;
  });
  if (announce && (have || partial)) {
    const parts = [];
    if (have) parts.push(`${have} ${have === 1 ? "ya está descargado" : "ya están descargados"} (en amarillo)`);
    if (partial) parts.push(`${partial} a medias, se continúan`);
    toast(parts.join(" · "));
  }
  renderCount();
}

/** A download just finished: if its video is on the grid, it turns yellow. */
let diskRefresh = 0;
function refreshDiskSoon() {
  if (!channel.entries.length || channel.loading) return;
  clearTimeout(diskRefresh);
  diskRefresh = setTimeout(() => markDownloaded(), 600);
}

function applyFilter() {
  $("#grid").classList.add("settled");
  channel.query = fold($("#csearch").value.trim());
  channel.entries.forEach((e, i) => {
    const tile = $(`.tile[data-i="${i}"]`);
    if (tile) tile.hidden = !matches(e);
  });
  renderCount();
}

function onBatch({ token, channel: name, entries }) {
  if (token !== channel.token) return;
  if (name) channel.name = name;
  const frag = document.createDocumentFragment();
  const first = channel.entries.length;
  for (const e of entries) {
    const i = channel.entries.push(e) - 1;
    frag.appendChild(tileFor(e, i));
  }
  $("#grid").appendChild(frag);
  // Mark as the list streams in, not only when it ends.
  if (channel.disk) for (let i = first; i < channel.entries.length; i++) applyDisk(i);
  else if (channel.name && !channel.diskAsked) {
    channel.diskAsked = true;
    readDisk().then((d) => {
      if (!d || token !== channel.token || channel.disk) return;
      channel.disk = d;
      channel.entries.forEach((_, i) => applyDisk(i));
      renderCount();
    });
  }
  $(".grid-wrap").classList.add("filled");
  renderCount();
}

/** Thumbnails and durations arriving from the slower second pass. */
function onDetails({ token, items }) {
  if (token !== channel.token) return;
  for (const { index, thumbnail, duration } of items) {
    const entry = channel.entries[index];
    const tile = $(`.tile[data-i="${index}"]`);
    if (!entry || !tile) continue;
    if (duration && !entry.duration) {
      entry.duration = duration;
      tile.insertAdjacentHTML("afterbegin", `<span class="dur">${stamp(duration)}</span>`);
    }
    if (thumbnail && !entry.thumbnail) {
      entry.thumbnail = thumbnail;
      const img = new Image();
      img.alt = "";
      img.draggable = false;
      img.referrerPolicy = "no-referrer";
      img.className = "late";
      img.onload = () => { tile.classList.remove("noimg"); tile.prepend(img); };
      img.src = thumbnail;
    }
  }
}

async function downloadSelection() {
  const items = channel.entries
    .filter((_, i) => !channel.off.has(i))
    .map((e) => ({ url: e.url, title: e.title, thumbnail: e.thumbnail }));
  if (!items.length) return;
  await invoke("add_many", { items, mode, quality: currentQuality("channel"), folder: channel.name });
  setTab("link");
  toast(`${items.length} ${items.length === 1 ? "vídeo" : "vídeos"} en cola`);
}

/* ---------- Wiring ---------- */

function wire() {
  $("#min").addEventListener("click", () => appWindow.minimize());
  $("#close").addEventListener("click", () => appWindow.close());
  $("#brand").addEventListener("dblclick", () => appWindow.setSize(new LogicalSize(DEFAULT_SIZE, DEFAULT_SIZE)));
  $("#grip").addEventListener("mousedown", (e) => {
    e.preventDefault();
    appWindow.startResizeDragging("SouthEast");
  });
  window.addEventListener("resize", onResize);

  $$(".tabs button").forEach((b) => b.addEventListener("click", () => setTab(b.dataset.tab)));
  $$(".mode button").forEach((b) => b.addEventListener("click", () => setMode(b.dataset.mode)));

  $("#link-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const input = $("#url");
    if (await addLinks(input.value)) {
      lastClip = input.value.trim() || lastClip;
      input.value = "";
    }
  });

  $("#live-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    const input = $("#live-url");
    const url = normalizeUrl(input.value);
    if (!url) {
      shake($("#live-form"));
      toast("Eso no parece un enlace", true);
      return;
    }
    await startRecording(url);
    lastClip = input.value.trim() || lastClip;
    input.value = "";
  });

  $("#paste").addEventListener("click", async () => {
    const text = await invoke("clipboard_text").catch(() => null);
    const input = $("#url");
    if (text) input.value = text.trim();
    input.focus();
  });

  $("#folder").addEventListener("click", () => {
    if (MOBILE) toast("Las descargas están en Descargas/Oval");
    else invoke("reveal", { id: null });
  });
  $("#folder-change").addEventListener("click", changeFolder);

  $$(".qchip").forEach((c) => c.addEventListener("click", (e) => { e.stopPropagation(); openQualityMenu(c); }));
  $("#qmenu").addEventListener("click", (e) => {
    e.stopPropagation();
    const b = e.target.closest("button[data-q]");
    if (!b) return;
    const key = $("#qmenu").dataset.key;
    quality[key] = b.dataset.q;
    store.set(`q-${key}`, b.dataset.q);
    renderQuality();
    closeMenu();
  });
  document.addEventListener("click", closeMenu);
  $("#clear").addEventListener("click", () => invoke("clear_finished"));

  const conc = (n) => invoke("set_concurrency", { value: n }).then((v) => ($("#conc").textContent = v));
  $("#less").addEventListener("click", () => conc(Math.max(1, +$("#conc").textContent - 1)));
  $("#more").addEventListener("click", () => conc(Math.min(8, +$("#conc").textContent + 1)));

  $("#channel-form").addEventListener("submit", (e) => { e.preventDefault(); loadChannel(); });
  $("#channel-url").addEventListener("input", syncChannelTools);
  $$("#ctabs button").forEach((b) =>
    b.addEventListener("click", () => {
      $$("#ctabs button").forEach((x) => x.classList.toggle("on", x === b));
      const input = $("#channel-url");
      // Keep the field in sync so the URL's own section doesn't override the click.
      input.value = input.value.trim().replace(/\/(videos|shorts|streams)\/?$/, `/${b.dataset.ctab}`);
      if (input.value) loadChannel();
    }),
  );

  $("#grid").addEventListener("click", (e) => {
    const tile = e.target.closest(".tile");
    if (!tile) return;
    const i = +tile.dataset.i;
    const off = !channel.off.has(i);
    if (e.shiftKey && channel.last != null) {
      const [a, b] = [Math.min(channel.last, i), Math.max(channel.last, i)];
      for (let k = a; k <= b; k++) setOff(k, off);
    } else {
      setOff(i, off);
    }
    channel.last = i;
    renderCount();
  });
  // "Todos" / "Ninguno" act on what the search shows.
  $("#all").addEventListener("click", () => {
    // Already-downloaded videos stay out; tap one to get it again.
    channel.entries.forEach((e, i) => matches(e) && !channel.have.has(i) && setOff(i, false));
    renderCount();
  });
  $("#none").addEventListener("click", () => {
    channel.entries.forEach((e, i) => matches(e) && setOff(i, true));
    renderCount();
  });
  $("#csearch").addEventListener("input", applyFilter);
  $("#csearch").addEventListener("keydown", (e) => {
    if (e.key === "Escape" && e.target.value) {
      e.stopPropagation();
      e.target.value = "";
      applyFilter();
    }
  });
  $("#cdownload").addEventListener("click", downloadSelection);

  // Ctrl+V anywhere pastes into the active tab's field.
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "v" && !(document.activeElement instanceof HTMLInputElement)) {
      $(INPUTS[tab]).focus();
    }
    if (e.key === "Escape") { closeMenu(); if (document.activeElement && document.activeElement.blur) document.activeElement.blur(); }
  });

  // Links dropped on the oval go to whatever tab fits them.
  document.addEventListener("dragover", (e) => e.preventDefault());
  document.addEventListener("drop", (e) => {
    e.preventDefault();
    const text = e.dataTransfer.getData("text/uri-list") || e.dataTransfer.getData("text/plain");
    if (text) addLinks(text.split("\n").filter((l) => !l.startsWith("#")).join(" "));
  });

  window.addEventListener("focus", suggestFromClipboard);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden) suggestFromClipboard();
  });
  document.addEventListener("contextmenu", (e) => {
    if (!(e.target instanceof HTMLInputElement)) e.preventDefault();
  });
}

async function main() {
  const os = await invoke("platform").catch(() => "");
  MOBILE = os === "android";
  document.documentElement.classList.toggle("mobile", MOBILE);
  document.documentElement.classList.toggle("web", os === "web"); // demo on the website
  fitStage();
  setMode(mode);
  setTab(store.get("tab", "link"));
  wire();
  restoreSize();

  await listen("job", (e) => {
    const prev = jobs.get(e.payload.id);
    const was = prev && prev.data.status;
    renderJob(e.payload);
    if (e.payload.status === "done" && was && was !== "done") refreshDiskSoon();
  });
  await listen("job-removed", (e) => removeJob(e.payload));
  await listen("engine", (e) => { engine = e.payload; renderEngine(); });
  await listen("channel-batch", (e) => onBatch(e.payload));
  await listen("channel-details", (e) => onDetails(e.payload));

  for (const j of await invoke("jobs")) renderJob(j);
  $("#conc").textContent = await invoke("get_concurrency");
  renderFolder(await invoke("download_folder"));
  engine = await invoke("engine_status");
  renderEngine();
  renderCount();
  suggestFromClipboard();
}

main();
