// Stand-in for Oval's Rust backend so the real interface can run on a web
// page. Nothing is downloaded: jobs progress on timers. Loaded before app.js.
(() => {
  "use strict";

  const handlers = {};
  const emit = (name, payload) => handlers[name]?.({ payload });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const MB = 1024 * 1024;

  const art = (h) =>
    "data:image/svg+xml," +
    encodeURIComponent(
      `<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 320 180'><defs><linearGradient id='g' x1='0' y1='0' x2='1' y2='1'>` +
        `<stop offset='0' stop-color='hsl(${h},70%,58%)'/><stop offset='1' stop-color='hsl(${(h + 50) % 360},65%,26%)'/></linearGradient></defs>` +
        `<rect width='320' height='180' fill='url(#g)'/><circle cx='${80 + ((h * 7) % 160)}' cy='${50 + ((h * 3) % 80)}' r='70' fill='white' opacity='.13'/>` +
        `<circle cx='${240 - ((h * 5) % 120)}' cy='140' r='50' fill='black' opacity='.15'/></svg>`,
    );

  const TITLES = [
    "Vlog: un día en la ciudad", "Tutorial de edición rápida", "Directo del sábado", "Receta en 5 minutos",
    "Review del nuevo portátil", "Paseo por la montaña", "Preguntas y respuestas", "Top 10 trucos",
    "Making of del corto", "Unboxing sorpresa", "Música en vivo", "Resumen del mes", "Viaje en tren",
    "Primeras impresiones", "Detrás de cámaras", "Cortometraje animado", "Clase de dibujo", "Especial 100k",
  ];

  const titleFor = (url) => {
    try {
      const u = new URL(url);
      const host = u.hostname.replace(/^www\./, "");
      const tail = u.pathname.split("/").filter(Boolean).pop() || "";
      return `${host}${tail ? " / " + decodeURIComponent(tail).slice(0, 40) : ""}`;
    } catch {
      return url;
    }
  };

  let nextId = 1;
  let concurrency = 3;
  const jobs = new Map(); // id -> job
  const timers = new Map(); // id -> interval

  const pub = (j) => emit("job", { ...j });
  const active = () => [...jobs.values()].filter((j) => !j.live && ["starting", "downloading", "processing"].includes(j.status)).length;

  function schedule() {
    for (const j of jobs.values()) {
      if (j.status !== "queued") continue;
      if (!j.live && active() >= concurrency) continue;
      start(j);
    }
  }

  function start(j) {
    j.status = "starting";
    pub(j);
    const hue = (j.id * 67) % 360;
    setTimeout(() => {
      if (!jobs.has(j.id) || j.status !== "starting") return;
      j.thumbnail = j.thumbnail || art(hue);
      j.status = "downloading";
      if (j.live) return runLive(j);
      j.total = j.total || (j.mode === "audio" ? 6 + Math.random() * 6 : 40 + Math.random() * 160) * MB;
      const speed = (4 + Math.random() * 9) * MB;
      const tick = setInterval(() => {
        if (j.status !== "downloading") return;
        j.speed = speed * (0.8 + Math.random() * 0.4);
        j.downloaded = Math.min(j.total, j.downloaded + j.speed / 4);
        j.progress = j.downloaded / j.total;
        j.eta = (j.total - j.downloaded) / j.speed;
        if (j.downloaded >= j.total) {
          clearInterval(tick);
          timers.delete(j.id);
          j.status = "processing";
          j.speed = 0;
          j.eta = null;
          pub(j);
          setTimeout(() => {
            if (!jobs.has(j.id)) return;
            j.status = "done";
            j.progress = 1;
            pub(j);
            schedule();
          }, 1200);
          return;
        }
        pub(j);
      }, 250);
      timers.set(j.id, tick);
      pub(j);
    }, 900);
  }

  function runLive(j) {
    j.elapsed = 0;
    const tick = setInterval(() => {
      if (j.status !== "downloading") return;
      j.elapsed += 1;
      j.speed = (650 + Math.random() * 250) * 1024;
      j.downloaded += j.speed;
      pub(j);
    }, 1000);
    timers.set(j.id, tick);
    pub(j);
  }

  function stopTimer(id) {
    clearInterval(timers.get(id));
    timers.delete(id);
  }

  function add({ url, mode = "video", title, thumbnail, live = false }) {
    const j = {
      id: nextId++, url, mode, live, quality: null,
      title: title || titleFor(url), thumbnail: thumbnail || null,
      status: "queued", progress: 0, downloaded: 0, total: 0, speed: 0, eta: null,
      error: null, path: null, elapsed: 0,
    };
    jobs.set(j.id, j);
    pub(j);
    schedule();
    return { ...j };
  }

  const commands = {
    platform: () => "web",
    jobs: () => [...jobs.values()].map((j) => ({ ...j })),
    engine_status: () => ({ ready: true, message: "Listo", progress: null }),
    get_concurrency: () => concurrency,
    set_concurrency: ({ value }) => {
      concurrency = Math.max(1, Math.min(8, value));
      schedule();
      return concurrency;
    },
    download_folder: () => "Descargas/Oval",
    clipboard_text: () => null,
    choose_folder: () => null,
    reveal: () => null,
    add_download: ({ url, mode }) => [add({ url, mode })],
    add_many: ({ items, mode }) => items.map((it) => add({ ...it, mode })),
    add_live: ({ url }) => [add({ url, live: true, title: "Directo · " + titleFor(url) })],
    pause: ({ id }) => {
      const j = jobs.get(id);
      if (!j) return;
      j.status = "paused";
      j.speed = 0;
      j.eta = null;
      pub(j);
      schedule();
    },
    resume: ({ id }) => {
      const j = jobs.get(id);
      if (!j) return;
      if (j.status === "paused" && timers.has(id)) {
        j.status = "downloading";
        pub(j);
      } else if (["paused", "error"].includes(j.status)) {
        j.status = "queued";
        pub(j);
        schedule();
      }
    },
    stop_live: ({ id }) => {
      const j = jobs.get(id);
      if (!j) return;
      stopTimer(id);
      j.status = "processing";
      j.speed = 0;
      pub(j);
      setTimeout(() => {
        j.status = "done";
        j.total = j.downloaded;
        j.progress = 1;
        pub(j);
      }, 1100);
    },
    cancel: ({ id }) => {
      stopTimer(id);
      jobs.delete(id);
      emit("job-removed", id);
      schedule();
    },
    clear_finished: () => {
      for (const [id, j] of jobs) {
        if (j.status === "done") {
          jobs.delete(id);
          emit("job-removed", id);
        }
      }
    },
    cancel_listing: () => null,
    // A couple of the sample videos count as already downloaded.
    downloaded_ids: () => ["demo2", "demo7"],
    list_channel: async ({ token }) => {
      const count = 18;
      for (let b = 0; b < 3; b++) {
        await sleep(350);
        const entries = TITLES.slice(b * 6, b * 6 + 6).map((title, k) => {
          const i = b * 6 + k;
          return { id: "demo" + i, url: "https://example.com/video/" + i, title, duration: 75 + i * 41, thumbnail: art((i * 37) % 360) };
        });
        emit("channel-batch", { token, channel: "Canal de ejemplo", entries });
      }
      return { token, channel: "Canal de ejemplo", count };
    },
  };

  window.__TAURI__ = {
    core: {
      invoke: async (cmd, args = {}) => {
        const fn = commands[cmd];
        if (!fn) throw new Error("demo: " + cmd + " no disponible");
        return fn(args);
      },
    },
    event: { listen: async (name, fn) => { handlers[name] = fn; return () => delete handlers[name]; } },
    window: {
      getCurrentWindow: () => ({
        minimize: async () => {},
        close: async () => {},
        setSize: async () => {},
        startResizeDragging: async () => {},
      }),
    },
    dpi: { LogicalSize: class { constructor(w, h) { this.width = w; this.height = h; } } },
  };
})();
