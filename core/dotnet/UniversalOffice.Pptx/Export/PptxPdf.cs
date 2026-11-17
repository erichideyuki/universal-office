using UniversalOffice.Pptx;

namespace UniversalOffice.Export;

/// <summary>The 0.9 entry point of the Slides PDF, kept: <c>PptxPdf.ToPdf(bytes)</c>. The 0.9 <c>SlidesPdfOptions.Fonts</c>
/// (TTFs the caller handed over, "no base-14 in the generator") is gone: fonts come from the converter's font policy.</summary>
public static class PptxPdf
{
    /// <summary>.pptx/.ppt → PDF with the shared converter (the machine's fonts and the metric substitutes).</summary>
    public static byte[] ToPdf(byte[] bytes, SlidesPdfOptions? opts = null) => SlidesPdfConverter.Shared.ToPdf(bytes, opts);

    /// <summary>.pptx/.ppt → PDF with the given converter.</summary>
    public static byte[] ToPdf(byte[] bytes, SlidesPdfConverter converter, SlidesPdfOptions? opts = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(converter);
        return converter.ToPdf(bytes, opts, ct);
    }
}
