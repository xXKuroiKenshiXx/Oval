// The floating Oval panel. Injected into the current tab when the toolbar
// button is pressed; pressing it again hides or shows the same panel.
// Everything is built with DOM calls (no innerHTML) because sites like
// YouTube enforce Trusted Types, and lives in a closed shadow root so page
// styles can't leak in.
(() => {
  "use strict";

  if (window.__ovalPanel) {
    window.__ovalPanel.toggle();
    return;
  }

  const api = globalThis.browser || globalThis.chrome;

  /* ---------- DOM helpers ---------- */

  function h(tag, props, ...kids) {
    const el = document.createElement(tag);
    for (const [k, v] of Object.entries(props || {})) {
      if (v == null || v === false) continue;
      if (k === "class") el.className = v;
      else if (k === "text") el.textContent = v;
      else if (k.startsWith("on")) el.addEventListener(k.slice(2), v);
      else el.setAttribute(k, v === true ? "" : v);
    }
    for (const kid of kids.flat()) if (kid != null) el.append(kid);
    return el;
  }

  const SVG = "http://www.w3.org/2000/svg";
  /** A 24x24 stroke icon from path data. */
  function icon(...paths) {
    const svg = document.createElementNS(SVG, "svg");
    svg.setAttribute("viewBox", "0 0 24 24");
    svg.setAttribute("aria-hidden", "true");
    for (const d of paths) {
      const p = document.createElementNS(SVG, "path");
      p.setAttribute("d", d);
      svg.append(p);
    }
    return svg;
  }

  const I = {
    min: () => icon("M7 12h10"),
    close: () => icon("M8 8l8 8M16 8l-8 8"),
    plus: () => icon("M12 6.5v11M6.5 12h11"),
    down: () => icon("M12 5v11M7 11.5l5 5 5-5M7 20h10"),
    doc: () => icon("M7 3.5h7l4 4V19a1.5 1.5 0 0 1-1.5 1.5h-9A1.5 1.5 0 0 1 6 19V5a1.5 1.5 0 0 1 1-1.5z", "M13.5 3.5V8H18M9 12.5h6M9 16h6"),
    image: () => icon("M5.5 4.5h13a1 1 0 0 1 1 1v13a1 1 0 0 1-1 1h-13a1 1 0 0 1-1-1v-13a1 1 0 0 1 1-1z", "M4.5 16l4.5-4.5 4 4 2.5-2.5 4 4", "M15 9.5h.01"),
    play: () => icon("M8.5 6.6v10.8a1 1 0 0 0 1.5.86l8.6-5.4a1 1 0 0 0 0-1.72L10 5.74a1 1 0 0 0-1.5.86z"),
    folder: () => icon("M3 7.5A2.5 2.5 0 0 1 5.5 5h3.6l2 2h7.4A2.5 2.5 0 0 1 21 9.5v7a2.5 2.5 0 0 1-2.5 2.5h-13A2.5 2.5 0 0 1 3 16.5z"),
    pencil: () => icon("M14.5 5.5l4 4M4 20l1-4.5L15.8 4.7a1.8 1.8 0 0 1 2.5 0l1 1a1.8 1.8 0 0 1 0 2.5L8.5 19z"),
    open: () => icon("M14 4.5h5.5V10M19.5 4.5l-8 8M10 5.5H6A1.5 1.5 0 0 0 4.5 7v11A1.5 1.5 0 0 0 6 19.5h11a1.5 1.5 0 0 0 1.5-1.5v-4"),
    copy: () => icon("M9 9h9.5v11H9z", "M15 9V4.5H5.5v11H9"),
    check: () => icon("M5.5 12.5l4 4 9-9"),
    bang: () => icon("M12 7v6M12 17h.01"),
    chevron: () => icon("M8 10l4 4 4-4"),
  };

  /* ---------- Formatting ---------- */

  function stamp(sec) {
    if (sec == null || !isFinite(sec)) return "--:--";
    const tenths = Math.round(sec * 10);
    const s = Math.floor(tenths / 10);
    const h = Math.floor(s / 3600);
    const m = Math.floor((s % 3600) / 60);
    const ss = String(s % 60).padStart(2, "0");
    const t = tenths % 10;
    return (h ? `${h}:${String(m).padStart(2, "0")}` : `${m}`) + `:${ss}.${t}`;
  }

  function bytes(n) {
    if (!n || n <= 0) return "";
    const units = ["B", "KB", "MB", "GB"];
    let i = 0;
    while (n >= 1024 && i < units.length - 1) { n /= 1024; i++; }
    return `${n.toFixed(i >= 2 ? 1 : 0)} ${units[i]}`;
  }

  const clockTime = () => new Date().toTimeString().slice(0, 8);

  /* ---------- The page ---------- */

  const isYouTube = () => /(^|\.)youtube\.com$/.test(location.hostname);

  /** The video on screen: YouTube's player, or the biggest one elsewhere. */
  function pageVideo() {
    const yt = document.querySelector("#movie_player video");
    if (yt) return yt;
    let best = null;
    let bestArea = 0;
    for (const v of document.querySelectorAll("video")) {
      const r = v.getBoundingClientRect();
      const area = r.width * r.height;
      if (area > bestArea) { best = v; bestArea = area; }
    }
    return best;
  }

  /** A clean link: YouTube watch/shorts become ?v=ID, without playlists. */
  function pageUrl() {
    const u = new URL(location.href);
    if (isYouTube()) {
      const short = u.pathname.match(/^\/(?:shorts|live)\/([\w-]{6,})/);
      const id = short ? short[1] : u.searchParams.get("v");
      if (id) return `https://www.youtube.com/watch?v=${id}`;
    }
    u.hash = "";
    return u.toString();
  }

  function pageTitle() {
    if (isYouTube()) {
      const h1 = document.querySelector("ytd-watch-metadata h1, h1.ytd-watch-metadata");
      if (h1 && h1.textContent.trim()) return h1.textContent.trim();
      return document.title.replace(/^\(\d+\)\s*/, "").replace(/\s*-\s*YouTube$/, "");
    }
    const og = document.querySelector('meta[property="og:title"]');
    return (og && og.content) || document.title || location.hostname;
  }

  function pageThumbnail() {
    const u = new URL(pageUrl());
    if (isYouTube() && u.searchParams.get("v")) return `https://i.ytimg.com/vi/${u.searchParams.get("v")}/hqdefault.jpg`;
    const og = document.querySelector('meta[property="og:image"]');
    return og ? og.content : null;
  }

  /* ---------- State ---------- */

  const state = {
    connected: null,
    ovalFolder: "",
    dest: null, // { path, name } chosen in the panel, or null for Oval's folder
    clipIn: null,
    clipOut: null,
    quality: "max",
    format: "md",
    url: pageUrl(),
    jobs: new Map(), // id -> { job, label, el }
    logs: [],
    minimized: false,
  };

  /* ---------- Talking to Oval (through the background script) ---------- */

  async function oval(path, body) {
    const res = await api.runtime.sendMessage({ type: "oval", path, body });
    if (!res) throw new Error("La extensión no respondió");
    if (!res.ok) throw new Error(res.error);
    return res.data;
  }

  async function connect() {
    try {
      const hello = await oval("/v1/hello");
      const first = !state.connected;
      if (first) log(`Conectado con Oval ${hello.version}`);
      state.connected = true;
      if (first) setTimeout(checkLive, 0);
      state.ovalFolder = hello.folder || "";
    } catch (err) {
      if (state.connected !== false) log(String(err.message || err), "error");
      state.connected = false;
    }
    renderStatus();
    renderDest();
    return state.connected;
  }

  /* ---------- Styles ---------- */

  const CSS = `
:host { all: initial; }
.panel, .bubble {
  --bg-1: #24242c; --bg-2: #15151a; --bg-3: #0d0d10;
  --surface: rgba(255,255,255,.045); --surface-2: rgba(255,255,255,.075); --surface-3: rgba(255,255,255,.11);
  --line: rgba(255,255,255,.08); --line-2: rgba(255,255,255,.14);
  --text: #f2f2f5; --text-2: rgba(242,242,245,.62); --text-3: rgba(242,242,245,.38);
  --mint: #3ddc97; --mint-ink: #06291a; --mint-soft: rgba(61,220,151,.16);
  --red: #ff5d6c; --red-soft: rgba(255,93,108,.18); --amber: #ffc861;
  --shadow: rgba(0,0,0,.45);
  --ease: cubic-bezier(.22,1,.36,1);
  font: 500 12.5px/1.35 "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, "Helvetica Neue", sans-serif;
  color: var(--text);
  color-scheme: dark;
  -webkit-font-smoothing: antialiased;
}
@media (prefers-color-scheme: light) {
  .panel, .bubble {
    --bg-1: #ffffff; --bg-2: #f3f3f6; --bg-3: #e9e9ee;
    --surface: rgba(20,20,40,.04); --surface-2: rgba(20,20,40,.07); --surface-3: rgba(20,20,40,.11);
    --line: rgba(20,20,40,.08); --line-2: rgba(20,20,40,.14);
    --text: #17171c; --text-2: rgba(23,23,28,.62); --text-3: rgba(23,23,28,.4);
    --mint: #17b878; --mint-ink: #ffffff; --mint-soft: rgba(23,184,120,.14);
    --red: #ec3f52; --red-soft: rgba(236,63,82,.13);
    --shadow: rgba(20,20,40,.25);
    color-scheme: light;
  }
}
* { box-sizing: border-box; }
svg { width: 100%; height: 100%; fill: none; stroke: currentColor; stroke-width: 1.8; stroke-linecap: round; stroke-linejoin: round; }
button { font: inherit; color: inherit; border: 0; background: none; padding: 0; margin: 0; cursor: pointer; }
button:disabled { cursor: default; opacity: .45; }
button:focus-visible, select:focus-visible, textarea:focus-visible, input:focus-visible { outline: 2px solid var(--mint); outline-offset: 2px; }

.panel {
  position: fixed; z-index: 2147483647;
  width: 340px;
  display: flex; flex-direction: column; gap: 10px;
  padding: 14px 14px 12px;
  border-radius: 30px;
  overflow: hidden auto; scrollbar-width: none;
  background:
    radial-gradient(70% 40% at 50% 0%, rgba(61,220,151,.08), transparent 70%),
    radial-gradient(120% 90% at 50% 0%, var(--bg-1) 0%, var(--bg-2) 60%, var(--bg-3) 100%);
  box-shadow: 0 18px 50px -12px var(--shadow), 0 2px 8px var(--shadow), inset 0 1px 0 rgba(255,255,255,.07), inset 0 0 0 1px var(--line);
  animation: rise .45s var(--ease) both;
  user-select: none;
}
.panel::-webkit-scrollbar { display: none; }
@keyframes rise { from { opacity: 0; transform: translateY(8px) scale(.96); } }
.hidden { display: none !important; }

/* Title bar */
.bar { display: flex; align-items: center; gap: 8px; height: 30px; padding: 0 2px; cursor: grab; }
.bar:active { cursor: grabbing; }
.logo { width: 22px; height: 22px; border-radius: 50%; background: conic-gradient(var(--mint) calc(var(--p, 0) * 1turn), var(--surface-3) 0); padding: 3px; flex: none; }
.logo::after { content: ""; display: block; width: 100%; height: 100%; border-radius: 50%; background: var(--bg-1); }
.brand { font-size: 13px; font-weight: 600; letter-spacing: .32em; color: var(--text-2); text-transform: lowercase; }
.state { display: inline-flex; align-items: center; gap: 6px; margin-left: auto; font-size: 11px; color: var(--text-3); }
.state::before { content: ""; width: 7px; height: 7px; border-radius: 50%; background: var(--text-3); }
.state.on::before { background: var(--mint); box-shadow: 0 0 0 3px var(--mint-soft); }
.state.off { color: var(--red); cursor: pointer; }
.state.off::before { background: var(--red); box-shadow: 0 0 0 3px var(--red-soft); }
.dot { width: 22px; height: 22px; display: grid; place-items: center; border-radius: 50%; background: var(--surface); color: var(--text-3); box-shadow: inset 0 0 0 1px var(--line); transition: background .2s, color .2s; flex: none; }
.dot svg { width: 12px; height: 12px; }
.dot:hover { background: var(--surface-3); color: var(--text); }
.dot.close:hover { background: var(--red); color: #fff; box-shadow: none; }

/* Cards */
.card { display: flex; flex-direction: column; gap: 9px; padding: 12px; border-radius: 22px; background: var(--surface); box-shadow: inset 0 0 0 1px var(--line); }
.label { font-size: 10.5px; font-weight: 700; letter-spacing: .14em; text-transform: uppercase; color: var(--text-3); padding: 0 4px; }
.row { display: flex; align-items: center; gap: 8px; }
.row.two > * { flex: 1; }

/* Clip marks */
.mark { height: 30px; padding: 0 13px; border-radius: 999px; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--line); font-weight: 700; font-size: 11.5px; letter-spacing: .04em; transition: background .2s; }
.mark:hover { background: var(--surface-3); }
.mark.set { background: var(--mint-soft); color: var(--mint); box-shadow: inset 0 0 0 1px rgba(61,220,151,.35); }
.mark-time { font-variant-numeric: tabular-nums; color: var(--text-2); font-size: 12px; cursor: pointer; }
.mark-time:hover { color: var(--text); }
.reset { margin-left: auto; }
.reset:hover { background: var(--red-soft) !important; color: var(--red) !important; }

/* Two-handle range */
.range { position: relative; height: 22px; margin: 0 4px; }
.range .track { position: absolute; left: 0; right: 0; top: 9px; height: 4px; border-radius: 4px; background: var(--surface-3); }
.range .fill { position: absolute; top: 9px; height: 4px; border-radius: 4px; background: var(--mint); }
.range input { position: absolute; inset: 0; width: 100%; margin: 0; background: none; pointer-events: none; -webkit-appearance: none; appearance: none; }
.range input::-webkit-slider-thumb { -webkit-appearance: none; pointer-events: auto; width: 16px; height: 16px; border-radius: 50%; background: #fff; box-shadow: 0 0 0 3px var(--mint), 0 2px 6px var(--shadow); cursor: grab; }
.range input::-moz-range-thumb { pointer-events: auto; width: 14px; height: 14px; border: 0; border-radius: 50%; background: #fff; box-shadow: 0 0 0 3px var(--mint), 0 2px 6px var(--shadow); cursor: grab; }
.range input::-moz-range-track { background: none; }
.range.off { opacity: .4; }
.times { display: flex; justify-content: space-between; font-size: 11px; color: var(--text-3); font-variant-numeric: tabular-nums; padding: 0 2px; }
.times b { color: var(--text-2); font-weight: 600; }

.check { display: inline-flex; align-items: center; gap: 8px; color: var(--text-2); cursor: pointer; font-size: 12px; padding: 0 2px; }
.check input { -webkit-appearance: none; appearance: none; width: 16px; height: 16px; margin: 0; border-radius: 50%; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--line-2); display: grid; place-items: center; cursor: pointer; }
.check input:checked { background: var(--mint); box-shadow: none; }
.check input:checked::after { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--mint-ink); }

/* Segmented control */
.seg { display: flex; padding: 3px; gap: 2px; border-radius: 999px; background: var(--surface); box-shadow: inset 0 0 0 1px var(--line); }
.seg button { flex: 1; height: 26px; border-radius: 999px; font-size: 11.5px; font-weight: 700; letter-spacing: .03em; color: var(--text-3); transition: background .2s, color .2s; }
.seg button:hover { color: var(--text-2); }
.seg button.on { background: var(--surface-3); color: var(--text); box-shadow: inset 0 0 0 1px var(--line); }

/* Buttons */
.cta { display: flex; align-items: center; justify-content: center; gap: 7px; height: 38px; border-radius: 999px; background: var(--mint); color: var(--mint-ink); font-weight: 700; font-size: 12.5px; box-shadow: 0 6px 18px -8px rgba(61,220,151,.75), inset 0 1px 0 rgba(255,255,255,.35); transition: transform .2s var(--ease), filter .2s; }
.cta svg { width: 16px; height: 16px; stroke-width: 2.2; }
.cta:hover:not(:disabled) { transform: scale(1.02); }
.cta:active:not(:disabled) { transform: scale(.97); }
.pill { display: flex; align-items: center; justify-content: center; gap: 7px; height: 34px; padding: 0 12px; border-radius: 999px; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--line); font-weight: 600; font-size: 12px; white-space: nowrap; transition: background .2s, transform .2s var(--ease); }
.pill svg { width: 15px; height: 15px; color: var(--text-2); flex: none; }
.pill:hover:not(:disabled) { background: var(--surface-3); }
.pill:active:not(:disabled) { transform: scale(.97); }
.pill.clip span::after { content: ""; display: inline-block; width: 6px; height: 6px; margin-left: 6px; border-radius: 50%; background: var(--mint); vertical-align: 1px; }
.pill.busy svg { animation: spin .9s linear infinite; }
@keyframes spin { to { transform: rotate(360deg); } }
.round { width: 34px; height: 34px; flex: none; display: grid; place-items: center; border-radius: 50%; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--line); color: var(--text-2); transition: background .2s, color .2s; }
.round svg { width: 15px; height: 15px; }
.round:hover { background: var(--surface-3); color: var(--text); }

select { flex: 1; min-width: 0; height: 32px; padding: 0 12px; border: 0; border-radius: 999px; background: var(--surface-2); box-shadow: inset 0 0 0 1px var(--line); color: var(--text); font: inherit; font-weight: 600; font-size: 12px; cursor: pointer; }
select option { background: var(--bg-2); color: var(--text); }
.row-label { color: var(--text-2); font-size: 12px; padding-left: 4px; white-space: nowrap; }

textarea { width: 100%; min-height: 58px; resize: vertical; padding: 10px 14px; border: 0; border-radius: 20px; background: var(--surface); box-shadow: inset 0 0 0 1px var(--line); color: var(--text); font: inherit; font-weight: 500; user-select: text; }
textarea::placeholder { color: var(--text-3); }

.slot { display: contents; }
.live-label { display: flex; align-items: center; gap: 7px; color: var(--red); }
.live-label::before { content: ""; width: 7px; height: 7px; border-radius: 50%; background: var(--red); animation: blink 1.4s ease-in-out infinite; }
@keyframes blink { 50% { opacity: .25; } }
.live-clock { margin-left: auto; color: var(--text-2); letter-spacing: 0; font-variant-numeric: tabular-nums; }
.pill .rec-dot { width: 10px; height: 10px; border-radius: 50%; background: var(--red); flex: none; }
.hint { padding: 0 4px; font-size: 11px; color: var(--text-3); }
.get-oval { align-self: flex-start; margin-left: 6px; text-decoration: none; }

/* Jobs */
.jobs { display: flex; flex-direction: column; gap: 6px; }
.jobs:empty { display: none; }
.job { display: flex; align-items: center; gap: 10px; padding: 6px 8px 6px 6px; border-radius: 999px; background: var(--surface); box-shadow: inset 0 0 0 1px var(--line); animation: pop .35s var(--ease) both; }
.job.done { cursor: pointer; }
.job.done:hover { background: var(--surface-2); }
@keyframes pop { from { opacity: 0; transform: translateY(6px) scale(.97); } }
.ring { --p: 0; --c: var(--mint); position: relative; flex: none; width: 30px; height: 30px; border-radius: 50%; padding: 3px; background: conic-gradient(var(--c) calc(var(--p) * 1turn), var(--surface-2) 0); }
.ring span { display: grid; place-items: center; width: 100%; height: 100%; border-radius: 50%; background: var(--bg-2); color: var(--mint); }
.ring svg { width: 13px; height: 13px; stroke-width: 2.4; }
.job.error .ring { --c: var(--red); --p: 1; }
.job.live.downloading .ring, .job.live.starting .ring { --c: var(--red); --p: 1 !important; animation: pulse 1.8s ease-in-out infinite; }
@keyframes pulse { 0%, 100% { box-shadow: 0 0 0 0 rgba(255,93,108,.45); } 50% { box-shadow: 0 0 0 5px rgba(255,93,108,0); } }
.job.error .ring span { color: var(--red); }
.job.queued .ring, .job.starting .ring { animation: breathe 1.4s ease-in-out infinite; }
@keyframes breathe { 50% { opacity: .55; } }
.job-info { flex: 1; min-width: 0; }
.job-title { font-weight: 600; font-size: 12px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; font-variant-numeric: tabular-nums; }
.job-meta { font-size: 11px; color: var(--text-3); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.job.error .job-meta { color: var(--red); }
.job .x { width: 24px; height: 24px; flex: none; display: grid; place-items: center; border-radius: 50%; color: var(--text-3); }
.job .x svg { width: 12px; height: 12px; }
.job .x:hover { background: var(--red-soft); color: var(--red); }

/* Destination */
.dest { display: flex; align-items: center; gap: 6px; }
.dest .pill { flex: 1; min-width: 0; justify-content: flex-start; }
.dest .pill span { overflow: hidden; text-overflow: ellipsis; }
.dest-kind { flex: none; max-width: 120px; }

/* Status and log */
.status { min-height: 16px; padding: 0 6px; font-size: 11.5px; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.status.error { color: var(--red); }
.log { border-radius: 20px; background: var(--surface); box-shadow: inset 0 0 0 1px var(--line); overflow: hidden; }
.log-head { display: flex; align-items: center; gap: 6px; padding: 6px 6px 6px 12px; }
.log-head .label { flex: 1; cursor: pointer; padding: 0; display: flex; align-items: center; gap: 4px; }
.log-head .label svg { width: 12px; height: 12px; transition: transform .2s; }
.log.closed .log-head .label svg { transform: rotate(-90deg); }
.mini { height: 24px; padding: 0 10px; display: inline-flex; align-items: center; gap: 5px; border-radius: 999px; background: var(--surface-2); color: var(--text-2); font-size: 11px; font-weight: 600; }
.mini svg { width: 12px; height: 12px; }
.mini:hover { background: var(--surface-3); color: var(--text); }
.log pre { margin: 0; padding: 4px 12px 10px; max-height: 120px; overflow: auto; font: 11px/1.45 ui-monospace, "Cascadia Mono", Consolas, monospace; color: var(--text-2); white-space: pre-wrap; user-select: text; scrollbar-width: thin; }
.log.closed pre { display: none; }
.log pre .error { color: var(--red); }
.log pre .warn { color: var(--amber); }

/* Minimized: a round bubble with the overall progress */
.bubble { position: fixed; z-index: 2147483647; width: 54px; height: 54px; border-radius: 50%; padding: 4px; cursor: pointer;
  background: conic-gradient(var(--mint) calc(var(--p, 0) * 1turn), var(--surface-3) 0);
  box-shadow: 0 10px 30px -8px var(--shadow), 0 2px 6px var(--shadow); animation: rise .35s var(--ease) both; }
.bubble span { display: grid; place-items: center; width: 100%; height: 100%; border-radius: 50%; background: radial-gradient(120% 95% at 50% 8%, var(--bg-1), var(--bg-2)); color: var(--text-2); font-size: 10px; font-weight: 700; letter-spacing: .2em; text-indent: .2em; }
.bubble:hover span { color: var(--text); }
`;

  /* ---------- Building the panel ---------- */

  const host = h("div", { id: "oval-panel-host" });
  const root = host.attachShadow({ mode: "closed" });
  // A constructed sheet isn't blocked by a page's style-src policy; Firefox
  // content scripts can't always adopt one, so fall back to <style>.
  try {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(CSS);
    root.adoptedStyleSheets = [sheet];
  } catch {
    root.append(h("style", { text: CSS }));
  }

  const el = {};

  el.state = h("span", { class: "state", text: "Conectando…", title: "Reintentar", onclick: () => (state.connected ? connect() : openOval()) });
  el.logo = h("span", { class: "logo" });
  const bar = h("header", { class: "bar" },
    el.logo,
    h("span", { class: "brand", text: "oval" }),
    el.state,
    h("button", { class: "dot", title: "Minimizar", "aria-label": "Minimizar", onclick: () => setMinimized(true) }, I.min()),
    h("button", { class: "dot close", title: "Cerrar", "aria-label": "Cerrar", onclick: () => setVisible(false) }, I.close()),
  );

  // Clip
  el.inBtn = h("button", { class: "mark", title: "Marcar el inicio en el momento actual", onclick: () => mark("in") }, "[ IN");
  el.outBtn = h("button", { class: "mark", title: "Marcar el final en el momento actual", onclick: () => mark("out") }, "OUT ]");
  el.reset = h("button", { class: "dot reset", title: "Quitar el tramo", "aria-label": "Quitar el tramo", onclick: () => { state.clipIn = state.clipOut = null; renderClip(); } }, I.close());
  el.rangeIn = h("input", { type: "range", min: 0, max: 1, step: 0.1, value: 0, "aria-label": "Inicio" });
  el.rangeOut = h("input", { type: "range", min: 0, max: 1, step: 0.1, value: 1, "aria-label": "Final" });
  el.fill = h("span", { class: "fill" });
  el.range = h("div", { class: "range" }, h("span", { class: "track" }), el.fill, el.rangeIn, el.rangeOut);
  el.timeIn = h("span", { class: "mark-time", title: "Ir al inicio", onclick: () => seek(state.clipIn) });
  el.timeOut = h("span", { class: "mark-time", title: "Ir al final", onclick: () => seek(state.clipOut) });
  el.len = h("b");
  el.precise = h("input", { type: "checkbox" });
  el.quality = h("div", { class: "seg", role: "group", "aria-label": "Calidad" },
    ...[["max", "Máx"], ["1080", "1080p"], ["720", "720p"], ["audio", "Audio"]].map(([q, label]) =>
      h("button", { "data-q": q, class: q === state.quality ? "on" : null, onclick: () => setQuality(q) }, label)));
  el.queueBtn = h("button", { class: "cta", onclick: sendClip }, I.plus(), "Mandar a la cola");
  el.jobs = h("div", { class: "jobs" });
  el.preciseRow = h("label", { class: "check", title: "Corta en el fotograma exacto (recodifica los bordes, tarda un poco más)" }, el.precise, "Preciso (fotograma)");
  // Quality, precise cut and the job list are shared: they move to whichever
  // card is showing.
  el.clipSlot = h("div", { class: "slot" }, el.preciseRow, el.quality);
  el.clipJobs = h("div", { class: "slot" }, el.jobs);
  const clipCard = h("section", { class: "card" },
    h("div", { class: "label", text: "Clip actual" }),
    h("div", { class: "row" }, el.inBtn, el.outBtn, el.reset),
    el.range,
    h("div", { class: "times" }, el.timeIn, el.len, el.timeOut),
    el.clipSlot,
    el.queueBtn,
    el.clipJobs,
  );

  // Live stream (Twitch, Kick): parts are cut from the recording the site
  // keeps while the stream runs, in seconds from the start of the stream.
  el.liveClock = h("span", { class: "live-clock" });
  el.liveIn = h("button", { class: "mark", title: "Marcar el inicio en este momento del directo", onclick: () => markLive("in") }, "[ IN");
  el.liveOut = h("button", { class: "mark", title: "Marcar el final en este momento del directo", onclick: () => markLive("out") }, "OUT ]");
  el.liveReset = h("button", { class: "dot reset", title: "Quitar el tramo", "aria-label": "Quitar el tramo", onclick: () => { state.liveIn = state.liveOut = null; renderLive(); } }, I.close());
  el.liveTimes = h("div", { class: "times" });
  el.liveLast = h("div", { class: "seg", role: "group", "aria-label": "Últimos minutos" },
    ...[[1, "Último min"], [5, "5 min"], [15, "15 min"], [60, "1 h"]].map(([m, label]) =>
      h("button", { title: `Los últimos ${m === 60 ? "60" : m} minutos del directo`, onclick: () => lastMinutes(m) }, label)));
  el.liveSlot = h("div", { class: "slot" });
  el.liveJobs = h("div", { class: "slot" });
  el.liveQueue = h("button", { class: "cta", onclick: sendLiveClip }, I.plus(), "Mandar a la cola");
  el.recordBtn = h("button", { class: "pill", title: "Graba el directo en Oval hasta que lo detengas", onclick: sendRecord }, h("span", { class: "rec-dot" }), "Grabar desde ahora");
  el.fromStart = h("button", { class: "pill", title: "Todo lo emitido hasta ahora", onclick: sendFromStart }, I.down(), "Desde el inicio");
  el.liveHint = h("div", { class: "hint" });
  const liveCard = h("section", { class: "card hidden" },
    h("div", { class: "label live-label" }, "En directo", el.liveClock),
    h("div", { class: "row" }, el.liveIn, el.liveOut, el.liveReset),
    el.liveTimes,
    el.liveLast,
    el.liveSlot,
    el.liveQueue,
    h("div", { class: "row two" }, el.recordBtn, el.fromStart),
    el.liveHint,
    el.liveJobs,
  );

  // Export
  el.format = h("select", { "aria-label": "Formato de la transcripción", onchange: () => { state.format = el.format.value; save({ format: state.format }); } },
    ...[["md", "Markdown limpio"], ["md-times", "Markdown con tiempos"], ["txt", "Texto"], ["srt", "Subtítulos SRT"], ["vtt", "Subtítulos VTT"]].map(([v, label]) =>
      h("option", { value: v, text: label })));
  el.fullBtn = h("button", { class: "pill", onclick: sendFull }, I.down(), "Vídeo completo");
  el.transcriptBtn = h("button", { class: "pill", onclick: sendTranscript }, I.doc(), h("span", { text: "Transcripción" }));
  el.thumbBtn = h("button", { class: "pill", onclick: sendThumbnail }, I.image(), "Miniatura");
  el.ytBtn = h("button", { class: "pill", title: "Abre el panel de transcripción de YouTube", onclick: openYouTubeTranscript }, I.play(), "Ver en YouTube");
  const exportCard = h("section", { class: "card" },
    h("div", { class: "label", text: "Exportar" }),
    h("div", { class: "row" }, h("span", { class: "row-label", text: "Transcripción" }), el.format),
    h("div", { class: "row two" }, el.fullBtn, el.transcriptBtn),
    h("div", { class: "row two" }, el.thumbBtn, el.ytBtn),
  );

  el.note = h("textarea", { placeholder: "Nota personal (TL;DR) — opcional, va en la transcripción", rows: 2 });

  // Destination: a folder for now; OpenPremier projects come later.
  el.destName = h("span");
  el.destBtn = h("button", { class: "pill", title: "Abrir la carpeta de destino", onclick: revealDest }, I.folder(), el.destName);
  el.destKind = h("select", { class: "dest-kind", "aria-label": "Destino", onchange: onDestKind },
    h("option", { value: "folder", text: "Carpeta" }),
    h("option", { value: "openpremier", text: "OpenPremier · pronto", disabled: true }));
  const dest = h("div", { class: "dest" },
    el.destKind,
    el.destBtn,
    h("button", { class: "round", title: "Elegir carpeta", "aria-label": "Elegir carpeta", onclick: pickDest }, I.pencil()),
  );

  el.status = h("div", { class: "status", role: "status" });
  el.getOval = h("a", { class: "mini get-oval hidden", href: "https://github.com/xXKuroiKenshiXx/Oval/releases/latest", target: "_blank", rel: "noopener" }, I.down(), "Descargar Oval");
  el.logText = h("pre");
  el.log = h("div", { class: "log closed" },
    h("div", { class: "log-head" },
      h("span", { class: "label", onclick: () => el.log.classList.toggle("closed") }, I.chevron(), "Registro"),
      h("button", { class: "mini", onclick: copyLog }, I.copy(), "copiar"),
      h("button", { class: "mini", title: "Vaciar", "aria-label": "Vaciar el registro", onclick: () => { state.logs = []; renderLog(); } }, I.close())),
    el.logText,
  );

  const panel = h("div", { class: "panel", role: "dialog", "aria-label": "Oval" }, bar, clipCard, liveCard, exportCard, el.note, dest, el.status, el.getOval, el.log);
  const bubble = h("button", { class: "bubble hidden", title: "Abrir Oval", "aria-label": "Abrir Oval", onclick: () => setMinimized(false) }, h("span", { text: "oval" }));
  root.append(panel, bubble);
  (document.body || document.documentElement).append(host);

  /* ---------- Position (draggable, remembered) ---------- */

  /** Keeps the panel inside the window; `null` means the top right corner. */
  function place(pos) {
    const w = panel.offsetWidth || 340;
    const width = window.innerWidth || document.documentElement.clientWidth;
    const height = window.innerHeight || document.documentElement.clientHeight;
    const want = pos || { x: width - w - 24, y: 72 };
    const x = Math.max(8, Math.min(want.x, width - Math.min(w, 120)));
    const y = Math.max(8, Math.min(want.y, height - 60));
    for (const node of [panel, bubble]) {
      node.style.left = `${x}px`;
      node.style.top = `${y}px`;
    }
    panel.style.maxHeight = `${Math.max(240, height - y - 12)}px`;
    state.pos = pos;
  }
  state.pos = null;
  place(null);
  window.addEventListener("resize", () => place(state.pos));

  bar.addEventListener("pointerdown", (e) => {
    if (e.target.closest("button, .state")) return;
    const r = (state.minimized ? bubble : panel).getBoundingClientRect();
    const start = { x: r.left, y: r.top };
    bar.setPointerCapture(e.pointerId);
    const move = (ev) => place({ x: start.x + ev.clientX - start.x0, y: start.y + ev.clientY - start.y0 });
    start.x0 = e.clientX;
    start.y0 = e.clientY;
    const up = () => {
      bar.removeEventListener("pointermove", move);
      bar.removeEventListener("pointerup", up);
      save({ pos: state.pos });
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", up);
  });

  /* ---------- Visibility ---------- */

  function setVisible(on) {
    host.style.display = on ? "" : "none";
    if (on) connect();
  }

  function setMinimized(on) {
    state.minimized = on;
    panel.classList.toggle("hidden", on);
    bubble.classList.toggle("hidden", !on);
  }

  window.__ovalPanel = {
    toggle() {
      const hidden = host.style.display === "none";
      if (hidden) { setVisible(true); setMinimized(false); }
      else if (state.minimized) setMinimized(false);
      else setVisible(false);
    },
  };

  /* ---------- Storage ---------- */

  function save(values) {
    api.storage.local.set(values).catch(() => {});
  }

  /* ---------- Log and status ---------- */

  function log(msg, level = "info") {
    state.logs.push({ t: clockTime(), msg, level });
    if (state.logs.length > 200) state.logs.shift();
    renderLog();
    setStatus(msg, level === "error");
  }

  function setStatus(msg, error = false) {
    el.status.textContent = msg;
    el.status.classList.toggle("error", error);
  }

  function renderLog() {
    el.logText.replaceChildren(...state.logs.map((l) =>
      h("div", { class: l.level, text: `[${l.t}] ${l.msg}` })));
    el.logText.scrollTop = el.logText.scrollHeight;
  }

  async function copyLog() {
    const text = state.logs.map((l) => `[${l.t}] ${l.msg}`).join("\n");
    try {
      await navigator.clipboard.writeText(text);
      setStatus("Registro copiado");
    } catch {
      setStatus("No se pudo copiar el registro", true);
    }
  }

  function renderStatus() {
    el.state.className = "state" + (state.connected ? " on" : state.connected === false ? " off" : "");
    el.state.textContent = state.connected ? "Oval" : state.connected === false ? "Abrir Oval" : "Conectando…";
    el.state.title = state.connected ? "Conectado con la app de Oval" : "Abre la app de Oval de este equipo";
    el.getOval.classList.toggle("hidden", !!state.connected || !state.missing);
  }

  /* ---------- Destination ---------- */

  const baseName = (p) => (p || "").split(/[\\/]/).filter(Boolean).pop() || p || "";

  function destPath() {
    return state.dest && state.dest.path ? state.dest.path : undefined;
  }

  function renderDest() {
    const path = destPath() || state.ovalFolder;
    el.destName.textContent = state.dest ? baseName(state.dest.path) : `${baseName(state.ovalFolder) || "Oval"} (Oval)`;
    el.destBtn.title = path ? `Abrir ${path}` : "Abrir la carpeta de destino";
  }

  async function pickDest() {
    if (!(await ensureConnected())) return;
    log("Elige la carpeta en la ventana de Oval…");
    try {
      const res = await oval("/v1/pick-folder", { folder: destPath() || state.ovalFolder });
      if (!res || !res.path) return setStatus("Sin cambios");
      state.dest = { path: res.path };
      save({ dest: state.dest });
      renderDest();
      log(`Destino: ${res.path}`);
    } catch (err) {
      log(String(err.message || err), "error");
    }
  }

  async function revealDest() {
    if (!(await ensureConnected())) return;
    try {
      await oval("/v1/reveal", { path: destPath() });
    } catch (err) {
      log(String(err.message || err), "error");
    }
  }

  function onDestKind() {
    // Only folders exist for now; OpenPremier projects will be listed here.
    el.destKind.value = "folder";
  }

  /* ---------- Clip ---------- */

  function video() {
    const v = pageVideo();
    return v && isFinite(v.duration) && v.duration > 0 ? v : null;
  }

  function seek(t) {
    const v = video();
    if (v && t != null) v.currentTime = t;
  }

  function mark(which) {
    const v = video();
    if (!v) return log("No hay un vídeo reproduciéndose en esta página", "warn");
    const t = Math.round(v.currentTime * 10) / 10;
    if (which === "in") {
      state.clipIn = t;
      if (state.clipOut != null && state.clipOut <= t) state.clipOut = null;
    } else {
      state.clipOut = t;
      if (state.clipIn != null && state.clipIn >= t) state.clipIn = null;
    }
    renderClip();
  }

  /** The selected range, or null when the whole video is selected. */
  function section() {
    const v = video();
    if (state.clipIn == null && state.clipOut == null) return null;
    const start = state.clipIn == null ? 0 : state.clipIn;
    const end = state.clipOut == null ? (v ? v.duration : null) : state.clipOut;
    if (end == null || end - start < 0.1) return null;
    return { start, end };
  }

  function renderClip() {
    const v = video();
    const dur = v ? v.duration : 0;
    const s = section();
    el.range.classList.toggle("off", !v);
    for (const input of [el.rangeIn, el.rangeOut]) {
      input.max = dur || 1;
      input.disabled = !v;
    }
    const a = s ? s.start : 0;
    const b = s ? s.end : dur || 1;
    el.rangeIn.value = a;
    el.rangeOut.value = b;
    const total = dur || 1;
    el.fill.style.left = `${(a / total) * 100}%`;
    el.fill.style.width = `${((b - a) / total) * 100}%`;
    el.inBtn.classList.toggle("set", state.clipIn != null);
    el.outBtn.classList.toggle("set", state.clipOut != null);
    el.timeIn.textContent = stamp(s ? s.start : v ? 0 : null);
    el.timeOut.textContent = stamp(s ? s.end : v ? dur : null);
    el.len.textContent = s ? `${(s.end - s.start).toFixed(1)} s` : v ? "vídeo entero" : "sin vídeo";
    el.reset.disabled = !s;
    el.queueBtn.disabled = !s;
    // With a range marked, only that part is transcribed.
    el.transcriptBtn.classList.toggle("clip", !!s);
    el.transcriptBtn.title = s ? "Solo el tramo marcado" : "Todo el vídeo";
  }

  function onRange(which) {
    const v = video();
    if (!v) return;
    let a = Number(el.rangeIn.value);
    let b = Number(el.rangeOut.value);
    if (which === "in") a = Math.min(a, b - 0.1);
    else b = Math.max(b, a + 0.1);
    const round = (t) => Math.round(t * 10) / 10;
    state.clipIn = a <= 0 ? null : round(a);
    state.clipOut = b >= v.duration - 0.1 ? null : round(b);
    // Show the frame under the handle while dragging.
    v.currentTime = which === "in" ? Math.max(0, a) : b;
    renderClip();
  }
  el.rangeIn.addEventListener("input", () => onRange("in"));
  el.rangeOut.addEventListener("input", () => onRange("out"));

  function setQuality(q) {
    state.quality = q;
    for (const b of el.quality.querySelectorAll("button")) b.classList.toggle("on", b.dataset.q === q);
    save({ quality: q });
  }

  /* ---------- Sending work to Oval ---------- */

  async function ensureConnected() {
    if (state.connected) return true;
    if (await connect()) return true;
    return openOval();
  }

  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  /** Starts the Oval app through its oval:// link, which every copy of
   *  Oval (installed or portable) registers for itself when it runs. */
  async function openOval() {
    log("Abriendo Oval…");
    const link = h("a", { href: "oval://open", style: "display:none" });
    root.append(link);
    link.click();
    link.remove();
    for (let i = 0; i < 25; i++) {
      await sleep(1000);
      if (await connect()) {
        state.missing = false;
        renderStatus();
        return true;
      }
    }
    state.missing = true;
    renderStatus();
    log("No se pudo abrir Oval. Si no lo tienes, descárgalo; si lo tienes, ábrelo una vez y luego este botón lo abrirá.", "error");
    return false;
  }

  function downloadBody(extra) {
    const audio = state.quality === "audio";
    return {
      url: pageUrl(),
      thumbnail: pageThumbnail(),
      mode: audio ? "audio" : "video",
      quality: audio || state.quality === "max" ? undefined : state.quality,
      folder: destPath(),
      ...extra,
    };
  }

  async function sendClip() {
    const s = section();
    if (!s || !(await ensureConnected())) return;
    const label = `${stamp(s.start)} → ${stamp(s.end)} (${(s.end - s.start).toFixed(1)} s)`;
    try {
      const job = await oval("/v1/download", downloadBody({
        section: s,
        precise: el.precise.checked,
        title: `${pageTitle()} · ${stamp(s.start)}–${stamp(s.end)}`,
      }));
      track(job, label);
      log(`Clip en la cola: ${label}`);
    } catch (err) {
      log(String(err.message || err), "error");
    }
  }

  async function sendFull() {
    if (state.live) return sendFromStart();
    if (!(await ensureConnected())) return;
    try {
      const job = await oval("/v1/download", downloadBody({ title: pageTitle() }));
      track(job, state.quality === "audio" ? "Audio completo" : "Vídeo completo");
      log("Vídeo completo en la cola");
    } catch (err) {
      log(String(err.message || err), "error");
    }
  }

  async function busy(button, msg, work) {
    if (button.disabled || !(await ensureConnected())) return;
    button.disabled = true;
    button.classList.add("busy");
    log(msg);
    try {
      await work();
    } catch (err) {
      log(String(err.message || err), "error");
    } finally {
      button.disabled = false;
      button.classList.remove("busy");
    }
  }

  function sendTranscript() {
    const live = state.live && state.live.vod;
    const s = live ? liveSection(true) : section();
    return busy(el.transcriptBtn, s ? "Bajando la transcripción del tramo…" : "Bajando la transcripción…", async () => {
      const res = await oval("/v1/transcript", {
        url: live ? state.live.vod : pageUrl(),
        format: state.format,
        section: s || undefined,
        note: el.note.value.trim() || undefined,
        folder: destPath(),
      });
      log(`Transcripción guardada: ${baseName(res.path)}`);
      state.lastPath = res.path;
    });
  }

  function sendThumbnail() {
    return busy(el.thumbBtn, "Bajando la miniatura…", async () => {
      const res = await oval("/v1/thumbnail", { url: pageUrl(), folder: destPath() });
      log(`Miniatura guardada: ${baseName(res.path)}`);
    });
  }

  function openYouTubeTranscript() {
    if (!isYouTube()) return;
    const expand = document.querySelector("ytd-watch-metadata #description-inline-expander #expand, ytd-text-inline-expander #expand");
    if (expand) expand.click();
    setTimeout(() => {
      const btn = document.querySelector("ytd-video-description-transcript-section-renderer button");
      if (btn) {
        btn.click();
        log("Transcripción abierta en YouTube");
      } else {
        log("Este vídeo no tiene transcripción en YouTube", "warn");
      }
    }, 450);
  }

  /* ---------- Live streams ---------- */

  // Twitch and Kick players run a few seconds behind the broadcast.
  const PLAYER_DELAY = 4;
  // The site's recording trails the stream; wait this much past a section's end.
  const RECORDING_DELAY = 25;

  const isStreamSite = () => /(^|\.)(twitch\.tv|kick\.com)$/.test(location.hostname);

  /** Seconds into the broadcast of what the player shows now. */
  function liveNow() {
    if (!state.live || !state.live.started_at) return null;
    return Math.max(0, Date.now() / 1000 - PLAYER_DELAY - state.live.started_at);
  }

  function clock(sec) {
    sec = Math.max(0, Math.floor(sec || 0));
    const hh = Math.floor(sec / 3600);
    const mm = String(Math.floor((sec % 3600) / 60)).padStart(2, "0");
    const ss = String(sec % 60).padStart(2, "0");
    return hh ? `${hh}:${mm}:${ss}` : `${Number(mm)}:${ss}`;
  }

  async function checkLive() {
    const url = pageUrl();
    if (!isStreamSite() || !state.connected) {
      setLive(null);
      return;
    }
    try {
      const info = await oval("/v1/live", { url });
      if (url !== pageUrl()) return; // navigated meanwhile
      setLive(info && info.live ? info : null);
    } catch (err) {
      setLive(null);
      log(String(err.message || err), "warn");
    }
  }

  function setLive(info) {
    const was = !!state.live;
    state.live = info;
    if (was !== !!info) {
      state.liveIn = state.liveOut = null;
      if (info) log(`En directo: ${info.title || pageTitle()}`);
    }
    liveCard.classList.toggle("hidden", !info);
    clipCard.classList.toggle("hidden", !!info);
    (info ? el.liveSlot : el.clipSlot).append(el.preciseRow, el.quality);
    (info ? el.liveJobs : el.clipJobs).append(el.jobs);
    el.fullBtn.lastChild.textContent = info ? "Desde el inicio" : "Vídeo completo";
    renderLive();
  }

  function markLive(which) {
    const t = liveNow();
    if (t == null) return log("No se sabe cuándo empezó este directo", "warn");
    if (which === "in") {
      state.liveIn = t;
      if (state.liveOut != null && state.liveOut <= t) state.liveOut = null;
    } else {
      state.liveOut = t;
      if (state.liveIn != null && state.liveIn >= t) state.liveIn = null;
    }
    renderLive();
  }

  function lastMinutes(minutes) {
    const t = liveNow();
    if (t == null) return;
    state.liveIn = Math.max(0, t - minutes * 60);
    state.liveOut = t;
    renderLive();
  }

  /** The marked part; an open end means "up to now". `ready` keeps it inside
   *  what the site has already saved. */
  function liveSection(ready = false) {
    const now = liveNow();
    if (now == null || (state.liveIn == null && state.liveOut == null)) return null;
    let end = state.liveOut == null ? now : state.liveOut;
    if (ready) end = Math.min(end, now - RECORDING_DELAY);
    const start = state.liveIn == null ? Math.max(0, end - 60) : state.liveIn;
    return end - start >= 1 ? { start, end } : null;
  }

  function renderLive() {
    if (!state.live) return;
    const now = liveNow();
    el.liveClock.textContent = now == null ? "" : clock(now);
    const s = liveSection();
    el.liveIn.classList.toggle("set", state.liveIn != null);
    el.liveOut.classList.toggle("set", state.liveOut != null);
    el.liveTimes.replaceChildren(
      h("span", { text: state.liveIn == null ? "--:--" : clock(state.liveIn) }),
      h("b", { text: s ? `${Math.round(s.end - s.start)} s` : "marca un tramo" }),
      h("span", { text: state.liveOut == null ? (state.liveIn == null ? "--:--" : "ahora") : clock(state.liveOut) }),
    );
    const vod = !!state.live.vod;
    el.liveReset.disabled = !s;
    el.liveQueue.disabled = !s || !vod;
    el.fromStart.disabled = !vod;
    for (const b of el.liveLast.querySelectorAll("button")) b.disabled = !vod;
    el.liveHint.textContent = vod
      ? "Los tramos salen de la grabación del directo; los más recientes tardan unos segundos."
      : "Este canal no guarda sus directos: solo se puede grabar desde ahora.";
  }

  async function sendLiveClip() {
    const s = liveSection();
    if (!s || !state.live || !state.live.vod || !(await ensureConnected())) return;
    const live = state.live;
    const label = `${clock(s.start)} → ${clock(s.end)} (${Math.round(s.end - s.start)} s)`;
    state.liveIn = state.liveOut = null;
    renderLive();
    // Wait until the site has saved the end of the part.
    while (liveNow() != null && liveNow() < s.end + RECORDING_DELAY) {
      setStatus(`Esperando a que se guarde ${label}… ${Math.ceil(s.end + RECORDING_DELAY - liveNow())} s`);
      await sleep(1000);
    }
    try {
      const job = await oval("/v1/download", downloadBody({
        url: live.vod,
        section: s,
        precise: el.precise.checked,
        title: `${live.title || pageTitle()} · ${clock(s.start)}–${clock(s.end)}`,
      }));
      track(job, label);
      log(`Tramo del directo en la cola: ${label}`);
    } catch (err) {
      log(String(err.message || err), "error");
    }
  }

  async function sendFromStart() {
    const live = state.live;
    const now = liveNow();
    if (!live || !live.vod || now == null || !(await ensureConnected())) return;
    const s = { start: 0, end: Math.max(1, now - RECORDING_DELAY) };
    try {
      const job = await oval("/v1/download", downloadBody({
        url: live.vod,
        section: s,
        title: `${live.title || pageTitle()} · hasta ${clock(s.end)}`,
      }));
      track(job, `Desde el inicio hasta ${clock(s.end)}`);
      log("Directo desde el inicio en la cola");
    } catch (err) {
      log(String(err.message || err), "error");
    }
  }

  async function sendRecord() {
    if (!(await ensureConnected())) return;
    try {
      const job = await oval("/v1/download", downloadBody({
        live: true,
        title: `${(state.live && state.live.title) || pageTitle()} · directo`,
      }));
      track(job, "Grabación del directo");
      log("Grabando el directo en Oval");
    } catch (err) {
      log(String(err.message || err), "error");
    }
  }

  /* ---------- Jobs ---------- */

  function jobMeta(j) {
    const pct = j.progress > 0 ? `${Math.round(j.progress * 100)}%` : "";
    if (j.live && (j.status === "downloading" || j.status === "starting")) {
      return ["Grabando", clock(j.elapsed), bytes(j.downloaded)].filter(Boolean).join(" · ");
    }
    if (j.live && j.status === "processing") return "Guardando la grabación…";
    switch (j.status) {
      case "queued": return "En la cola de Oval";
      case "starting": return "Empezando…";
      case "downloading": return [pct, j.speed > 0 ? `${bytes(j.speed)}/s` : "", bytes(j.total)].filter(Boolean).join(" · ") || "Descargando…";
      case "processing": return j.section ? "Cortando el tramo…" : j.mode === "audio" ? "Convirtiendo a MP3…" : "Uniendo vídeo y audio…";
      case "paused": return "En pausa (en Oval)";
      case "done": return "Listo · clic para mostrar";
      case "error": return j.error || "Error";
      default: return "";
    }
  }

  function track(job, label) {
    if (!job || job.id == null) return;
    const ring = h("span", { class: "ring" }, h("span"));
    const title = h("div", { class: "job-title", text: label });
    const meta = h("div", { class: "job-meta" });
    const x = h("button", { class: "x", title: "Cancelar", "aria-label": "Cancelar" }, I.close());
    const row = h("div", { class: "job" }, ring, h("div", { class: "job-info" }, title, meta), x);
    const entry = { job, label, row, ring, meta, x };
    x.addEventListener("click", async (e) => {
      e.stopPropagation();
      const j = entry.job;
      if (j.live && j.status !== "done" && j.status !== "error") {
        // A recording is stopped and kept, like the ■ button in Oval.
        try { await oval("/v1/stop", { id: j.id }); log(`Grabación detenida: ${label}`); } catch (err) { log(String(err.message || err), "error"); }
        return;
      }
      if (j.status !== "done" && j.status !== "error") {
        try { await oval("/v1/cancel", { id: j.id }); log(`Cancelado: ${label}`); } catch (err) { log(String(err.message || err), "error"); }
      }
      state.jobs.delete(j.id);
      row.remove();
      renderProgress();
    });
    row.addEventListener("click", () => {
      if (entry.job.status === "done" && entry.job.path) oval("/v1/reveal", { path: entry.job.path }).catch(() => {});
    });
    state.jobs.set(job.id, entry);
    el.jobs.append(row);
    renderJob(entry);
    poll();
  }

  function renderJob(entry) {
    const j = entry.job;
    entry.row.className = `job ${j.status}${j.live ? " live" : ""}`;
    const full = j.status === "done" || j.status === "error";
    entry.ring.style.setProperty("--p", full ? 1 : j.progress || 0);
    entry.ring.firstChild.replaceChildren(j.status === "done" ? I.check() : j.status === "error" ? I.bang() : "");
    entry.meta.textContent = jobMeta(j);
    entry.x.title = full ? "Quitar de la lista" : j.live ? "Detener y guardar" : "Cancelar";
  }

  /** Average progress of what is running, shown on the logo and the bubble. */
  function renderProgress() {
    const active = [...state.jobs.values()].map((e) => e.job).filter((j) => j.status !== "done" && j.status !== "error");
    const p = active.length ? active.reduce((sum, j) => sum + (j.progress || 0), 0) / active.length : 0;
    el.logo.style.setProperty("--p", p);
    bubble.style.setProperty("--p", p);
  }

  let polling = 0;
  function poll() {
    if (polling) return;
    polling = setInterval(async () => {
      const ids = [...state.jobs.values()].filter((e) => e.job.status !== "done" && e.job.status !== "error").map((e) => e.job.id);
      if (!ids.length) {
        clearInterval(polling);
        polling = 0;
        return;
      }
      let jobs;
      try {
        jobs = await oval(`/v1/jobs?ids=${ids.join(",")}`);
      } catch {
        return; // Oval closed for a moment; keep trying
      }
      for (const id of ids) {
        const entry = state.jobs.get(id);
        const fresh = jobs.find((j) => j.id === id);
        if (!entry) continue;
        if (!fresh) {
          // Canceled or removed from Oval's own window.
          entry.job = { ...entry.job, status: "error", error: "Quitado en Oval" };
        } else {
          if (fresh.status !== entry.job.status) {
            if (fresh.status === "done") log(`Listo: ${entry.label}`);
            if (fresh.status === "error") log(`${entry.label}: ${fresh.error || "error"}`, "error");
          }
          entry.job = fresh;
        }
        renderJob(entry);
      }
      renderProgress();
    }, 800);
  }

  /* ---------- Page changes (YouTube navigates without reloading) ---------- */

  function onPageChange() {
    const url = pageUrl();
    if (url === state.url) return;
    state.url = url;
    state.clipIn = state.clipOut = null;
    el.note.value = "";
    renderPage();
    checkLive();
  }

  function renderPage() {
    el.ytBtn.disabled = !isYouTube();
    el.ytBtn.title = isYouTube() ? "Abre el panel de transcripción de YouTube" : "Solo en YouTube";
    renderClip();
  }

  document.addEventListener("yt-navigate-finish", onPageChange);
  window.addEventListener("popstate", onPageChange);
  // Other single-page sites, and videos that load after the panel opened.
  setInterval(() => {
    onPageChange();
    if (!host.isConnected) (document.body || document.documentElement).append(host);
    if (host.style.display !== "none" && !state.minimized) renderClip();
  }, 1500);
  setInterval(() => {
    if (state.live && host.style.display !== "none" && !state.minimized) renderLive();
  }, 1000);

  /* ---------- Start ---------- */

  api.storage.local.get(["dest", "quality", "format", "pos"]).then((saved) => {
    if (saved.dest && saved.dest.path) state.dest = saved.dest;
    if (saved.quality) setQuality(saved.quality);
    if (saved.format) { state.format = saved.format; el.format.value = saved.format; }
    if (saved.pos) place(saved.pos);
    renderDest();
  }).catch(() => {});

  renderStatus();
  renderPage();
  renderDest();
  connect();
})();
