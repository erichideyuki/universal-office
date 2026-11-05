//! The `odraw_records` target of `uo_drawingml::odraw` (OfficeArt of the binary formats, record
//! `core/docs/modules/uo-drawingml.md`, "odraw — OfficeArt"): one buffer read as Word's
//! `OfficeArtContent`, every BLIP of its store decoded, and every record of the buffer (walked
//! into containers) read as a FOPT, a shape, a group, a BLIP store entry and a BLIP under the
//! three flavors, and the shape types through the preset map.
//!
//! Input: a selector byte (bit 0: small limits; bits 1-2: the budget; bits 3-7: the box of the
//! preset map), then the stream.

use std::collections::HashSet;
use std::hint::black_box;
use std::path::Path;

use uo_drawingml::odraw::msospt::{PresetVariant, preset_for, raw_adjust};
use uo_drawingml::odraw::{
    BlipBudget, Flavor, OdrawLimits, PropTable, Rec, Shape, decode_blip, read_bstore, read_fbse, read_fopt, read_sp_container,
    read_spgr_container, read_word_drawings, recs_in,
};

/// Small limits: every limit is reached by small inputs.
pub const ODRAW_LIMITS: OdrawLimits =
    OdrawLimits { max_blip_bytes: 1 << 16, max_group_depth: 6, max_word_drawings: 4, max_shapes: 200, max_master_props: 500 };

const FLAVORS: [Flavor; 3] = [Flavor::Word, Flavor::Slides, Flavor::Sheet];

fn count(s: &Shape<'_>) -> u64 {
    1 + s.children.iter().flatten().map(count).sum::<u64>()
}

/// Every record reachable from `[start, end)`, into containers down to `depth`, at most `left`.
fn walk<'a>(b: &'a [u8], start: u64, end: u64, depth: u32, left: &mut u32, out: &mut Vec<Rec<'a>>) {
    for r in recs_in(b, start, end) {
        if *left == 0 {
            return;
        }
        *left -= 1;
        out.push(r);
        if r.is_container() && depth > 0 {
            walk(b, r.start, r.end, depth - 1, left, out);
        }
    }
}

pub fn odraw_records(data: &[u8]) {
    let (sel, b) = match data.split_first() {
        Some((&s, rest)) => (s, rest),
        None => (0, &[][..]),
    };
    let limits = if sel & 1 == 1 { ODRAW_LIMITS } else { OdrawLimits::WORD };
    let left0 = [0, 300, 1 << 16, 512 << 20][usize::from((sel >> 1) & 3)];
    let wh = [1.0, 0.0, f64::NAN, 1037.0, -5.0, 1e300, 0.5, 21600.0][usize::from((sel >> 3) & 7)];
    let len = u32::try_from(b.len()).unwrap_or(u32::MAX);

    let d = read_word_drawings(b, 0, len, &limits);
    let shapes: u64 = d.main.iter().chain(d.hdr.iter()).map(count).sum();
    assert!(shapes <= u64::from(limits.max_shapes), "{shapes} shapes over the limit");
    assert_eq!(d, read_word_drawings(b, 0, len, &limits), "not deterministic");
    let mut budget = BlipBudget { left: left0 };
    for e in d.bstore.iter().flatten() {
        if let Some(r) = &e.embedded {
            let before = budget.left;
            if let Some(blip) = decode_blip(r, &mut budget, Flavor::Word, &limits) {
                assert!(before - budget.left <= limits.max_blip_bytes);
                black_box(blip.kind.ext());
            } else {
                assert_eq!(before, budget.left, "a refused BLIP must not spend the budget");
            }
        }
    }
    for s in d.main.iter().chain(d.hdr.iter()) {
        let adj = raw_adjust(&s.props);
        for v in [PresetVariant::WordSlides, PresetVariant::Sheet] {
            if let Some(r) = preset_for(s.shape_type, &adj, wh, f64::from(s.spid), v) {
                assert!(r.av().count() <= 2);
            }
        }
        assert!(d.shape(s.spid, false).is_some());
    }
    black_box(d.background_shape());

    let mut recs = Vec::new();
    walk(b, 0, u64::from(len), 4, &mut 256, &mut recs);
    let mut seen = HashSet::new();
    for r in recs {
        if !seen.insert((r.hdr, r.end)) {
            continue;
        }
        for flavor in FLAVORS {
            let mut t = PropTable::new();
            read_fopt(&r, &mut t, flavor);
            assert!(t.len() <= usize::from(r.inst));
            assert!(t.iter_complex().count() <= usize::from(r.inst));
            black_box(read_sp_container(&r, flavor));
            let g = read_spgr_container(&r, flavor, &limits);
            assert!(count(&g) <= u64::from(limits.max_shapes) + 1);
            let mut bud = BlipBudget { left: left0 };
            if let Some(blip) = decode_blip(&r, &mut bud, flavor, &limits) {
                assert!(left0 - bud.left <= limits.max_blip_bytes);
                black_box(blip.bytes.len());
            }
        }
        let e = read_fbse(&r);
        if let Some(emb) = e.embedded {
            assert!(emb.is_blip() && emb.end <= r.end);
        }
        black_box(read_bstore(&r));
    }
}

/// Seeds: the OfficeArt content of the `.doc` fixtures (at `fcDggInfo`, from the parity capture's
/// `src` facets) and the generated contents, BLIPs and FOPT blocks of the capture.
pub fn odraw_seeds(repo: &Path) -> Vec<(&'static str, Vec<u8>)> {
    let mut out = Vec::new();
    let Ok(cap) = std::fs::read_to_string(repo.join("core/crates/uo-drawingml/tests/captures/odraw.capture")) else { return out };
    let unhex = |h: &str| -> Vec<u8> {
        let v = |c: u8| match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => 0,
        };
        h.as_bytes().chunks(2).map(|p| (v(p[0]) << 4) | v(p.get(1).copied().unwrap_or(b'0'))).collect()
    };
    let with_sel = |sel: u8, b: &[u8]| [&[sel][..], b].concat();
    for line in cap.lines() {
        let mut fields = line.split('\t');
        let key = fields.next().unwrap_or("");
        let facet = |name: &str| line.split('\t').find_map(|f| f.strip_prefix(name).and_then(|f| f.strip_prefix('=')));
        if let Some(name) = key.strip_prefix("doc/") {
            let Some(src) = facet("src") else { continue };
            let mut it = src.split(':');
            let (Some(t), Some(fc), Some(lcb)) = (it.next(), it.next(), it.next()) else { continue };
            let (Ok(fc), Ok(lcb)) = (fc.parse::<usize>(), lcb.parse::<usize>()) else { continue };
            let Ok(bytes) = std::fs::read(repo.join(format!("write/packages/docx-core/test/fixtures/doc/{name}.doc"))) else { continue };
            let Ok(cfb) = uo_opc::cfb::CfbFile::open(&bytes) else { continue };
            if let Ok(Some(table)) = cfb.stream(t)
                && let Some(content) = table.get(fc..fc + lcb)
            {
                out.push(("odraw_records", with_sel(0x06, content)));
                out.push(("odraw_records", with_sel(0x19, content)));
            }
        } else if key.starts_with("gen/") {
            let (Some(h), Some(fc)) = (facet("hex"), facet("fc").and_then(|f| f.parse::<usize>().ok())) else { continue };
            let t = unhex(h);
            out.push(("odraw_records", with_sel(0x06, t.get(fc..).unwrap_or(&[]))));
        } else if (key.starts_with("blip/") || key.starts_with("fopt/"))
            && let Some(h) = facet("hex")
        {
            out.push(("odraw_records", with_sel(0x03, &unhex(h))));
        }
    }
    // one deep group chain and one record header claiming 4 GiB
    let mut deep = Vec::new();
    for i in 0..40u32 {
        deep.extend_from_slice(&0x000fu16.to_le_bytes());
        deep.extend_from_slice(&0xf003u16.to_le_bytes());
        deep.extend_from_slice(&((39 - i) * 8).to_le_bytes());
    }
    out.push(("odraw_records", with_sel(1, &deep)));
    out.push(("odraw_records", with_sel(0, &[0x0f, 0, 0, 0xf0, 0xff, 0xff, 0xff, 0xff])));
    out
}
