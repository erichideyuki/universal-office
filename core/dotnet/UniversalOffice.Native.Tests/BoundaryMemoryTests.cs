using System.Runtime.CompilerServices;
using Xunit;

namespace UniversalOffice.Native.Tests;

/// <summary>Memory gate M4 at the .NET boundary: 10,000 native documents created and
/// released through Dispose() and through the finalizer; the native live bytes must return
/// exactly to where they started.</summary>
public class BoundaryMemoryTests
{
    private const int Count = 10_000;
    private static readonly byte[] Payload = Enumerable.Range(0, 256).Select(i => (byte)i).ToArray();

    [Fact]
    public void The_native_library_counts_memory()
    {
        Assert.True(NativeCore.IsCountingMemory);
        Assert.StartsWith("0.10.0", NativeCore.Version);
    }

    [Fact]
    public void Dispose_releases_every_native_document()
    {
        long before = Settle();
        for (int i = 0; i < Count; i++)
        {
            using var doc = new NativeDocument();
            doc.Append(Payload);
            Assert.Equal(Payload.Length, doc.Length);
        }
        Assert.Equal(before, Settle());
    }

    [Fact]
    public void The_finalizer_releases_documents_that_were_not_disposed()
    {
        long before = Settle();
        CreateAndAbandon();
        long during = NativeCore.LiveBytes;
        Assert.True(during > before, "the abandoned documents are counted while alive");
        Assert.Equal(before, Settle());
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static void CreateAndAbandon()
    {
        for (int i = 0; i < Count; i++)
        {
            var doc = new NativeDocument();
            doc.Append(Payload);
        }
    }

    /// <summary>Runs the finalizers until no native memory is pending, then reads the counter.</summary>
    private static long Settle()
    {
        for (int i = 0; i < 3; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }
        return NativeCore.LiveBytes;
    }
}
