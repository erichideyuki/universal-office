//! The part of the QuickDraw PICT reader (`pict.ts`) the converter uses: `fitPictFrame`, which
//! shortens the picture frame of a PICT whose recorded size has another aspect ratio than its
//! `picFrame` (the player is the renderer's, not the converter's).

use std::borrow::Cow;

use uo_jsnum::math::round;

/// Offset of the PICT start: 0, or 512 with the Mac file header; `None` when it is no PICT.
fn pict_start(b: &[u8]) -> Option<usize> {
    if b.len() < 16 {
        return None;
    }
    let at = |i: usize| b.get(i).copied().unwrap_or(0);
    // the PICT has no signature: formats that have one are refused first
    let sig = (u32::from(at(0)) << 24) | (u32::from(at(1)) << 16) | (u32::from(at(2)) << 8) | u32::from(at(3));
    if (sig >> 8) == 0xff_d8ff
        || sig == 0x8950_4e47
        || sig == 0x4749_4638
        || (sig >> 16) == 0x424d
        || sig == 0x4949_2a00
        || sig == 0x4d4d_002a
        || sig == 0x0100_0000
        || sig == 0xd7cd_c69a
        || sig == 0x0100_0900
        || sig == 0x0200_0900
        || sig == 0x5249_4646
        || (sig >> 24) == 0x3c
    {
        return None;
    }
    let i16at = |o: usize| i16::from_be_bytes([at(o), at(o + 1)]);
    for o in [0usize, 512] {
        if b.len() < o + 16 {
            continue;
        }
        let (t, l, bt, r) = (i16at(o + 2), i16at(o + 4), i16at(o + 6), i16at(o + 8));
        if bt <= t || r <= l {
            continue;
        }
        if at(o + 10) == 0x00 && at(o + 11) == 0x11 && at(o + 12) == 0x02 && at(o + 13) == 0xff && at(o + 14) == 0x0c && at(o + 15) == 0x00
        {
            return Some(o);
        }
        if at(o + 10) == 0x11 && at(o + 11) == 0x01 {
            return Some(o);
        }
    }
    None
}

#[derive(Clone, Copy)]
struct Frame {
    top: i32,
    left: i32,
    bottom: i32,
    right: i32,
}

struct Frames {
    frame: Frame,
    /// the source rectangle of the extended header, when it differs from the frame
    src: Option<Frame>,
}

fn pict_frames(b: &[u8]) -> Option<Frames> {
    let s = pict_start(b)?;
    let i16at = |o: usize| -> i32 {
        match (b.get(o), b.get(o + 1)) {
            (Some(&h), Some(&l)) => i32::from(i16::from_be_bytes([h, l])),
            // `DataView.getInt16` out of range throws: the TS has this inside `try` of the caller only
            // for the player; `fitPictFrame` would throw and the shape would be skipped. A PICT
            // whose header passed `pict_start` has 16 bytes at `s`, enough for the frame.
            _ => 0,
        }
    };
    let rect = |o: usize| Frame { top: i16at(o), left: i16at(o + 2), bottom: i16at(o + 4), right: i16at(o + 6) };
    let frame = rect(s + 2);
    if b.get(s + 10) == Some(&0x11) {
        return Some(Frames { frame, src: None });
    }
    let body = s + 14;
    let mut src = None;
    let u16at = |o: usize| u32::from(u16::from_be_bytes([b.get(o).copied().unwrap_or(0), b.get(o + 1).copied().unwrap_or(0)]));
    if u16at(body) == 0x0c00 && body + 26 <= b.len() && i16at(body + 2) == -2 {
        let r = rect(body + 2 + 16);
        if r.right > r.left && r.bottom > r.top {
            src = Some(r);
        }
    }
    Some(Frames { frame, src })
}

/// A PICT of a `.ppt` BLIP whose recorded size (`ptSize` of the header, EMU) has another aspect
/// ratio than its `picFrame`: the application draws it with the HORIZONTAL scale on both axes,
/// anchored at the top, and cuts what passes the height. Returns a copy with the frame (and the
/// source rectangle of the v2 header) shortened (`fitPictFrame`).
#[must_use]
pub fn fit_pict_frame(b: Cow<'_, [u8]>, pt_w: i32, pt_h: i32) -> Cow<'_, [u8]> {
    let Some(fr) = pict_frames(&b) else { return b };
    if pt_w <= 0 || pt_h <= 0 {
        return b;
    }
    let fw = f64::from(fr.frame.right) - f64::from(fr.frame.left);
    let fh = f64::from(fr.frame.bottom) - f64::from(fr.frame.top);
    if fw <= 0.0 || fh <= 0.0 {
        return b;
    }
    let want = (fw * f64::from(pt_h)) / f64::from(pt_w);
    let ratio = want / fh - 1.0;
    if ratio.abs() < 0.01 {
        return b;
    }
    let Some(s) = pict_start(&b) else { return b };
    let mut out = b.into_owned();
    let new_bottom = round(f64::from(fr.frame.top) + want);
    set_i16(&mut out, s + 6, new_bottom);
    if let Some(src) = fr.src {
        let sw = f64::from(src.right) - f64::from(src.left);
        let v = round(f64::from(src.top) + (sw * f64::from(pt_h)) / f64::from(pt_w));
        set_i16(&mut out, s + 14 + 2 + 16 + 4, v);
    }
    Cow::Owned(out)
}

/// `DataView.setInt16(o, v)` (big-endian, the value taken modulo 2^16); out of range is a no-op
/// (the TS would throw; the offsets are inside the header that was read).
fn set_i16(b: &mut [u8], o: usize, v: f64) {
    let x = uo_jsnum::conv::to_int16(v).to_be_bytes();
    if let Some(slot) = b.get_mut(o..o + 2) {
        slot.copy_from_slice(&x);
    }
}
