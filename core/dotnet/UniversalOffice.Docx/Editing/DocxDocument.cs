using UniversalOffice.Core;
using UniversalOffice.Native;

namespace UniversalOffice.Docx.Editing;

/// <summary>A relationship of a part.</summary>
public sealed record DocxRelationship(string Id, string Type, string Target, bool External);

/// <summary>
/// A .docx package opened for editing: the Rust core holds the document (parsed on demand), this
/// object holds its native handle. Every call takes a per-document lock, so a document may be
/// shared by threads (one call at a time); a call after <see cref="Dispose"/> throws
/// <see cref="ObjectDisposedException"/>. A host that never disposes still frees the native memory
/// through the finalizer of the handle.
/// </summary>
public sealed unsafe class DocxDocument : IDisposable
{
    private static long s_order;

    internal readonly object Gate = new();

    /// <summary>The order two documents are locked in (ImportBody), so two threads cannot deadlock.</summary>
    internal readonly long Order = Interlocked.Increment(ref s_order);

    private readonly DocxHandle _handle;
    private bool _disposed;
    private long _pressure;

    private DocxDocument(DocxHandle handle, DocxLimits limits)
    {
        _handle = handle;
        Limits = limits;
        EditingErrors.Check(NativeMethods.DocxMainPartPath(_handle, out var path));
        MainPartPath = path.Text();
        // native memory is invisible to the GC: tell it what this document holds
        long bytes = (long)Math.Min(NativeMethods.DocxNativeBytes(_handle), long.MaxValue / 2);
        if (bytes > 0)
        {
            _pressure = bytes;
            GC.AddMemoryPressure(bytes);
        }
    }

    ~DocxDocument() => RemovePressure();

    private void RemovePressure()
    {
        long p = Interlocked.Exchange(ref _pressure, 0);
        if (p > 0) GC.RemoveMemoryPressure(p);
    }

    /// <summary>Path of the main document part (usually <c>word/document.xml</c>).</summary>
    public string MainPartPath { get; }

    /// <summary>The resource caps this document was opened with.</summary>
    public DocxLimits Limits { get; }

    /// <summary>Opens a .docx with the default caps.</summary>
    public static DocxDocument Open(byte[] docx) => Open(docx, DocxLimits.Default);

    /// <summary>Opens a .docx; exceeding a cap throws <see cref="OfficeLimitExceededException"/>.</summary>
    public static DocxDocument Open(byte[] docx, DocxLimits limits)
    {
        ArgumentNullException.ThrowIfNull(docx);
        ArgumentNullException.ThrowIfNull(limits);
        var l = new NativeMethods.UoDocxLimits
        {
            Size = (uint)sizeof(NativeMethods.UoDocxLimits),
            MaxPartBytes = Cap(limits.Package.MaxPartBytes),
            MaxTotalBytes = Cap(limits.Package.MaxTotalBytes),
            MaxXmlDepth = NonNegative(limits.MaxXmlDepth),
            MaxFieldsPerStory = NonNegative(limits.MaxFieldsPerStory),
            MaxFieldNesting = NonNegative(limits.MaxFieldNesting),
        };
        DocxHandle handle;
        fixed (byte* p = docx)
        {
            EditingErrors.Check(NativeMethods.DocxOpen(p, (nuint)docx.Length, &l, out handle));
        }
        return new DocxDocument(handle, limits);
    }

    // a negative cap becomes 0 (decision D3A-11); null is no cap
    private static ulong Cap(long? v) => v is null ? ulong.MaxValue : v.Value < 0 ? 0 : (ulong)v.Value;

    private static uint NonNegative(int v) => v < 0 ? 0 : (uint)v;

    internal void ThrowIfDisposed() => ObjectDisposedException.ThrowIf(_disposed, this);

    /// <summary>Runs <paramref name="body"/> with the lock held and the handle checked.</summary>
    internal T With<T>(Func<DocxHandle, T> body)
    {
        lock (Gate)
        {
            ThrowIfDisposed();
            return body(_handle);
        }
    }

    internal void With(Action<DocxHandle> body) => With<object?>(h => { body(h); return null; });

    /// <summary>Releases the native document. Safe to call twice.</summary>
    public void Dispose()
    {
        lock (Gate)
        {
            if (_disposed) return;
            _disposed = true;
            _handle.Dispose();
        }
        RemovePressure();
        GC.SuppressFinalize(this);
    }

    /// <summary>The package as .docx bytes (parts that were not touched are written as they came).</summary>
    public byte[] Save() => With(h =>
    {
        EditingErrors.Check(NativeMethods.DocxSave(h, out var b));
        return b.BytesOrNull() ?? [];
    });

    /// <summary>An independent copy (<c>Open(Save(), Limits)</c>).</summary>
    public DocxDocument Clone()
    {
        var h = With(h =>
        {
            EditingErrors.Check(NativeMethods.DocxClone(h, out var c));
            return c;
        });
        return new DocxDocument(h, Limits);
    }

    /// <summary>Names of the parts (without the leading slash).</summary>
    public IReadOnlyList<string> PartNames => With(h =>
    {
        EditingErrors.Check(NativeMethods.DocxPartNames(h, out var b));
        return (IReadOnlyList<string>)b.StringList();
    });

    public bool HasPart(string path)
    {
        ArgumentNullException.ThrowIfNull(path);
        using var a = Arg.Of(path);
        return With(h =>
        {
            EditingErrors.Check(NativeMethods.DocxHasPart(h, a.Ptr, a.Len, out int has));
            return has != 0;
        });
    }

    /// <summary>The bytes of a part, or null when there is none.</summary>
    public byte[]? GetPartBytes(string path)
    {
        ArgumentNullException.ThrowIfNull(path);
        using var a = Arg.Of(path);
        return With(h =>
        {
            EditingErrors.Check(NativeMethods.DocxGetPartBytes(h, a.Ptr, a.Len, out var b));
            return b.BytesOrNull();
        });
    }

    public IReadOnlyList<DocxRelationship> GetRelationships(string partPath)
    {
        ArgumentNullException.ThrowIfNull(partPath);
        using var a = Arg.Of(partPath);
        return With(h =>
        {
            EditingErrors.Check(NativeMethods.DocxGetRelationships(h, a.Ptr, a.Len, out var b));
            var r = b.Reader();
            int n = r.Count();
            var list = new List<DocxRelationship>(Math.Min(n, 1024));
            for (int i = 0; i < n; i++) list.Add(ReadRelationship(r));
            return (IReadOnlyList<DocxRelationship>)list;
        });
    }

    public DocxRelationship? GetRelationship(string partPath, string id)
    {
        ArgumentNullException.ThrowIfNull(partPath);
        ArgumentNullException.ThrowIfNull(id);
        using var a = Arg.Of(partPath);
        using var i = Arg.Of(id);
        return With(h =>
        {
            EditingErrors.Check(NativeMethods.DocxGetRelationship(h, a.Ptr, a.Len, i.Ptr, i.Len, out var b));
            var bytes = b.BytesOrNull();
            return bytes is null ? null : ReadRelationship(new WireReader(bytes));
        });
    }

    private static DocxRelationship ReadRelationship(WireReader r) => new(r.Str(), r.Str(), r.Str(), r.U8() != 0);

    /// <summary>The content type of a part (its override or the default of its extension), or null.</summary>
    public string? GetContentType(string partPath)
    {
        ArgumentNullException.ThrowIfNull(partPath);
        using var a = Arg.Of(partPath);
        return With(h =>
        {
            EditingErrors.Check(NativeMethods.DocxGetContentType(h, a.Ptr, a.Len, out var b));
            return b.TextOrNull();
        });
    }

    /// <summary>The document properties (core.xml, app.xml, custom.xml).</summary>
    public DocxProperties Properties => new(this);
}
