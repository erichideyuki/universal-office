namespace UniversalOffice.Core;

/// <summary>The limit for untrusted input that was exceeded (see <see cref="OfficeLimitExceededException"/>).</summary>
public enum OfficeLimit
{
    /// <summary>Uncompressed bytes of one package part (<see cref="OpcPackageLimits.MaxPartBytes"/>).</summary>
    PartBytes,

    /// <summary>Uncompressed bytes of all parts (<see cref="OpcPackageLimits.MaxTotalBytes"/>).</summary>
    TotalBytes,

    /// <summary>Nesting depth of the XML of a part.</summary>
    XmlDepth,

    /// <summary>Complex fields (w:fldChar begin) in a story.</summary>
    FieldsPerStory,

    /// <summary>Complex fields open at once (nested) in a story.</summary>
    FieldNesting,

    /// <summary>Entries of the archive (a cap of the Rust core; the C# 0.9 had none).</summary>
    Entries,

    /// <summary>Elements of one XML part (a cap of the Rust core; the C# 0.9 had none).</summary>
    XmlElements,

    /// <summary>A limit of the document-to-PDF path (reader, layout or PDF), whose kind that ABI does not report.</summary>
    Conversion,
}

/// <summary>
/// Input refused for exceeding a resource cap (uncompressed size, XML depth, number of fields…).
/// A type of its own so a host can tell "too big or abusive" from "corrupt file"
/// (<see cref="InvalidDataException"/>, <see cref="System.Xml.XmlException"/>).
/// </summary>
public class OfficeLimitExceededException : UniversalOfficeException
{
    public OfficeLimitExceededException(OfficeLimit limit, long maximum, string? partName)
        : this($"Limit {limit} exceeded (maximum {maximum}){(partName is null ? "" : $" in {partName}")}", limit, maximum, partName, "")
    {
    }

    internal OfficeLimitExceededException(string message, OfficeLimit limit, long maximum, string? partName, string stage)
        : base(message, 5, stage)
    {
        Limit = limit;
        Maximum = maximum;
        PartName = partName;
    }

    /// <summary>Which cap was exceeded.</summary>
    public OfficeLimit Limit { get; }

    /// <summary>The configured maximum (0 when the limit does not say).</summary>
    public long Maximum { get; }

    /// <summary>The part where the cap was exceeded (null when it applies to the whole package).</summary>
    public string? PartName { get; }
}

/// <summary>
/// Caps applied when decompressing an OPC package. They count the bytes REALLY read from each
/// entry (the size declared in the zip directory is never used). Exceeding one throws
/// <see cref="OfficeLimitExceededException"/>. Null = no cap.
/// </summary>
public sealed record OpcPackageLimits
{
    /// <summary>Uncompressed bytes of one part (default 256 MiB).</summary>
    public long? MaxPartBytes { get; init; } = 256L * 1024 * 1024;

    /// <summary>Uncompressed bytes of all parts together (default 512 MiB).</summary>
    public long? MaxTotalBytes { get; init; } = 512L * 1024 * 1024;

    /// <summary>The default caps (256 MiB per part, 512 MiB total).</summary>
    public static OpcPackageLimits Default { get; } = new();

    /// <summary>No caps (trusted input only).</summary>
    public static OpcPackageLimits None { get; } = new() { MaxPartBytes = null, MaxTotalBytes = null };
}
