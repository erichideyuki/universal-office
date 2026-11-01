using System.Runtime.InteropServices;

namespace UniversalOffice.Native;

/// <summary>P/Invoke layer over the C ABI of the Rust core (crate uo-ffi).</summary>
internal static partial class NativeMethods
{
    internal const string Library = "uo_ffi";

    [LibraryImport(Library, EntryPoint = "uo_version")]
    internal static partial nint Version();

    [LibraryImport(Library, EntryPoint = "uo_document_new")]
    internal static partial DocumentHandle DocumentNew();

    [LibraryImport(Library, EntryPoint = "uo_document_append")]
    internal static unsafe partial int DocumentAppend(DocumentHandle doc, byte* data, nuint len);

    [LibraryImport(Library, EntryPoint = "uo_document_len")]
    internal static partial nuint DocumentLen(DocumentHandle doc);

    [LibraryImport(Library, EntryPoint = "uo_document_free")]
    internal static partial void DocumentFree(nint doc);

    [LibraryImport(Library, EntryPoint = "uo_memory_counting")]
    internal static partial int MemoryCounting();

    [LibraryImport(Library, EntryPoint = "uo_memory_live_bytes")]
    internal static partial long MemoryLiveBytes();
}
