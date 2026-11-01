// Mede um editor de documentos no Chrome (via DevTools Protocol), com a CPU desacelerada para
// simular máquinas mais fracas (Lighthouse: 4× ≈ notebook/celular intermediário, 6× ≈ fraco)
// e a mesma rede emulada para todos. Métricas iguais para qualquer página:
//   carga     bytes baixados, tempo até ficar interativo (fim da última tarefa longa antes de
//             2 s de silêncio), tempo total de bloqueio (TBT), tarefas longas
//   memória   heap JS depois de GC, nós do DOM, pixels de canvas (backing store)
//   rolagem   quadros por segundo e quadros perdidos rolando o documento com a roda
//   digitação latência por tecla (Event Timing API: entrada → próximo quadro pintado)
//
//   node harness.mjs <url> <nome> [--rates 1,4,6] [--runs 3] [--type] [--profile] [--ready <expr JS>]
// --ready: expressão avaliada na página até ser verdadeira (o documento está na tela); o
// tempo até ela é "ready" e a janela de silêncio do "interativo" só começa depois dela
import { createServer } from 'node:http';
import { readFileSync, existsSync, statSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, resolve, dirname, extname } from 'node:path';
import { brotliCompressSync, gzipSync, constants as zc } from 'node:zlib';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const root = resolve(here, '../..');
const { chromium } = await import(pathToFileURL(join(root, 'sheet/node_modules/playwright-core/index.mjs')).href);

const args = process.argv.slice(2);
const opt = (k, d) => { const i = args.indexOf(k); return i >= 0 ? args[i + 1] : d; };
const url0 = args[0], name = args[1] ?? 'run';
const rates = opt('--rates', '1,4,6').split(',').map(Number);
const runs = Number(opt('--runs', '3'));
const doType = args.includes('--type');
const profile = args.includes('--profile');
const profileTyping = args.includes('--profile-typing');
const siteDir = resolve(root, opt('--site', 'site'));
const readyExpr = opt('--ready', 'true');

// servidor estático do site (só para URLs locais "site:/caminho")
let base = '';
if (url0.startsWith('site:')) {
  const types = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.css': 'text/css', '.json': 'application/json',
    '.wasm': 'application/wasm', '.ttf': 'font/ttf', '.woff2': 'font/woff2', '.svg': 'image/svg+xml', '.png': 'image/png', '.docx': 'application/octet-stream' };
  // Brotli como um host de verdade (GitHub Pages, CDN) — menos nos formatos já comprimidos
  const packed = new Map();
  // pré-comprime tudo na partida (comprimir na hora atrasaria a primeira carga medida)
  const { readdirSync } = await import('node:fs');
  const walk = (d) => { for (const e of readdirSync(d, { withFileTypes: true })) { const f = join(d, e.name); if (e.isDirectory()) walk(f); else if (!['.png', '.jpg', '.gif', '.woff2', '.docx'].includes(extname(f))) {
    const raw = readFileSync(f);
    packed.set(`br:${f}`, brotliCompressSync(raw, { params: { [zc.BROTLI_PARAM_QUALITY]: 11 } }));
    packed.set(`gzip:${f}`, gzipSync(raw, { level: 9 }));
  } } };
  walk(siteDir);
  const srv = createServer((req, res) => {
    let p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    let f = join(siteDir, p);
    if (existsSync(f) && statSync(f).isDirectory()) f = join(f, 'index.html');
    if (!existsSync(f)) { res.writeHead(404); res.end(); return; }
    const ext = extname(f);
    const head = { 'content-type': types[ext] ?? 'application/octet-stream', 'cache-control': 'no-store' };
    // o Chrome só pede Brotli em HTTPS; em HTTP local vai gzip (o que um host serviria em HTTP)
    const accept = req.headers['accept-encoding'] ?? '';
    const enc = accept.includes('br') ? 'br' : accept.includes('gzip') ? 'gzip' : null;
    if (enc && !['.png', '.jpg', '.gif', '.woff2', '.docx'].includes(ext)) {
      const key = `${enc}:${f}`;
      let b = packed.get(key);
      if (!b) { b = enc === 'br' ? brotliCompressSync(readFileSync(f), { params: { [zc.BROTLI_PARAM_QUALITY]: 11 } }) : gzipSync(readFileSync(f), { level: 9 }); packed.set(key, b); }
      res.writeHead(200, { ...head, 'content-encoding': enc });
      res.end(b);
      return;
    }
    res.writeHead(200, head);
    res.end(readFileSync(f));
  });
  await new Promise((r) => srv.listen(0, '127.0.0.1', r));
  base = `http://127.0.0.1:${srv.address().port}`;
}
const url = url0.startsWith('site:') ? base + url0.slice(5) : url0;

// observadores na página, instalados antes de qualquer script dela
const INIT = () => {
  const P = (window.__perf = { long: [], events: [], frames: null });
  try { new PerformanceObserver((l) => { for (const e of l.getEntries()) P.long.push([e.startTime, e.duration]); }).observe({ type: 'longtask', buffered: true }); } catch {}
  try { new PerformanceObserver((l) => { for (const e of l.getEntries()) P.events.push([e.name, e.duration, e.startTime]); }).observe({ type: 'event', durationThreshold: 16, buffered: true }); } catch {}
  P.startFrames = () => {
    P.frames = [];
    const tick = (t) => { if (!P.frames) return; P.frames.push(t); requestAnimationFrame(tick); };
    requestAnimationFrame(tick);
  };
  P.stopFrames = () => { const f = P.frames; P.frames = null; return f; };
  // canvas de toda a página, inclusive dentro de shadow roots abertas
  P.canvasPixels = () => {
    let px = 0, n = 0;
    const walk = (r) => {
      for (const el of r.querySelectorAll('*')) {
        if (el instanceof HTMLCanvasElement) { px += el.width * el.height; n++; }
        if (el.shadowRoot) walk(el.shadowRoot);
      }
    };
    walk(document);
    return { canvases: n, megapixels: px / 1e6, mb: (px * 4) / 1048576 };
  };
};

const NET = { offline: false, latency: 60, downloadThroughput: (20 * 1024 * 1024) / 8, uploadThroughput: (5 * 1024 * 1024) / 8 };
const median = (a) => { const s = [...a].sort((x, y) => x - y); return s.length ? s[Math.floor(s.length / 2)] : NaN; };
const r1 = (v) => Math.round(v * 10) / 10;

const browser = await chromium.launch({ executablePath: 'C:/Program Files/Google/Chrome/Application/chrome.exe', headless: true, args: ['--enable-precise-memory-info', '--disable-extensions'] });

async function once(rate, runIdx) {
  const ctx = await browser.newContext({ viewport: { width: 1366, height: 768 }, deviceScaleFactor: 1, locale: 'pt-BR' });
  const page = await ctx.newPage();
  await page.addInitScript(INIT);
  const cdp = await ctx.newCDPSession(page);
  await cdp.send('Network.enable');
  await cdp.send('Network.emulateNetworkConditions', NET);
  await cdp.send('Performance.enable');
  await cdp.send('Emulation.setCPUThrottlingRate', { rate });
  let bytes = 0, requests = 0;
  const urls = new Map(), sizes = [];
  cdp.on('Network.requestWillBeSent', (e) => urls.set(e.requestId, e.request.url));
  cdp.on('Network.loadingFinished', (e) => { bytes += e.encodedDataLength; requests++; sizes.push([e.encodedDataLength, urls.get(e.requestId)]); });
  if (profile && runIdx === 0) { await cdp.send('Profiler.enable'); await cdp.send('Profiler.start'); }

  const t0 = Date.now();
  await page.goto(url, { waitUntil: 'load', timeout: 120000 });
  await page.waitForFunction(readyExpr, null, { timeout: 120000, polling: 50 });
  const readyAt = await page.evaluate(() => performance.now());
  // "interativo": 2 s sem tarefa longa (como o TTI do Lighthouse, janela menor), até 60 s
  let lastLong = 0, quietSince = Date.now();
  while (Date.now() - t0 < 60000) {
    await page.waitForTimeout(250);
    const long = await page.evaluate(() => window.__perf.long.length);
    if (long !== lastLong) { lastLong = long; quietSince = Date.now(); }
    if (Date.now() - quietSince >= 2000) break;
  }
  const load = await page.evaluate((ready) => {
    const L = window.__perf.long;
    const nav = performance.getEntriesByType('navigation')[0];
    const fcp = performance.getEntriesByName('first-contentful-paint')[0]?.startTime ?? null;
    const tti = L.length ? Math.max(...L.map(([s, d]) => s + d)) : (nav?.loadEventEnd ?? 0);
    const tbt = L.filter(([s]) => s <= tti).reduce((a, [, d]) => a + Math.max(0, d - 50), 0);
    return { fcp, tti: Math.max(tti, ready), tbt, longTasks: L.length, longest: L.reduce((a, [, d]) => Math.max(a, d), 0), domContentLoaded: nav?.domContentLoadedEventEnd ?? null, ready };
  }, readyAt);
  // bytes que trafegaram (comprimidos): Resource Timing; o encodedDataLength do DevTools vem
  // descomprimido com a rede emulada
  const rt = await page.evaluate(() => {
    const all = [...performance.getEntriesByType('navigation'), ...performance.getEntriesByType('resource')];
    return { transfer: all.reduce((a, e) => a + (e.transferSize || 0), 0), decoded: all.reduce((a, e) => a + (e.decodedBodySize || 0), 0),
      top: all.map((e) => [e.transferSize || 0, e.name]).sort((a, b) => b[0] - a[0]).slice(0, 8) };
  });
  const loadBytes = rt.transfer || bytes;
  const biggest = (rt.transfer ? rt.top : sizes.sort((a, b) => b[0] - a[0]).slice(0, 8)).map(([n, u]) => `${Math.round(n / 1024)} KB ${String(u).split('?')[0].split('/').slice(-2).join('/')}`);

  if (profile && runIdx === 0) {
    const { profile: p } = await cdp.send('Profiler.stop');
    mkdirSync(join(here, 'out'), { recursive: true });
    writeFileSync(join(here, 'out', `${name}-x${rate}.cpuprofile`), JSON.stringify(p));
  }

  // memória depois de GC
  await cdp.send('HeapProfiler.enable');
  await cdp.send('HeapProfiler.collectGarbage');
  const metrics = Object.fromEntries((await cdp.send('Performance.getMetrics')).metrics.map((m) => [m.name, m.value]));
  const dom = await cdp.send('Memory.getDOMCounters').catch(() => ({}));
  const canvas = await page.evaluate(() => window.__perf.canvasPixels());

  // rolagem: roda do mouse no meio da janela, 40 passos de 120 px descendo e 40 subindo
  await page.mouse.move(683, 450);
  await page.evaluate(() => { window.__perf.long.length = 0; window.__perf.startFrames(); });
  const s0 = Date.now();
  for (let k = 0; k < 80; k++) { await page.mouse.wheel(0, k < 40 ? 120 : -120); await page.waitForTimeout(25); }
  await page.waitForTimeout(400);
  const frames = await page.evaluate(() => window.__perf.stopFrames());
  const scrollMs = Date.now() - s0;
  const gaps = frames.slice(1).map((t, i) => t - frames[i]);
  const scrollLong = await page.evaluate(() => window.__perf.long.map(([, d]) => d));
  const scroll = {
    fps: r1((frames.length - 1) / ((frames.at(-1) - frames[0]) / 1000)),
    framesOver33: gaps.filter((g) => g > 33.4).length,
    framesOver50: gaps.filter((g) => g > 50).length,
    worstFrame: r1(Math.max(...gaps)),
    longTasks: scrollLong.length,
    blockedMs: r1(scrollLong.reduce((a, d) => a + d, 0)),
    wallMs: scrollMs,
  };

  // digitação (só onde se pode editar): 40 caracteres num parágrafo do corpo
  let typing = null;
  if (doType) {
    await page.evaluate(() => { window.__perf.events.length = 0; });
    await page.mouse.click(600, 420);
    await page.waitForTimeout(500);
    if (profileTyping && runIdx === 0) { await cdp.send('Profiler.enable'); await cdp.send('Profiler.setSamplingInterval', { interval: 200 }); await cdp.send('Profiler.start'); }
    await page.keyboard.type('a velocidade de digitação importa muito ', { delay: 60 });
    await page.waitForTimeout(800);
    if (profileTyping && runIdx === 0) {
      const { profile: pr } = await cdp.send('Profiler.stop');
      mkdirSync(join(here, 'out'), { recursive: true });
      writeFileSync(join(here, 'out', `${name}-typing-x${rate}.cpuprofile`), JSON.stringify(pr));
    }
    const ev = await page.evaluate(() => window.__perf.events.filter(([n]) => n === 'keydown' || n === 'keypress' || n === 'input' || n === 'keyup'));
    const byKey = ev.filter(([n]) => n === 'keydown' || n === 'keypress').map(([, d]) => d);
    typing = { events: ev.length, p50: median(byKey), max: byKey.length ? Math.max(...byKey) : 0, over50: byKey.filter((d) => d > 50).length, over100: byKey.filter((d) => d > 100).length };
  }

  await ctx.close();
  return {
    load: { bytesMB: +(loadBytes / 1048576).toFixed(2), decodedMB: +(rt.decoded / 1048576).toFixed(2), requests, biggest, ready: r1(load.ready), fcp: load.fcp && r1(load.fcp), tti: r1(load.tti), tbt: r1(load.tbt), longTasks: load.longTasks, longest: r1(load.longest) },
    memory: { jsHeapMB: +(metrics.JSHeapUsedSize / 1048576).toFixed(1), domNodes: dom.nodes ?? metrics.Nodes, listeners: dom.jsEventListeners ?? metrics.JSEventListeners, canvasMB: +canvas.mb.toFixed(1), canvases: canvas.canvases },
    scroll, typing,
  };
}

const result = { name, url: url0.split('?')[0], date: new Date().toISOString(), network: '20 Mbps / 60 ms', byRate: {} };
for (const rate of rates) {
  const rs = [];
  for (let i = 0; i < runs; i++) rs.push(await once(rate, i));
  const pick = (f) => median(rs.map(f));
  result.byRate[`${rate}x`] = {
    runs: rs.length,
    load: { bytesMB: pick((r) => r.load.bytesMB), ready: pick((r) => r.load.ready), tti: pick((r) => r.load.tti), tbt: pick((r) => r.load.tbt), longest: pick((r) => r.load.longest), fcp: pick((r) => r.load.fcp ?? NaN) },
    memory: { jsHeapMB: pick((r) => r.memory.jsHeapMB), domNodes: pick((r) => r.memory.domNodes), canvasMB: pick((r) => r.memory.canvasMB), canvases: pick((r) => r.memory.canvases) },
    scroll: { fps: pick((r) => r.scroll.fps), framesOver33: pick((r) => r.scroll.framesOver33), framesOver50: pick((r) => r.scroll.framesOver50), worstFrame: pick((r) => r.scroll.worstFrame), blockedMs: pick((r) => r.scroll.blockedMs) },
    ...(doType ? { typing: { p50: pick((r) => r.typing.p50), max: pick((r) => r.typing.max), over50: pick((r) => r.typing.over50), over100: pick((r) => r.typing.over100) } } : {}),
    raw: rs,
  };
  console.log(name, `${rate}x`, JSON.stringify({ ...result.byRate[`${rate}x`], raw: undefined }));
}
mkdirSync(join(here, 'out'), { recursive: true });
writeFileSync(join(here, 'out', `${name}.json`), JSON.stringify(result, null, 1));
await browser.close();
process.exit(0);
