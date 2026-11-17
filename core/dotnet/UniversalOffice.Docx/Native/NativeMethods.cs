using System.Runtime.InteropServices;
using UniversalOffice.Core;

namespace UniversalOffice.Native;

/// <summary>The C ABI of uo-ffi (core/crates/uo-ffi/src/lib.rs). Every handle is a
/// <see cref="SafeHandle"/>: released exactly once, by <c>Dispose()</c> or by the finalizer.</summary>
internal static unsafe partial class NativeMethods
{
    internal const string Library = "uo_ffi";

    // status codes (pipeline::code)
    internal const int Ok = 0;
    internal const int Argument = 1;
    internal const int InvalidOperation = 2;
    internal const int InvalidData = 3;
    internal const int Xml = 4;
    internal const int LimitExceeded = 5;
    internal const int Cancelled = 10;
    internal const int NoFont = 11;
    internal const int Internal = 12;
    internal const int Sink = 13;

    internal const uint ConverterSystemFonts = 1;
    internal const uint ConverterTrusted = 2;
    internal const uint JobCompress = 1;
    internal const uint JobNoHyphenation = 2;
    internal const uint JobHyphenationLanguages = 4;
    internal const uint JobTsStrict = 8;

    [StructLayout(LayoutKind.Sequential)]
    internal struct ConverterOptions
    {
        public uint Size;
        public uint Flags;
        public byte* FontDirs;
        public nuint FontDirsLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct PdfJob
    {
        public uint Size;
        public uint Flags;
        public double NowMs;
        public int TzOffsetMin;
        public uint Revisions;
        public uint MaxPages;
        public uint Reserved;
        public byte* Lang;
        public nuint LangLen;
        public byte* HyphLangs;
        public nuint HyphLangsLen;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct ErrorInfo
    {
        public int Code;
        public int Stage;
        public ulong Max;
        public ulong MessageLen;
    }

    [LibraryImport(Library, EntryPoint = "uo_version")]
    internal static partial nint Version();

    [LibraryImport(Library, EntryPoint = "uo_last_error")]
    internal static partial int LastError(ErrorInfo* info);

    [LibraryImport(Library, EntryPoint = "uo_last_error_message")]
    internal static partial nuint LastErrorMessage(byte* buf, nuint cap);

    [LibraryImport(Library, EntryPoint = "uo_last_error_clear")]
    internal static partial void LastErrorClear();

    [LibraryImport(Library, EntryPoint = "uo_converter_new")]
    internal static partial int ConverterNew(ConverterOptions* options, out ConverterHandle converter);

    [LibraryImport(Library, EntryPoint = "uo_converter_free")]
    internal static partial void ConverterFree(nint converter);

    [LibraryImport(Library, EntryPoint = "uo_converter_families")]
    internal static partial int ConverterFamilies(ConverterHandle converter, out BufferHandle families);

    [LibraryImport(Library, EntryPoint = "uo_docx_to_pdf")]
    internal static partial int DocxToPdf(ConverterHandle converter, byte* data, nuint len, PdfJob* job, CancelHandle cancel, out BufferHandle pdf);

    [LibraryImport(Library, EntryPoint = "uo_buf_ptr")]
    internal static partial byte* BufPtr(BufferHandle buf);

    [LibraryImport(Library, EntryPoint = "uo_buf_len")]
    internal static partial nuint BufLen(BufferHandle buf);

    [LibraryImport(Library, EntryPoint = "uo_buf_free")]
    internal static partial void BufFree(nint buf);

    [LibraryImport(Library, EntryPoint = "uo_cancel_new")]
    internal static partial CancelHandle CancelNew();

    [LibraryImport(Library, EntryPoint = "uo_cancel_raise")]
    internal static partial void CancelRaise(CancelHandle cancel);

    [LibraryImport(Library, EntryPoint = "uo_cancel_free")]
    internal static partial void CancelFree(nint cancel);

    [LibraryImport(Library, EntryPoint = "uo_memory_counting")]
    internal static partial int MemoryCounting();

    [LibraryImport(Library, EntryPoint = "uo_memory_live_bytes")]
    internal static partial long MemoryLiveBytes();

    [LibraryImport(Library, EntryPoint = "uo_clear_caches")]
    internal static partial void ClearCaches();

    /// <summary>The last error of this thread as the .NET exception the facade throws.</summary>
    internal static Exception ErrorOf(int status, string what, CancellationToken ct)
    {
        ErrorInfo info;
        LastError(&info);
        string message = "";
        if (info.MessageLen > 0)
        {
            var bytes = new byte[checked((int)info.MessageLen)];
            fixed (byte* p = bytes) LastErrorMessage(p, (nuint)bytes.Length);
            message = System.Text.Encoding.UTF8.GetString(bytes);
        }
        // the slot keeps no allocation once read (the memory gate counts it)
        LastErrorClear();
        var stage = info.Stage switch { 1 => "read", 2 => "layout", 3 => "pdf", _ => "" };
        string text = message.Length > 0 ? $"{what}: {message}" : $"{what}: status {status}";
        return status switch
        {
            Cancelled => new OperationCanceledException(text, ct),
            Argument => new ArgumentException(text),
            InvalidData or Xml => new InvalidDataException(text),
            LimitExceeded => new OfficeLimitExceededException(text, OfficeLimit.Conversion, (long)Math.Min(info.Max, long.MaxValue), null, stage),
            _ => new UniversalOfficeException(text, status, stage),
        };
    }
}

/// <summary>Base of the native handles: invalid when zero.</summary>
internal abstract class NativeHandle : SafeHandle
{
    protected NativeHandle() : base(nint.Zero, ownsHandle: true) { }
    public override bool IsInvalid => handle == nint.Zero;
}

/// <summary>A <c>UoConverter</c> (released by <c>uo_converter_free</c>).</summary>
internal sealed class ConverterHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.ConverterFree(handle); return true; }
}

/// <summary>A <c>UoBuf</c> (released by <c>uo_buf_free</c>).</summary>
internal sealed class BufferHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.BufFree(handle); return true; }

    /// <summary>A copy of the bytes.</summary>
    internal unsafe byte[] ToArray()
    {
        nuint len = NativeMethods.BufLen(this);
        if (len == 0) return [];
        var data = new byte[checked((int)len)];
        new ReadOnlySpan<byte>(NativeMethods.BufPtr(this), data.Length).CopyTo(data);
        return data;
    }
}

/// <summary>A <c>UoCancel</c> (released by <c>uo_cancel_free</c>; a running job keeps its own reference).</summary>
internal sealed class CancelHandle : NativeHandle
{
    protected override bool ReleaseHandle() { NativeMethods.CancelFree(handle); return true; }
}

/// <summary>Version and memory counters of the native core.</summary>
public static class NativeCore
{
    /// <summary>The version of the native library.</summary>
    public static string Version => Marshal.PtrToStringUTF8(NativeMethods.Version()) ?? "";

    /// <summary>The native library was built with the memory counters (tests only).</summary>
    public static bool IsCountingMemory => NativeMethods.MemoryCounting() != 0;

    /// <summary>Bytes live in the native core (all threads), or -1 without counters.</summary>
    public static long LiveBytes => NativeMethods.MemoryLiveBytes();

    /// <summary>Drops the process-wide font caches (the face cache and the shared system font index;
    /// a live converter keeps what it holds).</summary>
    public static void ClearCaches() => NativeMethods.ClearCaches();
}
