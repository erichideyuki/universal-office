using System.Runtime.InteropServices;
using System.Text;

namespace UniversalOffice.Native;

/// <summary>The Sheet exports of the C ABI (core/crates/uo-ffi/src/sheet.rs). The buffer and cancel handles and the status
/// codes are those of <c>UniversalOffice.Docx</c>.</summary>
internal static unsafe partial class SheetNative
{
    internal const string Library = NativeMethods.Library;

    internal const uint Trusted = 1;
    internal const uint ReadFormulas = 1;
    internal const uint ReadStyles = 2;
    internal const uint MergeAllowDataLoss = 1;
    internal const int ArgumentOutOfRange = 9;

    [StructLayout(LayoutKind.Sequential)]
    internal struct Options
    {
        public uint Size;
        public uint Flags;
        public byte* Lang;
        public nuint LangLen;
        public double NowSerial;
        public double Year;
        public ulong Seed;
    }

    [LibraryImport(Library, EntryPoint = "uo_xlsx_open")]
    internal static partial int Open(byte* data, nuint len, Options* options, CancelHandle cancel, out WorkbookHandle workbook);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_new")]
    internal static partial int New(Options* options, out WorkbookHandle workbook);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_free")]
    internal static partial void Free(nint workbook);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_is_legacy")]
    internal static partial int IsLegacy(WorkbookHandle workbook);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_info")]
    internal static partial int Info(WorkbookHandle workbook, out BufferHandle info);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_read_range")]
    internal static partial int ReadRange(WorkbookHandle workbook, uint sheet, uint c1, uint r1, uint c2, uint r2, uint flags, CancelHandle cancel, out BufferHandle cells);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_set_cell_text")]
    internal static partial int SetCellText(WorkbookHandle workbook, uint sheet, uint col, uint row, byte* text, nuint len, out BufferHandle result);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_set_number_format")]
    internal static partial int SetNumberFormat(WorkbookHandle workbook, uint sheet, uint c1, uint r1, uint c2, uint r2, byte* format, nuint len);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_merge")]
    internal static partial int Merge(WorkbookHandle workbook, uint sheet, uint c1, uint r1, uint c2, uint r2, uint kind, uint flags);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_unmerge")]
    internal static partial int Unmerge(WorkbookHandle workbook, uint sheet, uint c1, uint r1, uint c2, uint r2);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_clear")]
    internal static partial int Clear(WorkbookHandle workbook, uint sheet, uint c1, uint r1, uint c2, uint r2, uint kind);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_add_sheet")]
    internal static partial int AddSheet(WorkbookHandle workbook, byte* name, nuint len, out BufferHandle sheetName);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_undo")]
    internal static partial int Undo(WorkbookHandle workbook, byte* did);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_redo")]
    internal static partial int Redo(WorkbookHandle workbook, byte* did);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_recalculate")]
    internal static partial int Recalculate(WorkbookHandle workbook);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_save")]
    internal static partial int Save(WorkbookHandle workbook, CancelHandle cancel, out BufferHandle xlsx);

    [LibraryImport(Library, EntryPoint = "uo_xlsx_mark_saved")]
    internal static partial int MarkSaved(WorkbookHandle workbook);

    [LibraryImport(Library, EntryPoint = "uo_xls_to_xlsx")]
    internal static partial int XlsToXlsx(byte* data, nuint len, Options* options, out BufferHandle xlsx);

    /// <summary>The last error of this thread as the exception the Sheet facade throws: the Docx mapping, plus an out-of-range
    /// argument (9) as <see cref="ArgumentOutOfRangeException"/> and an invalid operation (2) as
    /// <see cref="InvalidOperationException"/>.</summary>
    internal static Exception Fail(int status, string what, CancellationToken ct)
    {
        var e = NativeMethods.ErrorOf(status, what, ct);
        if (e is not UniversalOfficeException u) return e;
        return u.Code switch
        {
            ArgumentOutOfRange => new ArgumentOutOfRangeException(null, u.Message),
            NativeMethods.InvalidOperation => new InvalidOperationException(u.Message),
            _ => e,
        };
    }

    internal static byte[] Utf8(string? s) => Encoding.UTF8.GetBytes(s ?? "");
}

/// <summary>A <c>UoWorkbook</c> (released by <c>uo_xlsx_free</c>).</summary>
internal sealed class WorkbookHandle : NativeHandle
{
    protected override bool ReleaseHandle() { SheetNative.Free(handle); return true; }
}
