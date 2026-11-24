//! The drawings of the `.xls` sheet → `xl/drawings/drawingN.xml` (+ media and charts) (`drawing.ts`).
//!
//! The shapes come from the concatenated MSODRAWING (`officeart`); each is linked, by the offset of its ClientData, to the OBJ that
//! follows it in the substream (ot = 8 picture, 5 chart, 0x19 note…). The cell anchor of the BIFF stores the offsets in 1/1024 of the
//! column width and 1/256 of the row height — converted here to EMU with the SAME widths/heights in px as the grid.

use std::collections::HashMap;

use uo_jsnum::math::{max, min, round};

use super::chart::{ChartContext, chart_records, chart_space_xml};
use super::globals::XlsGlobals;
use super::metrics::{col_width_to_px, default_col_px, excel_mdw, excel_row_px, row_pt_to_px};
use super::officeart::{BlipStore, CellAnchor, flatten, read_drawing_defaults, read_shapes};
use super::shapes::{Env, ShapeKind, is_hidden_shape, object_xml, shape_kind};
use super::sheet::XlsSheet;
use super::sheet_xml::row_height_pt;
use super::styles_xml::font_at;
use super::xmlw::{Parts, REL, Rel, XML_HEAD, rels_xml};

pub struct DrawingOut {
    pub parts: Parts,
    pub overrides: Vec<(String, String)>,
    pub defaults: Vec<(String, String)>,
}

/// The state shared between the sheets: counters and the media already written.
pub struct MediaState<'a> {
    pub n: u32,
    pub charts: u32,
    /// pib → path of the media (a picture used in several sheets is written once).
    pub by_pib: HashMap<u32, String>,
    pub store: Option<BlipStore<'a>>,
    /// The chart converter's view of the workbook: the cells, the sheet names, the stream.
    pub chart_ctx: ChartContext<'a>,
    pub buf: &'a [u8],
    /// What may still be inflated from the metroBlobs of the shapes, in all (the TS has no bound; record C2, decision C2-8).
    pub metro_left: u64,
}

const EMU_PER_PX: f64 = 9525.0;

fn mime(ext: &str) -> &'static str {
    match ext {
        "png" => "image/png",
        "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "tiff" => "image/tiff",
        "emf" => "image/x-emf",
        "wmf" => "image/x-wmf",
        "pict" => "image/pict",
        _ => "application/octet-stream",
    }
}

/// The widths/heights in px of the grid by the SAME rules as the renderer (grid-metrics: MDW of the Normal font; a row without a
/// manual height = auto-fit by the largest font of the row — here without the text wraps).
pub struct GridMetrics<'s> {
    g: &'s XlsGlobals,
    sh: &'s XlsSheet,
    mdw: f64,
    def_col: f64,
    normal_px: f64,
    def_row: f64,
    font_px: HashMap<u16, f64>,
}

impl<'s> GridMetrics<'s> {
    pub fn new(g: &'s XlsGlobals, sh: &'s XlsSheet) -> Self {
        let f0 = font_at(g, 0);
        let size0 = f64::from(f0.height) / 20.0;
        let mdw = excel_mdw(&f0.name, size0).unwrap_or(7.0);
        let def_col = default_col_px(mdw, sh.std_width.map(|w| f64::from(w) / 256.0), sh.def_col_width.map(f64::from));
        let normal_px = excel_row_px(&f0.name, size0, f0.weight >= 600);
        let def_row = if sh.def_row_custom { row_pt_to_px(row_height_pt(f64::from(sh.def_row_height.unwrap_or(255)))) } else { normal_px };
        GridMetrics { g, sh, mdw, def_col, normal_px, def_row, font_px: HashMap::new() }
    }

    fn font_px(&mut self, ixf: u16) -> f64 {
        if let Some(&v) = self.font_px.get(&ixf) {
            return v;
        }
        let f = font_at(self.g, self.g.xfs.get(usize::from(ixf)).map_or(0, |x| x.font));
        let v = excel_row_px(&f.name, f64::from(f.height) / 20.0, f.weight >= 600);
        self.font_px.insert(ixf, v);
        v
    }

    pub fn col_px(&self, c: u32) -> f64 {
        for ci in &self.sh.cols {
            if c >= ci.first && c <= ci.last {
                return if ci.hidden { 0.0 } else { col_width_to_px(f64::from(ci.width) / 256.0, self.mdw) };
            }
        }
        self.def_col
    }

    pub fn row_px(&mut self, r: u32) -> f64 {
        let info = self.sh.rows.get(&r);
        if info.is_some_and(|i| i.hidden) {
            return 0.0;
        }
        if let Some(i) = info.filter(|i| i.custom)
            && let Some(h) = i.height
        {
            return row_pt_to_px(row_height_pt(f64::from(h)));
        }
        if info.is_none() {
            return self.def_row;
        }
        let mut h = self.normal_px;
        let xfs: Vec<u16> = self.sh.cells.row_xfs(r).collect();
        for xf in xfs {
            h = max(h, self.font_px(xf));
        }
        h
    }
}

fn anchor_xml(a: &CellAnchor, m: &mut GridMetrics<'_>) -> String {
    let off = |frac: f64, size: f64| -> String { uo_jsnum::format::to_js_string(round(frac * size * EMU_PER_PX)) };
    let dx1 = off(min(f64::from(a.dx1), 1024.0) / 1024.0, m.col_px(a.c1));
    let dy1 = off(min(f64::from(a.dy1), 256.0) / 256.0, m.row_px(a.r1));
    let dx2 = off(min(f64::from(a.dx2), 1024.0) / 1024.0, m.col_px(a.c2));
    let dy2 = off(min(f64::from(a.dy2), 256.0) / 256.0, m.row_px(a.r2));
    format!(
        "<xdr:from><xdr:col>{}</xdr:col><xdr:colOff>{dx1}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{dy1}</xdr:rowOff></xdr:from>\
         <xdr:to><xdr:col>{}</xdr:col><xdr:colOff>{dx2}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{dy2}</xdr:rowOff></xdr:to>",
        a.c1, a.r1, a.c2, a.r2
    )
}

/// The size in EMU of the box of the anchor (the proportion for the adjusts of the presets).
fn anchor_size(a: &CellAnchor, m: &mut GridMetrics<'_>) -> (f64, f64) {
    let mut span = |c1: u32, d1: u32, c2: u32, d2: u32, rows: bool, unit: f64| -> f64 {
        if c2 - c1 > 2000 {
            return 0.0;
        }
        let mut size = |i: u32| if rows { m.row_px(i) } else { m.col_px(i) };
        let mut px = 0.0;
        for i in c1..c2 {
            px += size(i);
        }
        let s1 = size(c1);
        let s2 = size(c2);
        (px - (min(f64::from(d1), unit) / unit) * s1 + (min(f64::from(d2), unit) / unit) * s2) * EMU_PER_PX
    };
    let w = max(0.0, span(a.c1, a.dx1, a.c2, a.dx2, false, 1024.0));
    let h = max(0.0, span(a.r1, a.dy1, a.r2, a.dy2, true, 256.0));
    (w, h)
}

/// The mutable state of the conversion of one sheet's drawing: ids, relationships, parts and the shared media.
pub struct DrawState<'m, 'a> {
    id: u32,
    rels: Vec<Rel>,
    image_rels: HashMap<u32, String>,
    parts: Parts,
    overrides: Vec<(String, String)>,
    defaults: Vec<(String, String)>,
    media: &'m mut MediaState<'a>,
}

impl DrawState<'_, '_> {
    /// The workbook-wide budget of the metroBlob inflation.
    pub fn metro_budget(&mut self) -> &mut u64 {
        &mut self.media.metro_left
    }

    /// `nextId`: the next cNvPr id, unique in the drawing.
    pub fn next_id(&mut self) -> u32 {
        self.id += 1;
        self.id
    }

    /// The rId of the picture `pib` (media written once per workbook, relationship once per drawing); `None` = no picture.
    pub fn image_rel(&mut self, pib: u32) -> Option<String> {
        if let Some(hit) = self.image_rels.get(&pib).filter(|h| !h.is_empty()) {
            return Some(hit.clone());
        }
        let path = match self.media.by_pib.get(&pib).filter(|p| !p.is_empty()) {
            Some(p) => p.clone(),
            None => {
                let (ext, bytes) = {
                    let blip = self.media.store.as_mut()?.get(pib)?;
                    (blip.kind.ext(), blip.bytes.to_vec())
                };
                self.media.n += 1;
                let path = format!("xl/media/image{}.{ext}", self.media.n);
                self.parts.set(&path, bytes);
                self.defaults.push((ext.to_owned(), mime(ext).to_owned()));
                self.media.by_pib.insert(pib, path.clone());
                path
            }
        };
        let rid = format!("rId{}", self.rels.len() + 1);
        self.rels.push(Rel {
            id: rid.clone(),
            kind: "image",
            target: format!("../media/{}", path.strip_prefix("xl/media/").unwrap_or(&path)),
            external: false,
        });
        self.image_rels.insert(pib, rid.clone());
        Some(rid)
    }

    /// The bytes of the picture `pib` (pattern fills).
    pub fn blip_bytes(&mut self, pib: u32) -> Option<Vec<u8>> {
        self.media.store.as_mut()?.get(pib).map(|b| b.bytes.to_vec())
    }

    /// Writes the chart (`chartN.xml` + rel) of a range of the stream; gives the rId.
    pub fn chart_rel(&mut self, range: Option<(usize, usize)>) -> Option<String> {
        let recs = chart_records(self.media.buf, range?);
        let xml = chart_space_xml(&self.media.chart_ctx, recs).filter(|x| !x.is_empty())?;
        self.media.charts += 1;
        let n = self.media.charts;
        let rid = format!("rId{}", self.rels.len() + 1);
        self.rels.push(Rel { id: rid.clone(), kind: "chart", target: format!("../charts/chart{n}.xml"), external: false });
        self.parts.set(&format!("xl/charts/chart{n}.xml"), xml.into_bytes());
        self.overrides
            .push((format!("/xl/charts/chart{n}.xml"), "application/vnd.openxmlformats-officedocument.drawingml.chart+xml".to_owned()));
        Some(rid)
    }
}

/// `drawingParts`: the drawing of a sheet; `None` when it has nothing to draw.
pub fn drawing_parts<'a>(g: &'a XlsGlobals, sh: &XlsSheet, index: usize, media: &mut MediaState<'a>) -> Option<DrawingOut> {
    if sh.drawing.is_none() && sh.chart_records.is_none() {
        return None;
    }
    let mut m = GridMetrics::new(g, sh);
    let mut anchors = String::new();
    let mut st = DrawState {
        id: 1,
        rels: Vec::new(),
        image_rels: HashMap::new(),
        parts: Parts::default(),
        overrides: Vec::new(),
        defaults: Vec::new(),
        media,
    };

    fn add_chart(st: &mut DrawState<'_, '_>, anchors: &mut String, obj_id: Option<u16>, range: Option<(usize, usize)>, anchor: &str) {
        let Some(rid) = st.chart_rel(range) else { return };
        st.id += 1;
        let name_id = obj_id.map_or(st.media.charts, u32::from);
        anchors.push_str(&format!(
            "<xdr:twoCellAnchor>{anchor}<xdr:graphicFrame macro=\"\"><xdr:nvGraphicFramePr><xdr:cNvPr id=\"{}\" name=\"Gráfico {name_id}\"/><xdr:cNvGraphicFramePr/></xdr:nvGraphicFramePr>\
             <xdr:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"0\" cy=\"0\"/></xdr:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/chart\">\
             <c:chart xmlns:c=\"http://schemas.openxmlformats.org/drawingml/2006/chart\" r:id=\"{rid}\"/></a:graphicData></a:graphic></xdr:graphicFrame><xdr:clientData/></xdr:twoCellAnchor>",
            st.id
        ));
    }

    if let Some(dg) = &sh.drawing {
        let shapes = read_shapes(dg);
        // links shape → OBJ by the offset (the OBJ comes right after the MSODRAWING of the shape), groups included
        let mut flat = Vec::new();
        flatten(&shapes, &mut flat);
        let mut with_cd: Vec<&_> = flat.into_iter().filter(|x| x.client_data_at.is_some()).collect();
        with_cd.sort_by_key(|x| x.client_data_at.unwrap_or(0));
        let objs = &sh.objs;
        let mut obj_of: HashMap<usize, usize> = HashMap::new();
        let mut j = 0usize;
        for s in with_cd {
            let at = usize::try_from(s.client_data_at.unwrap_or(0)).unwrap_or(usize::MAX);
            while j < objs.len() && objs[j].drawing_at <= at {
                j += 1;
            }
            if j < objs.len() {
                obj_of.insert(s.seq, j);
            }
        }
        if st.media.store.is_none() {
            st.media.store = Some(BlipStore::new(g.drawing_group.as_deref()));
        }
        let defaults = read_drawing_defaults(g.drawing_group.as_deref());
        for s in &shapes {
            let Some(a) = s.anchor.filter(|_| !is_hidden_shape(s)) else { continue };
            if a.c1 > 16383 || a.r1 > 1_048_575 || a.c2 < a.c1 || a.r2 < a.r1 {
                continue;
            }
            let anchor = anchor_xml(&a, &mut m);
            let obj = obj_of.get(&s.seq).and_then(|&i| objs.get(i));
            let kind = shape_kind(s, obj);
            if kind == Some(ShapeKind::Chart) {
                add_chart(&mut st, &mut anchors, obj.map(|o| o.id), obj.and_then(|o| o.chart), &anchor);
                continue;
            }
            let Some(kind) = kind else { continue };
            let size = anchor_size(&a, &mut m);
            let xml = {
                let mut env = Env { g, defaults: &defaults, obj_of: &obj_of, objs, st: Some(&mut st), chart: false };
                object_xml(&mut env, s, size, false).unwrap_or_default()
            };
            if !xml.is_empty() {
                anchors.push_str(&format!(
                    "<xdr:twoCellAnchor{}>{anchor}{xml}<xdr:clientData/></xdr:twoCellAnchor>",
                    if kind == ShapeKind::Pic { " editAs=\"oneCell\"" } else { "" }
                ));
            }
        }
    }
    // chart sheet: the chart takes the initial visible area (no anchor in the file)
    if let Some(range) = sh.chart_records {
        add_chart(
            &mut st,
            &mut anchors,
            None,
            Some(range),
            "<xdr:from><xdr:col>0</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>0</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:from>\
             <xdr:to><xdr:col>12</xdr:col><xdr:colOff>0</xdr:colOff><xdr:row>30</xdr:row><xdr:rowOff>0</xdr:rowOff></xdr:to>",
        );
    }
    if anchors.is_empty() {
        return None;
    }
    let path = format!("xl/drawings/drawing{index}.xml");
    let mut out = DrawingOut { parts: st.parts, overrides: st.overrides, defaults: st.defaults };
    out.parts.set(
        &path,
        format!(
            "{XML_HEAD}<xdr:wsDr xmlns:xdr=\"http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"{REL}\">{anchors}</xdr:wsDr>"
        )
        .into_bytes(),
    );
    out.overrides.push((format!("/{path}"), "application/vnd.openxmlformats-officedocument.drawing+xml".to_owned()));
    out.parts.set(&format!("xl/drawings/_rels/drawing{index}.xml.rels"), rels_xml(&st.rels).into_bytes());
    Some(out)
}
