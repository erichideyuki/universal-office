// POSIX `join` and `dirname` for the URL paths the runner serves.
export function join(...parts) {
  const out = [];
  for (const segment of parts.join('/').split('/')) {
    if (segment === '..') out.pop();
    else if (segment !== '' && segment !== '.') out.push(segment);
  }
  return '/' + out.join('/');
}

export function dirname(path) {
  return path.slice(0, path.lastIndexOf('/')) || '/';
}
