using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>Document properties: core.xml (title, author, dates…), app.xml (company, application) and custom properties.</summary>
public sealed unsafe class DocxProperties
{
    private readonly DocxDocument _doc;

    internal DocxProperties(DocxDocument doc) => _doc = doc;

    public string? Title { get => Get(0); set => Set(0, value); }
    public string? Subject { get => Get(1); set => Set(1, value); }
    public string? Author { get => Get(2); set => Set(2, value); }
    public string? Keywords { get => Get(3); set => Set(3, value); }
    public string? Comments { get => Get(4); set => Set(4, value); }
    public string? LastAuthor { get => Get(5); set => Set(5, value); }
    public string? Category { get => Get(6); set => Set(6, value); }
    public string? Company { get => Get(7); set => Set(7, value); }
    public string? Manager { get => Get(8); set => Set(8, value); }

    /// <summary>The application that saved (app.xml Application).</summary>
    public string? ApplicationName { get => Get(9); set => Set(9, value); }

    /// <summary>Date of the last save (dcterms:modified, UTC).</summary>
    public DateTime? LastSaveDate { get => GetDate(0); set => SetDate(0, value); }

    public DateTime? CreatedDate { get => GetDate(1); set => SetDate(1, value); }

    private string? Get(uint which) => _doc.With(h =>
    {
        EditingErrors.Check(NativeMethods.DocxPropGet(h, which, out var b));
        return b.TextOrNull();
    });

    private void Set(uint which, string? value)
    {
        using var a = Arg.Of(value);
        _doc.With(h => EditingErrors.Check(NativeMethods.DocxPropSet(h, which, a.Ptr, a.Len)));
    }

    private DateTime? GetDate(uint which) => _doc.With(h =>
    {
        NativeMethods.UoDateTime d;
        EditingErrors.Check(NativeMethods.DocxPropGetDate(h, which, &d, out int present));
        if (present == 0) return (DateTime?)null;
        return new DateTime((int)d.Year, (int)d.Month, (int)d.Day, (int)d.Hour, (int)d.Minute, (int)d.Second, DateTimeKind.Utc).AddTicks(d.Ticks);
    });

    private void SetDate(uint which, DateTime? value)
    {
        _doc.With(h =>
        {
            if (value is null)
            {
                EditingErrors.Check(NativeMethods.DocxPropSetDate(h, which, null));
                return;
            }
            // a Local date is converted to UTC, as the 0.9 setter did; Unspecified is written as it is
            var utc = value.Value.Kind == DateTimeKind.Local ? value.Value.ToUniversalTime() : value.Value;
            var d = new NativeMethods.UoDateTime
            {
                Year = utc.Year,
                Month = (uint)utc.Month,
                Day = (uint)utc.Day,
                Hour = (uint)utc.Hour,
                Minute = (uint)utc.Minute,
                Second = (uint)utc.Second,
                Ticks = (uint)(utc.Ticks % TimeSpan.TicksPerSecond),
            };
            EditingErrors.Check(NativeMethods.DocxPropSetDate(h, which, &d));
        });
    }

    /// <summary>Names of the custom properties.</summary>
    public IReadOnlyList<string> CustomPropertyNames => _doc.With(h =>
    {
        EditingErrors.Check(NativeMethods.DocxCustomPropertyNames(h, out var b));
        return (IReadOnlyList<string>)b.StringList();
    });

    /// <summary>A text custom property, replacing one of the same name.</summary>
    public void SetCustomProperty(string name, string value)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(value);
        using var n = Arg.Of(name);
        using var v = Arg.Of(value);
        _doc.With(h => EditingErrors.Check(NativeMethods.DocxSetCustomProperty(h, n.Ptr, n.Len, v.Ptr, v.Len)));
    }

    /// <summary>Removes every custom property (and the part).</summary>
    public void RemoveCustomProperties() => DocxOps.Run(_doc, 10);

    /// <summary>Clears author, company, title… and the custom properties.</summary>
    public void RemovePersonalMetadata() => DocxOps.Run(_doc, 11);
}

/// <summary>The no-argument document operations (one export, <c>uo_docx_run</c>).</summary>
internal static class DocxOps
{
    internal static void Run(DocxDocument doc, uint op)
    {
        ArgumentNullException.ThrowIfNull(doc);
        doc.With(h => EditingErrors.Check(NativeMethods.DocxRun(h, op)));
    }
}
