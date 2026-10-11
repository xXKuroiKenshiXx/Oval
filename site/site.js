// Points the download buttons at the files of the latest GitHub release and
// highlights the one for the visitor's system.
(async () => {
  "use strict";
  const REPO = "xXKuroiKenshiXx/Oval";
  const ua = navigator.userAgent;
  const mine = /Android/i.test(ua) ? "apk" : /Windows/i.test(ua) ? "windows" : /Linux|X11/i.test(ua) ? "appimage" : null;
  const LABEL = { windows: "Descargar para Windows", apk: "Descargar para Android", appimage: "Descargar para Linux" };
  const MATCH = {
    windows: (n) => /setup\.exe$/i.test(n),
    portable: (n) => /portable\.exe$/i.test(n),
    appimage: (n) => /\.AppImage$/i.test(n),
    deb: (n) => /\.deb$/i.test(n),
    apk: (n) => /\.apk$/i.test(n),
  };

  if (mine) {
    document.querySelector(`.dl[data-kind="${mine}"]`)?.classList.add("mine");
    document.querySelector("#primary-download span").textContent = LABEL[mine];
  }

  try {
    const res = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`);
    if (!res.ok) throw new Error(res.status);
    const release = await res.json();
    for (const el of document.querySelectorAll(".dl")) {
      const asset = release.assets.find((a) => MATCH[el.dataset.kind](a.name));
      if (asset) el.href = asset.browser_download_url;
      else el.classList.add("missing");
    }
    const primary = mine && release.assets.find((a) => MATCH[mine](a.name));
    if (primary) document.querySelector("#primary-download").href = primary.browser_download_url;
    document.querySelector("#release-note").innerHTML =
      `Versión ${release.tag_name.replace(/^v/, "")} · ` +
      `<a href="${release.html_url}">notas de la versión</a> · ` +
      `<a href="https://github.com/${REPO}/releases">todas las versiones</a>`;
  } catch {
    // No release yet (or offline): the links keep pointing at the releases page.
    document.querySelector("#release-note").textContent =
      "Todavía no hay una versión publicada: pronto aparecerán aquí los archivos.";
  }
})();
