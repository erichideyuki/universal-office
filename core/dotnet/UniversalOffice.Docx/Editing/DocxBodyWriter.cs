using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>How <see cref="DocxBodyWriter"/> writes the content.</summary>
public sealed record DocxBodyWriterOptions
{
    /// <summary>Schemes accepted in links (the others become text; "www.…" gets http://).</summary>
    public IReadOnlyCollection<string> AllowedLinkSchemes { get; init; } = DocxFields.DefaultAllowedSchemes;

    /// <summary>
    /// Colour of links ("RRGGBB" or a CSS colour), no underline unless the text itself is underlined.
    /// Null = the "Hyperlink" character style (created if the document has none).
    /// </summary>
    public string? LinkColor { get; init; }

    /// <summary>The first paragraph written goes into the body's last paragraph if it is empty (keeping its w:sectPr).</summary>
    public bool ReuseEmptyLastParagraph { get; init; } = true;

    /// <summary>
    /// <see cref="DocxBodyWriter.Finish"/> removes the last paragraph written if it ends up with only
    /// spaces (and no image), as long as the body keeps another block.
    /// </summary>
    public bool TrimTrailingEmptyParagraph { get; init; } = true;

    /// <summary><see cref="DocxBodyWriter.Finish"/> shrinks the pictures written that do not fit the page's usable area.</summary>
    public bool FitPictures { get; init; } = true;
}

/// <summary>
/// Writes rich content (runs with formatting, line breaks, images, paragraphs) at the end of a
/// document's body, in the paragraph formats of the template. Call <see cref="Finish"/> at the end.
/// The writer belongs to ONE document: using it with another throws <see cref="InvalidOperationException"/>.
/// </summary>
public sealed unsafe class DocxBodyWriter : IDisposable
{
    private readonly DocxDocument _doc;
    private readonly BodyWriterHandle _handle;

    public DocxBodyWriter(DocxDocument document, DocxBodyWriterOptions? options = null)
    {
        ArgumentNullException.ThrowIfNull(document);
        options ??= new DocxBodyWriterOptions();
        ArgumentNullException.ThrowIfNull(options.AllowedLinkSchemes);
        _doc = document;
        using var schemes = Arg.List(options.AllowedLinkSchemes, nameof(options.AllowedLinkSchemes));
        using var color = Arg.Of(options.LinkColor);
        _handle = document.With(h =>
        {
            var o = new NativeMethods.UoBodyWriterOptions
            {
                Size = (uint)sizeof(NativeMethods.UoBodyWriterOptions),
                Flags = (options.ReuseEmptyLastParagraph ? 1u : 0u) | (options.TrimTrailingEmptyParagraph ? 2u : 0u) | (options.FitPictures ? 4u : 0u),
                AllowedLinkSchemes = schemes.Str,
                LinkColor = color.Str,
            };
            EditingErrors.Check(NativeMethods.BodyWriterNew(h, &o, out var w));
            return w;
        });
    }

    /// <summary>Is the current line (the paragraph not closed yet) empty?</summary>
    public bool LineIsEmpty
    {
        get
        {
            _doc.ThrowIfDisposed();
            lock (_doc.Gate)
            {
                EditingErrors.Check(NativeMethods.BodyWriterLineIsEmpty(_handle, out int empty));
                return empty != 0;
            }
        }
    }

    /// <summary>Text on the current line; <c>\t</c> becomes w:tab and line breaks become w:br. Empty text does nothing.</summary>
    public void AddText(string text, DocxRunFormat format)
    {
        ArgumentNullException.ThrowIfNull(text);
        ArgumentNullException.ThrowIfNull(format);
        using var t = Arg.Of(text);
        using var color = Arg.Of(format.Color);
        using var back = Arg.Of(format.Background);
        using var link = Arg.Of(format.Link);
        _doc.With(h =>
        {
            var f = new NativeMethods.UoRunFormat
            {
                Size = (uint)sizeof(NativeMethods.UoRunFormat),
                Flags = (format.Bold ? 1u : 0u) | (format.Italic ? 2u : 0u) | (format.Underline ? 4u : 0u) | (format.Strike ? 8u : 0u),
                VerticalAlign = format.VerticalAlign is null ? -1 : (int)format.VerticalAlign.Value,
                Color = color.Str,
                Background = back.Str,
                Link = link.Str,
            };
            EditingErrors.Check(NativeMethods.BodyWriterAddText(_handle, h, t.Ptr, t.Len, &f));
        });
    }

    /// <summary>A line break (w:br).</summary>
    public void AddBreak() => _doc.With(h => EditingErrors.Check(NativeMethods.BodyWriterAddBreak(_handle, h)));

    /// <summary>An inline image (PNG, JPEG, GIF or BMP) on the current line; false when the format is not recognized.</summary>
    public bool AddImage(byte[] image, string? link = null)
    {
        ArgumentNullException.ThrowIfNull(image);
        using var i = Arg.Of(image);
        using var l = Arg.Of(link);
        return _doc.With(h =>
        {
            EditingErrors.Check(NativeMethods.BodyWriterAddImage(_handle, h, i.Ptr, i.Len, l.Ptr, l.Len, out int written));
            return written != 0;
        });
    }

    /// <summary>Closes the current line as a paragraph with the format.</summary>
    public void EndParagraph(DocxParagraphFormat format)
    {
        ArgumentNullException.ThrowIfNull(format);
        using var style = Arg.Of(format.StyleId);
        using var num = Arg.Of(format.ListNumId);
        using var align = Arg.Of(format.Alignment);
        var baseProps = format.Base;
        _doc.With(h =>
        {
            // the base handle stays alive for the call: its ref count is held
            bool added = false;
            nint baseHandle = 0;
            try
            {
                if (baseProps is not null)
                {
                    baseHandle = baseProps.AddRef(ref added);
                }
                var f = new NativeMethods.UoParagraphFormat
                {
                    Size = (uint)sizeof(NativeMethods.UoParagraphFormat),
                    Flags = format.KeepWithNext ? 1u : 0u,
                    ListLevel = format.ListLevel,
                    LeftIndentTwips = format.LeftIndentTwips,
                    Base = baseHandle,
                    StyleId = style.Str,
                    ListNumId = num.Str,
                    Alignment = align.Str,
                };
                EditingErrors.Check(NativeMethods.BodyWriterEndParagraph(_handle, h, &f));
            }
            finally
            {
                if (added) baseProps!.Release();
            }
        });
    }

    /// <summary>Ends the writing: trims the trailing empty paragraph and fits the pictures, as the options say.</summary>
    public void Finish() => _doc.With(h => EditingErrors.Check(NativeMethods.BodyWriterFinish(_handle, h)));

    /// <summary>Releases the native writer (the document is not touched).</summary>
    public void Dispose() => _handle.Dispose();
}
