// SPIKE 3b.0 C4 — dirige o Chrome real (flatpak, :9222) via CDP/WebSocket.
// Prova: <img src=presigned(:8333)> carregado por página em :3000 →
//   request à :8333 = 200, imagem decodifica (naturalWidth>0),
//   e NENHUM erro de console / log-error / blocked / loadingFailed.
const CDP_HOST = '127.0.0.1', CDP_PORT = 9222;

async function j(method, path) {
  const res = await fetch(`http://${CDP_HOST}:${CDP_PORT}${path}`, { method });
  return res.json();
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// reaproveita um target "page" existente (headless=new flaky com /json/new).
const pages = await j('GET', '/json/list');
const target = pages.find((t) => t.type === 'page' && !t.url.startsWith('http://localhost:3000')) || pages.find((t) => t.type === 'page');
if (!target) { console.error('nenhum target page'); process.exit(2); }
const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise((res, rej) => { ws.onopen = res; ws.onerror = (e) => rej(new Error('ws connect failed')); });

let idc = 0;
const pending = new Map();
const consoleErrors = [];
const logErrors = [];
const exceptions = [];
const failed = [];
const s3responses = [];

ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) { pending.get(msg.id)(msg); pending.delete(msg.id); return; }
  const m = msg.method, p = msg.params || {};
  if (m === 'Runtime.consoleAPICalled' && (p.type === 'error' || p.type === 'warning'))
    consoleErrors.push((p.args || []).map((a) => a.value ?? a.description ?? a.type).join(' '));
  if (m === 'Log.entryAdded' && p.entry && (p.entry.level === 'error' || p.entry.level === 'warning'))
    logErrors.push(`${p.entry.source}:${p.entry.text}`);
  if (m === 'Runtime.exceptionThrown')
    exceptions.push(p.exceptionDetails?.exception?.description || JSON.stringify(p.exceptionDetails));
  if (m === 'Network.loadingFailed')
    failed.push(`${p.errorText} blocked=${p.blockedReason || ''}`);
  if (m === 'Network.responseReceived')
    if (p.response.url.includes(':8333')) s3responses.push(`${p.response.status} ${p.type} ${p.response.mimeType}`);
};
function send(method, params = {}) {
  const id = ++idc;
  return new Promise((res) => { pending.set(id, res); ws.send(JSON.stringify({ id, method, params })); });
}

for (const d of ['Network', 'Runtime', 'Log', 'Page']) await send(d + '.enable');

const url = process.argv[2] || 'http://localhost:3000/';
await send('Page.navigate', { url });
await sleep(3500); // deixa o <img> resolver (rede local, mas margem p/ console)

const probe = await send('Runtime.evaluate', {
  expression: `JSON.stringify({
    title: document.title,
    naturalWidth: document.getElementById('i').naturalWidth,
    naturalHeight: document.getElementById('i').naturalHeight,
    complete: document.getElementById('i').complete,
    status: document.getElementById('out').textContent.slice(0,80),
  })`,
  returnByValue: true,
});
const img = JSON.parse(probe.result.result.value);

// navega o target de volta a about:blank p/ liberar (não fecha o tab do headless).
await send('Page.navigate', { url: 'about:blank' });
ws.close();

const imgRequest200 = s3responses.some((r) => r.startsWith('200'));
const noErrors = consoleErrors.length === 0 && logErrors.length === 0 && exceptions.length === 0 && failed.length === 0;
const passes = imgRequest200 && img.naturalWidth > 0 && noErrors;

console.log('===== C4: presigned <img> no browser (Chrome real, sem proxy) =====');
console.log('page          :', url);
console.log('respostas :8333:', s3responses.length ? s3responses.join(' | ') : '(nenhuma)');
console.log('img         :', `title=${img.title} naturalWidth=${img.naturalWidth}x${img.naturalHeight} complete=${img.complete}`);
console.log('status DOM  :', img.status.replace(/\n/g, ' / '));
console.log('consoleErr  :', consoleErrors.length ? consoleErrors.join(' || ') : 'nenhum');
console.log('logErr      :', logErrors.length ? logErrors.join(' || ') : 'nenhum');
console.log('exceptions  :', exceptions.length ? exceptions.join(' || ') : 'nenhuma');
console.log('loadingFailed:', failed.length ? failed.join(' || ') : 'nenhum');
console.log('---');
console.log('img request 200? ', imgRequest200);
console.log('imagem decodifica?', img.naturalWidth > 0);
console.log('zero erros console?', noErrors);
console.log('C4 RESULT    :', passes ? 'PASS' : 'FAIL');
process.exit(passes ? 0 : 1);
