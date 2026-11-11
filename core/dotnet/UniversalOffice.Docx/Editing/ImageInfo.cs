using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>Format, size in pixels and resolution read from an image's header (without decoding it).</summary>
public sealed record ImageInfo(string Extension, string ContentType, int WidthPx, int HeightPx, double DpiX, double DpiY)
{
    /// <summary>Physical width in EMU (by the image resolution; 96 dpi when it does not say).</summary>
    public long WidthEmu => (long)Math.Round(WidthPx * 914400.0 / DpiX);

    /// <summary>Physical height in EMU.</summary>
    public long HeightEmu => (long)Math.Round(HeightPx * 914400.0 / DpiY);

    /// <summary>PNG, JPEG, GIF or BMP; null for another format or an invalid header.</summary>
    public static unsafe ImageInfo? TryRead(ReadOnlySpan<byte> data)
    {
        NativeMethods.UoImageInfo info;
        int status;
        BufferHandle strings;
        int present;
        fixed (byte* p = data)
        {
            status = NativeMethods.ImageInfoTryRead(p, (nuint)data.Length, &info, out strings, out present);
        }
        EditingErrors.Check(status);
        if (present == 0) return null;
        var r = strings.Reader();
        string ext = r.Str(), type = r.Str();
        return new ImageInfo(ext, type, info.WidthPx, info.HeightPx, info.DpiX, info.DpiY);
    }
}
