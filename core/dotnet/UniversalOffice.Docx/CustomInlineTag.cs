using UniversalOffice.Native;

namespace UniversalOffice.Docx;

/// <summary>
/// The plugin-inline contract in the .docx (a port of docx-core serialize.ts / document.ts): a
/// <c>w:sdt</c> whose <c>w:tag</c> is <c>wordnext:inline:&lt;type&gt;:&lt;json with encodeURIComponent&gt;</c>
/// and whose content is the fallback text. Public for those who read or write these controls with
/// another library (a server host) without repeating the format.
/// </summary>
public static unsafe class CustomInlineTag
{
    public const string Prefix = "wordnext:inline:";

    /// <summary>The type and JSON payload, or false if the w:tag is not a plugin inline (or the JSON is not valid).</summary>
    public static bool TryParse(string? tag, out string type, out string json)
    {
        using var t = Arg.Of(tag);
        EditingErrors.Check(NativeMethods.TagTryParse(t.Ptr, t.Len, out var ty, out var js, out int ok));
        string? a = ty.TextOrNull(), b = js.TextOrNull();
        type = a ?? "";
        json = b ?? "";
        return ok != 0;
    }

    /// <summary>The w:tag of an inline of type <paramref name="type"/> with the JSON payload <paramref name="json"/>.</summary>
    public static string Format(string type, string json)
    {
        ArgumentNullException.ThrowIfNull(type);
        ArgumentNullException.ThrowIfNull(json);
        using var t = Arg.Of(type);
        using var j = Arg.Of(json);
        EditingErrors.Check(NativeMethods.TagFormat(t.Ptr, t.Len, j.Ptr, j.Len, out var b));
        return b.Text();
    }
}
