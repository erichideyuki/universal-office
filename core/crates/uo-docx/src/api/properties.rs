//! C# `DocxProperties`: `docProps/core.xml` (title, author…), `docProps/app.xml` (company,
//! manager, application) and `docProps/custom.xml`. Missing parts are created on the first
//! write; an empty or absent value removes the element. Dates are UTC (the facade converts a
//! local `DateTime` before calling, as the C# `ToUniversalTime` does).

use super::document::{DocxDocument, relative_target};
use super::dom::{Declaration, NS_XMLNS, Name, NodeId, n};
use super::error::Result;
use super::net;
use super::ooxml::{content_types, rel_types, remove_invalid_chars};

/// A UTC date and time (C# `DateTime` of kind Utc), to the tick (100 ns).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct UtcDateTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// fraction of the second in ticks (0..10,000,000)
    pub ticks: u32,
}

impl UtcDateTime {
    /// `ToString("o")` of a UTC `DateTime`: `yyyy-MM-ddTHH:mm:ss.fffffffZ`.
    pub fn round_trip(&self) -> String {
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:07}Z",
            self.year, self.month, self.day, self.hour, self.minute, self.second, self.ticks
        )
    }

    /// `yyyy-MM-dd'T'HH:mm:ss'Z'` (what the setter writes).
    pub fn w3cdtf(&self) -> String {
        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", self.year, self.month, self.day, self.hour, self.minute, self.second)
    }
}

/// The document properties of a document (C# `doc.Properties`).
pub struct Properties<'d> {
    doc: &'d mut DocxDocument,
}

impl DocxDocument {
    pub fn properties(&mut self) -> Properties<'_> {
        Properties { doc: self }
    }
}

const VT: &str = "http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes";

impl Properties<'_> {
    pub fn title(&mut self) -> Result<Option<String>> {
        self.core(n::dc_title)
    }
    pub fn set_title(&mut self, v: Option<&str>) -> Result<()> {
        self.set_core(n::dc_title, v)
    }
    pub fn subject(&mut self) -> Result<Option<String>> {
        self.core(n::dc_subject)
    }
    pub fn set_subject(&mut self, v: Option<&str>) -> Result<()> {
        self.set_core(n::dc_subject, v)
    }
    pub fn author(&mut self) -> Result<Option<String>> {
        self.core(n::dc_creator)
    }
    pub fn set_author(&mut self, v: Option<&str>) -> Result<()> {
        self.set_core(n::dc_creator, v)
    }
    pub fn keywords(&mut self) -> Result<Option<String>> {
        self.core(n::cp_keywords)
    }
    pub fn set_keywords(&mut self, v: Option<&str>) -> Result<()> {
        self.set_core(n::cp_keywords, v)
    }
    /// Comments (`dc:description`).
    pub fn comments(&mut self) -> Result<Option<String>> {
        self.core(n::dc_description)
    }
    pub fn set_comments(&mut self, v: Option<&str>) -> Result<()> {
        self.set_core(n::dc_description, v)
    }
    pub fn last_author(&mut self) -> Result<Option<String>> {
        self.core(n::cp_lastModifiedBy)
    }
    pub fn set_last_author(&mut self, v: Option<&str>) -> Result<()> {
        self.set_core(n::cp_lastModifiedBy, v)
    }
    pub fn category(&mut self) -> Result<Option<String>> {
        self.core(n::cp_category)
    }
    pub fn set_category(&mut self, v: Option<&str>) -> Result<()> {
        self.set_core(n::cp_category, v)
    }
    /// The last save (`dcterms:modified`).
    pub fn last_save_date(&mut self) -> Result<Option<UtcDateTime>> {
        Ok(self.core(n::dcterms_modified)?.and_then(|s| parse_date(&s)))
    }
    pub fn set_last_save_date(&mut self, v: Option<UtcDateTime>) -> Result<()> {
        self.set_core_date(n::dcterms_modified, v)
    }
    pub fn created_date(&mut self) -> Result<Option<UtcDateTime>> {
        Ok(self.core(n::dcterms_created)?.and_then(|s| parse_date(&s)))
    }
    pub fn set_created_date(&mut self, v: Option<UtcDateTime>) -> Result<()> {
        self.set_core_date(n::dcterms_created, v)
    }
    pub fn company(&mut self) -> Result<Option<String>> {
        self.app("Company")
    }
    pub fn set_company(&mut self, v: Option<&str>) -> Result<()> {
        self.set_app("Company", v)
    }
    pub fn manager(&mut self) -> Result<Option<String>> {
        self.app("Manager")
    }
    pub fn set_manager(&mut self, v: Option<&str>) -> Result<()> {
        self.set_app("Manager", v)
    }
    /// The application that saved (`app.xml` Application).
    pub fn application_name(&mut self) -> Result<Option<String>> {
        self.app("Application")
    }
    pub fn set_application_name(&mut self, v: Option<&str>) -> Result<()> {
        self.set_app("Application", v)
    }

    /// The names of the custom properties.
    pub fn custom_property_names(&mut self) -> Result<Vec<String>> {
        let Some(root) = self.custom_root(false)? else { return Ok(Vec::new()) };
        let dom = &self.doc.dom;
        Ok(dom.elements_named(root, n::custom_property).into_iter().map(|p| dom.attr(p, n::a_name).unwrap_or("").to_owned()).collect())
    }

    /// A text custom property (`vt:lpwstr`), replacing one of the same name.
    pub fn set_custom_property(&mut self, name: &str, value: &str) -> Result<()> {
        let root = self.custom_root(true)?.ok_or_else(|| super::error::ApiError::invalid_operation("no custom properties part"))?;
        let dom = &mut self.doc.dom;
        let same: Vec<NodeId> =
            dom.elements_named(root, n::custom_property).into_iter().filter(|&p| dom.attr(p, n::a_name) == Some(name)).collect();
        for p in same {
            dom.remove_if_attached(p);
        }
        let pid = dom
            .elements_named(root, n::custom_property)
            .into_iter()
            .map(|p| dom.attr(p, n::pid).and_then(net::parse_int).unwrap_or(1))
            .max()
            .unwrap_or(1)
            .wrapping_add(1);
        let vt = dom.ns(VT);
        let lpwstr = dom.name(vt, "lpwstr");
        let prop = dom.new_element_with(
            n::custom_property,
            &[(n::fmtid, "{D5CDD505-2E9C-101B-9397-08002B2CF9AE}"), (n::pid, &pid.to_string()), (n::a_name, name)],
        );
        let v = dom.new_element(lpwstr);
        dom.append_text(v, value);
        dom.append(prop, v);
        dom.append(root, prop);
        Ok(())
    }

    /// Removes every custom property (and the `custom.xml` part).
    pub fn remove_custom_properties(&mut self) -> Result<()> {
        let Some(rel) = self.doc.get_relationships("")?.into_iter().find(|r| rel_types::is(&r.rel_type, "custom-properties")) else {
            return Ok(());
        };
        let part = self.doc.resolve_target("", &rel);
        self.doc.remove_relationship("", &rel.id)?;
        if let Some(p) = part {
            self.doc.delete_part(&p)?;
        }
        Ok(())
    }

    /// Clears the personal and organizational metadata: author, last author, company, manager,
    /// category, comments, keywords, subject, title and the custom properties.
    pub fn remove_personal_metadata(&mut self) -> Result<()> {
        self.set_author(None)?;
        self.set_last_author(None)?;
        self.set_company(None)?;
        self.set_manager(None)?;
        self.set_category(None)?;
        self.set_comments(None)?;
        self.set_keywords(None)?;
        self.set_subject(None)?;
        self.set_title(None)?;
        self.remove_custom_properties()
    }

    // ── core ──

    fn core_root(&mut self, create: bool) -> Result<Option<NodeId>> {
        self.package_part_root(
            "core-properties",
            create,
            "docProps/core.xml",
            content_types::CORE_PROPERTIES,
            rel_types::CORE_PROPERTIES,
            |d| {
                let cp = d.ns_uri(super::dom::NS_CP).to_owned();
                let dc = d.ns_uri(super::dom::NS_DC).to_owned();
                let dcterms = d.ns_uri(super::dom::NS_DCTERMS).to_owned();
                let xsi = d.ns_uri(super::dom::NS_XSI).to_owned();
                d.new_element_with(
                    n::cp_coreProperties,
                    &[
                        (n::xmlns_cp, &cp),
                        (n::xmlns_dc, &dc),
                        (n::xmlns_dcterms, &dcterms),
                        (n::xmlns_dcmitype, "http://purl.org/dc/dcmitype/"),
                        (n::xmlns_xsi, &xsi),
                    ],
                )
            },
        )
    }

    fn core(&mut self, name: Name) -> Result<Option<String>> {
        let Some(root) = self.core_root(false)? else { return Ok(None) };
        Ok(self.doc.dom.element(root, name).map(|e| self.doc.dom.value(e)))
    }

    fn set_core(&mut self, name: Name, value: Option<&str>) -> Result<()> {
        match value {
            None | Some("") => {
                if let Some(root) = self.core_root(false)? {
                    let dom = &mut self.doc.dom;
                    for e in dom.elements_named(root, name) {
                        dom.remove_if_attached(e);
                    }
                }
                Ok(())
            }
            Some(v) => {
                let root = self.core_root(true)?.ok_or_else(|| super::error::ApiError::invalid_operation("no core properties part"))?;
                set_child_text(&mut self.doc.dom, root, name, &remove_invalid_chars(v));
                Ok(())
            }
        }
    }

    fn set_core_date(&mut self, name: Name, value: Option<UtcDateTime>) -> Result<()> {
        let Some(v) = value else {
            if let Some(root) = self.core_root(false)? {
                let dom = &mut self.doc.dom;
                for e in dom.elements_named(root, name) {
                    dom.remove_if_attached(e);
                }
            }
            return Ok(());
        };
        let root = self.core_root(true)?.ok_or_else(|| super::error::ApiError::invalid_operation("no core properties part"))?;
        let dom = &mut self.doc.dom;
        for e in dom.elements_named(root, name) {
            dom.remove_if_attached(e);
        }
        let el = dom.new_element_with(name, &[(n::xsi_type, "dcterms:W3CDTF")]);
        dom.append_text(el, &v.w3cdtf());
        dom.append(root, el);
        Ok(())
    }

    // ── app ──

    fn app_root(&mut self, create: bool) -> Result<Option<NodeId>> {
        self.package_part_root(
            "extended-properties",
            create,
            "docProps/app.xml",
            content_types::EXTENDED_PROPERTIES,
            rel_types::EXTENDED_PROPERTIES,
            |d| d.new_element_with(n::ext_Properties, &[(n::xmlns_vt, VT)]),
        )
    }

    fn app(&mut self, local: &str) -> Result<Option<String>> {
        let Some(root) = self.app_root(false)? else { return Ok(None) };
        let name = self.doc.dom.name(super::dom::NS_EXT_PROPS, local);
        Ok(self.doc.dom.element(root, name).map(|e| self.doc.dom.value(e)))
    }

    fn set_app(&mut self, local: &str, value: Option<&str>) -> Result<()> {
        let name = self.doc.dom.name(super::dom::NS_EXT_PROPS, local);
        match value {
            None | Some("") => {
                if let Some(root) = self.app_root(false)? {
                    let dom = &mut self.doc.dom;
                    for e in dom.elements_named(root, name) {
                        dom.remove_if_attached(e);
                    }
                }
                Ok(())
            }
            Some(v) => {
                let root = self.app_root(true)?.ok_or_else(|| super::error::ApiError::invalid_operation("no app properties part"))?;
                set_child_text(&mut self.doc.dom, root, name, &remove_invalid_chars(v));
                Ok(())
            }
        }
    }

    // ── custom ──

    fn custom_root(&mut self, create: bool) -> Result<Option<NodeId>> {
        self.package_part_root(
            "custom-properties",
            create,
            "docProps/custom.xml",
            content_types::CUSTOM_PROPERTIES,
            rel_types::CUSTOM_PROPERTIES,
            |d| d.new_element_with(n::custom_Properties, &[(n::xmlns_vt, VT)]),
        )
    }

    fn package_part_root(
        &mut self,
        suffix: &str,
        create: bool,
        default_path: &str,
        content_type: &str,
        rel_type: &str,
        new_root: impl FnOnce(&mut super::dom::Dom) -> NodeId,
    ) -> Result<Option<NodeId>> {
        let rel = self.doc.get_relationships("")?.into_iter().find(|r| rel_types::is(&r.rel_type, suffix) && !r.external);
        if let Some(part) = rel.and_then(|r| self.doc.resolve_target("", &r)) {
            let d = self.doc.xml(&part)?;
            return Ok(self.doc.dom.root(d));
        }
        if !create {
            return Ok(None);
        }
        let path = self.doc.unique_part_path(default_path);
        let d = self.doc.dom.new_document(Some(Declaration {
            version: Some("1.0".into()),
            encoding: Some("UTF-8".into()),
            standalone: Some("yes".into()),
        }));
        let root = new_root(&mut self.doc.dom);
        self.doc.dom.append(d, root);
        self.doc.set_xml_part(&path, d, Some(content_type))?;
        let _ = relative_target;
        let _ = NS_XMLNS;
        self.doc.add_relationship("", rel_type, &path, false)?;
        Ok(self.doc.dom.root(d))
    }
}

/// `root.Element(name)` set to the text, or a new element with it at the end.
fn set_child_text(dom: &mut super::dom::Dom, root: NodeId, name: Name, value: &str) {
    match dom.element(root, name) {
        Some(e) => dom.set_value(e, value),
        None => {
            let e = dom.new_element(name);
            dom.append_text(e, value);
            dom.append(root, e);
        }
    }
}

/// `DateTime.TryParse(text, InvariantCulture, AdjustToUniversal | AssumeUniversal)` for the
/// forms of `dcterms` dates: ISO 8601 date, date-time with optional seconds, fraction and zone
/// (`Z`, `±hh:mm`), with leading and trailing white space; other forms .NET also reads
/// (`MM/dd/yyyy`, RFC 1123…) are `None` (accepted by name, record "Accepted differences").
pub fn parse_date(text: &str) -> Option<UtcDateTime> {
    let s = net::trim(text);
    if let Some(d) = parse_other_forms(s) {
        return Some(d);
    }
    let b = s.as_bytes();
    let num = |from: usize, len: usize| -> Option<u32> {
        let t = b.get(from..from + len)?;
        if !t.iter().all(u8::is_ascii_digit) {
            return None;
        }
        Some(t.iter().fold(0u32, |a, &d| a * 10 + u32::from(d - b'0')))
    };
    let year = num(0, 4)?;
    if b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let month = num(5, 2)?;
    let day = num(8, 2)?;
    let mut i = 10;
    let (mut hour, mut minute, mut second, mut ticks) = (0, 0, 0, 0);
    let mut offset_minutes: i64 = 0;
    if i < b.len() {
        if !matches!(b.get(i), Some(b'T' | b' ')) {
            return None;
        }
        i += 1;
        hour = num(i, 2)?;
        if b.get(i + 2) != Some(&b':') {
            return None;
        }
        minute = num(i + 3, 2)?;
        i += 5;
        if b.get(i) == Some(&b':') {
            second = num(i + 1, 2)?;
            i += 3;
            if b.get(i) == Some(&b'.') {
                i += 1;
                let start = i;
                while b.get(i).is_some_and(u8::is_ascii_digit) {
                    i += 1;
                }
                if i == start {
                    return None;
                }
                let digits = b.get(start..i)?;
                let mut t = 0u32;
                for k in 0..7 {
                    t = t * 10 + digits.get(k).map_or(0, |&d| u32::from(d - b'0'));
                }
                ticks = t;
            }
        }
        match b.get(i) {
            None => {}
            Some(b'Z') if i + 1 == b.len() => {}
            Some(&c @ (b'+' | b'-')) => {
                let oh = num(i + 1, 2)?;
                if b.get(i + 3) != Some(&b':') {
                    return None;
                }
                let om = num(i + 4, 2)?;
                if i + 6 != b.len() || oh > 14 || om > 59 {
                    return None;
                }
                let m = i64::from(oh * 60 + om);
                offset_minutes = if c == b'+' { m } else { -m };
            }
            _ => return None,
        }
    }
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) || hour > 23 || minute > 59 || second > 59 || year == 0 {
        return None;
    }
    // to UTC: subtract the offset
    let mut total = days_from_civil(i64::from(year), month, day) * 1440 + i64::from(hour) * 60 + i64::from(minute) - offset_minutes;
    let days = total.div_euclid(1440);
    total = total.rem_euclid(1440);
    let (y, m, d) = civil_from_days(days);
    if !(1..=9999).contains(&y) {
        return None;
    }
    Some(UtcDateTime {
        year: i32::try_from(y).ok()?,
        month: m,
        day: d,
        hour: u32::try_from(total / 60).ok()?,
        minute: u32::try_from(total % 60).ok()?,
        second,
        ticks,
    })
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

/// The other invariant forms measured in the corpus of tests: `M/d/yyyy[ HH:mm[:ss]]`,
/// `d MMM yyyy`, RFC 1123 (`ddd, dd MMM yyyy HH:mm:ss GMT`).
fn parse_other_forms(s: &str) -> Option<UtcDateTime> {
    let mk = |y: u32, m: u32, d: u32, h: u32, mi: u32, sec: u32| -> Option<UtcDateTime> {
        ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(y, m) && (1..=9999).contains(&y) && h < 24 && mi < 60 && sec < 60)
            .then(|| UtcDateTime { year: i32::try_from(y).unwrap_or(1), month: m, day: d, hour: h, minute: mi, second: sec, ticks: 0 })
    };
    let num = |t: &str| -> Option<u32> {
        (!t.is_empty() && t.len() <= 4 && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| t.bytes().fold(0u32, |a, d| a * 10 + u32::from(d - b'0')))
    };
    let month = |t: &str| {
        MONTHS
            .iter()
            .position(|m| t.len() >= 3 && t.get(..3).is_some_and(|x| x.eq_ignore_ascii_case(m)))
            .and_then(|k| u32::try_from(k + 1).ok())
    };
    let hms = |t: &str| -> Option<(u32, u32, u32)> {
        let p: Vec<&str> = t.split(':').collect();
        match p.as_slice() {
            [h, m] => Some((num(h)?, num(m)?, 0)),
            [h, m, s] => Some((num(h)?, num(m)?, num(s)?)),
            _ => None,
        }
    };
    if s.contains('/') {
        let (date, time) = s.split_once(' ').unwrap_or((s, ""));
        let p: Vec<&str> = date.split('/').collect();
        let [m, d, y] = p.as_slice() else { return None };
        if y.len() != 4 {
            return None;
        }
        let (h, mi, sec) = if time.is_empty() { (0, 0, 0) } else { hms(time.trim_matches(' '))? };
        return mk(num(y)?, num(m)?, num(d)?, h, mi, sec);
    }
    let words: Vec<&str> = s.split(' ').filter(|w| !w.is_empty()).collect();
    match words.as_slice() {
        [d, m, y] if month(m).is_some() && y.len() == 4 => mk(num(y)?, month(m)?, num(d)?, 0, 0, 0),
        [w, d, m, y, t, z] if w.ends_with(',') && z.eq_ignore_ascii_case("GMT") && month(m).is_some() => {
            let (h, mi, sec) = hms(t)?;
            mk(num(y)?, month(m)?, num(d)?, h, mi, sec)
        }
        _ => None,
    }
}

fn days_in_month(y: u32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400) => 29,
        _ => 28,
    }
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let m = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    (if m <= 2 { y + 1 } else { y }, m, d)
}
