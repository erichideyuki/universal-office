using System.Runtime.InteropServices;

namespace UniversalOffice.Native;

/// <summary>Placeholder document of the foundation skeleton: proves ownership across the
/// boundary (the real document API arrives with the modules).</summary>
public sealed class NativeDocument : IDisposable
{
    private readonly DocumentHandle _handle = NativeMethods.DocumentNew();

    public unsafe void Append(ReadOnlySpan<byte> data)
    {
        fixed (byte* p = data)
        {
            int status = NativeMethods.DocumentAppend(_handle, p, (nuint)data.Length);
            if (status != 0) throw new InvalidOperationException($"uo_document_append failed with status {status}");
        }
    }

    public long Length => (long)NativeMethods.DocumentLen(_handle);

    public void Dispose() => _handle.Dispose();
}

/// <summary>Information and memory counters of the native core.</summary>
public static class NativeCore
{
    public static string Version => Marshal.PtrToStringUTF8(NativeMethods.Version()) ?? "";

    /// <summary>The native library was built with the memory counters (tests only).</summary>
    public static bool IsCountingMemory => NativeMethods.MemoryCounting() != 0;

    /// <summary>Bytes live in the native core (all threads), or -1 without counters.</summary>
    public static long LiveBytes => NativeMethods.MemoryLiveBytes();
}
