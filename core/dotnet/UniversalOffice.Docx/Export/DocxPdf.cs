using UniversalOffice.DocxLayout;

namespace UniversalOffice.Export;

/// <summary>.docx (or Word 97-2003 .doc) → PDF by the Rust core's layout engine. 0.10: the font
/// argument is a <see cref="DocxPdfConverter"/> (a font policy over a native font index) instead of an
/// <c>IFontProvider</c>; the v1 "fixed fonts" overload is gone (record uo-bindings.md, API breaks).</summary>
public static class DocxPdf
{
    /// <summary>The machine's fonts with the metric substitutes (<see cref="DocxPdfConverter.Shared"/>).</summary>
    public static byte[] ToPdf(byte[] bytes) => DocxPdfConverter.Shared.ToPdf(bytes);

    /// <summary>A converter of the host + the engine's options.</summary>
    public static byte[] ToPdf(byte[] bytes, DocxPdfConverter fonts, LayoutOptions? opts = null, PdfRenderOptions? pdfOpts = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(fonts);
        return fonts.ToPdf(bytes, opts, pdfOpts, ct);
    }
}
