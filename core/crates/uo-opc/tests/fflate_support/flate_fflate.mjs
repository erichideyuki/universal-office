// Generator of the fflate expectations in tests/flate_fflate.rs.
//
// Run: node tests/fflate_support/flate_fflate.mjs <fflate location>
//   e.g. <fflate location> = a UniversalOffice checkout with write/node_modules installed.
// Prints JSON: case name -> [input hex, fflate result ("ok:<output hex>" or "err:<message>")].
// The inputs are the hex literals of the tests (the constants OVERSUB, INCOMPLETE,
// INCOMPLETE_OK and ALL_ZERO are the inputs of cases oversub, incomplete, incomplete_ok and
// allzero) and the results are what each test asserts. "normal"/"zlib" outputs are
// "hello hello hello hello world, " x 20 (`text()` in the test).

import { existsSync, readFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

// Loads fflate from the directory given as the first argument: the fflate package itself, a
// directory with node_modules/fflate, or a checkout whose write/node_modules has it.
async function loadFflate(arg) {
  if (!arg) {
    console.error('usage: node <script> <fflate package dir | dir with node_modules | UniversalOffice checkout>');
    process.exit(2);
  }
  const base = resolve(arg);
  for (const dir of [base, join(base, 'node_modules', 'fflate'), join(base, 'write', 'node_modules', 'fflate')]) {
    const entry = join(dir, 'esm', 'index.mjs');
    if (existsSync(entry)) {
      const { version } = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8'));
      if (version !== '0.8.3') console.error(`warning: fflate ${version}; the fixtures were made with 0.8.3`);
      return import(pathToFileURL(entry).href);
    }
  }
  console.error(`fflate not found under ${base}`);
  process.exit(2);
}
const f = await loadFflate(process.argv[2]);
const hex = (u) => Buffer.from(u).toString('hex');
// LSB-first bit writer
class BW { constructor(){this.b=[];this.n=0;} w(v,k){for(let i=0;i<k;i++){const bit=(v>>i)&1; const o=this.n>>3; if(o>=this.b.length)this.b.push(0); this.b[o]|=bit<<(this.n&7); this.n++;}} code(c,l){for(let i=l-1;i>=0;i--)this.w((c>>i)&1,1);} out(){return new Uint8Array(this.b);} }
// dynamic block: code-length alphabet = symbols 0..15 with length 4 (code = symbol)
function dyn(lit, dist, data){ const w=new BW(); w.w(1,1); w.w(2,2); w.w(lit.length-257,5); w.w(dist.length-1,5); w.w(19-4,4);
  const clim=[16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15]; for(const s of clim) w.w(s<16?4:0,3);
  for(const l of [...lit,...dist]) w.code(l,4); data(w); return w.out(); }
const res = (fn) => { try { return 'ok:' + hex(fn()); } catch (e) { return 'err:' + e.message; } };
const cases = {};
const text = new TextEncoder().encode('hello hello hello hello world, '.repeat(20));
cases.normal = [hex(f.deflateSync(text,{level:6})), res(()=>f.inflateSync(f.deflateSync(text,{level:6})))];
// stored block with bad NLEN
const st = new Uint8Array([1, 3, 0, 0x12, 0x34, 0x41, 0x42, 0x43]);
cases.stored_bad_nlen = [hex(st), res(()=>f.inflateSync(st))];
// code 16 at index 0 + lit lengths: 'A'(65) and 256 length 1; using alphabet with 16 = length 4? use clt with 16 too
{ const w=new BW(); w.w(1,1); w.w(2,2); w.w(0,5); w.w(0,5); w.w(19-4,4);
  // clt: 0..15 len 5 would be over-sub; use: 16 len 1, 0 len 2, 1 len 3, 18 len 3 -> canonical: 16:0, 0:10, 1:110, 18:111
  const L={16:1,0:2,1:3,18:3}; const clim=[16,17,18,0,8,7,9,6,10,5,11,4,12,3,13,2,14,1,15]; for(const s of clim) w.w(L[s]||0,3);
  const C={16:[0,1],0:[2,2],1:[6,3],18:[7,3]};
  const put=(s)=>w.code(C[s][0],C[s][1]);
  put(16); w.w(0,2); // repeat ldt[-1] (0) 3 times -> 0..2
  put(18); w.w(62-11,7); // 3..64
  put(1); // 65
  put(18); w.w(138-11,7); put(18); w.w(52-11,7); // 66..255
  put(1); // 256
  put(0); // dist[0]
  w.code(0,1); w.code(0,1); w.code(1,1); // A A EOB
  const d=w.out(); cases.repeat16_first=[hex(d), res(()=>f.inflateSync(d))]; }
// fixed block: 'A', length 3 (sym 257), distance code 31 (NaN), EOB
{ const w=new BW(); w.w(1,1); w.w(1,2); w.code(0x30+65,8); w.code(1,7); w.code(31,5); w.code(0,7); const d=w.out(); cases.dist31=[hex(d),res(()=>f.inflateSync(d))]; }
{ const w=new BW(); w.w(1,1); w.w(1,2); w.code(0x30+65,8); w.code(1,7); w.code(30,5); w.code(0,7); const d=w.out(); cases.dist30=[hex(d),res(()=>f.inflateSync(d))]; }
// length symbol 286 (base 260) and 287 with distance 1
{ const w=new BW(); w.w(1,1); w.w(1,2); w.code(0x30+66,8); w.code(0xC0+(286-280),8); w.code(0,5); w.code(0xC0+(287-280),8); w.code(0,5); w.code(0,7); const d=w.out(); cases.len286=[hex(d),res(()=>f.inflateSync(d))]; }
// over-subscribed: lengths 1 for 65, 66, 256 -> bit 0 decodes 256 per fflate's table
{ const lit=new Array(257).fill(0); lit[65]=1; lit[66]=1; lit[256]=1; const d=dyn(lit,[0],(w)=>{w.w(1,1); w.w(0,1);}); cases.oversub=[hex(d),res(()=>f.inflateSync(d))]; }
// incomplete tree: only 65 length 2 and 256 length 2; bits 11 unassigned -> invalid length/literal
{ const lit=new Array(257).fill(0); lit[65]=2; lit[256]=2; const d=dyn(lit,[0],(w)=>{w.code(0,2); w.code(3,2);}); cases.incomplete=[hex(d),res(()=>f.inflateSync(d))]; }
{ const lit=new Array(257).fill(0); lit[65]=2; lit[256]=2; const d=dyn(lit,[0],(w)=>{w.code(0,2); w.code(0,2); w.code(1,2);}); cases.incomplete_ok=[hex(d),res(()=>f.inflateSync(d))]; }
// all-zero code lengths -> invalid length/literal
{ const d=dyn(new Array(257).fill(0),[0],(w)=>{w.w(0,8);}); cases.allzero=[hex(d),res(()=>f.inflateSync(d))]; }
// truncated
{ const d=f.deflateSync(text); const t=d.subarray(0,d.length-3); cases.truncated=[hex(t),res(()=>f.inflateSync(t))]; }
cases.btype3=['07', res(()=>f.inflateSync(new Uint8Array([7])))];
// declared sizes
{ const d=f.deflateSync(new TextEncoder().encode('abcdefghij'.repeat(3))); cases.decl_small=[hex(d), res(()=>f.inflateSync(d,{out:new Uint8Array(7)}))]; cases.decl_large=[hex(d), res(()=>f.inflateSync(d,{out:new Uint8Array(100)}))]; }
cases.decl_stored_over=[hex(st), res(()=>f.inflateSync(st,{out:new Uint8Array(2)}))];
cases.decl_stored_exact=[hex(st), res(()=>f.inflateSync(st,{out:new Uint8Array(3)}))];
// two blocks: fixed literals beyond declared, then stored block (bt already past end)
{ const w=new BW(); w.w(0,1); w.w(1,2); for(const c of [65,66,67,68]) w.code(0x30+c,8); w.code(0,7); w.w(1,1); w.w(0,2); const pre=w.out(); const d=new Uint8Array([...pre,0,0,0,0]); cases.decl_lit_then_stored0=[hex(d), res(()=>f.inflateSync(d,{out:new Uint8Array(2)}))]; cases.lit_then_stored0=[hex(d), res(()=>f.inflateSync(d))]; }
cases.empty_decl=['', res(()=>f.inflateSync(new Uint8Array(0),{out:new Uint8Array(5)}))];
// zlib
{ const z=f.zlibSync(text,{level:6}); cases.zlib=[hex(z), res(()=>f.unzlibSync(z))]; const bad=z.slice(); bad[bad.length-1]^=0xff; cases.zlib_bad_adler=[hex(bad),res(()=>f.unzlibSync(bad))];
  cases.zlib_hdr=['7801', res(()=>f.unzlibSync(new Uint8Array([0x78,0x9c,1,2])))]; cases.zlib_bad=['7800',res(()=>f.unzlibSync(new Uint8Array([0x78,0x00,1,2,3,4,5])))];
  cases.zlib_dict=['78bb', res(()=>f.unzlibSync(new Uint8Array([0x78,0xbb,1,2,3,4,5,6,7,8])))];
  const s=[]; const u=new f.Unzlib(); u.ondata=(c)=>s.push(hex(c)); cases.unzlib_stream_short=['789c0102', (()=>{try{u.push(new Uint8Array([0x78,0x9c,1,2]),true); return 'ok:'+s.join('')}catch(e){return 'err:'+e.message}})()];
  for (const lv of [0,1,5,6,9]) cases['zlib_hdr_'+lv]=[hex(f.zlibSync(new Uint8Array([1,2,3]),{level:lv}).subarray(0,2)),''];
  cases.adler=[hex(text), (()=>{const z=f.zlibSync(text); return hex(z.subarray(z.length-4));})()];
}
console.log(JSON.stringify(cases, null, 1));
