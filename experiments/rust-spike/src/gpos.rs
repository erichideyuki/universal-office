//! Porta de docx-layout/src/typography/font/gpos.ts: kerning de pares do GPOS (lookup 2,
//! formatos 1 e 2, com extensão 9). Mesma ordem de leitura e a mesma regra "o primeiro par
//! encontrado vale" — por isso as listas que no TS são Set/Map com ordem de inserção aqui
//! são Vec.
use std::rc::Rc;

use crate::fx::{FxMap, FxSet};
use crate::read::{R, View};

/// kernKey(g1, g2) → ajuste de xAdvance (unidades da fonte).
pub type KernMap = FxMap<u32, i32>;

/// PairPos formato 2: ajuste do 1º glifo contra a classe 0 do ClassDef2 (os glifos fora dele).
pub struct Class0 {
    pub adv: i32,
    pub cd2: Rc<FxMap<u32, u32>>,
}

pub type KernClass0 = FxMap<u32, Class0>;

#[inline]
pub fn kern_key(g1: u32, g2: u32) -> u32 {
    g1 * 65536 + g2
}

/// A feature 'kern' está no script latino do GPOS (latn; sem ele, DFLT)?
pub fn gpos_kern_in_latin(v: View, gpos: usize) -> R<bool> {
    let script_list = gpos + v.u16(gpos + 4)? as usize;
    let feature_list = gpos + v.u16(gpos + 6)? as usize;
    let (mut latn, mut dflt): (Option<usize>, Option<usize>) = (None, None);
    let n = v.u16(script_list)? as usize;
    for i in 0..n {
        let rec = script_list + 2 + i * 6;
        let tag = v.tag(rec)?;
        if &tag == b"latn" {
            latn = Some(script_list + v.u16(rec + 4)? as usize);
        } else if &tag == b"DFLT" {
            dflt = Some(script_list + v.u16(rec + 4)? as usize);
        }
    }
    let Some(script) = latn.or(dflt) else { return Ok(false) };
    let default_lang = v.u16(script)? as usize;
    if default_lang == 0 {
        return Ok(false);
    }
    let ls = script + default_lang;
    let is_kern = |fi: u16| -> R<bool> { Ok(fi != 0xffff && &v.tag(feature_list + 2 + fi as usize * 6)? == b"kern") };
    if is_kern(v.u16(ls + 2)?)? {
        return Ok(true);
    }
    let n = v.u16(ls + 4)? as usize;
    for k in 0..n {
        if is_kern(v.u16(ls + 6 + k * 2)?)? {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn parse_gpos_kern(v: View, gpos: usize, class0: &mut KernClass0) -> R<KernMap> {
    let mut kern = KernMap::default();
    let feature_list = gpos + v.u16(gpos + 6)? as usize;
    let lookup_list = gpos + v.u16(gpos + 8)? as usize;

    let feature_count = v.u16(feature_list)? as usize;
    let mut kern_features = Vec::new();
    for i in 0..feature_count {
        if &v.tag(feature_list + 2 + i * 6)? == b"kern" {
            kern_features.push(i);
        }
    }
    if kern_features.is_empty() {
        return Ok(kern);
    }

    let mut lookups = Vec::new();
    let mut seen = FxSet::default();
    for &fi in &kern_features {
        let feat = feature_list + v.u16(feature_list + 2 + fi * 6 + 4)? as usize;
        let n = v.u16(feat + 2)? as usize;
        for k in 0..n {
            let li = v.u16(feat + 4 + k * 2)?;
            if seen.insert(li) {
                lookups.push(li);
            }
        }
    }

    let lookup_count = v.u16(lookup_list)?;
    for li in lookups {
        if li >= lookup_count {
            continue;
        }
        let lookup = lookup_list + v.u16(lookup_list + 2 + li as usize * 2)? as usize;
        let ty = v.u16(lookup)?;
        let sub_count = v.u16(lookup + 4)? as usize;
        for s in 0..sub_count {
            let mut sub = lookup + v.u16(lookup + 6 + s * 2)? as usize;
            let mut t = ty;
            if t == 9 {
                t = v.u16(sub + 2)?;
                sub += v.u32(sub + 4)? as usize;
            }
            if t != 2 {
                continue;
            }
            parse_pair_pos(v, sub, &mut kern, class0)?;
        }
    }
    Ok(kern)
}

fn read_coverage(v: View, off: usize) -> R<Vec<u32>> {
    let fmt = v.u16(off)?;
    let mut out = Vec::new();
    if fmt == 1 {
        let n = v.u16(off + 2)? as usize;
        out.reserve(n);
        for i in 0..n {
            out.push(v.u16(off + 4 + i * 2)? as u32);
        }
    } else if fmt == 2 {
        let n = v.u16(off + 2)? as usize;
        for r in 0..n {
            let start = v.u16(off + 4 + r * 6)? as u32;
            let end = v.u16(off + 6 + r * 6)? as u32;
            for g in start..=end {
                out.push(g);
            }
        }
    }
    Ok(out)
}

fn read_class_def(v: View, off: usize) -> R<FxMap<u32, u32>> {
    let mut map = FxMap::default();
    let fmt = v.u16(off)?;
    if fmt == 1 {
        let start = v.u16(off + 2)? as u32;
        let n = v.u16(off + 4)? as usize;
        for i in 0..n {
            map.insert(start + i as u32, v.u16(off + 6 + i * 2)? as u32);
        }
    } else if fmt == 2 {
        let n = v.u16(off + 2)? as usize;
        for r in 0..n {
            let s = v.u16(off + 4 + r * 6)? as u32;
            let e = v.u16(off + 6 + r * 6)? as u32;
            let c = v.u16(off + 8 + r * 6)? as u32;
            for g in s..=e {
                map.insert(g, c);
            }
        }
    }
    Ok(map)
}

#[inline]
fn value_size(format: u16) -> usize {
    format.count_ones() as usize * 2
}

/// xAdvance de um ValueRecord (0x0004 = XAdvance).
#[inline]
fn read_x_advance(v: View, off: usize, format: u16) -> R<i32> {
    let mut p = off;
    if format & 0x0001 != 0 {
        p += 2;
    }
    if format & 0x0002 != 0 {
        p += 2;
    }
    if format & 0x0004 != 0 {
        return Ok(v.i16(p)? as i32);
    }
    Ok(0)
}

fn parse_pair_pos(v: View, off: usize, kern: &mut KernMap, class0: &mut KernClass0) -> R<()> {
    let fmt = v.u16(off)?;
    let coverage = read_coverage(v, off + v.u16(off + 2)? as usize)?;
    let vf1 = v.u16(off + 4)?;
    let vf2 = v.u16(off + 6)?;
    let (size1, size2) = (value_size(vf1), value_size(vf2));
    if fmt == 1 {
        let pair_set_count = v.u16(off + 8)? as usize;
        let rec_size = 2 + size1 + size2;
        for i in 0..pair_set_count.min(coverage.len()) {
            let ps = off + v.u16(off + 10 + i * 2)? as usize;
            let n = v.u16(ps)? as usize;
            for r in 0..n {
                let rec = ps + 2 + r * rec_size;
                let g2 = v.u16(rec)? as u32;
                let adv = read_x_advance(v, rec + 2, vf1)?;
                if adv != 0 {
                    kern.entry(kern_key(coverage[i], g2)).or_insert(adv);
                }
            }
        }
    } else if fmt == 2 {
        let cd1 = read_class_def(v, off + v.u16(off + 8)? as usize)?;
        let cd2 = Rc::new(read_class_def(v, off + v.u16(off + 10)? as usize)?);
        let c1n = v.u16(off + 12)? as usize;
        let c2n = v.u16(off + 14)? as usize;
        let rec_size = size1 + size2;
        let mut class_adv = Vec::with_capacity(c1n * c2n);
        for c1 in 0..c1n {
            for c2 in 0..c2n {
                class_adv.push(read_x_advance(v, off + 16 + (c1 * c2n + c2) * rec_size, vf1)?);
            }
        }
        let adv_of = |c1: u32, c2: u32| -> i32 {
            let (c1, c2) = (c1 as usize, c2 as usize);
            if c1 < c1n && c2 < c2n { class_adv[c1 * c2n + c2] } else { 0 }
        };
        // classe de cada glifo coberto, na ordem da cobertura (o Map do TS: 1ª posição, último valor)
        let mut class1: Vec<(u32, u32)> = Vec::with_capacity(coverage.len());
        let mut pos = FxMap::default();
        for &g in &coverage {
            let c = cd1.get(&g).copied().unwrap_or(0);
            match pos.get(&g) {
                Some(&i) => class1[i] = (g, c),
                None => {
                    pos.insert(g, class1.len());
                    class1.push((g, c));
                }
            }
        }
        for &(g1, c1) in &class1 {
            let adv = adv_of(c1, 0);
            if adv != 0 && !class0.contains_key(&g1) {
                class0.insert(g1, Class0 { adv, cd2: Rc::clone(&cd2) });
            }
        }
        for &(g1, c1) in &class1 {
            for (&g2, &c2) in cd2.iter() {
                let adv = adv_of(c1, c2);
                if adv != 0 {
                    kern.entry(kern_key(g1, g2)).or_insert(adv);
                }
            }
        }
    }
    Ok(())
}
