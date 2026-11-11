namespace UniversalOffice.Docx.Editing;

/// <summary>Vertical position of the text (w:vertAlign).</summary>
public enum DocxVerticalAlign
{
    Superscript,
    Subscript,
}

/// <summary>Character formatting of the text written by <see cref="DocxBodyWriter"/>.</summary>
public sealed record DocxRunFormat
{
    public bool Bold { get; init; }
    public bool Italic { get; init; }
    public bool Underline { get; init; }
    public bool Strike { get; init; }

    /// <summary>Text colour: hexadecimal ("RRGGBB", "#RGB"…), <c>rgb(…)</c> or a CSS name; an invalid one is ignored.</summary>
    public string? Color { get; init; }

    /// <summary>Background colour (w:shd), in the same format as <see cref="Color"/>.</summary>
    public string? Background { get; init; }

    public DocxVerticalAlign? VerticalAlign { get; init; }

    /// <summary>
    /// The link target (becomes <c>w:hyperlink</c> with an external relationship). A URL outside
    /// <see cref="DocxBodyWriterOptions.AllowedLinkSchemes"/> (anchors "#…", relative ones) stays
    /// text; "www.…" gets http://.
    /// </summary>
    public string? Link { get; init; }

    /// <summary>No formatting.</summary>
    public static DocxRunFormat None { get; } = new();
}
