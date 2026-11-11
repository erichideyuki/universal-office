using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>What the sanitizer does. Use the ready sets <see cref="Content"/> and <see cref="Output"/>.</summary>
public sealed record DocxSanitizeOptions
{
    /// <summary>Accepts tracked changes and turns tracking off.</summary>
    public bool AcceptRevisions { get; init; } = true;

    /// <summary>Removes comments (marks, references and parts).</summary>
    public bool RemoveComments { get; init; } = true;

    /// <summary>Removes macros/VBA and saves as a plain .docx.</summary>
    public bool RemoveMacros { get; init; } = true;

    /// <summary>Removes document protection and the attached template.</summary>
    public bool RemoveProtectionAndAttachedTemplate { get; init; } = true;

    /// <summary>Clears author, company, title… and the custom properties.</summary>
    public bool RemovePersonalMetadata { get; init; }

    /// <summary>Removes headers and footers (references and parts).</summary>
    public bool RemoveHeadersAndFooters { get; init; }

    /// <summary>Merges all sections into the last one (section breaks disappear).</summary>
    public bool MergeSections { get; init; }

    /// <summary>Removes the customXml/ parts.</summary>
    public bool RemoveCustomXml { get; init; }

    /// <summary>
    /// Removes active or external content: OLE objects, ActiveX controls, altChunk, subdocuments,
    /// frames and external relationships that are not hyperlinks to an allowed scheme (hyperlinks
    /// with another scheme become text).
    /// </summary>
    public bool RemoveUnsafeContent { get; init; } = true;

    /// <summary>Fields kept; the others become their cached result (never evaluated). Null = fields are left alone.</summary>
    public IReadOnlyCollection<string>? AllowedFields { get; init; } = DocxFields.DefaultAllowedFields.ToArray();

    /// <summary>Schemes accepted in hyperlinks (fields and external relationships).</summary>
    public IReadOnlyCollection<string> AllowedHyperlinkSchemes { get; init; } = DocxFields.DefaultAllowedSchemes.ToArray();

    /// <summary>Converts HYPERLINK fields into native w:hyperlink.</summary>
    public bool ConvertHyperlinkFields { get; init; } = true;

    /// <summary>
    /// Validation of the plugin inlines (type, JSON): those that return false are unwrapped (the
    /// fallback text stays). Null = no validation. The Rust core runs it in two phases: it lists
    /// the inlines, every callback runs, and only then are the invalid ones unwrapped (an exception
    /// from the callback leaves the document mid-sanitization, with no inline unwrapped).
    /// </summary>
    public Func<string, string, bool>? IsCustomInlineValid { get; init; }

    /// <summary>
    /// Sanitization of received content (uploaded by a user, to store or apply to a template):
    /// revisions, metadata, headers and footers, sections, unsafe content, fields and customXml.
    /// </summary>
    public static DocxSanitizeOptions Content { get; } = new()
    {
        RemovePersonalMetadata = true,
        RemoveHeadersAndFooters = true,
        MergeSections = true,
        RemoveCustomXml = true,
    };

    /// <summary>
    /// Sanitization of the document generated from a template (the clean-output step): revisions,
    /// unsafe content and fields (the wider list, <see cref="DocxFields.DefaultOutputAllowedFields"/>);
    /// keeps headers, sections and metadata.
    /// </summary>
    public static DocxSanitizeOptions Output { get; } = new() { AllowedFields = DocxFields.DefaultOutputAllowedFields.ToArray() };
}

/// <summary>Sanitizes a .docx before storing received content or delivering a generated document.</summary>
public static unsafe class DocxSanitizer
{
    /// <summary>Applies <paramref name="options"/> to <paramref name="doc"/>.</summary>
    public static void Sanitize(DocxDocument doc, DocxSanitizeOptions options)
    {
        ArgumentNullException.ThrowIfNull(doc);
        ArgumentNullException.ThrowIfNull(options);
        ArgumentNullException.ThrowIfNull(options.AllowedHyperlinkSchemes);
        uint flags = 0;
        void Bit(bool on, int bit) { if (on) flags |= 1u << bit; }
        Bit(options.AcceptRevisions, 0);
        Bit(options.RemoveComments, 1);
        Bit(options.RemoveMacros, 2);
        Bit(options.RemoveProtectionAndAttachedTemplate, 3);
        Bit(options.RemovePersonalMetadata, 4);
        Bit(options.RemoveHeadersAndFooters, 5);
        Bit(options.MergeSections, 6);
        Bit(options.RemoveCustomXml, 7);
        Bit(options.RemoveUnsafeContent, 8);
        Bit(options.ConvertHyperlinkFields, 9);
        using var fields = Arg.List(options.AllowedFields, nameof(options.AllowedFields));
        using var schemes = Arg.List(options.AllowedHyperlinkSchemes, nameof(options.AllowedHyperlinkSchemes));
        var o = new NativeMethods.UoSanitizeOptions
        {
            Size = (uint)sizeof(NativeMethods.UoSanitizeOptions),
            Flags = flags,
            AllowedFields = fields.Str,
            AllowedSchemes = schemes.Str,
        };
        var validator = options.IsCustomInlineValid;
        if (validator is null)
        {
            doc.With(h =>
            {
                var local = o;
                EditingErrors.Check(NativeMethods.DocxSanitize(h, &local));
            });
            return;
        }
        // phase 1: the steps up to the inline validation; the host judges; phase 2: the verdicts
        doc.With(h =>
        {
            var local = o;
            EditingErrors.Check(NativeMethods.DocxSanitizeBegin(h, &local, out var stage));
            using (stage)
            {
                EditingErrors.Check(NativeMethods.StageInlines(h, stage, out var buf));
                var r = buf.Reader();
                int n = r.Count();
                var verdicts = new byte[n];
                for (int i = 0; i < n; i++)
                {
                    r.U32();
                    string type = r.Str(), json = r.Str();
                    r.Str();
                    verdicts[i] = validator(type, json) ? (byte)1 : (byte)0;
                }
                fixed (byte* v = verdicts)
                    EditingErrors.Check(NativeMethods.StageFinish(h, stage, v, (nuint)n, out _));
            }
        });
    }

    /// <summary>Removes headers and footers (references and parts).</summary>
    public static void RemoveHeadersAndFooters(DocxDocument doc) => DocxOps.Run(doc, 7);

    /// <summary>Merges all sections into the last one.</summary>
    public static void MergeSections(DocxDocument doc) => DocxOps.Run(doc, 8);

    /// <summary>Removes the customXml/ parts.</summary>
    public static void RemoveCustomXml(DocxDocument doc) => DocxOps.Run(doc, 9);

    /// <summary>Removes active or external content (see <see cref="DocxSanitizeOptions.RemoveUnsafeContent"/>).</summary>
    public static void RemoveUnsafeContent(DocxDocument doc, IEnumerable<string>? allowedSchemes = null)
    {
        ArgumentNullException.ThrowIfNull(doc);
        using var s = Arg.List(allowedSchemes, nameof(allowedSchemes));
        doc.With(h => EditingErrors.Check(NativeMethods.DocxRemoveUnsafeContent(h, s.Ptr, s.Len)));
    }
}
