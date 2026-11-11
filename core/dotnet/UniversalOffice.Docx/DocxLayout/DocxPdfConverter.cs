using System.Text;
using UniversalOffice.Native;

namespace UniversalOffice.DocxLayout;

/// <summary>Options of the layout engine for one conversion (the subset of 0.9's
/// <c>LayoutOptions</c> that reaches the Rust core; the lab-only fields are gone).</summary>
public sealed class LayoutOptions
{
    /// <summary>Language of numbers and dates (default pt-BR).</summary>
    public string? Lang;

    /// <summary>The instant the DATE/TIME fields show (default: now). A <c>Local</c> or
    /// <c>Unspecified</c> value is the local wall clock; a <c>Utc</c> value is shown in local time.</summary>
    public DateTime? Now;

    /// <summary>The local offset the date fields use (default: the machine's offset at <see cref="Now"/>).</summary>
    public TimeSpan? UtcOffset;

    /// <summary>Automatic hyphenation (<c>w:autoHyphenation</c>); default on.</summary>
    public bool? Hyphenation;

    /// <summary>Hyphenate only these languages (BCP 47 tags or prefixes: "pt-BR", "en"…); default: every
    /// language with patterns.</summary>
    public List<string>? HyphenationLanguages;

    /// <summary>How tracked changes show (default: the final view without marks).</summary>
    public RevisionView? Revisions;

    /// <summary>Pages of one layout before the document is refused (default: the engine's limit).</summary>
    public int? MaxPages;
}

/// <summary>How tracked changes show: final, simple, all or original (the 0.9 author filters and
/// colours do not cross to the native core in 0.10's milestone 1).</summary>
public sealed class RevisionView
{
    /// <summary>final | simple | all | original</summary>
    public string Mode = "final";
}

/// <summary>Options of the PDF writer.</summary>
public sealed class PdfRenderOptions
{
    /// <summary>Compress the content streams (default true).</summary>
    public bool? Compress;
}

/// <summary>How a converter finds fonts and how much it trusts its documents.</summary>
public sealed class DocxPdfConverterOptions
{
    /// <summary>Index the machine's font folders (Windows, Linux, macOS and <c>EXTRA_FONT_DIRS</c>).</summary>
    public bool SystemFonts = true;

    /// <summary>More folders to index (after the system ones), or the only ones without <see cref="SystemFonts"/>.</summary>
    public IList<string> FontDirectories = new List<string>();

    /// <summary>The documents come from a trusted source: the default effect and image caps instead of the
    /// lower ones for untrusted input (default false: a server converts what users upload).</summary>
    public bool TrustedInput;
}

/// <summary>.docx (and Word 97-2003 .doc) to PDF through the Rust core, with the
/// <c>server-substitute</c> font policy: the family the document asks for when the machine has it,
/// else its metric-compatible substitute (Calibri → Carlito, Cambria → Caladea, Arial/Helvetica →
/// Liberation Sans, Times → Liberation Serif, Courier → Liberation Mono); fonts embedded in the
/// document come first. One instance serves any number of threads at once.</summary>
public sealed class DocxPdfConverter : IDisposable
{
    private static readonly Lazy<DocxPdfConverter> SharedLazy = new(() => new DocxPdfConverter(new DocxPdfConverterOptions()));
    private static readonly CancelHandle NoCancel = new();
    private readonly ConverterHandle _handle;

    /// <summary>The process-wide converter: the machine's fonts with the substitutes, untrusted input
    /// (the fonts are scanned once).</summary>
    public static DocxPdfConverter Shared => SharedLazy.Value;

    public unsafe DocxPdfConverter(DocxPdfConverterOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        byte[] dirs = Encoding.UTF8.GetBytes(string.Join('\n', options.FontDirectories));
        uint flags = (options.SystemFonts ? NativeMethods.ConverterSystemFonts : 0) | (options.TrustedInput ? NativeMethods.ConverterTrusted : 0);
        fixed (byte* d = dirs)
        {
            var o = new NativeMethods.ConverterOptions { Size = (uint)sizeof(NativeMethods.ConverterOptions), Flags = flags, FontDirs = d, FontDirsLen = (nuint)dirs.Length };
            int status = NativeMethods.ConverterNew(&o, out _handle);
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "DocxPdfConverter", default);
        }
    }

    /// <summary>The families the converter's font index knows.</summary>
    public IReadOnlyList<string> Families
    {
        get
        {
            int status = NativeMethods.ConverterFamilies(_handle, out var buf);
            using (buf)
            {
                if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "Families", default);
                return Encoding.UTF8.GetString(buf.ToArray()).Split('\n', StringSplitOptions.RemoveEmptyEntries);
            }
        }
    }

    /// <summary>.docx or .doc bytes → PDF bytes.</summary>
    /// <exception cref="InvalidDataException">not a Word document, or a damaged one</exception>
    /// <exception cref="UniversalOffice.Core.OfficeLimitExceededException">a limit for untrusted documents</exception>
    /// <exception cref="OperationCanceledException"><paramref name="ct"/> was cancelled (checked by the
    /// native core before and inside the layout, between the stages and while the PDF is written)</exception>
    public unsafe byte[] ToPdf(byte[] docx, LayoutOptions? opts = null, PdfRenderOptions? pdfOpts = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(docx);
        ct.ThrowIfCancellationRequested();
        // the instant and the local offset (the engine reads local fields, as the TS reads a JS Date)
        var now = opts?.Now ?? DateTime.Now;
        DateTime instant;
        TimeSpan offset;
        if (now.Kind == DateTimeKind.Utc)
        {
            instant = now;
            offset = opts?.UtcOffset ?? TimeZoneInfo.Local.GetUtcOffset(now);
        }
        else if (opts?.UtcOffset is { } given)
        {
            offset = given;
            instant = DateTime.SpecifyKind(now, DateTimeKind.Utc) - given;
        }
        else
        {
            instant = DateTime.SpecifyKind(now, DateTimeKind.Local).ToUniversalTime();
            offset = TimeZoneInfo.Local.GetUtcOffset(instant);
        }
        double nowMs = Math.Floor((instant - DateTime.UnixEpoch).TotalMilliseconds);
        uint flags = (pdfOpts?.Compress ?? true) ? NativeMethods.JobCompress : 0;
        if (opts?.Hyphenation == false) flags |= NativeMethods.JobNoHyphenation;
        if (opts?.HyphenationLanguages is not null) flags |= NativeMethods.JobHyphenationLanguages;
        uint revisions = opts?.Revisions?.Mode switch
        {
            null => 0,
            "final" => 1,
            "simple" => 2,
            "all" => 3,
            "original" => 4,
            var m => throw new ArgumentException($"unknown revision mode '{m}'", nameof(opts)),
        };
        int maxPages = opts?.MaxPages ?? 0;
        if (maxPages < 0) throw new ArgumentOutOfRangeException(nameof(opts), "MaxPages");
        byte[] lang = Encoding.UTF8.GetBytes(opts?.Lang ?? "");
        byte[] hyph = Encoding.UTF8.GetBytes(string.Join('\n', opts?.HyphenationLanguages ?? []));

        using var cancel = ct.CanBeCanceled ? NativeMethods.CancelNew() : null;
        // disposed before `cancel` (reverse order): waits for a running callback
        using var registration = cancel is null ? default : ct.Register(static c => NativeMethods.CancelRaise((CancelHandle)c!), cancel);
        int status;
        BufferHandle pdf;
        fixed (byte* data = docx)
        fixed (byte* l = lang)
        fixed (byte* h = hyph)
        {
            var job = new NativeMethods.PdfJob
            {
                Size = (uint)sizeof(NativeMethods.PdfJob),
                Flags = flags,
                NowMs = nowMs,
                TzOffsetMin = (int)Math.Round(offset.TotalMinutes),
                Revisions = revisions,
                MaxPages = (uint)maxPages,
                Lang = l,
                LangLen = (nuint)lang.Length,
                HyphLangs = h,
                HyphLangsLen = (nuint)hyph.Length,
            };
            status = NativeMethods.DocxToPdf(_handle, data, (nuint)docx.Length, &job, cancel ?? NoCancel, out pdf);
        }
        using (pdf)
        {
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "ToPdf", ct);
            return pdf.ToArray();
        }
    }

    public void Dispose() => _handle.Dispose();
}

/// <summary>The 0.9 entry point, kept: <c>DocxLayoutPipeline.ToPdf(docx, SubstituteFontProvider.Shared, options)</c>.</summary>
public static class DocxLayoutPipeline
{
    /// <summary>.docx/.doc → PDF with the converter's font policy (links and the outline of the headings
    /// are always written).</summary>
    public static byte[] ToPdf(byte[] docx, DocxPdfConverter fonts, LayoutOptions? opts = null, PdfRenderOptions? pdfOpts = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(fonts);
        return fonts.ToPdf(docx, opts, pdfOpts, ct);
    }
}

/// <summary>The 0.9 name of the server font policy: the machine's fonts with the metric substitutes.</summary>
public static class SubstituteFontProvider
{
    /// <summary>The process-wide converter (<see cref="DocxPdfConverter.Shared"/>).</summary>
    public static DocxPdfConverter Shared => DocxPdfConverter.Shared;
}
