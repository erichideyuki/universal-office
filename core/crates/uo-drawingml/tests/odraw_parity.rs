//! Replay of `tests/captures/odraw.capture` (`tools/capture-odraw.mjs`, the frozen TS OfficeArt
//! readers): the Word drawings of the `.doc` fixtures and of generated and mutated
//! `OfficeArtContent` streams (`read_word_drawings`, every BStore entry decoded, the PICF walk of
//! the Data stream), FOPT blocks and BLIPs under the three flavors, and the msospt preset map
//! under its two variants. The canonical streams mirror the script's functions of the same name.
#![cfg(feature = "legacy")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::Path;

use uo_drawingml::odraw::msospt::{PresetResult, PresetVariant, preset_for};
use uo_drawingml::odraw::{
    Blip, BlipBudget, Bse, Flavor, OdrawLimits, PropTable, Rec, Shape, ShapeMap, WordDrawings, decode_blip, read_fbse, read_fopt, read_rec,
    read_sp_container, read_word_drawings, recs_in, rt,
};
use uo_jsnum::conv::canonicalize_nan;
use uo_jsnum::format::Js;
use uo_testkit::stream::Digest;
use uo_testkit::{Capture, Replay, Stream};

const CAPTURE: &str = include_str!("captures/odraw.capture");
const DOC_BUDGET: u64 = 512 << 20;
const L: OdrawLimits = OdrawLimits::WORD;

fn summary(s: Stream) -> String {
    let n = s.items();
    format!("{n}:{}", s.finish().short())
}

fn unhex(h: &str) -> Vec<u8> {
    let b = h.as_bytes();
    let v = |c: u8| match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        _ => panic!("bad hex"),
    };
    b.chunks(2).map(|p| (v(p[0]) << 4) | v(p[1])).collect()
}

fn props_digest(s: &mut Stream, props: &PropTable<'_>) {
    s.tag(b'p').u32(u32::try_from(props.len()).unwrap());
    for p in props.iter() {
        s.u32(u32::from(p.id)).u32(u32::from(p.id)).u32(p.value).bool(p.bid).bool(p.complex.is_some());
        if let Some(c) = p.complex {
            s.digest_of(c);
        }
    }
}

fn rect(s: &mut Stream, r: Option<uo_drawingml::odraw::RectI32>) {
    s.bool(r.is_some());
    if let Some(r) = r {
        s.i64(r.l.into()).i64(r.t.into()).i64(r.r.into()).i64(r.b.into());
    }
}

fn shape_digest(s: &mut Stream, sh: &Shape<'_>) {
    s.tag(b'S').u32(sh.spid).u32(sh.shape_type.into()).u32(sh.flags).bool(sh.z.is_some());
    if let Some(z) = sh.z {
        s.u32(z);
    }
    props_digest(s, &sh.props);
    rect(s, sh.group_rect());
    rect(s, sh.child_rect());
    s.bool(sh.children.is_some());
    if let Some(c) = &sh.children {
        s.u32(u32::try_from(c.len()).unwrap());
        for k in c {
            shape_digest(s, k);
        }
    }
}

fn rec_digest(s: &mut Stream, r: Option<&Rec<'_>>) {
    s.bool(r.is_some());
    if let Some(r) = r {
        s.u32(r.rec_type.into()).u32(r.inst.into()).u32(r.ver.into()).u32(r.len).u64(r.hdr).u64(r.start).u64(r.end);
    }
}

fn bse_digest(s: &mut Stream, e: Option<&Bse<'_>>) {
    s.bool(e.is_some());
    if let Some(e) = e {
        s.u32(e.bt_win32.into()).u32(e.size).u32(e.fo_delay);
        rec_digest(s, e.embedded.as_ref());
    }
}

fn blip_digest(s: &mut Stream, b: Option<&Blip<'_>>) {
    match b {
        None => {
            s.tag(b'n');
        }
        Some(b) => {
            s.tag(b'b').str(b.kind.ext()).str(b.kind.mime()).digest_of(&b.bytes);
        }
    }
}

fn map_digest(s: &mut Stream, tag: u8, m: &ShapeMap<'_>) {
    s.tag(tag).u32(u32::try_from(m.len()).unwrap());
    for sh in m.iter() {
        s.u32(sh.spid);
        shape_digest(s, sh);
    }
}

fn drawings_digest(d: &WordDrawings<'_>) -> String {
    let mut s = Stream::new();
    s.tag(b'B').u32(u32::try_from(d.bstore.len()).unwrap());
    for e in &d.bstore {
        bse_digest(&mut s, e.as_ref());
    }
    map_digest(&mut s, b'M', &d.main);
    map_digest(&mut s, b'H', &d.hdr);
    s.tag(b'X').u32(u32::try_from(d.masters.len()).unwrap());
    for &m in &d.masters {
        s.u32(m);
    }
    summary(s)
}

/// `blipAt` of pictures.ts.
fn blip_at<'a>(stream: Option<&'a [u8]>, off: u32, budget: &mut BlipBudget) -> Option<Blip<'a>> {
    let stream = stream?;
    let off = u64::from(off);
    if off + 8 > stream.len() as u64 {
        return None;
    }
    let rec = read_rec(stream, off, stream.len() as u64).filter(Rec::is_blip)?;
    decode_blip(&rec, budget, Flavor::Word, &L)
}

fn blips_digest(bstore: &[Option<Bse<'_>>], word: Option<&[u8]>, left: u64) -> String {
    let mut s = Stream::new();
    let mut budget = BlipBudget { left };
    for e in bstore {
        let Some(e) = e else {
            s.tag(b'-');
            continue;
        };
        if let Some(emb) = &e.embedded {
            blip_digest(&mut s, decode_blip(emb, &mut budget, Flavor::Word, &L).as_ref());
        } else if e.fo_delay != 0xffff_ffff {
            blip_digest(&mut s, blip_at(word, e.fo_delay, &mut budget).as_ref());
        } else {
            s.tag(b'0');
        }
    }
    s.u64(budget.left);
    summary(s)
}

fn i32_at(b: &[u8], o: u64) -> i32 {
    let o = usize::try_from(o).unwrap();
    b.get(o..o + 4).map_or(0, |x| i32::from_le_bytes(x.try_into().unwrap()))
}

fn u16_at(b: &[u8], o: u64) -> u16 {
    let o = usize::try_from(o).unwrap();
    b.get(o..o + 2).map_or(0, |x| u16::from_le_bytes(x.try_into().unwrap()))
}

fn inline_digest(data: Option<&[u8]>, word: Option<&[u8]>, left: u64) -> String {
    let mut s = Stream::new();
    let mut budget = BlipBudget { left };
    if let Some(data) = data {
        let len = data.len() as u64;
        let (mut o, mut n) = (0u64, 0);
        while o + 0x44 <= len && n < 64 {
            n += 1;
            let lcb = i32_at(data, o);
            let cb_header = u16_at(data, o + 4);
            if lcb < i32::from(cb_header) || cb_header < 0x44 {
                break;
            }
            let lcb = u64::try_from(lcb).unwrap();
            let end = len.min(o + lcb);
            let mut p = o + u64::from(cb_header);
            if u16_at(data, o + 6) == 0x66 {
                p += 1 + u64::from(data.get(usize::try_from(p).unwrap()).copied().unwrap_or(0));
            }
            s.tag(b'P').u64(o);
            for rec in recs_in(data, p, end) {
                if rec.rec_type == rt::SP_CONTAINER {
                    shape_digest(&mut s, &read_sp_container(&rec, Flavor::Word));
                } else if rec.rec_type == rt::FBSE {
                    let e = read_fbse(&rec);
                    bse_digest(&mut s, Some(&e));
                    if let Some(emb) = &e.embedded {
                        blip_digest(&mut s, decode_blip(emb, &mut budget, Flavor::Word, &L).as_ref());
                    } else if e.fo_delay != 0 && e.fo_delay != 0xffff_ffff {
                        blip_digest(&mut s, blip_at(word, e.fo_delay, &mut budget).as_ref());
                    } else {
                        s.tag(b'0');
                    }
                } else if rec.is_blip() {
                    blip_digest(&mut s, decode_blip(&rec, &mut budget, Flavor::Word, &L).as_ref());
                } else {
                    s.tag(b'?').u32(rec.rec_type.into());
                }
            }
            o += lcb;
        }
    }
    s.u64(budget.left);
    summary(s)
}

struct Doc {
    table: Vec<u8>,
    fc: u32,
    lcb: u32,
    word: Option<Vec<u8>>,
    data: Option<Vec<u8>>,
    budget: u64,
}

fn doc_facets(rp: &mut Replay<'_>, key: &str, d: &Doc) {
    let dr = read_word_drawings(&d.table, d.fc, d.lcb, &L);
    rp.check(key, "drawings", &drawings_digest(&dr));
    rp.check(key, "blips", &blips_digest(&dr.bstore, d.word.as_deref(), d.budget));
    rp.check(key, "inline", &inline_digest(d.data.as_deref(), d.word.as_deref(), d.budget));
}

fn sha16(b: Option<&[u8]>) -> String {
    b.map_or_else(|| "-".to_owned(), |b| Digest::of(b).hex()[..16].to_owned())
}

fn num<T: std::str::FromStr>(cap: &Capture, key: &str, facet: &str) -> T
where
    T::Err: std::fmt::Debug,
{
    cap.get(key, facet).unwrap_or_else(|| panic!("{key} {facet}")).parse().unwrap()
}

fn fixture_and_generated(cap: &Capture, rp: &mut Replay<'_>) -> BTreeMap<String, Doc> {
    let root = uo_testkit::repo_root(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("write/packages/docx-core/test/fixtures/doc");
    let mut bases = BTreeMap::new();
    for r in cap.records() {
        let key = r.key.as_str();
        if let Some(name) = key.strip_prefix("doc/") {
            let bytes = std::fs::read(dir.join(format!("{name}.doc"))).unwrap();
            let cfb = uo_opc::cfb::CfbFile::open(&bytes).unwrap();
            let src = r.get("src").unwrap();
            let mut it = src.split(':');
            let (tname, fc, lcb) = (it.next().unwrap(), it.next().unwrap().parse().unwrap(), it.next().unwrap().parse().unwrap());
            rp.skip(key, "src");
            let table = cfb.stream(tname).unwrap().unwrap();
            let word = cfb.stream("WordDocument").unwrap();
            let data = cfb.stream("Data").unwrap();
            let streams = format!("{}:{}:{}", sha16(Some(&table)), sha16(word.as_deref()), sha16(data.as_deref()));
            rp.check(key, "streams", &streams);
            let d = Doc { table, fc, lcb, word, data, budget: DOC_BUDGET };
            doc_facets(rp, key, &d);
            bases.insert(key.to_owned(), d);
        } else if key.starts_with("gen/") {
            for f in ["hex", "fc", "lcb", "word", "data", "budget"] {
                rp.skip(key, f);
            }
            let data = r.get("data").unwrap();
            let d = Doc {
                table: unhex(r.get("hex").unwrap()),
                fc: num(cap, key, "fc"),
                lcb: num(cap, key, "lcb"),
                word: Some(unhex(r.get("word").unwrap())),
                data: (data != "-").then(|| unhex(data)),
                budget: num(cap, key, "budget"),
            };
            doc_facets(rp, key, &d);
            bases.insert(key.to_owned(), d);
        }
    }
    bases
}

fn mutations(cap: &Capture, rp: &mut Replay<'_>, bases: &BTreeMap<String, Doc>) -> usize {
    let mut n = 0;
    for r in cap.records() {
        let Some(rest) = r.key.strip_prefix("mut/") else { continue };
        let base_key = rest.rsplit_once('/').unwrap().0;
        let b = &bases[base_key];
        let on = r.get("on").unwrap();
        let mut table = b.table.clone();
        let mut word = b.word.clone();
        let mut data = b.data.clone();
        let (target, lo) = match on {
            "t" => (&mut table, u64::from(b.fc)),
            "w" => (word.as_mut().unwrap(), 0),
            _ => (data.as_mut().unwrap(), 0),
        };
        for e in r.get("edits").unwrap().split(',') {
            let (pos, val) = e.split_once(':').unwrap();
            let at = usize::try_from(lo + pos.parse::<u64>().unwrap()).unwrap();
            // a typed-array store past the end is ignored in JS
            if let Some(x) = target.get_mut(at) {
                *x = val.parse().unwrap();
            }
        }
        for f in ["on", "edits", "cut"] {
            rp.skip(&r.key, f);
        }
        let d = Doc { table, fc: b.fc, lcb: num(cap, &r.key, "cut"), word, data, budget: b.budget };
        doc_facets(rp, &r.key, &d);
        n += 1;
    }
    n
}

fn table_digest(t: &PropTable<'_>) -> String {
    let mut s = Stream::new();
    s.u32(u32::try_from(t.len()).unwrap());
    for p in t.iter() {
        s.u32(p.id.into()).u32(p.id.into()).u32(p.value).bool(p.bid).bool(p.complex.is_some());
        if let Some(c) = p.complex {
            s.digest_of(c);
        }
    }
    summary(s)
}

fn fopt_blocks(cap: &Capture, rp: &mut Replay<'_>) -> usize {
    let mut n = 0;
    for r in cap.records().iter().filter(|r| r.key.starts_with("fopt/")) {
        let block = unhex(r.get("hex").unwrap());
        rp.skip(&r.key, "hex");
        let read = |flavor| {
            let mut t = PropTable::new();
            for c in recs_in(&block, 0, block.len() as u64) {
                if matches!(c.rec_type, rt::FOPT | rt::SECONDARY_FOPT | rt::TERTIARY_FOPT) {
                    read_fopt(&c, &mut t, flavor);
                }
            }
            t
        };
        rp.check(&r.key, "word", &table_digest(&read(Flavor::Word)));
        rp.check(&r.key, "slides", &table_digest(&read(Flavor::Slides)));
        let x = read(Flavor::Sheet);
        let mut s = Stream::new();
        s.u32(u32::try_from(x.len()).unwrap());
        for p in x.iter() {
            s.u32(p.id.into()).u32(p.value);
        }
        let complex: Vec<_> = x.iter_complex().collect();
        s.u32(u32::try_from(complex.len()).unwrap());
        for p in complex {
            s.u32(p.id.into()).digest_of(p.complex.unwrap_or(&[]));
        }
        rp.check(&r.key, "sheet", &summary(s));
        n += 1;
    }
    n
}

fn blips(cap: &Capture, rp: &mut Replay<'_>) -> usize {
    let mut n = 0;
    for r in cap.records().iter().filter(|r| r.key.starts_with("blip/")) {
        let b = unhex(r.get("hex").unwrap());
        let left: u64 = num(cap, &r.key, "budget");
        rp.skip(&r.key, "hex");
        rp.skip(&r.key, "budget");
        let rec = read_rec(&b, 0, b.len() as u64).unwrap();
        let one = |flavor| {
            let mut s = Stream::new();
            let mut budget = BlipBudget { left };
            blip_digest(&mut s, decode_blip(&rec, &mut budget, flavor, &L).as_ref());
            s.u64(budget.left);
            summary(s)
        };
        rp.check(&r.key, "word", &one(Flavor::Word));
        if r.get("slides").is_some() {
            rp.check(&r.key, "slides", &one(Flavor::Slides));
        }
        let mut s = Stream::new();
        let mut budget = BlipBudget::document();
        let sheet = if rec.is_blip() { decode_blip(&rec, &mut budget, Flavor::Sheet, &OdrawLimits::SHEET) } else { None };
        blip_digest(&mut s, sheet.as_ref());
        rp.check(&r.key, "sheet", &summary(s));
        n += 1;
    }
    n
}

fn preset_digest(s: &mut Stream, r: Option<&PresetResult>) {
    let Some(r) = r else {
        s.tag(b'n');
        return;
    };
    let av: Vec<_> = r.av().collect();
    s.tag(b'r').str(r.prst).bool(r.flip_v).u32(u32::try_from(av.len()).unwrap());
    for a in av {
        s.str(a.name).f64(canonicalize_nan(a.val));
    }
}

fn sheet_xml(r: &PresetResult) -> String {
    let av: String = r.av().map(|a| format!("<a:gd name=\"{}\" fmla=\"val {}\"/>", a.name, Js(a.val))).collect();
    format!("<a:prstGeom prst=\"{}\"><a:avLst>{av}</a:avLst></a:prstGeom>", r.prst)
}

fn presets(cap: &Capture, rp: &mut Replay<'_>) -> usize {
    let bits = |key: &str, f: &str| f64::from_bits(u64::from_str_radix(cap.get(key, f).unwrap(), 16).unwrap());
    let mut cases = Vec::new();
    for r in cap.records().iter().filter(|r| r.key.starts_with("case/")) {
        let adj: Vec<Option<f64>> =
            r.get("adj").unwrap().split(',').map(|v| (!v.is_empty()).then(|| f64::from(v.parse::<i32>().unwrap()))).collect();
        cases.push((adj, bits(&r.key, "w"), bits(&r.key, "h")));
        for f in ["adj", "w", "h"] {
            rp.skip(&r.key, f);
        }
    }
    let mut n = 0;
    for r in cap.records().iter().filter(|r| r.key.starts_with("msospt/")) {
        let spt: u16 = r.key["msospt/".len()..].parse().unwrap();
        let (mut ws, mut xs) = (Stream::new(), Stream::new());
        for (adj, w, h) in &cases {
            preset_digest(&mut ws, preset_for(spt, adj, *w, *h, PresetVariant::WordSlides).as_ref());
            match preset_for(spt, adj, *w, *h, PresetVariant::Sheet) {
                Some(x) => {
                    xs.tag(b'x').str(&sheet_xml(&x)).bool(x.flip_v);
                }
                None => {
                    xs.tag(b'n');
                }
            }
        }
        let word = summary(ws);
        rp.check(&r.key, "word", &word);
        rp.check(&r.key, "slides", &word);
        rp.check(&r.key, "sheet", &summary(xs));
        n += 1;
    }
    n
}

#[test]
fn odraw_parity() {
    let cap = Capture::parse(CAPTURE).unwrap();
    let mut rp = Replay::new(&cap);
    let bases = fixture_and_generated(&cap, &mut rp);
    let muts = mutations(&cap, &mut rp, &bases);
    let fopts = fopt_blocks(&cap, &mut rp);
    let blip_n = blips(&cap, &mut rp);
    let preset_n = presets(&cap, &mut rp);
    assert!(
        bases.len() >= 111 && muts > 400 && fopts >= 250 && blip_n >= 250 && preset_n == 257,
        "{} {muts} {fopts} {blip_n} {preset_n}",
        bases.len()
    );
    let report = rp.finish();
    eprintln!("odraw parity: {}", report.summary());
    report.assert_clean();
    assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/capture-odraw.mjs").exists());
}
