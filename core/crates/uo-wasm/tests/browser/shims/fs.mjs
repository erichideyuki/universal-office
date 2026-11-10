// `readFileSync`, `readdirSync`, `existsSync` and `statSync` over the runner's HTTP origin (a synchronous XHR; `?__fs=stat` and
// `?__fs=list` are answered by run.mjs). A synchronous XHR cannot return an ArrayBuffer in a window, so the bytes come back as a
// binary string (x-user-defined) and are unpacked. Writing is not available.
function get(path, query, binary) {
  const xhr = new XMLHttpRequest();
  xhr.open('GET', query ? `${path}?__fs=${query}` : path, false);
  if (binary) xhr.overrideMimeType('text/plain; charset=x-user-defined');
  xhr.send();
  return xhr;
}

export function readFileSync(path, options) {
  const xhr = get(path, '', true);
  if (xhr.status !== 200) throw new Error(`ENOENT: ${path} (HTTP ${xhr.status})`);
  const text = xhr.responseText;
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i) & 0xff;
  const encoding = typeof options === 'string' ? options : options?.encoding;
  return encoding === 'utf8' || encoding === 'utf-8' ? new TextDecoder().decode(bytes) : bytes;
}

function stat(path) {
  const xhr = get(path, 'stat', false);
  return xhr.status === 200 ? JSON.parse(xhr.responseText) : { exists: false };
}

export const existsSync = (path) => stat(path).exists;

export function statSync(path) {
  const s = stat(path);
  if (!s.exists) throw new Error(`ENOENT: ${path}`);
  return { isDirectory: () => s.dir, isFile: () => !s.dir, size: s.size };
}

export function readdirSync(path) {
  const xhr = get(path, 'list', false);
  if (xhr.status !== 200) throw new Error(`ENOTDIR: ${path} (HTTP ${xhr.status})`);
  return JSON.parse(xhr.responseText);
}

export function writeFileSync() {
  throw new Error('writeFileSync is not available in the browser runner');
}
