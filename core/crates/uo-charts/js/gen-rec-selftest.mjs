// Generates `tests/captures/rec-selftest.txt`: hand-written canvas call sequences (a small line DSL, the calls render.ts makes)
// and the draw-form op streams the frozen recorder (`record-ctx.mjs`, the TS side of the harness) gives for them. The native
// `RecCanvas` (`tests/support/rec.rs`) runs the same sequences through the `Canvas` trait and must print the same lines
// (`parity.rs` `rec_canvas_matches_the_recorder`).
//
//   node core/crates/uo-charts/js/gen-rec-selftest.mjs
//
// File format: `#case <name>`, then `> <call>` lines (the DSL below), then `< <expected draw-form line>` lines.
//
// DSL (JSON for strings, numbers as written; `S` = the chart font stack of the test: `'F', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif`):
//   save | restore | translate x y | rotate a | alphaMul a | beginPath | closePath | fill | stroke | clip
//   moveTo x y | lineTo x y | bezier x1 y1 x2 y2 x y | rect x y w h | roundRect x y w h r | arc cx cy r a0 a1 ccw |
//   ellipse cx cy rx ry rot a0 a1 ccw | fillRect x y w h | strokeRect x y w h | fillText "t" x y | drawImage media x y w h
//   fillCss "css" | fillLin x0 y0 x1 y1 off "color" off "color"... | fillRad x0 y0 r0 x1 y1 r1 off "color"... |
//   fillStretch media a b c d e f | strokeCss "css" | lineWidth w | dash a,b,... (`-` = none) | cap butt|round|square |
//   join round|bevel|miter | shadow "color" blur dx dy | font "Family" bold italic size | align left|center|right |
//   baseline middle|alphabetic
import { createHash } from 'node:crypto';
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createRecorder } from '../../uo-pptx/js/lib/record-ctx.mjs';
import { form } from '../../uo-pptx/js/lib/opstream.mjs';

const here = fileURLToPath(new URL('.', import.meta.url));
const out = join(resolve(here, '..'), 'tests', 'captures', 'rec-selftest.txt');
const STACK = (f) => `'${f}', 'Aptos', Calibri, 'Liberation Sans', Arial, sans-serif`;

const CASES = {
  'transform-and-rects': `
save
translate 10.5 20.25
fillCss "#112233"
fillRect 0 0 100 50
strokeCss "rgba(1, 2, 3, 0.5)"
lineWidth 2.5
strokeRect 1 1 98 48
save
rotate 0.5235987755982988
translate -3 4
alphaMul 0.5
rect 0 0 10 10
fill
restore
restore
lineWidth -1
lineWidth 0
rect 5 5 5 5
stroke`,
  'strokes-dash-cap-shadow': `
strokeCss "#336699"
lineWidth 1.5
dash 4,2,1
cap round
join bevel
shadow "rgba(0, 0, 0, 0.3)" 4 2 -2
beginPath
moveTo 0 0
lineTo 50 25
bezier 60 0 70 40 80 10
closePath
stroke
dash 2,-1
dash -
beginPath
moveTo 1 1
lineTo 9 9
stroke`,
  'arcs-ellipse-roundrect-clip': `
save
translate 100 50
rotate 1
beginPath
arc 0 0 30 0 6.283185307179586 0
fillCss "#ff0000"
fill
beginPath
ellipse 5 5 20 10 0.7853981633974483 0 3.141592653589793 1
clip
beginPath
roundRect 0 0 40 30 6
fillCss "#00ff00"
fill
save
clip
fillRect 0 0 5 5
restore
restore`,
  'text-and-fonts': `
font "Calibri" 0 0 13.333333333333334
fillCss "#222222"
align center
baseline middle
fillText "Revenue (R$ mil)" 100 20
font "Cambria" 1 1 NaN
fillText "invalid font keeps the previous one" 100 40
font "Cambria" 1 1 -2
fillText "negative too" 100 50
font "Cambria" 1 1 9.996
align right
baseline alphabetic
save
translate 10 100
rotate -1.5707963267948966
fillText "Ünï \\u00e7 日本" 0 0
restore`,
  'gradients-and-patterns': `
fillLin 0 0 100 0 0 "rgba(255, 0, 0, 1)" 0.5 "#00ff00" 1 "rgb(0, 0, 255)"
fillRect 10 10 80 40
save
translate 5 5
scale_skip
fillRad 50 50 0 50 50 40 0 "#ffffff" 1 "#000000"
beginPath
rect 0 0 100 100
fill
restore
fillStretch 0 0.5 0 0 0.5 10 20
fillRect 0 0 60 30
drawImage 1 3 4 50 25
fillStretch 0 1 0 0 1 0 0
rect 0 0 5 5
fill`,
};

function run(script) {
  const rec = createRecorder({ width: 480, height: 300, caps: { roundRect: true, strictFont: true } });
  rec.install();
  const ctx = rec.ctx;
  const media = [0, 1].map((i) => rec.image(Buffer.from(`media${i}`), 32, 32));
  const calls = [];
  try {
    for (const raw of script.trim().split('\n')) {
      const line = raw.trim();
      if (!line || line === 'scale_skip') continue;
      calls.push(line);
      const toks = tokenize(line);
      const [op, ...a] = toks;
      const n = a.map((x) => (typeof x === 'string' && /^-?[\d.]+(e[+-]?\d+)?$|^NaN$/.test(x) ? Number(x) : x));
      switch (op) {
        case 'save': ctx.save(); break;
        case 'restore': ctx.restore(); break;
        case 'translate': ctx.translate(n[0], n[1]); break;
        case 'rotate': ctx.rotate(n[0]); break;
        case 'alphaMul': ctx.globalAlpha = ctx.globalAlpha * n[0]; break;
        case 'beginPath': ctx.beginPath(); break;
        case 'closePath': ctx.closePath(); break;
        case 'fill': ctx.fill(); break;
        case 'stroke': ctx.stroke(); break;
        case 'clip': ctx.clip(); break;
        case 'moveTo': ctx.moveTo(n[0], n[1]); break;
        case 'lineTo': ctx.lineTo(n[0], n[1]); break;
        case 'bezier': ctx.bezierCurveTo(...n); break;
        case 'rect': ctx.rect(...n); break;
        case 'roundRect': ctx.roundRect(...n); break;
        case 'arc': ctx.arc(n[0], n[1], n[2], n[3], n[4], n[5] === 1); break;
        case 'ellipse': ctx.ellipse(n[0], n[1], n[2], n[3], n[4], n[5], n[6], n[7] === 1); break;
        case 'fillRect': ctx.fillRect(...n); break;
        case 'strokeRect': ctx.strokeRect(...n); break;
        case 'fillText': ctx.fillText(a[0], n[1], n[2]); break;
        case 'drawImage': ctx.drawImage(media[n[0]], n[1], n[2], n[3], n[4]); break;
        case 'fillCss': ctx.fillStyle = a[0]; break;
        case 'fillLin': { const g = ctx.createLinearGradient(n[0], n[1], n[2], n[3]); for (let i = 4; i < n.length; i += 2) g.addColorStop(n[i], a[i + 1]); ctx.fillStyle = g; break; }
        case 'fillRad': { const g = ctx.createRadialGradient(n[0], n[1], n[2], n[3], n[4], n[5]); for (let i = 6; i < n.length; i += 2) g.addColorStop(n[i], a[i + 1]); ctx.fillStyle = g; break; }
        case 'fillStretch': { const p = ctx.createPattern(media[n[0]], 'no-repeat'); p.setTransform(new DOMMatrix(n.slice(1, 7))); ctx.fillStyle = p; break; }
        case 'strokeCss': ctx.strokeStyle = a[0]; break;
        case 'lineWidth': ctx.lineWidth = n[0]; break;
        case 'dash': ctx.setLineDash(a[0] === '-' ? [] : a[0].split(',').map(Number)); break;
        case 'cap': ctx.lineCap = a[0]; break;
        case 'join': ctx.lineJoin = a[0]; break;
        case 'shadow': ctx.shadowColor = a[0]; ctx.shadowBlur = n[1]; ctx.shadowOffsetX = n[2]; ctx.shadowOffsetY = n[3]; break;
        case 'font': ctx.font = `${n[2] ? 'italic ' : ''}${n[1] ? 'bold ' : ''}${(+n[3]).toFixed(2)}px ${STACK(a[0])}`; break;
        case 'align': ctx.textAlign = a[0]; break;
        case 'baseline': ctx.textBaseline = a[0]; break;
        default: throw new Error(`unknown DSL op ${op}`);
      }
    }
  } finally { rec.uninstall(); }
  return { calls, lines: form(rec.lines, 'draw'), media: [0, 1].map((i) => `img(${createHash('sha1').update(Buffer.from(`media${i}`)).digest('hex').slice(0, 12)})`) };
}

/** whitespace-separated tokens; a JSON string is one token (decoded) */
function tokenize(line) {
  const out = [];
  let i = 0;
  while (i < line.length) {
    if (line[i] === ' ') { i++; continue; }
    if (line[i] === '"') {
      let j = i + 1;
      while (line[j] !== '"') j += line[j] === '\\' ? 2 : 1;
      out.push(JSON.parse(line.slice(i, j + 1)));
      i = j + 1;
    } else {
      let j = i;
      while (j < line.length && line[j] !== ' ') j++;
      out.push(line.slice(i, j));
      i = j;
    }
  }
  return out;
}

let text = '# Hand-written canvas call sequences and the draw-form streams of the frozen recorder; generated by js/gen-rec-selftest.mjs.\n';
for (const [name, script] of Object.entries(CASES)) {
  const r = run(script);
  text += `#case ${name}\n` + r.calls.map((c) => `> ${c}\n`).join('') + r.lines.map((l) => `< ${l}\n`).join('');
}
mkdirSync(dirname(out), { recursive: true });
writeFileSync(out, text);
console.log(`${Object.keys(CASES).length} cases -> ${out}`);
