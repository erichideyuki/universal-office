using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>Lists (numbering.xml).</summary>
public static unsafe class DocxNumbering
{
    /// <summary>
    /// A new list instance (w:num) of the same definition as <paramref name="numId"/> that restarts
    /// at 1; returns its numId, or null when <paramref name="numId"/> does not exist.
    /// </summary>
    public static string? AddRestartedInstance(DocxDocument doc, string numId)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(numId);
        using var n = Arg.Of(numId);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.NumberingAddRestarted(h, n.Ptr, n.Len, out var b));
            return b.TextOrNull();
        });
    }
}
