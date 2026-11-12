// Endereço de hiperlink digitado ou colado (diálogo de link, API, colagem):
// limpa como o navegador limpa uma URL, completa o esquema que falta e confere
// contra a lista de esquemas do hospedeiro (atributo `link-schemes`). Sem lista,
// o comportamento é o de sempre (link para arquivo, relativo, qualquer esquema),
// só com `www.` virando `http://`. Esquemas de script nunca passam.

export type LinkAddressRejection = 'empty' | 'scheme' | 'invalid';

export type LinkAddressResult =
  | { readonly ok: true; readonly address: string }
  | { readonly ok: false; readonly reason: LinkAddressRejection };

/** Tamanho máximo do endereço, depois da limpeza. */
export const MAX_LINK_ADDRESS_LENGTH = 2048;

/** Entrada maior que isto nem passa pela limpeza (um href colado com espaços sem fim). */
const MAX_INPUT_LENGTH = MAX_LINK_ADDRESS_LENGTH * 4;

const BLOCKED_SCHEMES: ReadonlySet<string> = new Set(['javascript', 'vbscript', 'data', 'blob']);

// domínios de topo de um endereço web sem esquema (`relatorio.pdf` não é endereço)
const TLD = [
  'com|org|net|gov|edu|mil|int|info|biz|io|dev|app|ai|co|me|tv|eu',
  'br|pt|us|uk|de|fr|es|it|nl|be|ch|at|ie|pl|se|no|dk|fi|ru',
  'mx|ar|cl|pe|uy|py|bo|ec|ve|gt|cr|pa|do|hn|sv|ni|cu|pr|ca',
  'jp|cn|in|kr|au|nz|za',
].join('|');

// esquema com 2+ letras: `C:\pasta` é caminho do Windows, não esquema
const SCHEME_RE = /^([a-z][a-z0-9+.-]+):(.?)/i;
const WEB_RE = /^https?:\/\/[^\s/?#\\]+(?:[/?#]\S*)?$/i;
const MAILTO_RE = /^mailto:[^\s?@]+@[^\s?@]+(?:\?\S*)?$/i;
const EMAIL_RE = /^[\p{L}\p{N}._%+-]+@[\p{L}\p{N}-]+(?:\.[\p{L}\p{N}-]+)+$/u;
const WWW_RE = /^www\.\S+$/i;
const DOMAIN_RE = new RegExp(String.raw`^[\p{L}\p{N}-]+(?:\.[\p{L}\p{N}-]+)*\.(?:${TLD})(?::\d{1,5})?(?:[/?#]\S*)?$`, 'iu');
const NEWLINE_TAB_RE = /[\t\n\r]/g;
const CONTROL_RE = /[\u0000-\u001F\u007F]/;
const LONE_SURROGATE_RE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/;

const accepted = (address: string): LinkAddressResult => ({ ok: true, address });
const rejected = (reason: LinkAddressRejection): LinkAddressResult => ({ ok: false, reason });

/** Tira controles C0 e espaços das pontas (laço linear: a regex `^…+|…+$` é quadrática). */
function trimEdges(input: string): string {
  let start = 0;
  let end = input.length;
  while (start < end && input.charCodeAt(start) <= 0x20) start++;
  while (end > start && input.charCodeAt(end - 1) <= 0x20) end--;
  return input.slice(start, end);
}

function clean(input: string): string {
  return trimEdges(input).replace(NEWLINE_TAB_RE, '');
}

/** http/https com host e porta que o servidor também aceita (o `new URL` recusa porta > 65535 e host vazio ou inválido). */
function validWeb(address: string): boolean {
  if (!WEB_RE.test(address)) return false;
  try {
    return new URL(address).hostname.length > 0;
  } catch {
    return false;
  }
}

function wellFormed(address: string): boolean {
  return address.length <= MAX_LINK_ADDRESS_LENGTH && !CONTROL_RE.test(address) && !LONE_SURROGATE_RE.test(address);
}

/** Esquema em minúsculas, ou nada: `exemplo.com:8080` é host com porta, não o esquema `exemplo.com`. */
function schemeOf(address: string): string | undefined {
  const m = SCHEME_RE.exec(address);
  if (!m) return undefined;
  const [, scheme = '', next = ''] = m;
  return scheme.includes('.') && /\d/.test(next) ? undefined : scheme.toLowerCase();
}

function withScheme(address: string, scheme: string, schemes: readonly string[] | null): LinkAddressResult {
  if (BLOCKED_SCHEMES.has(scheme)) return rejected('scheme');
  if (!schemes) return accepted(address);
  if (!schemes.includes(scheme)) return rejected('scheme');
  if (scheme === 'http' || scheme === 'https') return validWeb(address) ? accepted(address) : rejected('invalid');
  if (scheme === 'mailto') return MAILTO_RE.test(address) ? accepted(address) : rejected('invalid');
  return accepted(address);
}

function withoutScheme(address: string, schemes: readonly string[] | null): LinkAddressResult {
  if (!schemes) return accepted(WWW_RE.test(address) ? `http://${address}` : address);
  if (EMAIL_RE.test(address)) return schemes.includes('mailto') ? accepted(`mailto:${address}`) : rejected('scheme');
  if (!WWW_RE.test(address) && !DOMAIN_RE.test(address)) return rejected('invalid');
  const web = schemes.includes('http') ? 'http' : schemes.includes('https') ? 'https' : null;
  if (!web) return rejected('scheme');
  const full = `${web}://${address}`;
  return validWeb(full) ? accepted(full) : rejected('invalid');
}

/**
 * Endereço pronto para gravar no link, ou o motivo da recusa. `schemes` nulo =
 * sem restrição de esquema (padrão do componente); lista = só esses esquemas,
 * com e-mail sem esquema virando `mailto:` e endereço web virando `http://`
 * (ou `https://`, se só ele for permitido). `#âncora` passa sempre.
 */
export function normalizeLinkAddress(input: string, schemes: readonly string[] | null): LinkAddressResult {
  if (input.length > MAX_INPUT_LENGTH) return rejected('invalid');
  const address = clean(input);
  if (!address) return rejected('empty');
  if (!wellFormed(address)) return rejected('invalid');
  if (address.startsWith('#')) return address.length > 1 && !/\s/.test(address) ? accepted(address) : rejected('invalid');
  const scheme = schemeOf(address);
  return scheme ? withScheme(address, scheme, schemes) : withoutScheme(address, schemes);
}

/** Lista do atributo `link-schemes` (espaço ou vírgula); ausente = sem restrição. */
export function parseLinkSchemes(attr: string | null): readonly string[] | null {
  if (attr === null) return null;
  return attr
    .split(/[\s,]+/)
    .map((s) => s.replace(/:$/, '').toLowerCase())
    .filter(Boolean);
}
