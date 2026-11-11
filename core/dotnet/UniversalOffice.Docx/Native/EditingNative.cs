using System.Runtime.InteropServices;

namespace UniversalOffice.Native;

/// <summary>The Editing exports of uo-ffi (core/crates/uo-ffi/src/editing). Strings are UTF-8
/// <c>(ptr, len)</c> pairs (<c>len = nuint.MaxValue</c> is null); lists are the packed format of
/// <c>Wire.cs</c>; a view is a <c>uint</c> index into the document's view table.</summary>
internal static unsafe partial class NativeMethods
{
    // ───────────────────────────── structs ─────────────────────────────

    [StructLayout(LayoutKind.Sequential)]
    internal struct Str
    {
        public byte* Ptr;
        public nuint Len;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoDocxLimits
    {
        public uint Size, Reserved;
        public ulong MaxPartBytes, MaxTotalBytes;
        public uint MaxXmlDepth, MaxFieldsPerStory, MaxFieldNesting, Reserved2;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoSanitizeOptions
    {
        public uint Size, Flags;
        public Str AllowedFields, AllowedSchemes;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoApplyContentOptions
    {
        public uint Size, Flags;
        public Str SampleMarkers;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoBodyWriterOptions
    {
        public uint Size, Flags;
        public Str AllowedLinkSchemes, LinkColor;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoRunFormat
    {
        public uint Size, Flags;
        public int VerticalAlign, Reserved;
        public Str Color, Background, Link;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoParagraphFormat
    {
        public uint Size, Flags;
        public int ListLevel, LeftIndentTwips;
        public nint Base;
        public Str StyleId, ListNumId, Alignment;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoDateTime
    {
        public int Year;
        public uint Month, Day, Hour, Minute, Second, Ticks, Reserved;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoInlineRef
    {
        public uint Size, View;
        public Str Type, Json, Part;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoLimitInfo
    {
        public int Limit, Reserved;
        public ulong PartLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct UoImageInfo
    {
        public uint Size;
        public int WidthPx, HeightPx, Reserved;
        public double DpiX, DpiY;
    }

    // ───────────────────────────── errors ─────────────────────────────

    [LibraryImport(Library, EntryPoint = "uo_last_error_limit")]
    internal static partial int LastErrorLimit(UoLimitInfo* info);

    [LibraryImport(Library, EntryPoint = "uo_last_error_part")]
    internal static partial nuint LastErrorPart(byte* buf, nuint cap);

    [LibraryImport(Library, EntryPoint = "uo_editing_error_clear")]
    internal static partial void EditingErrorClear();

    // ───────────────────────────── document ─────────────────────────────

    [LibraryImport(Library, EntryPoint = "uo_docx_open")]
    internal static partial int DocxOpen(byte* data, nuint len, UoDocxLimits* limits, out DocxHandle doc);

    [LibraryImport(Library, EntryPoint = "uo_docx_free")]
    internal static partial void DocxFree(nint doc);

    [LibraryImport(Library, EntryPoint = "uo_docx_save")]
    internal static partial int DocxSave(DocxHandle d, out BufferHandle bytes);

    [LibraryImport(Library, EntryPoint = "uo_docx_clone")]
    internal static partial int DocxClone(DocxHandle d, out DocxHandle clone);

    [LibraryImport(Library, EntryPoint = "uo_docx_native_bytes")]
    internal static partial ulong DocxNativeBytes(DocxHandle d);

    [LibraryImport(Library, EntryPoint = "uo_docx_main_part_path")]
    internal static partial int DocxMainPartPath(DocxHandle d, out BufferHandle path);

    [LibraryImport(Library, EntryPoint = "uo_docx_part_names")]
    internal static partial int DocxPartNames(DocxHandle d, out BufferHandle names);

    [LibraryImport(Library, EntryPoint = "uo_docx_has_part")]
    internal static partial int DocxHasPart(DocxHandle d, byte* path, nuint plen, out int has);

    [LibraryImport(Library, EntryPoint = "uo_docx_get_part_bytes")]
    internal static partial int DocxGetPartBytes(DocxHandle d, byte* path, nuint plen, out BufferHandle bytes);

    [LibraryImport(Library, EntryPoint = "uo_docx_get_relationships")]
    internal static partial int DocxGetRelationships(DocxHandle d, byte* part, nuint plen, out BufferHandle rels);

    [LibraryImport(Library, EntryPoint = "uo_docx_get_relationship")]
    internal static partial int DocxGetRelationship(DocxHandle d, byte* part, nuint plen, byte* id, nuint idlen, out BufferHandle rel);

    [LibraryImport(Library, EntryPoint = "uo_docx_get_content_type")]
    internal static partial int DocxGetContentType(DocxHandle d, byte* part, nuint plen, out BufferHandle type);

    [LibraryImport(Library, EntryPoint = "uo_docx_prop_get")]
    internal static partial int DocxPropGet(DocxHandle d, uint which, out BufferHandle value);

    [LibraryImport(Library, EntryPoint = "uo_docx_prop_set")]
    internal static partial int DocxPropSet(DocxHandle d, uint which, byte* v, nuint vlen);

    [LibraryImport(Library, EntryPoint = "uo_docx_prop_get_date")]
    internal static partial int DocxPropGetDate(DocxHandle d, uint which, UoDateTime* value, out int present);

    [LibraryImport(Library, EntryPoint = "uo_docx_prop_set_date")]
    internal static partial int DocxPropSetDate(DocxHandle d, uint which, UoDateTime* value);

    [LibraryImport(Library, EntryPoint = "uo_docx_custom_property_names")]
    internal static partial int DocxCustomPropertyNames(DocxHandle d, out BufferHandle names);

    [LibraryImport(Library, EntryPoint = "uo_docx_set_custom_property")]
    internal static partial int DocxSetCustomProperty(DocxHandle d, byte* name, nuint nlen, byte* value, nuint vlen);

    // ───────────────────────────── operations ─────────────────────────────

    [LibraryImport(Library, EntryPoint = "uo_docx_run")]
    internal static partial int DocxRun(DocxHandle d, uint op);

    [LibraryImport(Library, EntryPoint = "uo_docx_sanitize")]
    internal static partial int DocxSanitize(DocxHandle d, UoSanitizeOptions* options);

    [LibraryImport(Library, EntryPoint = "uo_docx_sanitize_begin")]
    internal static partial int DocxSanitizeBegin(DocxHandle d, UoSanitizeOptions* options, out StageHandle stage);

    [LibraryImport(Library, EntryPoint = "uo_stage_inlines")]
    internal static partial int StageInlines(DocxHandle d, StageHandle stage, out BufferHandle inlines);

    [LibraryImport(Library, EntryPoint = "uo_stage_finish")]
    internal static partial int StageFinish(DocxHandle d, StageHandle stage, byte* verdicts, nuint n, out int unwrapped);

    [LibraryImport(Library, EntryPoint = "uo_stage_free")]
    internal static partial void StageFree(nint stage);

    [LibraryImport(Library, EntryPoint = "uo_docx_remove_unsafe_content")]
    internal static partial int DocxRemoveUnsafeContent(DocxHandle d, byte* schemes, nuint slen);

    [LibraryImport(Library, EntryPoint = "uo_docx_fields_replace_disallowed")]
    internal static partial int FieldsReplaceDisallowed(DocxHandle d, byte* allowed, nuint alen, byte* schemes, nuint slen, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_fields_convert_hyperlinks")]
    internal static partial int FieldsConvertHyperlinks(DocxHandle d, byte* schemes, nuint slen, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_fields_instructions")]
    internal static partial int FieldsInstructions(DocxHandle d, out BufferHandle list);

    [LibraryImport(Library, EntryPoint = "uo_docx_text_replace")]
    internal static partial int TextReplace(DocxHandle d, byte* find, nuint flen, byte* repl, nuint rlen, uint flags, uint scope, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_text_replace_parameter")]
    internal static partial int TextReplaceParameter(DocxHandle d, byte* name, nuint nlen, byte* value, nuint vlen, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_text_replace_parameters")]
    internal static partial int TextReplaceParameters(DocxHandle d, byte* pairs, nuint plen, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_text_contains")]
    internal static partial int TextContains(DocxHandle d, byte* find, nuint flen, uint flags, uint scope, out int has);

    [LibraryImport(Library, EntryPoint = "uo_docx_text_remove_paragraphs_containing")]
    internal static partial int TextRemoveParagraphsContaining(DocxHandle d, byte* texts, nuint tlen, uint flags, uint scope, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_text_plain")]
    internal static partial int TextPlain(DocxHandle d, int includeHeadersAndFooters, out BufferHandle text);

    [LibraryImport(Library, EntryPoint = "uo_docx_template_apply_content")]
    internal static partial int TemplateApplyContent(DocxHandle d, byte* content, nuint clen, UoApplyContentOptions* options);

    [LibraryImport(Library, EntryPoint = "uo_docx_template_import_body")]
    internal static partial int TemplateImportBody(DocxHandle target, DocxHandle source);

    [LibraryImport(Library, EntryPoint = "uo_docx_numbering_add_restarted")]
    internal static partial int NumberingAddRestarted(DocxHandle d, byte* numId, nuint nlen, out BufferHandle id);

    [LibraryImport(Library, EntryPoint = "uo_docx_pictures_fit_to_page")]
    internal static partial int PicturesFitToPage(DocxHandle d, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_samples_contains")]
    internal static partial int SamplesContains(DocxHandle d, byte* name, nuint nlen, out int has);

    // ───────────────────────────── views ─────────────────────────────

    [LibraryImport(Library, EntryPoint = "uo_docx_shapes_find_containing")]
    internal static partial int ShapesFindContaining(DocxHandle d, byte* text, nuint tlen, out BufferHandle views);

    [LibraryImport(Library, EntryPoint = "uo_docx_shapes_remove_containing")]
    internal static partial int ShapesRemoveContaining(DocxHandle d, byte* text, nuint tlen, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_shapes_replace_text_with_image")]
    internal static partial int ShapesReplaceTextWithImage(DocxHandle d, byte* text, nuint tlen, byte* img, nuint ilen, int hasW, float maxW, int hasH, float maxH, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_shapes_first_in")]
    internal static partial int ShapesFirstIn(DocxHandle d, int footer, uint type, out uint view, out int found);

    [LibraryImport(Library, EntryPoint = "uo_docx_shapes_clear_header_footer")]
    internal static partial int ShapesClearHeaderFooter(DocxHandle d, int footer, uint type, out int cleared);

    [LibraryImport(Library, EntryPoint = "uo_shape_text")]
    internal static partial int ShapeText(DocxHandle d, uint view, out BufferHandle text);

    [LibraryImport(Library, EntryPoint = "uo_shape_clear_text")]
    internal static partial int ShapeClearText(DocxHandle d, uint view);

    [LibraryImport(Library, EntryPoint = "uo_shape_set_fill_color")]
    internal static partial int ShapeSetFillColor(DocxHandle d, uint view, byte* rgb, nuint rlen);

    [LibraryImport(Library, EntryPoint = "uo_shape_set_fill_transparent")]
    internal static partial int ShapeSetFillTransparent(DocxHandle d, uint view);

    [LibraryImport(Library, EntryPoint = "uo_shape_set_text_color")]
    internal static partial int ShapeSetTextColor(DocxHandle d, uint view, byte* rgb, nuint rlen, int lastOnly);

    [LibraryImport(Library, EntryPoint = "uo_shape_remove")]
    internal static partial int ShapeRemove(DocxHandle d, uint view);

    [LibraryImport(Library, EntryPoint = "uo_docx_inlines_find")]
    internal static partial int InlinesFind(DocxHandle d, byte* type, nuint tlen, out BufferHandle list);

    [LibraryImport(Library, EntryPoint = "uo_docx_inlines_replace_text")]
    internal static partial int InlinesReplaceText(DocxHandle d, byte* marker, nuint mlen, byte* type, nuint tlen, byte* json, nuint jlen, byte* fallback, nuint flen, out int count);

    [LibraryImport(Library, EntryPoint = "uo_docx_inlines_unwrap_where")]
    internal static partial int InlinesUnwrapWhere(DocxHandle d, byte* records, nuint rlen, byte* verdicts, nuint n, out int count);

    [LibraryImport(Library, EntryPoint = "uo_inline_title")]
    internal static partial int InlineTitle(DocxHandle d, UoInlineRef* r, out BufferHandle title);

    [LibraryImport(Library, EntryPoint = "uo_inline_text")]
    internal static partial int InlineText(DocxHandle d, UoInlineRef* r, out BufferHandle text);

    [LibraryImport(Library, EntryPoint = "uo_inline_is_attached")]
    internal static partial int InlineIsAttached(DocxHandle d, UoInlineRef* r, out int attached);

    [LibraryImport(Library, EntryPoint = "uo_inline_replace_with_text")]
    internal static partial int InlineReplaceWithText(DocxHandle d, UoInlineRef* r, byte* text, nuint tlen);

    [LibraryImport(Library, EntryPoint = "uo_inline_replace_with_hyperlink")]
    internal static partial int InlineReplaceWithHyperlink(DocxHandle d, UoInlineRef* r, byte* url, nuint ulen, byte* text, nuint tlen, int style, byte* schemes, nuint slen, out int linked);

    [LibraryImport(Library, EntryPoint = "uo_inline_unwrap")]
    internal static partial int InlineUnwrap(DocxHandle d, UoInlineRef* r);

    // ───────────────────────────── writer, rows, samples ─────────────────────────────

    [LibraryImport(Library, EntryPoint = "uo_body_writer_new")]
    internal static partial int BodyWriterNew(DocxHandle d, UoBodyWriterOptions* options, out BodyWriterHandle writer);

    [LibraryImport(Library, EntryPoint = "uo_body_writer_free")]
    internal static partial void BodyWriterFree(nint writer);

    [LibraryImport(Library, EntryPoint = "uo_body_writer_line_is_empty")]
    internal static partial int BodyWriterLineIsEmpty(BodyWriterHandle w, out int empty);

    [LibraryImport(Library, EntryPoint = "uo_body_writer_add_text")]
    internal static partial int BodyWriterAddText(BodyWriterHandle w, DocxHandle d, byte* text, nuint tlen, UoRunFormat* format);

    [LibraryImport(Library, EntryPoint = "uo_body_writer_add_break")]
    internal static partial int BodyWriterAddBreak(BodyWriterHandle w, DocxHandle d);

    [LibraryImport(Library, EntryPoint = "uo_body_writer_add_image")]
    internal static partial int BodyWriterAddImage(BodyWriterHandle w, DocxHandle d, byte* img, nuint ilen, byte* link, nuint llen, out int written);

    [LibraryImport(Library, EntryPoint = "uo_body_writer_end_paragraph")]
    internal static partial int BodyWriterEndParagraph(BodyWriterHandle w, DocxHandle d, UoParagraphFormat* format);

    [LibraryImport(Library, EntryPoint = "uo_body_writer_finish")]
    internal static partial int BodyWriterFinish(BodyWriterHandle w, DocxHandle d);

    [LibraryImport(Library, EntryPoint = "uo_row_template_take")]
    internal static partial int RowTemplateTake(DocxHandle d, byte* marker, nuint mlen, out RowTemplateHandle template);

    [LibraryImport(Library, EntryPoint = "uo_row_template_append")]
    internal static partial int RowTemplateAppend(RowTemplateHandle t, DocxHandle d, byte* pairs, nuint plen, int removeRemaining);

    [LibraryImport(Library, EntryPoint = "uo_row_template_appended_rows")]
    internal static partial int RowTemplateAppendedRows(RowTemplateHandle t, out int rows);

    [LibraryImport(Library, EntryPoint = "uo_row_template_free")]
    internal static partial void RowTemplateFree(nint template);

    [LibraryImport(Library, EntryPoint = "uo_docx_samples_extract")]
    internal static partial int SamplesExtract(DocxHandle d, byte* names, nuint nlen, out SampleSetHandle set);

    [LibraryImport(Library, EntryPoint = "uo_sample_set_count")]
    internal static partial nuint SampleSetCount(SampleSetHandle s);

    [LibraryImport(Library, EntryPoint = "uo_sample_set_name")]
    internal static partial int SampleSetName(SampleSetHandle s, nuint i, out BufferHandle name);

    [LibraryImport(Library, EntryPoint = "uo_sample_set_take")]
    internal static partial int SampleSetTake(SampleSetHandle s, nuint i, out ParagraphPropsHandle props);

    [LibraryImport(Library, EntryPoint = "uo_sample_set_free")]
    internal static partial void SampleSetFree(nint set);

    [LibraryImport(Library, EntryPoint = "uo_paragraph_props_style_id")]
    internal static partial int ParagraphPropsStyleId(ParagraphPropsHandle p, out BufferHandle id);

    [LibraryImport(Library, EntryPoint = "uo_paragraph_props_num_id")]
    internal static partial int ParagraphPropsNumId(ParagraphPropsHandle p, out BufferHandle id);

    [LibraryImport(Library, EntryPoint = "uo_paragraph_props_free")]
    internal static partial void ParagraphPropsFree(nint props);

    // ───────────────────────────── pure ─────────────────────────────

    [LibraryImport(Library, EntryPoint = "uo_tag_try_parse")]
    internal static partial int TagTryParse(byte* tag, nuint tlen, out BufferHandle type, out BufferHandle json, out int ok);

    [LibraryImport(Library, EntryPoint = "uo_tag_format")]
    internal static partial int TagFormat(byte* type, nuint tlen, byte* json, nuint jlen, out BufferHandle tag);

    [LibraryImport(Library, EntryPoint = "uo_fields_is_allowed_url")]
    internal static partial int FieldsIsAllowedUrl(byte* url, nuint ulen, byte* schemes, nuint slen, out int allowed);

    [LibraryImport(Library, EntryPoint = "uo_fields_try_parse_hyperlink")]
    internal static partial int FieldsTryParseHyperlink(byte* instr, nuint ilen, byte* schemes, nuint slen, out int kind, out BufferHandle text);

    [LibraryImport(Library, EntryPoint = "uo_opc_resolve_relationship_target")]
    internal static partial int OpcResolveRelationshipTarget(byte* src, nuint slen, byte* target, nuint tlen, out BufferHandle name);

    [LibraryImport(Library, EntryPoint = "uo_opc_escape_segment")]
    internal static partial int OpcEscapeSegment(byte* segment, nuint len, out BufferHandle text);

    [LibraryImport(Library, EntryPoint = "uo_pictures_fit")]
    internal static partial int PicturesFit(long cx, long cy, int hasMx, long mx, int hasMy, long my, out long outCx, out long outCy);

    [LibraryImport(Library, EntryPoint = "uo_image_info_try_read")]
    internal static partial int ImageInfoTryRead(byte* data, nuint len, UoImageInfo* info, out BufferHandle strings, out int present);

    [LibraryImport(Library, EntryPoint = "uo_editing_defaults")]
    internal static partial int EditingDefaults(uint which, out BufferHandle list);

    [LibraryImport(Library, EntryPoint = "uo_docx_paragraph_style_name")]
    internal static partial int DocxParagraphStyleName(DocxHandle d, byte* styleId, nuint slen, out BufferHandle name);
}

/// <summary>A <c>UoDocx</c> (released by <c>uo_docx_free</c>).</summary>
internal sealed class DocxHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.DocxFree(handle); return true; }
}

/// <summary>A <c>UoBodyWriter</c>.</summary>
internal sealed class BodyWriterHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.BodyWriterFree(handle); return true; }
}

/// <summary>A <c>UoRowTemplate</c>.</summary>
internal sealed class RowTemplateHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.RowTemplateFree(handle); return true; }
}

/// <summary>A <c>UoSanitizeStage</c>.</summary>
internal sealed class StageHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.StageFree(handle); return true; }
}

/// <summary>A <c>UoSampleSet</c>.</summary>
internal sealed class SampleSetHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.SampleSetFree(handle); return true; }
}

/// <summary>A <c>UoParagraphProps</c>.</summary>
internal sealed class ParagraphPropsHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.ParagraphPropsFree(handle); return true; }
}
