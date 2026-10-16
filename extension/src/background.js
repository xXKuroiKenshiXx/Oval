// Talks to the Oval app on this computer and opens the panel in a tab.
// Runs as a service worker (Chrome, Edge, Safari) or event page (Firefox).
"use strict";

const api = globalThis.browser || globalThis.chrome;

// Must match PORTS in src/bridge.rs: Oval takes the first one that is free.
const PORTS = [17177, 27177, 37177];
let port = null;

async function call(path, body) {
  const tries = port ? [port, ...PORTS.filter((p) => p !== port)] : PORTS;
  let lastError = "Oval no está abierto en este equipo";
  for (const p of tries) {
    let resp;
    try {
      resp = await fetch(`http://127.0.0.1:${p}${path}`, {
        method: body === undefined ? "GET" : "POST",
        headers: { "Content-Type": "application/json", "X-Oval-Client": "extension" },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch {
      continue; // nothing listening on this port
    }
    let data = null;
    try { data = await resp.json(); } catch { /* empty answer */ }
    if (resp.status === 403 || (path === "/v1/hello" && (!data || data.app !== "oval"))) {
      lastError = "Otro programa ocupa el puerto de Oval";
      continue;
    }
    port = p;
    if (!resp.ok) throw new Error((data && data.error) || `Error ${resp.status}`);
    return data;
  }
  port = null;
  throw new Error(lastError);
}

api.runtime.onMessage.addListener((msg, _sender, reply) => {
  if (!msg || msg.type !== "oval") return false;
  call(msg.path, msg.body).then(
    (data) => reply({ ok: true, data }),
    (err) => reply({ ok: false, error: String(err.message || err) }),
  );
  return true; // reply comes later
});

async function openPanel(tab) {
  if (!tab || tab.id == null) return;
  try {
    await api.scripting.executeScript({ target: { tabId: tab.id }, files: ["src/panel.js"] });
  } catch {
    // Browser pages (settings, the store...) don't allow extensions.
    flash(tab.id, "!", "#ff5d6c");
  }
}

api.action.onClicked.addListener(openPanel);

function flash(tabId, text, color) {
  api.action.setBadgeBackgroundColor({ color, tabId }).catch(() => {});
  api.action.setBadgeText({ text, tabId }).catch(() => {});
  setTimeout(() => api.action.setBadgeText({ text: "", tabId }).catch(() => {}), 2500);
}

// Right click > "Descargar con Oval" on a link, a video or the page itself.
api.runtime.onInstalled.addListener(() => {
  Promise.resolve(api.contextMenus.removeAll()).then(() => {
    api.contextMenus.create({
      id: "oval-download",
      title: "Descargar con Oval",
      contexts: ["link", "video", "page"],
    });
    api.contextMenus.create({
      id: "oval-panel",
      title: "Abrir el panel de Oval",
      contexts: ["page", "video"],
    });
  });
});

api.contextMenus.onClicked.addListener(async (info, tab) => {
  if (info.menuItemId === "oval-panel") return openPanel(tab);
  if (info.menuItemId !== "oval-download") return;
  const src = info.srcUrl && /^https?:/.test(info.srcUrl) ? info.srcUrl : null;
  const url = info.linkUrl || src || info.pageUrl;
  const { dest } = await api.storage.local.get("dest");
  try {
    await call("/v1/download", { url, folder: dest && dest.path ? dest.path : undefined });
    if (tab) flash(tab.id, "✓", "#3ddc97");
  } catch {
    if (tab) flash(tab.id, "!", "#ff5d6c");
  }
});
