const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');

const source = fs.readFileSync('ui/app.js', 'utf8');
const context = vm.createContext({ URL });
vm.runInContext(source.slice(source.indexOf('function normalizeUrl('), source.indexOf('const hostOf =')), context);
const links = text => Array.from(vm.runInContext(`extractUrls(${JSON.stringify(text)})`, context));
const tiktok = 'https://www.tiktok.com/@greekk_16/video/7134386723718466822?lang=es';
for (const text of [tiktok, `Mira este video ${tiktok}`, `[TikTok](${tiktok})`, `<${tiktok}>`, `(${tiktok}).`, `${tiktok}\n${tiktok}`]) {
  assert.deepEqual(links(text), [tiktok]);
}
assert.deepEqual(links('x.com/webmobtech/status/1401028705259081728/video/1'), ['https://x.com/webmobtech/status/1401028705259081728/video/1']);
assert.deepEqual(links(encodeURIComponent(tiktok)), [tiktok]);
assert.deepEqual(links('//example.com/video.mp4'), ['https://example.com/video.mp4']);
assert.deepEqual(links('https://[::1]/video.mp4'), ['https://[::1]/video.mp4']);
const signed = 'https://example.com/video%20name.mp4?signature=a%2Bb%3D&expires=123';
assert.deepEqual(links(signed), [signed]);
assert.deepEqual(links(`${tiktok} example.com/a.mp4`), [tiktok, 'https://example.com/a.mp4']);
assert.deepEqual(links('texto sin enlaces'), []);
assert.deepEqual(links('javascript:alert(1)'), []);
console.log('Shared captions, Markdown, encoded URLs, signed URLs and mixed batches passed');
