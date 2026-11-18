//! The text half of the shape reader (`read.ts`): `readTxBody`, `readParagraph`,
//! `readBulletStyle`, `readRun`.

use uo_jsnum::parse::parse_int;
use uo_opc::xml::Element;

use crate::color::CssColor;
use crate::model::*;
use crate::read_dml::{apply_tile, fill_alpha_val, parse_grad_fill, parse_patt_fill, val_after};
use crate::source::{RelMap, is_int_text, pint, ta};
use crate::text_style::{BlipResolver, parse_tx_style_levels, read_run_extras, read_tabs, read_text_shadow};
use crate::theme::{Theme, color_from_dml, solid_fill_color, text_fill_color};

/// `readTxBody` (`read.ts:1829`).
pub fn read_tx_body(el: Element<'_>, theme: &Theme, rels: Option<&RelMap>, blip: Option<BlipResolver<'_>>) -> TextBody {
    let mut tb = TextBody::default();
    if let Some(body_pr) = el.first_child("a:bodyPr") {
        let int = |k: &str| body_pr.attr(k).map(pint);
        tb.anchor = ta(body_pr, "anchor").map(Into::into);
        tb.wrap = ta(body_pr, "wrap").map(Into::into);
        if let Some(v) = body_pr.attr("spcFirstLastPara") {
            tb.x_mut().spc_first_last_para = Some(v == "1");
        }
        if let Some(v) = ta(body_pr, "vert") {
            tb.x_mut().vert = Some(v.into()); // 'horz' explicit too (beats the inherited vertical)
        }
        if let Some(r) = ta(body_pr, "rot").filter(|r| is_int_text(r)) {
            tb.x_mut().rot = Some(pint(r));
        }
        // insets in EMU (the application's defaults are the layout/render's business)
        tb.ins_l = int("lIns");
        tb.ins_t = int("tIns");
        tb.ins_r = int("rIns");
        tb.ins_b = int("bIns");
        // text columns
        // (the rare group is created only for a present attribute)
        if let Some(v) = int("numCol") {
            tb.x_mut().num_col = Some(v);
        }
        if let Some(v) = int("spcCol") {
            tb.x_mut().spc_col = Some(v);
        }
        if let Some(v) = ta(body_pr, "vertOverflow") {
            tb.x_mut().vert_overflow = Some(v.into());
        }
        if let Some(v) = ta(body_pr, "horzOverflow") {
            tb.x_mut().horz_overflow = Some(v.into());
        }
        // WordArt: <a:prstTxWarp prst="textArchUp"><a:avLst><a:gd fmla="val N"/>…
        if let Some(warp) = body_pr.first_child("a:prstTxWarp")
            && let Some(prst) = ta(warp, "prst").filter(|p| *p != "textNoShape")
        {
            let adj: Vec<f64> = warp
                .first_child("a:avLst")
                .map(|av| {
                    av.children_named("a:gd").map(|g| parse_int(val_after(g.attr("fmla").unwrap_or("")).unwrap_or("0"), 10)).collect()
                })
                .unwrap_or_default();
            tb.x_mut().warp = Some(Box::new(TextWarp {
                prst: prst.into(),
                adj: (!adj.is_empty()).then(|| adj.into_boxed_slice()),
                from_word_art: (body_pr.attr("fromWordArt") == Some("1")).then_some(true),
            }));
        }
        // autofit: <a:normAutofit fontScale="62500" lnSpcReduction="20000"/>
        let na = body_pr.first_child("a:normAutofit");
        if na.is_some() {
            tb.autofit = Some(Autofit::Norm);
        } else if body_pr.first_child("a:noAutofit").is_some() {
            tb.autofit = Some(Autofit::None);
        } else if body_pr.first_child("a:spAutoFit").is_some() {
            tb.autofit = Some(Autofit::Shape);
        }
        if let Some(na) = na {
            if let Some(v) = na.attr("fontScale") {
                tb.x_mut().font_scale = Some(pint(v) / 100000.0);
            }
            if let Some(v) = na.attr("lnSpcReduction") {
                tb.x_mut().ln_spc_reduction = Some(pint(v) / 100000.0);
            }
        }
    }
    // the txBody's own <a:lstStyle> — highest priority of the cascade
    if let Some(lst) = el.first_child("a:lstStyle")
        && lst.child_elements().next().is_some()
    {
        tb.lst_style = Some(parse_tx_style_levels(Some(lst), theme, blip));
    }
    for p in el.children_named("a:p") {
        tb.paragraphs.push(read_paragraph(p, theme, rels, blip));
    }
    tb
}

/// `readParagraph` (`read.ts:1877`).
pub fn read_paragraph(p: Element<'_>, theme: &Theme, rels: Option<&RelMap>, blip: Option<BlipResolver<'_>>) -> Paragraph {
    let mut para = Paragraph::default();
    if let Some(ppr) = p.first_child("a:pPr") {
        para.align = ta(ppr, "algn").map(Into::into);
        para.level = ta(ppr, "lvl").map(pint);
        para.mar_l = ppr.attr("marL").map(pint);
        para.def_tab_sz = ppr.attr("defTabSz").map(pint);
        if let Some(tl) = ppr.first_child("a:tabLst") {
            para.x_mut().tabs = Some(read_tabs(tl));
        }
        para.indent = ppr.attr("indent").map(pint);
        if ppr.attr("eaLnBrk") == Some("0") {
            para.x_mut().ea_ln_brk = Some(false);
        }
        if ppr.attr("hangingPunct") == Some("0") {
            para.x_mut().hanging_punct = Some(false);
        }
        if let Some(fa @ ("t" | "ctr" | "b")) = ppr.attr("fontAlgn") {
            para.x_mut().font_algn = Some(fa.into());
        }
        // line and paragraph spacing
        if let Some(ln_spc) = ppr.first_child("a:lnSpc") {
            let pct = ln_spc.first_child("a:spcPct").and_then(|e| ta(e, "val"));
            let pts = ln_spc.first_child("a:spcPts").and_then(|e| ta(e, "val"));
            if let Some(v) = pct {
                para.line_spacing = Some(pint(v) / 100000.0);
            } else if let Some(v) = pts {
                para.line_spacing_pt = Some(pint(v) / 100.0);
            }
        }
        let spc = |name: &str| -> (Option<f64>, Option<f64>) {
            let Some(el) = ppr.first_child(name) else { return (None, None) };
            let pts = el.first_child("a:spcPts").and_then(|e| ta(e, "val")).map(|v| pint(v) / 100.0);
            let pct = el.first_child("a:spcPct").and_then(|e| ta(e, "val")).map(|v| pint(v) / 100000.0);
            (pts, pct)
        };
        (para.space_before_pt, para.space_before_pct) = spc("a:spcBef");
        (para.space_after_pt, para.space_after_pct) = spc("a:spcAft");
        // explicit bullet: buChar/buAutoNum turn it on; buNone turns it off (blocks inheritance)
        let bu_char = ppr.first_child("a:buChar");
        let bu_auto = ppr.first_child("a:buAutoNum");
        if let Some(bc) = bu_char {
            para.bullet = Some(true);
            para.bu_mut().bullet_char = ta(bc, "char").map(Into::into);
        } else if let Some(ba) = bu_auto {
            para.bullet = Some(true);
            para.bu_mut().bullet_auto_num = Some(ba.attr("type").unwrap_or("arabicPeriod").into());
        } else if ppr.first_child("a:buNone").is_some() {
            para.bullet = Some(false);
        }
        // IMAGE bullet (buBlip): replaces the character
        let bb = ppr.first_child("a:buBlip").and_then(|b| b.first_child("a:blip"));
        if let (Some(bb), Some(res)) = (bb, blip)
            && let Some(img) = res(bb)
        {
            para.bullet = Some(true);
            para.bu_mut().bullet_image = Some(Box::new(img));
            if para.bu().bullet_char.is_none() {
                para.bu_mut().bullet_char = Some("\u{2022}".into());
            }
        }
        read_bullet_style(ppr, theme, &mut para);
    }
    // end of paragraph: the line size of an EMPTY paragraph
    if let Some(sz) = p.first_child("a:endParaRPr").and_then(|e| ta(e, "sz")) {
        para.end_sz = Some(pint(sz) / 100.0);
    }
    for child in p.child_elements() {
        match child.name() {
            "a:r" => para.runs.push(read_run(child, theme, rels, blip)),
            "a:br" => para.runs.push(Run { text: "\n".into(), ..Run::default() }),
            "a:fld" => {
                // field: its own formatting as a run (rPr); the type stays for the live value
                let mut fr = read_run(child, theme, rels, blip);
                if let Some(t) = ta(child, "type") {
                    fr.extras.get_or_insert_with(Default::default).field = Some(t.into());
                }
                para.runs.push(fr);
            }
            _ => {}
        }
    }
    para
}

/// `readBulletStyle` (`read.ts:1955`): bullet font, colour and size (`buFont`/`buClr`/`buSzPct|Pts`).
fn read_bullet_style(ppr: Element<'_>, theme: &Theme, para: &mut Paragraph) {
    if let Some(f) = ppr.first_child("a:buFont").and_then(|b| ta(b, "typeface")) {
        para.bu_mut().bullet_font = Some(f.into());
    }
    if let Some(bc) = ppr.first_child("a:buClr")
        && let Some(c) = color_from_dml(bc.child_elements().next(), theme)
    {
        para.bu_mut().bullet_color = Some(c);
    }
    let pct = ppr.first_child("a:buSzPct").and_then(|e| ta(e, "val"));
    let pts = ppr.first_child("a:buSzPts").and_then(|e| ta(e, "val"));
    if let Some(v) = pct {
        para.bu_mut().bullet_size_pct = Some(pint(v) / 100000.0);
    } else if let Some(v) = pts {
        para.bu_mut().bullet_size_pt = Some(pint(v) / 100.0);
    } else if ppr.first_child("a:buSzTx").is_some() {
        para.bu_mut().bullet_size_pct = Some(1.0);
    }
}

/// `/^https?:/i.test(s)`.
fn is_http(s: &str) -> bool {
    s.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("http:")) || s.get(..6).is_some_and(|p| p.eq_ignore_ascii_case("https:"))
}

/// `readRun` (`read.ts:1989`).
pub fn read_run(r: Element<'_>, theme: &Theme, rels: Option<&RelMap>, blip: Option<BlipResolver<'_>>) -> Run {
    let t = r.first_child("a:t");
    let mut run = Run { text: t.map(|t| t.text_content()).unwrap_or_default().into(), ..Run::default() };
    let mut ex = RunExtras::default();
    if let Some(rpr) = r.first_child("a:rPr") {
        if let Some(sz) = ta(rpr, "sz") {
            run.sz = Some(pint(sz) / 100.0);
        }
        run.lang = ta(rpr, "lang").map(Into::into);
        // explicit b/i (including "0", which BLOCKS inheritance from the cascade)
        if let Some(b) = rpr.attr("b") {
            run.bold = Some(b == "1");
        }
        if let Some(i) = rpr.attr("i") {
            run.italic = Some(i == "1");
        }
        if ta(rpr, "u").is_some_and(|u| u != "none") {
            run.underline = Some(true);
        }
        let e = read_run_extras(rpr);
        ex.baseline = e.baseline;
        ex.cap = e.cap;
        ex.strike = e.strike;
        ex.spc = e.spc;
        run.kern = e.kern;
        if let Some(c) = text_fill_color(rpr, theme) {
            run.color = Some(c);
        }
        run.font = rpr.first_child("a:latin").and_then(|l| ta(l, "typeface")).map(Into::into);
        ex.sym_font = rpr.first_child("a:sym").and_then(|l| ta(l, "typeface")).map(Into::into);
        // letter outline and pattern fill (WordArt)
        if let Some(rln) = rpr.first_child("a:ln")
            && rln.first_child("a:noFill").is_none()
        {
            let oc = solid_fill_color(rln, theme);
            let oa = rln.first_child("a:solidFill").and_then(fill_alpha_val);
            let alpha = oa.map_or(1.0, |a| pint(a) / 100000.0);
            // a transparent outline (alpha 0, common in Korean decks) does not draw
            if let Some(oc) = oc
                && alpha > 0.0
            {
                let n = uo_jsnum::conv::to_int32(parse_int(oc.as_str().get(1..).unwrap_or(""), 16));
                let color = if alpha < 1.0 {
                    CssColor::new(&format!(
                        "rgba({},{},{},{})",
                        (n >> 16) & 255,
                        (n >> 8) & 255,
                        n & 255,
                        uo_jsnum::format::to_js_string(alpha)
                    ))
                } else {
                    oc
                };
                ex.fx_mut().outline = Some((color, rln.attr("w").filter(|w| !w.is_empty()).map_or(12700.0, pint)));
            }
        }
        if let Some(rpf) = rpr.first_child("a:pattFill") {
            let pf = parse_patt_fill(rpf, theme);
            if let Some(p) = pf.pattern {
                ex.fx_mut().patt_fill = Some(*p);
                run.color = pf.color;
            }
        }
        // gradient, image and opacity of the letters (the drawing uses the LINE's box — measured, syn-b-e11)
        if let Some(g) = rpr.first_child("a:gradFill").and_then(|g| parse_grad_fill(g, theme)) {
            ex.fx_mut().grad_fill = Some(Box::new(g));
        }
        let rbf = rpr.first_child("a:blipFill");
        let rbb = rbf.and_then(|b| b.first_child("a:blip"));
        if let (Some(rbb), Some(res), Some(rbf)) = (rbb, blip, rbf)
            && let Some(mut rimg) = res(rbb)
        {
            if apply_tile(&mut rimg, Some(rbf)) {
                ex.fx_mut().fill_tile = Some(true);
            }
            ex.fx_mut().fill_image = Some(rimg);
        }
        let ra = rpr.first_child("a:solidFill").and_then(fill_alpha_val);
        if let Some(ra) = ra {
            let v = pint(ra);
            if v < 100000.0 {
                ex.alpha = Some(uo_jsnum::math::max(0.0, v / 100000.0));
            }
        }
        // letters without fill (only the outline)
        if rpr.first_child("a:noFill").is_some() {
            ex.alpha = Some(0.0);
        }
        // hyperlink: <a:hlinkClick r:id="rIdN"/> → resolved through the slide's rels
        if let Some(hlink) = rpr.first_child("a:hlinkClick") {
            // link look: the theme's hlink colour + underline — unless `<ahyp:hlinkClr val="tx">`
            // (Office 2019+: follows the text colour) or an action that is not a jump
            let action = hlink.attr("action").unwrap_or("");
            let tx = hlink.descend("ahyp:hlinkClr").and_then(|e| e.attr("val")) == Some("tx");
            let is_link = ta(hlink, "r:id").is_some() || ta(hlink, "id").is_some() || action.starts_with("ppaction://hlink");
            if is_link && !tx {
                ex.link_color = theme.colors.get("hlink").cloned();
            }
            if let Some(rels) = rels {
                let rid = hlink.attr("r:id").or_else(|| hlink.attr("id"));
                if let Some(rid) = rid.filter(|r| !r.is_empty())
                    && let Some(direct) = rels.get(rid).filter(|d| !d.is_empty() && is_http(d))
                {
                    ex.hyperlink = Some(direct.into());
                }
            }
        }
    }
    // text shadow (a common effect in template titles over a photo)
    if let Some(rpr) = r.first_child("a:rPr")
        && let Some(shd) = rpr.first_child("a:effectLst").and_then(|e| e.first_child("a:outerShdw"))
    {
        // without <a:alpha> the shadow is OPAQUE (measured, syn-b-e7: FF8000 comes out FF8000)
        ex.fx_mut().shadow = Some(read_text_shadow(shd, theme, 1.0));
    }
    if ex != RunExtras::default() {
        run.extras = Some(Box::new(ex));
    }
    run
}
