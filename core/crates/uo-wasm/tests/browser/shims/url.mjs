// `fileURLToPath` for modules served over HTTP: the path of the URL.
export function fileURLToPath(url) {
  return new URL(url).pathname;
}
