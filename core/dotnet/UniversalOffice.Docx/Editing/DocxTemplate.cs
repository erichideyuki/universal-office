using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>How <see cref="DocxTemplate.ApplyContent"/> applies the content to the template.</summary>
public sealed record DocxApplyContentOptions
{
    /// <summary>
    /// Texts that mark sample paragraphs of the template (e.g. <c>#{Heading1}</c>), removed before
    /// the import. Case-sensitive, whole word.
    /// </summary>
    public IReadOnlyCollection<string> SampleMarkers { get; init; } = [];

    /// <summary>Shrinks the body pictures that do not fit the template's page (or column).</summary>
    public bool FitPictures { get; init; } = true;

    /// <summary>
    /// Keeps the content's sections (breaks and columns) instead of flattening them into the last
    /// section of the template. Each content section gets the properties of the template's last
    /// section (size, margins, headers, footers); only the columns and the start type come from the content.
    /// </summary>
    public bool PreserveSections { get; init; }
}

/// <summary>Templates (.docx of header, cover, styles) and applying content: empty the template and import another .docx's body into it.</summary>
public static unsafe class DocxTemplate
{
    /// <summary>
    /// Empties the template for editing: one empty paragraph in the Normal style under the LAST
    /// section's properties, without header/footer references; accepts revisions, removes comments,
    /// macros, protection, attached template and personal metadata.
    /// </summary>
    public static void Empty(DocxDocument doc) => DocxOps.Run(doc, 0);

    /// <summary>Empties the template and imports the body of <paramref name="contentDocx"/> into it.</summary>
    public static void ApplyContent(DocxDocument template, byte[] contentDocx, DocxApplyContentOptions? options = null)
    {
        ArgumentNullException.ThrowIfNull(template);
        ArgumentNullException.ThrowIfNull(contentDocx);
        options ??= new DocxApplyContentOptions();
        using var markers = Arg.List(options.SampleMarkers, nameof(options.SampleMarkers));
        var o = new NativeMethods.UoApplyContentOptions
        {
            Size = (uint)sizeof(NativeMethods.UoApplyContentOptions),
            Flags = (options.FitPictures ? 1u : 0u) | (options.PreserveSections ? 2u : 0u),
            SampleMarkers = markers.Str,
        };
        using var content = Arg.Of(contentDocx);
        template.With(h =>
        {
            var local = o;
            EditingErrors.Check(NativeMethods.TemplateApplyContent(h, content.Ptr, content.Len, &local));
        });
    }

    /// <summary>
    /// Imports the body of <paramref name="source"/> after the template's content in <paramref name="target"/>.
    /// The same instance twice imports from a copy of it.
    /// </summary>
    public static void ImportBody(DocxDocument target, DocxDocument source)
    {
        ArgumentNullException.ThrowIfNull(target);
        ArgumentNullException.ThrowIfNull(source);
        // both documents are locked, always in the same order, so two threads cannot deadlock
        var (first, second) = target.Order <= source.Order ? (target, source) : (source, target);
        lock (first.Gate)
        {
            lock (second.Gate)
            {
                target.With(t => source.With(s => EditingErrors.Check(NativeMethods.TemplateImportBody(t, s))));
            }
        }
    }
}
