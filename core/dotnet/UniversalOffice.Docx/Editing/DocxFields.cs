using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>Fields (<c>w:fldChar</c> / <c>w:fldSimple</c>): allow lists, instructions and the HYPERLINK conversion.</summary>
public static unsafe class DocxFields
{
    // the default lists live in Rust, once (uo_editing_defaults)
    private static IReadOnlyList<string> Defaults(uint which)
    {
        EditingErrors.Check(NativeMethods.EditingDefaults(which, out var b));
        return b.StringList().AsReadOnly();
    }

    public static readonly IReadOnlyList<string> DefaultAllowedFields = Defaults(0);

    /// <summary>The wider list of a generated document (dates, section and NOTEREF too).</summary>
    public static readonly IReadOnlyList<string> DefaultOutputAllowedFields = Defaults(1);

    public static readonly IReadOnlyList<string> DefaultAllowedSchemes = Defaults(2);

    /// <summary>The instruction of a field: its name (first word, upper-cased) and the whole text.</summary>
    public readonly record struct FieldInstruction(string Name, string Text);

    /// <summary>
    /// Replaces the fields outside <paramref name="allowedFields"/> by their cached result, and the
    /// HYPERLINK fields that are not an internal anchor or an absolute URL with an allowed scheme.
    /// Returns how many were replaced.
    /// </summary>
    public static int ReplaceDisallowedWithResult(DocxDocument doc, IEnumerable<string>? allowedFields = null, IEnumerable<string>? allowedSchemes = null)
    {
        ArgumentNullException.ThrowIfNull(doc);
        using var f = Arg.List(allowedFields, nameof(allowedFields));
        using var s = Arg.List(allowedSchemes, nameof(allowedSchemes));
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.FieldsReplaceDisallowed(h, f.Ptr, f.Len, s.Ptr, s.Len, out int n));
            return n;
        });
    }

    /// <summary>Parses a HYPERLINK instruction: an internal anchor or an absolute URL with an allowed scheme.</summary>
    public static bool TryParseHyperlink(string instruction, IEnumerable<string> allowedSchemes, out string? url, out string? anchor)
    {
        ArgumentNullException.ThrowIfNull(instruction);
        ArgumentNullException.ThrowIfNull(allowedSchemes);
        url = anchor = null;
        using var i = Arg.Of(instruction);
        using var s = Arg.List(allowedSchemes, nameof(allowedSchemes));
        EditingErrors.Check(NativeMethods.FieldsTryParseHyperlink(i.Ptr, i.Len, s.Ptr, s.Len, out int kind, out var text));
        string? t = text.TextOrNull();
        if (kind == 1) url = t;
        else if (kind == 2) anchor = t;
        return kind != 0;
    }

    /// <summary>An absolute URL whose scheme is in the list.</summary>
    public static bool IsAllowedUrl(string url, IEnumerable<string> allowedSchemes)
    {
        ArgumentNullException.ThrowIfNull(url);
        ArgumentNullException.ThrowIfNull(allowedSchemes);
        using var u = Arg.Of(url);
        using var s = Arg.List(allowedSchemes, nameof(allowedSchemes));
        EditingErrors.Check(NativeMethods.FieldsIsAllowedUrl(u.Ptr, u.Len, s.Ptr, s.Len, out int ok));
        return ok != 0;
    }

    /// <summary>Converts allowed HYPERLINK fields into native <c>w:hyperlink</c>. Returns how many.</summary>
    public static int ConvertHyperlinkFields(DocxDocument doc, IEnumerable<string>? allowedSchemes = null)
    {
        ArgumentNullException.ThrowIfNull(doc);
        using var s = Arg.List(allowedSchemes, nameof(allowedSchemes));
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.FieldsConvertHyperlinks(h, s.Ptr, s.Len, out int n));
            return n;
        });
    }

    /// <summary>The instructions of every field of every story, in document order.</summary>
    public static IReadOnlyList<FieldInstruction> GetInstructions(DocxDocument doc)
    {
        ArgumentNullException.ThrowIfNull(doc);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.FieldsInstructions(h, out var b));
            var r = b.Reader();
            int n = r.Count();
            var list = new List<FieldInstruction>(Math.Min(n, 1024));
            for (int i = 0; i < n; i++) list.Add(new FieldInstruction(r.Str(), r.Str()));
            return (IReadOnlyList<FieldInstruction>)list;
        });
    }
}
