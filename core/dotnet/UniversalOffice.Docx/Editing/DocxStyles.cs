using System.Xml.Linq;
using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>Styles (styles.xml).</summary>
public static unsafe class DocxStyles
{
    private static readonly XNamespace W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    /// <summary>
    /// The display name (w:name) of the effective paragraph style of a paragraph the host holds as an
    /// <see cref="XElement"/>: its own w:pStyle is read here, and the document's styles are resolved in the Rust core.
    /// </summary>
    public static string? ParagraphStyleName(DocxDocument doc, XElement paragraph)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(paragraph);
        string? id = (string?)paragraph.Element(W + "pPr")?.Element(W + "pStyle")?.Attribute(W + "val");
        using var s = Arg.Of(id);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.DocxParagraphStyleName(h, s.Ptr, s.Len, out var b));
            return b.TextOrNull();
        });
    }
}
