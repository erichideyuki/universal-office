// Shared by slides_api.test.mjs and slides_drawops.test.mjs: the deterministic measurement stub of the TS draw-op captures
// (uo-pptx/js/lib/record-ctx.mjs `stubMeasure`, uo-pptx/tests/support/stub_measure.rs: the same advances the recorded TS streams were
// made with, so the wasm list is compared exactly with no font files) and the loop that answers the module's requests with it.
// Plain JS with no Node-only import: the browser runner (browser/run.mjs) loads it as is.
const KERN = new Set(['AV', 'AW', 'AY', 'Av', 'Aw', 'Ay', 'FA', 'LT', 'LV', 'LY', 'P.', 'P,', 'Ta', 'Te', 'To', 'Tr', 'Tu', 'Tw', 'Ty', 'VA', 'Va', 'Ve', 'Vo', 'Wa', 'We', 'Wo', 'Ya', 'Ye', 'Yo', 'r.', 'r,', 'y.', 'y,']);

export function stubWidth(font, text, kerningNone) {
  const m = /(\d+(?:\.\d+)?)px/.exec(font);
  const size = m ? parseFloat(m[1]) : 10;
  const bold = /(^|\s)(bold|[6-9]00)(\s|$)/.test(font);
  let em = 0, prev = '';
  for (const ch of String(text)) {
    if (!kerningNone && KERN.has(prev + ch)) em += -0.07;
    prev = ch;
    const c = ch.codePointAt(0);
    let a;
    if (ch === ' ') a = 0.278;
    else if (/[.,:;'|!ijlI]/.test(ch)) a = 0.26;
    else if (/[mwMW]/.test(ch)) a = 0.82;
    else if (/[A-Z]/.test(ch)) a = 0.66;
    else if (/[0-9]/.test(ch)) a = 0.556;
    else if (c >= 0x2e80) a = 1;
    else a = 0.5;
    em += a;
  }
  if (bold) em *= 1.06;
  return em * size;
}

/** `imageSize` of ts-paint.mjs: PNG, GIF and JPEG headers; null otherwise (the answer is then 96 x 96). */
export function imageSize(b) {
  if (b.length > 24 && b[0] === 0x89 && b[1] === 0x50) return { w: ((b[16] << 24) | (b[17] << 16) | (b[18] << 8) | b[19]) >>> 0, h: ((b[20] << 24) | (b[21] << 16) | (b[22] << 8) | b[23]) >>> 0 };
  if (b.length > 10 && b[0] === 0x47 && b[1] === 0x49) return { w: b[6] | (b[7] << 8), h: b[8] | (b[9] << 8) };
  if (b.length > 4 && b[0] === 0xff && b[1] === 0xd8) {
    let i = 2;
    while (i + 9 < b.length) {
      if (b[i] !== 0xff) { i++; continue; }
      const mk = b[i + 1];
      if (mk >= 0xc0 && mk <= 0xcf && mk !== 0xc4 && mk !== 0xc8 && mk !== 0xcc) return { h: (b[i + 5] << 8) | b[i + 6], w: (b[i + 7] << 8) | b[i + 8] };
      i += 2 + ((b[i + 2] << 8) | b[i + 3]);
    }
  }
  return null;
}

const fontStr = (f, px) => `${f.italic ? 'italic ' : ''}${f.bold ? '700 ' : '400 '}${px}px '${f.family}', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif`;

/** The answers (one group per request, in order) the stub gives to the requests of one pass. */
export function stubAnswers(deck, reqs) {
  const out = [];
  for (const r of reqs) {
    if (r.kind === 'charAt1000') out.push(stubWidth(fontStr(r.face, 1000), r.ch, true));
    else if (r.kind === 'kernAt1000') { const f = fontStr(r.face, 1000); out.push(stubWidth(f, r.text, true), stubWidth(f, r.text, false)); }
    else if (r.kind === 'width') out.push(stubWidth(r.face.family === '' ? '10px sans-serif' : fontStr(r.face, r.sizePx), r.text, r.kerning === 'none'));
    else if (r.kind === 'imageSize') { const b = deck.part(r.part); const s = b && imageSize(b); out.push(s ? s.w : 96, s ? s.h : 96); }
    else if (r.kind === 'fontAvailable') out.push(1);
    else if (r.kind === 'metaText') out.push(stubWidth(r.font, r.text, false), NaN, NaN);
    else if (r.kind === 'metaDecode') out.push(NaN, NaN);
    else if (r.kind === 'metrics') {
      // the recorder's `measureText` (record-ctx.mjs): the actual box 0.72 / 0.21 of the size, the font box 0.9 / 0.21 (the `fontK` of the charts)
      const f = r.face.family === '' ? "10px sans-serif" : fontStr(r.face, r.sizePx);
      const w = stubWidth(f, r.text, r.kerning === 'none'), s = r.face.family === '' ? 10 : r.sizePx;
      out.push(w, 0, w, 0.72 * s, 0.21 * s, 0.9 * s, 0.21 * s);
    } else out.push(0, 0, 0, 0, 0, NaN, NaN);
  }
  return Float64Array.from(out);
}

/** B4b: the full loop of one playback frame (`deck.stage`): answers until the frame arrives. */
export function stageAll(uo, deck, view, src, at, which, scale, ox, oy, media = true, clock = {}) {
  for (let round = 0; round <= uo.maxRounds; round++) {
    const r = deck.stage(view, src, at, which, scale, ox, oy, media, clock);
    if (r.status === 12) { deck.answer(stubAnswers(deck, r.requests)); continue; }
    return { ...r, rounds: round + 1 };
  }
  throw new Error('too many rounds');
}

/** The full loop of one emission: answers until the list arrives (`rounds` counts the passes). */
export function emitAll(uo, deck, view, i, scale, ox, oy) {
  for (let round = 0; round <= uo.maxRounds; round++) {
    const r = deck.emit(view, i, scale, ox, oy);
    if (r.status === 12) { deck.answer(stubAnswers(deck, r.requests)); continue; }
    return { ...r, rounds: round + 1 };
  }
  throw new Error('too many rounds');
}
