// The three step buttons under the demo drive the real interface inside the
// iframe (same origin), so a visitor sees each tab working with one tap.
(() => {
  "use strict";
  const frame = document.querySelector(".try-frame iframe");
  const buttons = document.querySelectorAll(".steps button");
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

  const STEPS = {
    link: { form: "#link-form", input: "#url", value: "https://www.youtube.com/watch?v=aqz-KE-bpKQ" },
    channel: { form: "#channel-form", input: "#channel-url", value: "https://www.youtube.com/@BlenderStudio" },
    live: { form: "#live-form", input: "#live-url", value: "https://www.twitch.tv/nasa" },
  };

  async function ready() {
    if (frame.getAttribute("loading") === "lazy") frame.removeAttribute("loading");
    for (let i = 0; i < 50; i++) {
      const doc = frame.contentDocument;
      if (doc?.readyState === "complete" && doc.querySelector(".tabs")) return doc;
      await sleep(100);
    }
    return null;
  }

  /** Types the link a character at a time, like a paste would look slowed down. */
  async function type(input, text) {
    input.focus();
    input.value = "";
    for (let i = 0; i < text.length; i += 3) {
      input.value = text.slice(0, i + 3);
      await sleep(14);
    }
  }

  let busy = false;
  async function run(step) {
    if (busy) return;
    busy = true;
    buttons.forEach((b) => b.classList.toggle("on", b.dataset.step === step));
    try {
      const doc = await ready();
      if (!doc) return;
      const { form, input, value } = STEPS[step];
      doc.querySelector(`.tabs [data-tab="${step}"]`).click();
      await sleep(350);
      await type(doc.querySelector(input), value);
      await sleep(250);
      doc.querySelector(form).requestSubmit();
    } finally {
      busy = false;
    }
  }

  buttons.forEach((b) => b.addEventListener("click", () => run(b.dataset.step)));
})();
