// Requires Playwright (locally installed or available through NODE_PATH).
const { chromium } = require('playwright');
const assert = require('node:assert/strict');
const http = require('node:http');
const fs = require('node:fs/promises');
const path = require('node:path');
const os = require('node:os');

(async () => {
  const root = path.resolve(__dirname, '..', 'ui');
  const server = http.createServer(async (request, response) => {
    const name = request.url.split('?')[0];
    const files = { '/': 'index.html', '/app.js': 'app.js', '/style.css': 'style.css' };
    if (!files[name]) { response.writeHead(404).end(); return; }
    response.setHeader('Content-Type', name.endsWith('.js') ? 'text/javascript' : name.endsWith('.css') ? 'text/css' : 'text/html');
    response.end(await fs.readFile(path.join(root, files[name])));
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  const browser = await chromium.launch({ headless: true });
  try {
    const context = await browser.newContext({ viewport: { width: 700, height: 700 } });
    await context.addInitScript(() => {
      window.calls = [];
      window.compressionMode = 'cancel';
      window.__TAURI__ = {
        core: { invoke: async (command, args) => {
          window.calls.push({ command, args });
          if (command === 'platform') return new URLSearchParams(location.search).get('os') || 'windows';
          if (command === 'jobs' || command === 'add_download') return [];
          if (command === 'get_concurrency') return 3;
          if (command === 'download_folder') return 'Downloads/Oval';
          if (command === 'engine_status') return { ready: true, message: '', progress: null };
          if (command === 'app_version') return 'test';
          if (command === 'on_disk') return { ids: [], partial: [], names: [] };
          if (command === 'compress_video') {
            if (window.compressionMode === 'error') throw 'El video ya es liviano';
            if (window.compressionMode === 'cancel') return null;
            await new Promise(resolve => setTimeout(resolve, 100));
            return { path: 'sample-liviano.mp4', originalBytes: 1000, outputBytes: 200, savedPercent: 80 };
          }
          return null;
        } },
        event: { listen: async () => () => {} },
        window: { getCurrentWindow: () => ({ minimize: async () => {}, close: async () => {}, setSize: async () => {}, startResizeDragging: async () => {} }) },
        dpi: { LogicalSize: class {} },
      };
    });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', error => errors.push(error.message));
    const url = `http://127.0.0.1:${server.address().port}/`;
    await page.goto(url);
    await page.locator('#compress-video').waitFor({ state: 'visible' });
    await page.locator('.qchip[data-ctx="link"]').click();
    await page.locator('[data-q="light"]').click();
    assert.equal(await page.locator('.qchip[data-ctx="link"] .qlabel').textContent(), 'Liviano');
    await page.locator('#url').fill('https://example.com/video.mp4');
    await page.locator('#link-form .go').click();
    assert.equal(await page.evaluate(() => window.calls.find(call => call.command === 'add_download').args.quality), 'light');
    await page.reload();
    await page.locator('.qchip[data-ctx="link"] .qlabel').filter({ hasText: 'Liviano' }).waitFor();
    await page.locator('#compress-video').click();
    await page.waitForFunction(() => !document.querySelector('#compress-video').disabled);
    assert.equal(await page.locator('#compression-status').isHidden(), true);
    await page.evaluate(() => { window.compressionMode = 'success'; });
    await page.locator('#compress-video').click();
    await page.waitForFunction(() => document.querySelector('#compression-status').textContent.includes('80% menos'));
    const output = process.env.OVAL_UI_OUTPUT || path.join(os.tmpdir(), 'oval-ui-check');
    await fs.mkdir(output, { recursive: true });
    await page.screenshot({ path: path.join(output, 'compression.png') });
    await page.evaluate(() => { window.compressionMode = 'error'; });
    await page.locator('#compress-video').click();
    await page.waitForFunction(() => document.querySelector('#compression-status').textContent.includes('ya es liviano'));
    assert.equal(await page.locator('#compress-video').isEnabled(), true);
    await page.locator('#view-link [data-mode="audio"]').click();
    await page.locator('.qchip[data-ctx="link"]').click();
    assert.equal(await page.locator('[data-q="light"]').count(), 0);
    await page.locator('#url').click();
    await page.locator('#view-link [data-mode="video"]').click();
    await page.setViewportSize({ width: 460, height: 460 });
    await page.waitForTimeout(500);
    await page.locator('.qchip[data-ctx="link"]').click();
    await page.waitForTimeout(350);
    const menu = await page.locator('#qmenu').boundingBox();
    assert.ok(menu.x >= 0 && menu.x + menu.width <= 460 && menu.y >= 0 && menu.y + menu.height <= 460);
    await page.waitForTimeout(2800); // Let the previous error toast clear for the capture.
    await page.screenshot({ path: path.join(output, 'small-quality-menu.png') });
    await page.goto(url + '?os=android');
    await page.waitForFunction(() => document.documentElement.classList.contains('mobile'));
    assert.equal(await page.locator('#compress-video').isHidden(), true);
    assert.deepEqual(errors, []);
    console.log('PASS: lightweight download, persisted quality, compression success/cancel/error, small window and mobile visibility.');
  } finally {
    await browser.close();
    await new Promise(resolve => server.close(resolve));
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
