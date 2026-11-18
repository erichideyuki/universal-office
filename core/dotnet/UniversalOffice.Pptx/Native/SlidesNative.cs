using System.Runtime.InteropServices;

namespace UniversalOffice.Native;

/// <summary>The Slides exports of the C ABI (core/crates/uo-ffi/src/slides.rs). The converter, buffer and cancel
/// handles and the status codes are those of <c>UniversalOffice.Docx</c>.</summary>
internal static unsafe partial class SlidesNative
{
    internal const string Library = NativeMethods.Library;

    internal const uint DeckTrusted = 1;
    /// <summary>`DECK_TS_STRICT`: a .ppt converted as the frozen TS did (parity tests only).</summary>
    internal const uint DeckTsStrict = 2;
    internal const uint JobCompress = 1;
    /// <summary>`SLIDES_JOB_TS_STRICT`: the frozen TS Slides for this PDF (parity tests only).</summary>
    internal const uint JobTsStrict = 2;

    [StructLayout(LayoutKind.Sequential)]
    internal struct DeckOptions
    {
        public uint Size;
        public uint Flags;
    }

    [StructLayout(LayoutKind.Sequential)]
    internal struct SlidesJob
    {
        public uint Size;
        public uint Flags;
        public ulong Reserved;
    }

    [LibraryImport(Library, EntryPoint = "uo_deck_open")]
    internal static partial int DeckOpen(byte* data, nuint len, DeckOptions* options, out DeckHandle deck);

    [LibraryImport(Library, EntryPoint = "uo_deck_free")]
    internal static partial void DeckFree(nint deck);

    [LibraryImport(Library, EntryPoint = "uo_deck_is_legacy")]
    internal static partial int DeckIsLegacy(DeckHandle deck);

    [LibraryImport(Library, EntryPoint = "uo_deck_info")]
    internal static partial int DeckInfo(DeckHandle deck, uint* count, double* widthEmu, double* heightEmu);

    [LibraryImport(Library, EntryPoint = "uo_deck_slide_notes")]
    internal static partial int DeckSlideNotes(DeckHandle deck, uint index, out BufferHandle notes);

    [LibraryImport(Library, EntryPoint = "uo_deck_save")]
    internal static partial int DeckSave(DeckHandle deck, out BufferHandle pptx);

    [LibraryImport(Library, EntryPoint = "uo_deck_to_pdf")]
    internal static partial int DeckToPdf(DeckHandle deck, ConverterHandle converter, SlidesJob* job, CancelHandle cancel, out BufferHandle pdf);

    [LibraryImport(Library, EntryPoint = "uo_pptx_to_pdf")]
    internal static partial int PptxToPdf(ConverterHandle converter, byte* data, nuint len, SlidesJob* job, CancelHandle cancel, out BufferHandle pdf);

    [LibraryImport(Library, EntryPoint = "uo_ppt_to_pptx")]
    internal static partial int PptToPptx(byte* data, nuint len, DeckOptions* options, out BufferHandle pptx);
}

/// <summary>A <c>UoDeck</c> (released by <c>uo_deck_free</c>).</summary>
internal sealed class DeckHandle : NativeHandle
{
    protected override bool ReleaseHandle() { SlidesNative.DeckFree(handle); return true; }
}
