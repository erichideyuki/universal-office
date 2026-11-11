using System.Text.RegularExpressions;
using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>Where to look for text.</summary>
public enum DocxTextScope
{
    /// <summary>Body, headers, footers and notes.</summary>
    Document,

    /// <summary>The body only (main part, tables and text boxes included).</summary>
    Body,
}

/// <summary>
/// Searching, replacing and extracting text. The search is per paragraph (as in Word): the text may
/// be split across runs; a replacement inherits the formatting of the first character found.
/// </summary>
public static unsafe class DocxText
{
    /// <summary>The template parameter form <c>#{Name}</c>.</summary>
    public static string Parameter(string name) => "#{" + name + "}";

    /// <summary>Any <c>#{Name}</c> parameter (the name is letters and digits).</summary>
    public static readonly Regex AnyParameter = new(@"#\{[A-Za-z0-9]+\}", RegexOptions.CultureInvariant);

    private static uint Flags(bool caseSensitive, bool wholeWord) => (caseSensitive ? 1u : 0u) | (wholeWord ? 2u : 0u);

    /// <summary>Replaces every occurrence of <paramref name="find"/> (null replacement = empty). Returns how many.</summary>
    public static int Replace(DocxDocument doc, string find, string? replacement, bool caseSensitive = false, bool wholeWord = true, DocxTextScope scope = DocxTextScope.Document)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(find);
        using var f = Arg.Of(find);
        using var r = Arg.Of(replacement);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.TextReplace(h, f.Ptr, f.Len, r.Ptr, r.Len, Flags(caseSensitive, wholeWord), (uint)scope, out int n));
            return n;
        });
    }

    /// <summary>Replaces the parameter <c>#{name}</c> (case-insensitive, whole word).</summary>
    public static int ReplaceParameter(DocxDocument doc, string name, string? value)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(name);
        using var n = Arg.Of(name);
        using var v = Arg.Of(value);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.TextReplaceParameter(h, n.Ptr, n.Len, v.Ptr, v.Len, out int c));
            return c;
        });
    }

    /// <summary>Replaces several parameters, in order.</summary>
    public static int ReplaceParameters(DocxDocument doc, IEnumerable<KeyValuePair<string, string?>> parameters)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(parameters);
        using var p = Arg.Pairs(parameters, nameof(parameters));
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.TextReplaceParameters(h, p.Ptr, p.Len, out int c));
            return c;
        });
    }

    /// <summary>Does the text occur? (case-sensitive and whole word by default).</summary>
    public static bool Contains(DocxDocument doc, string find, bool caseSensitive = true, bool wholeWord = true, DocxTextScope scope = DocxTextScope.Document)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(find);
        using var f = Arg.Of(find);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.TextContains(h, f.Ptr, f.Len, Flags(caseSensitive, wholeWord), (uint)scope, out int has));
            return has != 0;
        });
    }

    /// <summary>
    /// Removes the paragraphs containing any of the texts (the only block of a cell or text box is
    /// emptied instead). Returns how many.
    /// </summary>
    public static int RemoveParagraphsContaining(DocxDocument doc, IEnumerable<string> texts, bool caseSensitive = true, bool wholeWord = true, DocxTextScope scope = DocxTextScope.Document)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(texts);
        using var t = Arg.List(texts, nameof(texts));
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.TextRemoveParagraphsContaining(h, t.Ptr, t.Len, Flags(caseSensitive, wholeWord), (uint)scope, out int n));
            return n;
        });
    }

    /// <summary>The plain text of the document (paragraphs separated by line breaks).</summary>
    public static string GetPlainText(DocxDocument doc, bool includeHeadersAndFooters = false)
    {
        ArgumentNullException.ThrowIfNull(doc);
        return doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.TextPlain(h, includeHeadersAndFooters ? 1 : 0, out var b));
            return b.Text();
        });
    }
}
