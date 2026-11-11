using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>The header or footer of a section (the type of the w:headerReference/w:footerReference).</summary>
public enum DocxHeaderFooterType
{
    Default,
    First,
    Even,
}

/// <summary>
/// A shape / text box of the document (DrawingML <c>wps:wsp</c> and/or the VML fallback of the same
/// mc:AlternateContent — the operations apply to both). A view over the native document: it is
/// valid while the document is, and an operation after the shape was removed throws.
/// </summary>
public sealed unsafe class DocxShape
{
    private readonly DocxDocument _doc;
    private readonly uint _view;

    internal DocxShape(DocxDocument doc, uint view)
    {
        _doc = doc;
        _view = view;
    }

    /// <summary>The text of the box (first representation), paragraphs joined by CRLF.</summary>
    public string Text => _doc.With(h =>
    {
        EditingErrors.Check(NativeMethods.ShapeText(h, _view, out var b));
        return b.Text();
    });

    /// <summary>Empties the box's text (one empty paragraph is left).</summary>
    public void ClearText() => _doc.With(h => EditingErrors.Check(NativeMethods.ShapeClearText(h, _view)));

    /// <summary>Solid fill (hex "RRGGBB", "#RGB", rgb(…) or a CSS name).</summary>
    public void SetFillColor(string rgb)
    {
        ArgumentNullException.ThrowIfNull(rgb);
        using var a = Arg.Of(rgb);
        _doc.With(h => EditingErrors.Check(NativeMethods.ShapeSetFillColor(h, _view, a.Ptr, a.Len)));
    }

    public void SetFillTransparent() => _doc.With(h => EditingErrors.Check(NativeMethods.ShapeSetFillTransparent(h, _view)));

    /// <summary>Colour of the box's text (of the last paragraph only by default).</summary>
    public void SetTextColor(string rgb, bool lastParagraphOnly = true)
    {
        ArgumentNullException.ThrowIfNull(rgb);
        using var a = Arg.Of(rgb);
        _doc.With(h => EditingErrors.Check(NativeMethods.ShapeSetTextColor(h, _view, a.Ptr, a.Len, lastParagraphOnly ? 1 : 0)));
    }

    /// <summary>Removes the shape's run from the document.</summary>
    public void Remove() => _doc.With(h => EditingErrors.Check(NativeMethods.ShapeRemove(h, _view)));
}

/// <summary>Shapes, text boxes, headers and footers of a template (a logo in a text box, a coloured strip with the page number in the footer).</summary>
public static unsafe class DocxShapes
{
    /// <summary>
    /// Shapes (text boxes) whose text contains <paramref name="text"/> (case-sensitive, whole word),
    /// in document order — body, headers and footers.
    /// </summary>
    public static IReadOnlyList<DocxShape> FindContaining(DocxDocument doc, string text)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(text);
        using var t = Arg.Of(text);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.ShapesFindContaining(h, t.Ptr, t.Len, out var b));
            var r = b.Reader();
            int n = r.Count();
            var list = new List<DocxShape>(Math.Min(n, 1024));
            for (int i = 0; i < n; i++) list.Add(new DocxShape(doc, r.U32()));
            return (IReadOnlyList<DocxShape>)list;
        });
    }

    /// <summary>Removes the shapes whose text contains <paramref name="text"/>. Returns how many.</summary>
    public static int RemoveContaining(DocxDocument doc, string text)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(text);
        using var t = Arg.Of(text);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.ShapesRemoveContaining(h, t.Ptr, t.Len, out int n));
            return n;
        });
    }

    /// <summary>
    /// Replaces each occurrence of the text (e.g. <c>#{Logo}</c>, case-insensitive, whole word) by an
    /// inline picture of its physical size, shrunk to fit <paramref name="maxWidthPt"/> ×
    /// <paramref name="maxHeightPt"/> (points) when given. The image is stored once per part. Returns how many.
    /// </summary>
    public static int ReplaceTextWithImage(DocxDocument doc, string text, byte[] image, float? maxWidthPt = null, float? maxHeightPt = null)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(text);
        ArgumentNullException.ThrowIfNull(image);
        using var t = Arg.Of(text);
        using var i = Arg.Of(image);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.ShapesReplaceTextWithImage(h, t.Ptr, t.Len, i.Ptr, i.Len,
                maxWidthPt.HasValue ? 1 : 0, maxWidthPt ?? 0f, maxHeightPt.HasValue ? 1 : 0, maxHeightPt ?? 0f, out int n));
            return n;
        });
    }

    /// <summary>The first shape (text box / DrawingML or VML shape) of the header or footer of the LAST section, or null.</summary>
    public static DocxShape? FirstShapeIn(DocxDocument doc, bool footer, DocxHeaderFooterType type = DocxHeaderFooterType.Default)
    {
        ArgumentNullException.ThrowIfNull(doc);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.ShapesFirstIn(h, footer ? 1 : 0, (uint)type, out uint view, out int found));
            return found == 0 ? null : new DocxShape(doc, view);
        });
    }

    /// <summary>
    /// Empties the header or footer of the last section (one empty paragraph with the properties of
    /// the first is left). False when the section has no such header/footer.
    /// </summary>
    public static bool ClearHeaderFooter(DocxDocument doc, bool footer, DocxHeaderFooterType type = DocxHeaderFooterType.Default)
    {
        ArgumentNullException.ThrowIfNull(doc);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.ShapesClearHeaderFooter(h, footer ? 1 : 0, (uint)type, out int cleared));
            return cleared != 0;
        });
    }
}
