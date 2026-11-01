// Minimal check of the uo-wasm build in real Safari (BrowserStack): serves an auto-running page
// (page.html), the wasm and four repository files, and appends every result the page POSTs to
// results.jsonl next to this file. BrowserStack's remote MCP cannot reach localhost, so the
// server is exposed for the run with a temporary anonymous tunnel and taken down after:
//   cargo build -p uo-wasm --features count,opc-memtest --target wasm32-unknown-unknown --profile wasm
//   node crates/uo-wasm/tests/browser/safari/server.mjs <core dir> <repo root> [port]
//   devtunnel host -p 8765 --allow-anonymous --expiration 2h   (then delete the tunnel)
import { createServer } from 'node:http';
import { readFileSync, appendFileSync } from 'node:fs';
import { join } from 'node:path';
const [core, repo, port] = [process.argv[2], process.argv[3], Number(process.argv[4] ?? 8765)];
const here = new URL('.', import.meta.url);
const files = {
  '/': ['text/html', () => readFileSync(new URL('page.html', here))],
  '/uo_wasm.wasm': ['application/wasm', () => readFileSync(join(core, 'target/wasm32-unknown-unknown/wasm/uo_wasm.wasm'))],
  '/f/0': ['application/octet-stream', () => readFileSync(join(repo, 'write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx'))],
  '/f/1': ['application/octet-stream', () => readFileSync(join(repo, 'sheet/samples/showcase.xlsx'))],
  '/f/2': ['application/octet-stream', () => readFileSync(join(repo, 'slides/samples/stress-media.pptx'))],
  '/f/3': ['application/octet-stream', () => readFileSync(join(repo, 'slides/samples/legacy/lo-reference.ppt'))],
};
createServer((req, res) => {
  const path = new URL(req.url, 'http://x').pathname;
  console.log(new Date().toISOString(), req.method, path, req.headers['user-agent'] ?? '');
  if (req.method === 'POST' && path === '/result') {
    let body = '';
    req.on('data', (c) => { body += c; if (body.length > 1e6) req.destroy(); });
    req.on('end', () => { appendFileSync(new URL('results.jsonl', here), body.replace(/\n/g, ' ') + '\n'); res.writeHead(204).end(); });
    return;
  }
  const f = files[path];
  if (!f || req.method !== 'GET') return res.writeHead(404).end();
  res.writeHead(200, { 'content-type': f[0], 'cache-control': 'no-store' }).end(f[1]());
}).listen(port, '127.0.0.1', () => console.log(`listening on ${port}`));
