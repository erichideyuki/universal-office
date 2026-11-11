using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>
/// A table row taken out of a template to be repeated: <see cref="Take"/> removes the row that holds
/// a marker, <see cref="Append"/> adds copies of it at the end of its table. It belongs to ONE document.
/// </summary>
public sealed unsafe class DocxTableRowTemplate : IDisposable
{
    private readonly DocxDocument _doc;
    private readonly RowTemplateHandle _handle;

    private DocxTableRowTemplate(DocxDocument doc, RowTemplateHandle handle)
    {
        _doc = doc;
        _handle = handle;
    }

    /// <summary>Rows appended so far.</summary>
    public int AppendedRows
    {
        get
        {
            _doc.ThrowIfDisposed();
            lock (_doc.Gate)
            {
                EditingErrors.Check(NativeMethods.RowTemplateAppendedRows(_handle, out int n));
                return n;
            }
        }
    }

    /// <summary>
    /// Takes the row of the first cell paragraph holding <paramref name="marker"/> (case-sensitive,
    /// whole word; body first, then headers and footers) out of its table; null when the marker is not in a cell.
    /// </summary>
    public static DocxTableRowTemplate? Take(DocxDocument doc, string marker)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(marker);
        using var m = Arg.Of(marker);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.RowTemplateTake(h, m.Ptr, m.Len, out var t));
            return t.IsInvalid ? null : new DocxTableRowTemplate(doc, t);
        });
    }

    /// <summary>
    /// Appends a copy of the row at the end of the table, replacing in each paragraph the parameters
    /// <c>#{key}</c> (case-insensitive, whole word, in the order given; null value = empty) and, with
    /// <paramref name="removeRemainingParameters"/>, erasing the <c>#{...}</c> left.
    /// </summary>
    public void Append(IEnumerable<KeyValuePair<string, string?>> parameters, bool removeRemainingParameters = true)
    {
        ArgumentNullException.ThrowIfNull(parameters);
        using var p = Arg.Pairs(parameters, nameof(parameters));
        _doc.With(h => EditingErrors.Check(NativeMethods.RowTemplateAppend(_handle, h, p.Ptr, p.Len, removeRemainingParameters ? 1 : 0)));
    }

    public void Dispose() => _handle.Dispose();
}
