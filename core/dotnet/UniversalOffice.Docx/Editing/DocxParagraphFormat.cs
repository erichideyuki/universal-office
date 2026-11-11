using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>Paragraph formatting of the content written by <see cref="DocxBodyWriter"/>.</summary>
public sealed record DocxParagraphFormat
{
    /// <summary>
    /// Properties copied as a starting point (e.g. the sample of the template read by
    /// <see cref="DocxParagraphSamples.Extract"/>); null = a paragraph without properties.
    /// The other options are applied on top.
    /// </summary>
    public DocxParagraphProperties? Base { get; init; }

    /// <summary>Paragraph style (w:pStyle); omitted when it is the document's default style.</summary>
    public string? StyleId { get; init; }

    /// <summary>List instance (w:numId); with <see cref="ListLevel"/> it forms the w:numPr.</summary>
    public string? ListNumId { get; init; }

    /// <summary>List level (w:ilvl), 0 to 8 (clamped outside that).</summary>
    public int ListLevel { get; init; }

    /// <summary>Left indent in twips (w:ind w:left), applied when greater than zero.</summary>
    public int LeftIndentTwips { get; init; }

    /// <summary>Alignment (w:jc): <c>left</c>, <c>center</c>, <c>right</c>, <c>both</c> or another ST_Jc value.</summary>
    public string? Alignment { get; init; }

    /// <summary>Keep with next (w:keepNext), as in headings.</summary>
    public bool KeepWithNext { get; init; }

    /// <summary>No formatting (the document's default style).</summary>
    public static DocxParagraphFormat None { get; } = new();
}

/// <summary>
/// An opaque copy of a paragraph's properties (w:pPr, without w:sectPr or w:pPrChange), to reuse the
/// formatting of a template paragraph in new paragraphs. It owns native memory: dispose it (or let
/// the finalizer do it).
/// </summary>
public sealed class DocxParagraphProperties : IDisposable
{
    private readonly ParagraphPropsHandle _handle;

    internal DocxParagraphProperties(ParagraphPropsHandle handle)
    {
        _handle = handle;
        EditingErrors.Check(NativeMethods.ParagraphPropsStyleId(handle, out var s));
        StyleId = s.TextOrNull();
        EditingErrors.Check(NativeMethods.ParagraphPropsNumId(handle, out var n));
        NumId = n.TextOrNull();
    }

    /// <summary>Paragraph style (w:pStyle), or null.</summary>
    public string? StyleId { get; }

    /// <summary>List instance (w:numPr/w:numId), or null.</summary>
    public string? NumId { get; }

    /// <summary>Holds the native handle's reference count for one native call (see <see cref="Release"/>).</summary>
    internal nint AddRef(ref bool added)
    {
        _handle.DangerousAddRef(ref added);
        return _handle.DangerousGetHandle();
    }

    internal void Release() => _handle.DangerousRelease();

    public void Dispose() => _handle.Dispose();
}
