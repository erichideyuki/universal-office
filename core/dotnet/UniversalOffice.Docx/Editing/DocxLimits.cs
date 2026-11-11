using UniversalOffice.Core;

namespace UniversalOffice.Docx.Editing;

/// <summary>
/// Resource caps of a <see cref="DocxDocument"/> opened from untrusted input. Exceeding one throws
/// <see cref="OfficeLimitExceededException"/> — never a hang, a stack overflow or a reservation
/// sized by what the file declares. A negative cap counts as 0.
/// </summary>
public sealed record DocxLimits
{
    /// <summary>Decompression caps of the package (default <see cref="OpcPackageLimits.Default"/>).</summary>
    public OpcPackageLimits Package { get; init; } = OpcPackageLimits.Default;

    /// <summary>
    /// Maximum XML depth of each part, counted as the <see cref="System.Xml.XmlReader.Depth"/> of any
    /// node (root = 0). Default 256. Checked while the part is loaded, before the tree is built.
    /// </summary>
    public int MaxXmlDepth { get; init; } = 256;

    /// <summary>Complex fields (w:fldChar begin) per story. Default 100,000.</summary>
    public int MaxFieldsPerStory { get; init; } = 100_000;

    /// <summary>Complex fields open at once in a story (nesting, including those that never close). Default 64.</summary>
    public int MaxFieldNesting { get; init; } = 64;

    /// <summary>The default caps.</summary>
    public static DocxLimits Default { get; } = new();
}
