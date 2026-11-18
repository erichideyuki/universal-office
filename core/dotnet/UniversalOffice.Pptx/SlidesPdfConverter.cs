using System.Text;
using UniversalOffice.Native;

namespace UniversalOffice.Pptx;

/// <summary>Options of the Slides PDF.</summary>
public sealed class SlidesPdfOptions
{
    /// <summary>Compress the content streams (default true).</summary>
    public bool? Compress;

    /// <summary>Every PowerPoint-over-TS switch off: the frozen TS Slides (hidden slides kept, the TS link rectangles,
    /// the TS fallback fonts and the other `SlidesRules`). For the facade's parity tests against the TS only (native
    /// `SLIDES_JOB_TS_STRICT`).</summary>
    internal bool TsStrict;
}

/// <summary>How a converter finds fonts and how much it trusts its decks.</summary>
public sealed class SlidesPdfConverterOptions
{
    /// <summary>Index the machine's font folders (Windows, Linux, macOS and <c>EXTRA_FONT_DIRS</c>).</summary>
    public bool SystemFonts = true;

    /// <summary>More folders to index (after the system ones), or the only ones without <see cref="SystemFonts"/>.</summary>
    public IList<string> FontDirectories = new List<string>();

    /// <summary>The decks come from a trusted source: the default picture cap and container limits instead of the lower
    /// ones for untrusted input (default false: a server converts what users upload).</summary>
    public bool TrustedInput;
}

/// <summary>.pptx (and PowerPoint 97-2003 .ppt) to PDF through the Rust core: one page per slide, the page the size of
/// the slide, with the <c>server-substitute</c> font policy (the family the deck asks for when the machine has it, else its
/// metric-compatible substitute, else Liberation Sans). One instance serves any number of threads at once.</summary>
public sealed class SlidesPdfConverter : IDisposable
{
    private static readonly Lazy<SlidesPdfConverter> SharedLazy = new(() => new SlidesPdfConverter(new SlidesPdfConverterOptions()));
    private static readonly CancelHandle NoCancel = new();
    private readonly ConverterHandle _handle;

    /// <summary>The process-wide converter: the machine's fonts with the substitutes, untrusted input.</summary>
    public static SlidesPdfConverter Shared => SharedLazy.Value;

    public unsafe SlidesPdfConverter(SlidesPdfConverterOptions options)
    {
        ArgumentNullException.ThrowIfNull(options);
        byte[] dirs = Encoding.UTF8.GetBytes(string.Join('\n', options.FontDirectories));
        uint flags = (options.SystemFonts ? NativeMethods.ConverterSystemFonts : 0) | (options.TrustedInput ? NativeMethods.ConverterTrusted : 0);
        fixed (byte* d = dirs)
        {
            var o = new NativeMethods.ConverterOptions { Size = (uint)sizeof(NativeMethods.ConverterOptions), Flags = flags, FontDirs = d, FontDirsLen = (nuint)dirs.Length };
            int status = NativeMethods.ConverterNew(&o, out _handle);
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "SlidesPdfConverter", default);
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

    /// <summary>.pptx or .ppt bytes → PDF bytes.</summary>
    /// <exception cref="InvalidDataException">not a presentation, or a damaged one</exception>
    /// <exception cref="UniversalOffice.Core.OfficeLimitExceededException">a limit for untrusted decks</exception>
    /// <exception cref="OperationCanceledException"><paramref name="ct"/> was cancelled (checked by the native core before
    /// the work and before the bytes come back; the Slides PDF has no finer hook yet, see the record)</exception>
    public unsafe byte[] ToPdf(byte[] deck, SlidesPdfOptions? opts = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(deck);
        ct.ThrowIfCancellationRequested();
        var job = new SlidesNative.SlidesJob { Size = (uint)sizeof(SlidesNative.SlidesJob), Flags = JobFlags(opts) };
        using var cancel = ct.CanBeCanceled ? NativeMethods.CancelNew() : null;
        // disposed before `cancel` (reverse order): waits for a running callback
        using var registration = cancel is null ? default : ct.Register(static c => NativeMethods.CancelRaise((CancelHandle)c!), cancel);
        int status;
        BufferHandle pdf;
        fixed (byte* data = deck)
            status = SlidesNative.PptxToPdf(_handle, data, (nuint)deck.Length, &job, cancel ?? NoCancel, out pdf);
        using (pdf)
        {
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "ToPdf", ct);
            return pdf.ToArray();
        }
    }

    /// <summary>An open deck → PDF bytes (see <see cref="PptxDeck.ToPdf"/>).</summary>
    public byte[] ToPdf(PptxDeck deck, SlidesPdfOptions? opts = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(deck);
        return deck.ToPdf(this, opts, ct);
    }

    internal unsafe byte[] DeckToPdf(DeckHandle deck, SlidesPdfOptions? opts, CancellationToken ct)
    {
        ct.ThrowIfCancellationRequested();
        var job = new SlidesNative.SlidesJob { Size = (uint)sizeof(SlidesNative.SlidesJob), Flags = JobFlags(opts) };
        using var cancel = ct.CanBeCanceled ? NativeMethods.CancelNew() : null;
        using var registration = cancel is null ? default : ct.Register(static c => NativeMethods.CancelRaise((CancelHandle)c!), cancel);
        int status = SlidesNative.DeckToPdf(deck, _handle, &job, cancel ?? NoCancel, out var pdf);
        using (pdf)
        {
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "ToPdf", ct);
            return pdf.ToArray();
        }
    }

    private static uint JobFlags(SlidesPdfOptions? opts) =>
        ((opts?.Compress ?? true) ? SlidesNative.JobCompress : 0) | (opts?.TsStrict == true ? SlidesNative.JobTsStrict : 0);

    public void Dispose() => _handle.Dispose();
}
