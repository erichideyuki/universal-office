// POSIX `join`, `dirname`, `resolve`, `relative`, `basename` and `sep` for the URL paths the runner serves.
export const sep = '/';

function normalize(parts) {
  const out = [];
  for (const segment of parts.join('/').split('/')) {
    if (segment === '..') out.pop();
    else if (segment !== '' && segment !== '.') out.push(segment);
  }
  return out;
}

export function join(...parts) {
  return '/' + normalize(parts).join('/');
}

export function resolve(...parts) {
  let at = 0;
  parts.forEach((p, i) => { if (String(p).startsWith('/')) at = i; });
  return '/' + normalize(parts.slice(at)).join('/');
}

export function relative(from, to) {
  const a = normalize([from]);
  const b = normalize([to]);
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  return [...a.slice(i).map(() => '..'), ...b.slice(i)].join('/');
}

export function dirname(path) {
  return path.slice(0, path.lastIndexOf('/')) || '/';
}

export function basename(path) {
  return path.slice(path.lastIndexOf('/') + 1);
}
