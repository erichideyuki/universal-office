// `fileURLToPath` and `pathToFileURL` for modules served over HTTP: the path of the URL, and a URL on the runner's origin.
export function fileURLToPath(url) {
  return new URL(url).pathname;
}

export function pathToFileURL(path) {
  return new URL(path, location.origin);
}
