using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>
/// Sample paragraphs of a template: a <c>#{Name}</c> marker whose paragraph's formatting is the
/// mold for content written with <see cref="DocxBodyWriter"/>.
/// </summary>
public static unsafe class DocxParagraphSamples
{
    /// <summary>Does the document have the marker <c>#{name}</c> (whole word, case-sensitive)?</summary>
    public static bool Contains(DocxDocument document, string name)
    {
        ArgumentNullException.ThrowIfNull(document);
        ArgumentNullException.ThrowIfNull(name);
        using var n = Arg.Of(name);
        return document.With(h =>
        {
            EditingErrors.Check(NativeMethods.SamplesContains(h, n.Ptr, n.Len, out int has));
            return has != 0;
        });
    }

    /// <summary>
    /// Reads and REMOVES the samples: for each name, keeps the properties of the FIRST paragraph with
    /// the marker <c>#{name}</c> (whole word, case-sensitive, in body, text boxes, headers, footers
    /// and notes) and deletes every paragraph that holds it. Names without a sample are left out.
    /// </summary>
    public static IReadOnlyDictionary<string, DocxParagraphProperties> Extract(DocxDocument document, IEnumerable<string> names)
    {
        ArgumentNullException.ThrowIfNull(document);
        ArgumentNullException.ThrowIfNull(names);
        using var n = Arg.List(names, nameof(names));
        return document.With(h =>
        {
            EditingErrors.Check(NativeMethods.SamplesExtract(h, n.Ptr, n.Len, out var set));
            using (set)
            {
                var result = new Dictionary<string, DocxParagraphProperties>(StringComparer.Ordinal);
                nuint count = NativeMethods.SampleSetCount(set);
                for (nuint i = 0; i < count; i++)
                {
                    EditingErrors.Check(NativeMethods.SampleSetName(set, i, out var name));
                    EditingErrors.Check(NativeMethods.SampleSetTake(set, i, out var props));
                    result[name.Text()] = new DocxParagraphProperties(props);
                }
                return (IReadOnlyDictionary<string, DocxParagraphProperties>)result;
            }
        });
    }
}
