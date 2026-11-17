//! A small reader of the PDFs the Slides painter writes, for the tests: the streams of every object inflated, the
//! page boxes, and the text the pages draw read back through each font's ToUnicode CMap (text extraction).
#![allow(dead_code, clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use uo_opc::flate::inflate_zlib;

#[path = "../../../uo-pdf/tests/support/pdf_parse.rs"]
pub mod pdf_parse;

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// `(dictionary text, payload)` of an object body: the payload inflated when the dictionary says so.
pub fn split(body: &[u8]) -> (String, Option<Vec<u8>>) {
    let Some(at) = find(body, b"\nstream\n") else {
        return (body.iter().map(|&c| char::from(c)).collect(), None);
    };
    let dict: String = body[..at].iter().map(|&c| char::from(c)).collect();
    let start = at + 8;
    let end = body.len().checked_sub(10).filter(|e| *e >= start).unwrap_or(start);
    let payload = &body[start..end];
    let data = if dict.contains("/FlateDecode") { inflate_zlib(payload, None, 1 << 31).unwrap() } else { payload.to_vec() };
    (dict, Some(data))
}

pub struct View<'a> {
    pub objs: Vec<&'a [u8]>,
    pub root: u32,
}

pub fn view(bytes: &[u8]) -> Result<View<'_>, String> {
    let p = pdf_parse::parse_pdf(bytes)?;
    Ok(View { objs: p.objs, root: p.root })
}

impl View<'_> {
    pub fn dict_of(&self, n: u32) -> String {
        split(self.objs[n as usize - 1]).0
    }

    /// `(media box w, h)` of every page, in order.
    pub fn pages(&self) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        for body in &self.objs {
            let (d, _) = split(body);
            if d.starts_with("<< /Type /Page ") {
                let at = d.find("/MediaBox [").unwrap() + 11;
                let end = d[at..].find(']').unwrap() + at;
                let v: Vec<f64> = d[at..end].split(' ').map(uo_jsnum::parse::to_number).collect();
                out.push((v[2], v[3]));
            }
        }
        out
    }

    /// The `/Count` of the pages node.
    pub fn page_count(&self) -> usize {
        for body in &self.objs {
            let d = split(body).0;
            if d.starts_with("<< /Type /Pages ") {
                let at = d.find("/Count ").unwrap() + 7;
                let end = d[at..].find(' ').unwrap() + at;
                return d[at..end].parse().unwrap();
            }
        }
        0
    }

    /// The content streams that are not fonts, images or shadings: page contents, forms and patterns.
    pub fn content_streams(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for body in &self.objs {
            let (d, data) = split(body);
            let Some(data) = data else { continue };
            if find(&data, b"begincmap").is_some() {
                continue;
            }
            let is_other = d.contains("/Subtype /Image") || d.contains("/FunctionType") || d.contains("/Length1") || d.contains("/CIDInit");
            if is_other || d.contains("/Subtype /CIDFontType2") {
                continue;
            }
            if d.contains("/Type /XObject /Subtype /Form") || d.contains("/PatternType") || !d.contains("/Type") {
                out.push((d, String::from_utf8_lossy(&data).into_owned()));
            }
        }
        out
    }

    /// `font index -> (gid -> text)` from the ToUnicode CMaps of the Type0 fonts, by the order of `/F<n>`.
    fn to_unicode(&self) -> std::collections::BTreeMap<u32, std::collections::BTreeMap<u32, String>> {
        let mut fonts = std::collections::BTreeMap::new();
        // the page resources name `/F<n> <obj> 0 R`
        let mut res = String::new();
        for body in &self.objs {
            let d = split(body).0;
            if d.starts_with("<< /Type /Page ") {
                res = d;
                break;
            }
        }
        let mut rest = res.as_str();
        while let Some(at) = rest.find("/F") {
            let tail = &rest[at + 2..];
            let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
            let after = &tail[digits.len()..];
            if !digits.is_empty() && after.starts_with(' ') {
                let obj_digits: String = after[1..].chars().take_while(char::is_ascii_digit).collect();
                if let (Ok(idx), Ok(obj)) = (digits.parse::<u32>(), obj_digits.parse::<u32>())
                    && after[1 + obj_digits.len()..].starts_with(" 0 R")
                {
                    let d = self.dict_of(obj);
                    if let Some(p) = d.find("/ToUnicode ") {
                        let n: String = d[p + 11..].chars().take_while(char::is_ascii_digit).collect();
                        let n: u32 = n.parse().unwrap();
                        let cmap = String::from_utf8_lossy(&split(self.objs[n as usize - 1]).1.unwrap()).into_owned();
                        let mut map = std::collections::BTreeMap::new();
                        for line in cmap.lines() {
                            let parts: Vec<&str> = line.split(' ').collect();
                            if parts.len() == 2 && parts[0].starts_with('<') && parts[1].starts_with('<') {
                                let gid = u32::from_str_radix(parts[0].trim_matches(|c| c == '<' || c == '>'), 16).unwrap();
                                let hex = parts[1].trim_matches(|c| c == '<' || c == '>');
                                let units: Vec<u16> =
                                    (0..hex.len() / 4).map(|i| u16::from_str_radix(&hex[i * 4..i * 4 + 4], 16).unwrap()).collect();
                                map.insert(gid, String::from_utf16_lossy(&units));
                            }
                        }
                        fonts.insert(idx, map);
                    }
                }
            }
            rest = &rest[at + 2..];
        }
        fonts
    }

    /// The text of every `TJ` of every content stream, in stream order, through the ToUnicode maps; the number of
    /// glyphs that no map knew.
    pub fn text(&self) -> (String, usize) {
        let maps = self.to_unicode();
        let mut out = String::new();
        let mut unknown = 0;
        for (_, content) in self.content_streams() {
            let mut font_now: Option<u32> = None;
            for line in content.split('\n') {
                if let Some(tf) = line.strip_suffix(" Tf")
                    && let Some(name) = tf.split(' ').next().and_then(|s| s.strip_prefix("/F"))
                {
                    font_now = name.parse().ok();
                }
                if line.ends_with("TJ") {
                    let mut rest = line;
                    while let Some(a) = rest.find('<') {
                        let Some(b) = rest[a..].find('>') else { break };
                        let hex = &rest[a + 1..a + b];
                        for k in 0..hex.len() / 4 {
                            let gid = u32::from_str_radix(&hex[k * 4..k * 4 + 4], 16).unwrap();
                            match font_now.and_then(|f| maps.get(&f)).and_then(|m| m.get(&gid)) {
                                Some(s) => out.push_str(s),
                                None => unknown += 1,
                            }
                        }
                        rest = &rest[a + b + 1..];
                    }
                }
            }
        }
        (out, unknown)
    }
}
