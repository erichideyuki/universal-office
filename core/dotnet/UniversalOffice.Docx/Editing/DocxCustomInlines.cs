using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>
/// A plugin inline of the document (<see cref="CustomInlineTag"/>): a view over the native
/// document, valid while the document is. <see cref="Type"/>, <see cref="Json"/> and
/// <see cref="PartPath"/> are read when it is found.
/// </summary>
public sealed unsafe class DocxCustomInline
{
    private readonly DocxDocument _doc;
    private readonly uint _view;

    internal DocxCustomInline(DocxDocument doc, uint view, string type, string json, string part)
    {
        _doc = doc;
        _view = view;
        Type = type;
        Json = json;
        PartPath = part;
    }

    /// <summary>The inline's type (the tag's first segment).</summary>
    public string Type { get; }

    /// <summary>The decoded JSON payload.</summary>
    public string Json { get; }

    /// <summary>The part the control is in (<c>word/document.xml</c>, a header…).</summary>
    public string PartPath { get; }

    internal uint View => _view;

    private delegate T InlineCall<T>(DocxHandle h, NativeMethods.UoInlineRef* r);

    // the native struct for this inline is built inside the lock; the three strings stay pinned for the call
    private T Call<T>(InlineCall<T> body)
    {
        using var t = Arg.Of(Type);
        using var j = Arg.Of(Json);
        using var p = Arg.Of(PartPath);
        return _doc.With(h =>
        {
            var r = new NativeMethods.UoInlineRef { Size = (uint)sizeof(NativeMethods.UoInlineRef), View = _view, Type = t.Str, Json = j.Str, Part = p.Str };
            return body(h, &r);
        });
    }

    /// <summary>The control's title (w:alias), or null.</summary>
    public string? Title => Call((h, r) =>
    {
        EditingErrors.Check(NativeMethods.InlineTitle(h, r, out var b));
        return b.TextOrNull();
    });

    /// <summary>The fallback text of the control.</summary>
    public string Text => Call((h, r) =>
    {
        EditingErrors.Check(NativeMethods.InlineText(h, r, out var b));
        return b.Text();
    });

    /// <summary>False once the control was replaced or removed.</summary>
    public bool IsAttached => Call((h, r) =>
    {
        EditingErrors.Check(NativeMethods.InlineIsAttached(h, r, out int a));
        return a != 0;
    });

    /// <summary>Replaces the control by a run with the text (in the formatting of the content's first run); empty text removes it.</summary>
    public void ReplaceWithText(string text)
    {
        ArgumentNullException.ThrowIfNull(text);
        using var t = Arg.Of(text);
        Call((h, r) =>
        {
            EditingErrors.Check(NativeMethods.InlineReplaceWithText(h, r, t.Ptr, t.Len));
            return 0;
        });
    }

    /// <summary>
    /// Replaces the control by a NATIVE hyperlink (<c>w:hyperlink</c> + external relationship). A URL
    /// that is not absolute with an allowed scheme (http, https, mailto) becomes plain text and
    /// returns false.
    /// </summary>
    public bool ReplaceWithHyperlink(string url, string text, bool applyHyperlinkStyle = true, IEnumerable<string>? allowedSchemes = null)
    {
        ArgumentNullException.ThrowIfNull(url);
        ArgumentNullException.ThrowIfNull(text);
        using var u = Arg.Of(url);
        using var t = Arg.Of(text);
        using var s = Arg.List(allowedSchemes, nameof(allowedSchemes));
        return Call((h, r) =>
        {
            EditingErrors.Check(NativeMethods.InlineReplaceWithHyperlink(h, r, u.Ptr, u.Len, t.Ptr, t.Len, applyHyperlinkStyle ? 1 : 0, s.Ptr, s.Len, out int linked));
            return linked != 0;
        });
    }

    /// <summary>Removes the control and leaves its content (the fallback text) in place.</summary>
    public void Unwrap() => Call((h, r) =>
    {
        EditingErrors.Check(NativeMethods.InlineUnwrap(h, r));
        return 0;
    });
}

/// <summary>Finding and creating plugin inlines (<see cref="CustomInlineTag"/>) in a .docx.</summary>
public static unsafe class DocxCustomInlines
{
    private static DocxCustomInline Read(DocxDocument doc, WireReader r) => new(doc, r.U32(), r.Str(), r.Str(), r.Str());

    /// <summary>
    /// The plugin inlines of every part with text (body, headers, footers, notes), in document order;
    /// <paramref name="type"/> filters by type. Controls whose w:tag is outside the contract (or whose JSON is invalid) are skipped.
    /// </summary>
    public static IReadOnlyList<DocxCustomInline> Find(DocxDocument doc, string? type = null)
    {
        ArgumentNullException.ThrowIfNull(doc);
        using var t = Arg.Of(type);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.InlinesFind(h, t.Ptr, t.Len, out var b));
            var r = b.Reader();
            int n = r.Count();
            var list = new List<DocxCustomInline>(Math.Min(n, 1024));
            for (int i = 0; i < n; i++) list.Add(Read(doc, r));
            return (IReadOnlyList<DocxCustomInline>)list;
        });
    }

    /// <summary>
    /// Replaces each occurrence of the text <paramref name="marker"/> (even split across runs) by a plugin
    /// inline with the given fallback text, in the formatting of the occurrence's first run. Returns how many.
    /// </summary>
    public static int ReplaceText(DocxDocument doc, string marker, string type, string json, string fallbackText)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(marker);
        ArgumentNullException.ThrowIfNull(type);
        ArgumentNullException.ThrowIfNull(json);
        ArgumentNullException.ThrowIfNull(fallbackText);
        using var m = Arg.Of(marker);
        using var t = Arg.Of(type);
        using var j = Arg.Of(json);
        using var f = Arg.Of(fallbackText);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.InlinesReplaceText(h, m.Ptr, m.Len, t.Ptr, t.Len, j.Ptr, j.Len, f.Ptr, f.Len, out int n));
            return n;
        });
    }

    /// <summary>Unwraps the inlines for which <paramref name="isValid"/> returns false (the fallback content stays). Returns how many.</summary>
    public static int UnwrapInvalid(DocxDocument doc, Func<string, string, bool> isValid)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(isValid);
        var inlines = Find(doc);
        var verdicts = new byte[inlines.Count];
        for (int i = 0; i < verdicts.Length; i++) verdicts[i] = isValid(inlines[i].Type, inlines[i].Json) ? (byte)1 : (byte)0;
        var w = new WireWriter();
        w.Count(inlines.Count);
        // the same records Find returned, as the views the document holds
        foreach (var inline in inlines)
        {
            w.U32(ViewOf(inline));
            w.Str(inline.Type);
            w.Str(inline.Json);
            w.Str(inline.PartPath);
        }
        using var records = Arg.Of(w.ToArray());
        using var v = Arg.Of(verdicts);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.InlinesUnwrapWhere(h, records.Ptr, records.Len, v.Ptr, v.Len, out int n));
            return n;
        });
    }

    private static uint ViewOf(DocxCustomInline inline) => inline.View;
}
