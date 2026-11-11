using UniversalOffice.Native;

namespace UniversalOffice.Core;

/// <summary>
/// OPC part names in the form of the zip entry names (no leading slash, already unescaped). The ONE rule that
/// resolves a relationship target, shared with the Rust core (<c>uo_opc::part_name</c>): a host validator must use
/// this method, so that "the part the validator checked" and "the part UniversalOffice opens" are always the same.
/// </summary>
public static unsafe class OpcPartName
{
    /// <summary>
    /// Resolves the <c>Target</c> of an INTERNAL relationship to the name of the target part, or null when the target
    /// names no part (empty, a scheme, a query or backslash, <c>..</c> above the root, a malformed <c>%</c> escape, or
    /// a segment that unescapes to empty, <c>.</c>, <c>..</c> or contains a separator). Segments are unescaped ONCE.
    /// The name is compared with the zip names case-insensitively and must not be unescaped again.
    /// </summary>
    /// <param name="sourcePartName">The part that owns the .rels ("" for <c>_rels/.rels</c>); a leading slash is accepted.</param>
    /// <param name="target">The <c>Target</c> attribute.</param>
    public static string? ResolveRelationshipTarget(string sourcePartName, string target)
    {
        ArgumentNullException.ThrowIfNull(sourcePartName);
        ArgumentNullException.ThrowIfNull(target);
        using var s = Arg.Of(sourcePartName);
        using var t = Arg.Of(target);
        EditingErrors.Check(NativeMethods.OpcResolveRelationshipTarget(s.Ptr, s.Len, t.Ptr, t.Len, out var name));
        return name.TextOrNull();
    }

    /// <summary>A part-name segment escaped for a relationship <c>Target</c> (the inverse of
    /// <see cref="ResolveRelationshipTarget"/>: it returns the original text).</summary>
    public static string EscapeSegment(string segment)
    {
        ArgumentNullException.ThrowIfNull(segment);
        using var s = Arg.Of(segment);
        EditingErrors.Check(NativeMethods.OpcEscapeSegment(s.Ptr, s.Len, out var text));
        return text.Text();
    }
}
