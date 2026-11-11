using System.Runtime.InteropServices;
using System.Text;

namespace UniversalOffice.Native;

/// <summary>A UTF-8 string, byte array or packed list pinned for ONE native call. Null is signalled by
/// <c>Len == nuint.MaxValue</c>, never by the pointer (an empty array can pin to a null pointer).
/// Use in a <c>using</c> declaration, and pass <see cref="Ptr"/> and <see cref="Len"/> or <see cref="Str"/>.</summary>
internal readonly unsafe struct Arg : IDisposable
{
    private readonly GCHandle _handle;
    internal readonly byte* Ptr;
    internal readonly nuint Len;

    private Arg(GCHandle handle, byte* ptr, nuint len)
    {
        _handle = handle;
        Ptr = ptr;
        Len = len;
    }

    internal static Arg Null => new(default, null, nuint.MaxValue);

    internal NativeMethods.Str Str => new() { Ptr = Ptr, Len = Len };

    private static Arg Pin(byte[] bytes)
    {
        var h = GCHandle.Alloc(bytes, GCHandleType.Pinned);
        return new Arg(h, (byte*)h.AddrOfPinnedObject(), (nuint)bytes.Length);
    }

    /// <summary>A string; null is the null string. Lone surrogates become U+FFFD (the same as the 0.9 UTF-8 writer).</summary>
    internal static Arg Of(string? s) => s is null ? Null : Pin(Encoding.UTF8.GetBytes(s));

    /// <summary>Bytes (never null).</summary>
    internal static Arg Of(byte[] bytes) => Pin(bytes);

    /// <summary>Bytes of a span (copied, so the span need not stay valid).</summary>
    internal static Arg Of(ReadOnlySpan<byte> bytes) => Pin(bytes.ToArray());

    /// <summary>A packed string list; null is the null list. A null item is an <see cref="ArgumentNullException"/>.</summary>
    internal static Arg List(IEnumerable<string>? items, string paramName)
    {
        if (items is null) return Null;
        var w = new WireWriter();
        var list = items as IReadOnlyCollection<string> ?? items.ToList();
        w.Count(list.Count);
        foreach (var s in list) w.Str(s ?? throw new ArgumentNullException(paramName, "a list item is null"));
        return Pin(w.ToArray());
    }

    /// <summary>Packed (name, value?) pairs.</summary>
    internal static Arg Pairs(IEnumerable<KeyValuePair<string, string?>> items, string paramName)
    {
        var w = new WireWriter();
        var list = items as IReadOnlyCollection<KeyValuePair<string, string?>> ?? items.ToList();
        w.Count(list.Count);
        foreach (var (k, v) in list)
        {
            w.Str(k ?? throw new ArgumentNullException(paramName, "a parameter name is null"));
            w.Str(v);
        }
        return Pin(w.ToArray());
    }

    public void Dispose()
    {
        if (_handle.IsAllocated) _handle.Free();
    }
}

/// <summary>Builds a packed buffer (little-endian; strings are a u32 length, 0xFFFFFFFF = null, then UTF-8).</summary>
internal sealed class WireWriter
{
    private readonly MemoryStream _ms = new();

    internal void U32(uint v)
    {
        Span<byte> b = stackalloc byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(b, v);
        _ms.Write(b);
    }

    internal void Count(int n) => U32((uint)n);

    internal void Str(string? s)
    {
        if (s is null) { U32(uint.MaxValue); return; }
        var bytes = Encoding.UTF8.GetBytes(s);
        U32((uint)bytes.Length);
        _ms.Write(bytes);
    }

    internal byte[] ToArray() => _ms.ToArray();
}

/// <summary>Reads a packed buffer (the Rust side wrote it; a malformed one is an <see cref="InvalidDataException"/>).</summary>
internal sealed class WireReader
{
    private readonly byte[] _b;
    private int _at;

    internal WireReader(byte[] b) => _b = b;

    private ReadOnlySpan<byte> Take(int n)
    {
        if (n < 0 || _at + n > _b.Length) throw new InvalidDataException("malformed native buffer");
        var s = new ReadOnlySpan<byte>(_b, _at, n);
        _at += n;
        return s;
    }

    internal uint U32() => System.Buffers.Binary.BinaryPrimitives.ReadUInt32LittleEndian(Take(4));

    internal byte U8() => Take(1)[0];

    internal int Count() => checked((int)U32());

    internal string? StrOrNull()
    {
        uint n = U32();
        return n == uint.MaxValue ? null : Encoding.UTF8.GetString(Take(checked((int)n)));
    }

    internal string Str() => StrOrNull() ?? throw new InvalidDataException("malformed native buffer");

    internal List<string> StrList()
    {
        int n = Count();
        var list = new List<string>(Math.Min(n, 1024));
        for (int i = 0; i < n; i++) list.Add(Str());
        return list;
    }
}

internal static class BufferExtensions
{
    /// <summary>The UTF-8 text of a buffer (and releases it); null for no buffer.</summary>
    internal static string? TextOrNull(this BufferHandle b)
    {
        using (b)
        {
            return b.IsInvalid ? null : Encoding.UTF8.GetString(b.ToArray());
        }
    }

    internal static string Text(this BufferHandle b) => b.TextOrNull() ?? "";

    /// <summary>The bytes of a buffer (and releases it); null for no buffer.</summary>
    internal static byte[]? BytesOrNull(this BufferHandle b)
    {
        using (b)
        {
            return b.IsInvalid ? null : b.ToArray();
        }
    }

    /// <summary>A reader over a buffer (and releases it).</summary>
    internal static WireReader Reader(this BufferHandle b)
    {
        using (b)
        {
            return new WireReader(b.ToArray());
        }
    }

    internal static List<string> StringList(this BufferHandle b) => b.Reader().StrList();
}
