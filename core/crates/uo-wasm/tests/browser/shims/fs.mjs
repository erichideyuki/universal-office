// `readFileSync` over the runner's HTTP origin. A synchronous XHR cannot return an ArrayBuffer in
// a window, so the bytes come back as a binary string (x-user-defined) and are unpacked.
export function readFileSync(path) {
  const xhr = new XMLHttpRequest();
  xhr.open('GET', path, false);
  xhr.overrideMimeType('text/plain; charset=x-user-defined');
  xhr.send();
  if (xhr.status !== 200) throw new Error(`ENOENT: ${path} (HTTP ${xhr.status})`);
  const text = xhr.responseText;
  const bytes = new Uint8Array(text.length);
  for (let i = 0; i < text.length; i++) bytes[i] = text.charCodeAt(i) & 0xff;
  return bytes;
}
