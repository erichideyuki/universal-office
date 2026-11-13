// Wave 7 (A2) of the editor switch (record core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables, objects
// and the page"): the JS side of src/session/shell_completion.rs — the list answers, the table style gallery and the New
// Table Style dialog's queries, the ring's cells and kinds, and the shell edits (`ShellEdit` as JSON). Re-exported by
// session.mjs; the editor shell (`write/packages/editor/src/rust-completion-*.ts`) is its only user.
//
//   const c = bindCompletion(instance.exports, uo);    // uo = bindWrite(...)
//   c.listInfo(session);                               // { numId, ilvl, absId, kind, ..., canContinue } | null
//   c.edit(session, { t: 'move', anchor: 0, dx: 400, dy: 0 });   // an ExecOutcome
//
// JSON answers are parsed; status 1 is null; other statuses and a trap throw `SessionError`.

import * as codec from './gen/session.ts';
import { SessionError } from './session.mjs';

const enc = new TextEncoder();
const dec = new TextDecoder();

export function bindCompletion(w, uo) {
  const { Buf, put } = uo.render;
  const raw = (what, f) => {
    let ptr;
    try {
      ptr = f();
    } catch (e) {
      if (e instanceof WebAssembly.RuntimeError) throw new SessionError('trap', e.message, what);
      throw e;
    }
    return new Buf(ptr).take();
  };
  const json = (what, f) => {
    const { status, data } = raw(what, f);
    if (status === 1) return null;
    if (status !== 0) throw new SessionError(status, dec.decode(data), what);
    return JSON.parse(dec.decode(data));
  };
  const outcome = (what, f) => {
    const { status, data } = raw(what, f);
    if (status === 1) return null;
    if (status !== 0) throw new SessionError(status, dec.decode(data), what);
    return codec.decodeExecResult(data, codec.newTables()).outcome;
  };
  /** a consumed UTF-8 input buffer: `[ptr, len]` */
  const text = (s) => {
    const b = enc.encode(s ?? '');
    return b.length === 0 ? [0, 0] : [put(b), b.length];
  };
  const text_ = text;
  const hops = (path) => JSON.stringify((path ?? []).map((h) => [h.r, h.c, h.b ?? -1]));

  return {
    /** `listInfo()` + `canContinue`, or null outside a list */
    listInfo(s) { return json('listInfo', () => w.uo_shell_list_info(s.ptr)); },
    /** `documentLists()` */
    documentLists(s) { return json('documentLists', () => w.uo_shell_document_lists(s.ptr)) ?? []; },
    /** `listStylesInDocument()` */
    listStyles(s) { return json('listStyles', () => w.uo_shell_list_styles(s.ptr)) ?? []; },
    /** `numberingValueInfo()`, or null */
    numberingValue(s) { return json('numberingValue', () => w.uo_shell_numbering_value(s.ptr)); },
    /** `listLibrary(kind)` in the session's language of the interface */
    listLibrary(s, kind) {
      const k = kind === 'bullet' ? 0 : kind === 'number' ? 1 : 2;
      return json('listLibrary', () => w.uo_shell_list_library(s.ptr, k)) ?? [];
    },
    /** the table style gallery's entries before the shell's sort */
    tableStyles(s) { return json('tableStyles', () => w.uo_shell_table_styles(s.ptr)) ?? []; },
    /** `tableStyleXml(nameOrId)`, or null */
    tableStyleXml(s, nameOrId) { return json('tableStyleXml', () => w.uo_shell_table_style_xml(s.ptr, ...text(nameOrId))); },
    /** `tableStyleBases()` */
    tableStyleBases(s) { return json('tableStyleBases', () => w.uo_shell_table_style_bases(s.ptr)) ?? []; },
    /** `newTableStyleId(name)` */
    newTableStyleId(s, name) { return json('newTableStyleId', () => w.uo_shell_new_table_style_id(s.ptr, ...text(name))); },
    /** the grid column widths of the table at `anchor`/`path`, or null */
    tableGrid(s, anchor, path) { return json('tableGrid', () => w.uo_shell_table_grid(s.ptr, anchor >>> 0, ...text(hops(path)))); },
    /** `hasCellsClip()` of the page's ring */
    hasCellsClip() { return !!json('hasCellsClip', () => w.uo_shell_has_cells_clip()); },
    /** the kind of each ring item by id */
    clipboardKinds() { return new Map(json('clipboardKinds', () => w.uo_shell_clipboard_kinds()) ?? []); },
    /**
     * the layout of a preview mini-document (a WriteLayout: free it): `kind` 'cover' `{ id, lang }`, 'watermark'
     * `{ id, lang }`, 'tableStyleXml' `{ xml, look, lang }`, 'tableStyle' `{ name, look }`, 'renderClip' (the pending
     * Paste as Picture), 'headerFooter' `{ gallery, id, lang, section }`; throws the last error when there is nothing to show
     */
    preview(s, env, kind, args = {}, now = Date.now()) {
      const k = { cover: 0, watermark: 1, tableStyleXml: 2, tableStyle: 3, renderClip: 4, headerFooter: 5 }[kind];
      if (k === undefined) throw new SessionError(5, `preview kind ${kind}`, 'preview');
      const t = now instanceof Date ? now.getTime() : now;
      let p;
      try {
        p = w.uo_shell_preview(s.ptr, env.ptr, k, ...text(JSON.stringify(args ?? {})), t, -new Date(t).getTimezoneOffset());
      } catch (e) {
        if (e instanceof WebAssembly.RuntimeError) throw new SessionError('trap', e.message, 'preview');
        throw e;
      }
      if (!p) throw uo.lastError('preview');
      return uo.wrapLayout(p);
    },
    /** `watermarkTextBox(text, font, sizePt)` over the page's fonts `env`: `{ widthPt, heightPt }` */
    watermarkTextBox(s, env, text, font, sizePt) {
      return json('watermarkTextBox', () => w.uo_shell_watermark_text_box(s.ptr, env.ptr, ...text_(JSON.stringify({ text, font, sizePt }))));
    },
    /** `defineListLevel({ ..., picture })`: the level's format and the picture (`{ data, ext, wPt, hPt, name }`): an ExecOutcome */
    defineListLevelPicture(s, format, pic) {
      const d = pic.data.length ? [put(pic.data), pic.data.length] : [0, 0];
      const j = text(JSON.stringify({ format, ext: pic.ext, wPt: pic.wPt, hPt: pic.hPt, name: pic.name }));
      return outcome('defineListLevelPicture', () => w.uo_shell_define_list_level_picture(s.ptr, ...j, ...d));
    },
    /** the shape parts of `selectedShape().format` (ED-213): `{ fill, style?, textbox, body?, story? }`, or null */
    shapeParts(s) { return json('shapeParts', () => w.uo_shell_shape_parts(s.ptr)); },
    /** the clip identities of a Paste Link between two sessions: `{ me, clip, link }` */
    linkOrigin(s) { return json('linkOrigin', () => w.uo_shell_link_origin(s.ptr)); },
    /** `sectPrAt(section)`: the body section's properties (the TS SectPr object) */
    sectPrAt(s, section) { return json('sectPrAt', () => w.uo_shell_sect_pr_at(s.ptr, Math.max(0, Math.trunc(section) || 0) >>> 0)); },
    /** `headerFooterTemplateXml(gallery, id)` in the language `lang`, or null */
    hfTemplateXml(s, gallery, id, lang) {
      return json('hfTemplateXml', () => w.uo_shell_hf_template_xml(s.ptr, ...text(JSON.stringify({ gallery, id, lang }))));
    },
    /** the page margins of the pending Paste as Picture (`{ left, right }` twips), or null */
    renderClipMargins(s) { return json('renderClipMargins', () => w.uo_shell_render_clip_margins(s.ptr)); },
    /** `saveSelectionToTextboxGallery`'s item: `{ xml, name }` (the box's docPr name or null), or null */
    textboxGalleryItem(s) { return json('textboxGalleryItem', () => w.uo_shell_textbox_gallery_item(s.ptr)); },
    /** the embedded object at the body position (`oleRunAt`): `{ para, run, progId, ext }` or null */
    oleAt(s, para, offset) { return json('oleAt', () => w.uo_shell_ole_at(s.ptr, para >>> 0, offset >>> 0)); },
    /** the bytes of the embedded package at the body position, or null */
    oleData(s, para, offset) {
      const { status, data } = raw('oleData', () => w.uo_shell_ole_data(s.ptr, para >>> 0, offset >>> 0));
      if (status === 1) return null;
      if (status !== 0) throw new SessionError(status, dec.decode(data), 'oleData');
      return data;
    },
    /** `editEmbeddedObject`'s commit: the package and the preview of the body run `(para, run)`: an ExecOutcome */
    replaceOle(s, para, run, data, png, widthTwips, heightTwips) {
      const d = data.length ? [put(data), data.length] : [0, 0];
      const p = png.length ? [put(png), png.length] : [0, 0];
      return outcome('replaceOle', () => w.uo_shell_replace_ole(s.ptr, para >>> 0, run >>> 0, ...d, ...p, widthTwips, heightTwips));
    },
    /** a shell edit (`{ t, ... }`; the hops of `path` as StoryHop): an ExecOutcome */
    edit(s, e) {
      const v = e.path ? { ...e, path: (e.path ?? []).map((h) => [h.r, h.c, h.b ?? -1]) } : e;
      return outcome('edit', () => w.uo_shell_edit(s.ptr, ...text(JSON.stringify(v))));
    },
  };
}
