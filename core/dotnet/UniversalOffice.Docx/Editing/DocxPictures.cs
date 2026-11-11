using System.Globalization;
using System.Xml.Linq;
using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>The usable area of a section's page, in twips.</summary>
public readonly record struct DocxPageArea(int PageWidth, int PageHeight, int MarginLeft, int MarginRight, int MarginTop, int MarginBottom, int Gutter)
{
    private static readonly XNamespace W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    public int ClientWidth => Math.Max(0, PageWidth - MarginLeft - MarginRight - Gutter);
    public int ClientHeight => Math.Max(0, PageHeight - Math.Abs(MarginTop) - Math.Abs(MarginBottom));

    /// <summary>The area of the section (Word's default, Letter with 1" margins, when the sectPr does not say).</summary>
    public static DocxPageArea From(XElement? sectPr)
    {
        var size = sectPr?.Element(W + "pgSz");
        var mar = sectPr?.Element(W + "pgMar");
        return new DocxPageArea(
            Twips(size, W + "w", 12240), Twips(size, W + "h", 15840),
            Twips(mar, W + "left", 1440), Twips(mar, W + "right", 1440),
            Twips(mar, W + "top", 1440), Twips(mar, W + "bottom", 1440),
            Twips(mar, W + "gutter", 0));
    }

    private static int Twips(XElement? el, XName attr, int fallback) =>
        int.TryParse((string?)el?.Attribute(attr), NumberStyles.Integer, CultureInfo.InvariantCulture, out var v) ? v : fallback;
}

/// <summary>Pictures: shrink the ones that do not fit the page.</summary>
public static class DocxPictures
{
    public const long EmuPerTwip = 635;
    public const long EmuPerPoint = 12700;

    /// <summary>The size scaled down to fit <paramref name="maxCx"/> × <paramref name="maxCy"/>, keeping the proportion.</summary>
    public static (long Cx, long Cy) Fit(long cx, long cy, long? maxCx, long? maxCy)
    {
        EditingErrors.Check(NativeMethods.PicturesFit(cx, cy, maxCx.HasValue ? 1 : 0, maxCx ?? 0, maxCy.HasValue ? 1 : 0, maxCy ?? 0, out long a, out long b));
        return (a, b);
    }

    /// <summary>Shrinks the body pictures that do not fit the page (or the column). Returns how many.</summary>
    public static int FitToPage(DocxDocument doc)
    {
        ArgumentNullException.ThrowIfNull(doc);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.PicturesFitToPage(h, out int n));
            return n;
        });
    }
}
