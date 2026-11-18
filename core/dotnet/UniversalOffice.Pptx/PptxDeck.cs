using UniversalOffice.Native;

namespace UniversalOffice.Pptx;

/// <summary>An open presentation (.pptx, or a PowerPoint 97-2003 .ppt converted on opening) held by the Rust core. Dispose it
/// (or let the finalizer) to release the native memory. Calls may come from any thread; the core runs one at a time per deck.</summary>
public sealed class PptxDeck : IDisposable
{
    private readonly DeckHandle _handle;

    private PptxDeck(DeckHandle handle, int slideCount, double width, double height, bool legacy)
    {
        _handle = handle;
        SlideCount = slideCount;
        SlideWidthEmu = width;
        SlideHeightEmu = height;
        IsLegacyPpt = legacy;
    }

    /// <summary>Opens a presentation from its bytes: a .pptx, or a PowerPoint 97-2003 .ppt (detected by its signature).</summary>
    /// <param name="bytes">the file</param>
    /// <param name="trusted">the bytes come from a trusted source (the default container limits; untrusted input is
    /// capped at 256 MiB per inflated part and 512 MiB in all)</param>
    /// <exception cref="InvalidDataException">not a presentation, a damaged or password-protected one</exception>
    public static PptxDeck Open(byte[] bytes, bool trusted = false) => Open(bytes, trusted, tsStrict: false);

    /// <summary><see cref="Open(byte[], bool)"/>; <paramref name="tsStrict"/> converts a .ppt as the frozen TS did (charts as
    /// their fallback pictures; native `DECK_TS_STRICT`). Parity tests only.</summary>
    internal static unsafe PptxDeck Open(byte[] bytes, bool trusted, bool tsStrict)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        uint flags = (trusted ? SlidesNative.DeckTrusted : 0) | (tsStrict ? SlidesNative.DeckTsStrict : 0);
        var o = new SlidesNative.DeckOptions { Size = (uint)sizeof(SlidesNative.DeckOptions), Flags = flags };
        int status;
        DeckHandle handle;
        fixed (byte* data = bytes) status = SlidesNative.DeckOpen(data, (nuint)bytes.Length, &o, out handle);
        if (status != NativeMethods.Ok) { handle.Dispose(); throw NativeMethods.ErrorOf(status, "Open", default); }
        try
        {
            uint count;
            double w, h;
            status = SlidesNative.DeckInfo(handle, &count, &w, &h);
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "Open", default);
            return new PptxDeck(handle, checked((int)count), w, h, SlidesNative.DeckIsLegacy(handle) == 1);
        }
        catch
        {
            handle.Dispose();
            throw;
        }
    }

    /// <summary>The number of slides.</summary>
    public int SlideCount { get; }

    /// <summary>The slide width, in EMU (914,400 per inch).</summary>
    public double SlideWidthEmu { get; }

    /// <summary>The slide height, in EMU.</summary>
    public double SlideHeightEmu { get; }

    /// <summary>The deck was a PowerPoint 97-2003 .ppt (converted to the parts of a .pptx on opening).</summary>
    public bool IsLegacyPpt { get; }

    /// <summary>The text of the notes of a slide (0-based), paragraphs separated by a line feed; empty when it has none.</summary>
    public string GetSlideNotes(int index)
    {
        if ((uint)index >= (uint)SlideCount) throw new ArgumentOutOfRangeException(nameof(index), index, $"the deck has {SlideCount} slides");
        int status = SlidesNative.DeckSlideNotes(_handle, (uint)index, out var buf);
        using (buf)
        {
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "GetSlideNotes", default);
            return System.Text.Encoding.UTF8.GetString(buf.ToArray());
        }
    }

    /// <summary>The deck as .pptx bytes (a legacy .ppt comes out converted).</summary>
    public byte[] Save()
    {
        int status = SlidesNative.DeckSave(_handle, out var buf);
        using (buf)
        {
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "Save", default);
            return buf.ToArray();
        }
    }

    /// <summary>The deck as PDF bytes: one page per slide, the page the size of the slide.</summary>
    public byte[] ToPdf(SlidesPdfConverter converter, SlidesPdfOptions? opts = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(converter);
        return converter.DeckToPdf(_handle, opts, ct);
    }

    public void Dispose() => _handle.Dispose();
}

/// <summary>PowerPoint 97-2003 .ppt to .pptx.</summary>
public static class PptConverter
{
    /// <summary>Converts the bytes of a .ppt into a .pptx.</summary>
    /// <exception cref="InvalidDataException">not a PowerPoint 97-2003 file (a .pptx is refused), or a damaged one</exception>
    public static unsafe byte[] ToPptx(byte[] ppt, bool trusted = false)
    {
        ArgumentNullException.ThrowIfNull(ppt);
        var o = new SlidesNative.DeckOptions { Size = (uint)sizeof(SlidesNative.DeckOptions), Flags = trusted ? SlidesNative.DeckTrusted : 0 };
        int status;
        BufferHandle buf;
        fixed (byte* data = ppt) status = SlidesNative.PptToPptx(data, (nuint)ppt.Length, &o, out buf);
        using (buf)
        {
            if (status != NativeMethods.Ok) throw NativeMethods.ErrorOf(status, "ToPptx", default);
            return buf.ToArray();
        }
    }
}
