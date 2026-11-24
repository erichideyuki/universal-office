using System.Text;
using UniversalOffice.Native;

namespace UniversalOffice.Xlsx;

/// <summary>An open workbook (.xlsx, or an Excel 97-2003 .xls converted on opening) held by the Rust core: the cells, the
/// calculation engine, the edit history. Dispose it (or let the finalizer) to release the native memory. Calls may come from any
/// thread; the core runs one at a time per workbook. Rows and columns are 1-based as in Excel, sheets 0-based.</summary>
/// <remarks>Formulas and entries are in the workbook's language (<see cref="XlsxOpenOptions.Language"/>): <c>=SOMA(A1;A2)</c> and
/// <c>12,5</c> in pt-BR, <c>=SUM(A1,A2)</c> and <c>12.5</c> in en-US. Reading a cell never changes the workbook; reading its
/// formula builds the calculation engine, which makes a later save recalculate what Excel would on opening.</remarks>
public sealed class XlsxWorkbook : IDisposable
{
    private static readonly CancelHandle NoCancel = new();
    private readonly WorkbookHandle _handle;
    private readonly object _gate = new();
    private WorkbookState? _state;

    private XlsxWorkbook(WorkbookHandle handle, bool legacy)
    {
        _handle = handle;
        IsLegacyXls = legacy;
    }

    // ───────────────────────────── open ─────────────────────────────

    /// <summary>Opens a workbook from its bytes: a .xlsx, or an Excel 97-2003 .xls (detected by its signature). The open-recalc
    /// of the file is applied.</summary>
    /// <param name="bytes">the file</param>
    /// <param name="options">language, trust, clock; null for the defaults (pt-BR, untrusted)</param>
    /// <param name="ct">checked by the native core before the work and before the workbook comes back</param>
    /// <exception cref="InvalidDataException">not a workbook, a damaged or password-protected one</exception>
    /// <exception cref="UniversalOffice.Core.OfficeLimitExceededException">a limit for untrusted workbooks</exception>
    /// <exception cref="OperationCanceledException"><paramref name="ct"/> was cancelled</exception>
    public static unsafe XlsxWorkbook Open(byte[] bytes, XlsxOpenOptions? options = null, CancellationToken ct = default)
    {
        ArgumentNullException.ThrowIfNull(bytes);
        ct.ThrowIfCancellationRequested();
        byte[] lang = SheetNative.Utf8(options?.Language);
        using var cancel = ct.CanBeCanceled ? NativeMethods.CancelNew() : null;
        using var registration = cancel is null ? default : ct.Register(static c => NativeMethods.CancelRaise((CancelHandle)c!), cancel);
        int status;
        WorkbookHandle handle;
        fixed (byte* data = bytes)
        fixed (byte* l = lang)
        {
            var o = Native(options, l, lang.Length);
            status = SheetNative.Open(data, (nuint)bytes.Length, &o, cancel ?? NoCancel, out handle);
        }
        if (status != NativeMethods.Ok) { handle.Dispose(); throw SheetNative.Fail(status, "Open", ct); }
        return new XlsxWorkbook(handle, SheetNative.IsLegacy(handle) == 1);
    }

    /// <summary>A blank workbook with one sheet (<c>Planilha1</c> in Portuguese, <c>Sheet1</c> otherwise).</summary>
    public static unsafe XlsxWorkbook Create(XlsxOpenOptions? options = null)
    {
        byte[] lang = SheetNative.Utf8(options?.Language);
        int status;
        WorkbookHandle handle;
        fixed (byte* l = lang)
        {
            var o = Native(options, l, lang.Length);
            status = SheetNative.New(&o, out handle);
        }
        if (status != NativeMethods.Ok) { handle.Dispose(); throw SheetNative.Fail(status, "Create", default); }
        return new XlsxWorkbook(handle, false);
    }

    internal static unsafe SheetNative.Options Native(XlsxOpenOptions? options, byte* lang, int langLen)
    {
        var now = options?.Now ?? DateTime.Now;
        return new SheetNative.Options
        {
            Size = (uint)sizeof(SheetNative.Options),
            Flags = options?.Trusted == true ? SheetNative.Trusted : 0,
            Lang = lang,
            LangLen = (nuint)langLen,
            NowSerial = now.ToOADate(),
            Year = now.Year,
            Seed = options?.RandomSeed ?? 1,
        };
    }

    /// <summary>The workbook was an Excel 97-2003 .xls (converted to the parts of a .xlsx on opening).</summary>
    public bool IsLegacyXls { get; }

    // ───────────────────────────── state ─────────────────────────────

    private WorkbookState State()
    {
        lock (_gate)
        {
            if (_state is not null) return _state;
            int status = SheetNative.Info(_handle, out var buf);
            using (buf)
            {
                if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Info", default);
                return _state = SheetWire.ParseInfo(buf.ToArray());
            }
        }
    }

    private void Changed()
    {
        lock (_gate) _state = null;
    }

    /// <summary>The worksheets, in workbook order (chart sheets are not listed). Refreshed after every edit.</summary>
    public IReadOnlyList<XlsxSheetInfo> Sheets => State().Sheets;

    /// <summary>The index of the sheet the file opens on (and of the one the last edit selected).</summary>
    public int ActiveSheetIndex => State().Active;

    /// <summary>The 1904 date system.</summary>
    public bool Date1904 => State().Date1904;

    /// <summary>The workbook was edited since it was opened or <see cref="MarkSaved"/>.</summary>
    public bool IsDirty => State().Dirty;

    public bool CanUndo => State().CanUndo;

    public bool CanRedo => State().CanRedo;

    /// <summary>What <see cref="Undo"/> would undo (the core's own label, English), or null.</summary>
    public string? UndoLabel => State().UndoLabel;

    public string? RedoLabel => State().RedoLabel;

    /// <summary>The index of the sheet with this name (case-insensitive, as Excel), or -1.</summary>
    public int IndexOfSheet(string name)
    {
        ArgumentNullException.ThrowIfNull(name);
        foreach (var s in Sheets) if (string.Equals(s.Name, name, StringComparison.OrdinalIgnoreCase)) return s.Index;
        return -1;
    }

    // ───────────────────────────── read ─────────────────────────────

    /// <summary>The cells that exist in a range, row by row (an absent cell is not listed). At most 1,048,576 cells per call:
    /// read a larger area by rows, or with <see cref="ReadSheet"/>.</summary>
    /// <exception cref="ArgumentOutOfRangeException">a sheet that does not exist, or a range outside Excel's grid</exception>
    /// <exception cref="UniversalOffice.Core.OfficeLimitExceededException">more cells than one call hands over</exception>
    public IReadOnlyList<XlsxCell> ReadRange(int sheet, XlsxRange range, XlsxReadOptions options = XlsxReadOptions.None, CancellationToken ct = default) =>
        SheetWire.ParseCells(ReadRangeBytes(sheet, range, options, ct), Flags(options));

    /// <summary>One cell, or null when it does not exist.</summary>
    public XlsxCell? GetCell(int sheet, int column, int row, XlsxReadOptions options = XlsxReadOptions.None) =>
        ReadRange(sheet, XlsxRange.Cell(column, row), options).FirstOrDefault();

    /// <summary>Every cell of a sheet, row by row, read 500 rows at a time (lazy: stop whenever you like).</summary>
    public IEnumerable<XlsxCell> ReadSheet(int sheet, XlsxReadOptions options = XlsxReadOptions.None, CancellationToken ct = default)
    {
        const int Page = 500;
        var used = Sheets.ElementAtOrDefault(sheet) ?? throw new ArgumentOutOfRangeException(nameof(sheet), sheet, "no such sheet");
        if (used.UsedRange is not { } all) yield break;
        for (int r = 1; r <= all.LastRow; r += Page)
            foreach (var c in ReadRange(sheet, new XlsxRange(1, r, all.LastColumn, Math.Min(r + Page - 1, all.LastRow)), options, ct))
                yield return c;
    }

    internal static uint Flags(XlsxReadOptions o) =>
        ((o & XlsxReadOptions.Formulas) != 0 ? SheetNative.ReadFormulas : 0) | ((o & XlsxReadOptions.Styles) != 0 ? SheetNative.ReadStyles : 0);

    internal byte[] ReadRangeBytes(int sheet, XlsxRange r, XlsxReadOptions options, CancellationToken ct)
    {
        CheckSheet(sheet);
        ct.ThrowIfCancellationRequested();
        using var cancel = ct.CanBeCanceled ? NativeMethods.CancelNew() : null;
        using var registration = cancel is null ? default : ct.Register(static c => NativeMethods.CancelRaise((CancelHandle)c!), cancel);
        int status = SheetNative.ReadRange(_handle, (uint)sheet, U(r.FirstColumn), U(r.FirstRow), U(r.LastColumn), U(r.LastRow), Flags(options), cancel ?? NoCancel, out var buf);
        // reading builds the engine on the first formula: the state (dirty sets, labels) does not change, nothing to refresh
        using (buf)
        {
            if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "ReadRange", ct);
            return buf.ToArray();
        }
    }

    internal byte[] InfoBytes()
    {
        int status = SheetNative.Info(_handle, out var buf);
        using (buf)
        {
            if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Info", default);
            return buf.ToArray();
        }
    }

    private static uint U(int v) => v < 0 ? uint.MaxValue : (uint)v;

    private static void CheckSheet(int sheet)
    {
        if (sheet < 0) throw new ArgumentOutOfRangeException(nameof(sheet), sheet, "a sheet index is 0-based");
    }

    // ───────────────────────────── edit ─────────────────────────────

    /// <summary>Enters a text in a cell as typed in Excel (one undo step): a formula in the workbook's language, a number with the
    /// decimal separator of the language, a date, <c>'text</c> to force text, anything else as text. A refusal Excel would show
    /// (an invalid formula) is returned, not thrown.</summary>
    /// <exception cref="ArgumentOutOfRangeException">a sheet or cell that does not exist, or a text over 32,767 characters</exception>
    public unsafe XlsxEditResult SetCellText(int sheet, int column, int row, string text)
    {
        ArgumentNullException.ThrowIfNull(text);
        CheckSheet(sheet);
        byte[] t = SheetNative.Utf8(text);
        int status;
        BufferHandle buf;
        fixed (byte* p = t) status = SheetNative.SetCellText(_handle, (uint)sheet, U(column), U(row), p, (nuint)t.Length, out buf);
        Changed();
        using (buf)
        {
            if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "SetCellText", default);
            var r = new SheetWire(buf.ToArray());
            bool ok = r.U8() != 0;
            return new XlsxEditResult(ok, r.Str());
        }
    }

    /// <summary><see cref="SetCellText(int, int, int, string)"/> by A1 reference (<c>B2</c>).</summary>
    public XlsxEditResult SetCellText(int sheet, string reference, string text)
    {
        var r = XlsxRange.Parse(reference);
        if (r.FirstColumn != r.LastColumn || r.FirstRow != r.LastRow) throw new ArgumentException("a single cell is expected", nameof(reference));
        return SetCellText(sheet, r.FirstColumn, r.FirstRow, text);
    }

    /// <summary>Sets the number format of a range to a format code (<c>0.00</c>, <c>dd/mm/yyyy</c>, <c>#,##0</c>…; one undo step).</summary>
    public unsafe void SetNumberFormat(int sheet, XlsxRange range, string formatCode)
    {
        ArgumentNullException.ThrowIfNull(formatCode);
        CheckSheet(sheet);
        byte[] t = SheetNative.Utf8(formatCode);
        int status;
        fixed (byte* p = t) status = SheetNative.SetNumberFormat(_handle, (uint)sheet, U(range.FirstColumn), U(range.FirstRow), U(range.LastColumn), U(range.LastRow), p, (nuint)t.Length);
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "SetNumberFormat", default);
    }

    /// <summary>Merges a range (one undo step).</summary>
    /// <param name="sheet">0-based sheet index</param>
    /// <param name="range">the cells to merge</param>
    /// <param name="kind">how to combine them</param>
    /// <param name="allowDataLoss">merge although cells but the first hold data (Excel asks; the data is discarded)</param>
    /// <exception cref="InvalidOperationException">cells but the first hold data and <paramref name="allowDataLoss"/> is false</exception>
    public void Merge(int sheet, XlsxRange range, XlsxMergeKind kind = XlsxMergeKind.Center, bool allowDataLoss = false)
    {
        CheckSheet(sheet);
        int status = SheetNative.Merge(_handle, (uint)sheet, U(range.FirstColumn), U(range.FirstRow), U(range.LastColumn), U(range.LastRow), (uint)kind, allowDataLoss ? SheetNative.MergeAllowDataLoss : 0);
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Merge", default);
    }

    /// <summary>Unmerges the merged ranges the range touches.</summary>
    public void Unmerge(int sheet, XlsxRange range)
    {
        CheckSheet(sheet);
        int status = SheetNative.Unmerge(_handle, (uint)sheet, U(range.FirstColumn), U(range.FirstRow), U(range.LastColumn), U(range.LastRow));
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Unmerge", default);
    }

    /// <summary>Clears a range (one undo step).</summary>
    public void Clear(int sheet, XlsxRange range, XlsxClearKind kind = XlsxClearKind.Contents)
    {
        CheckSheet(sheet);
        int status = SheetNative.Clear(_handle, (uint)sheet, U(range.FirstColumn), U(range.FirstRow), U(range.LastColumn), U(range.LastRow), (uint)kind);
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Clear", default);
    }

    /// <summary>Adds a sheet at the end and selects it; the name of the new sheet. Null or empty picks the default name
    /// (<c>Planilha2</c>, <c>Sheet2</c>…). As in the Sheet element, inserting a sheet rebuilds the workbook from its saved bytes:
    /// the undo history is emptied.</summary>
    /// <exception cref="ArgumentException">a name over 31 characters or with <c>[ ] : * ? / \</c>, one that is taken (names
    /// compare case-insensitively), or the reserved <c>History</c></exception>
    public unsafe string AddSheet(string? name = null)
    {
        byte[] t = SheetNative.Utf8(name);
        int status;
        BufferHandle buf;
        fixed (byte* p = t) status = SheetNative.AddSheet(_handle, p, (nuint)t.Length, out buf);
        Changed();
        using (buf)
        {
            if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "AddSheet", default);
            return Encoding.UTF8.GetString(buf.ToArray());
        }
    }

    /// <summary>Undoes the last step; false when there was nothing to undo.</summary>
    public unsafe bool Undo()
    {
        byte did = 0;
        int status = SheetNative.Undo(_handle, &did);
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Undo", default);
        return did != 0;
    }

    /// <summary>Redoes the last undone step; false when there was nothing to redo.</summary>
    public unsafe bool Redo()
    {
        byte did = 0;
        int status = SheetNative.Redo(_handle, &did);
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Redo", default);
        return did != 0;
    }

    /// <summary>Recalculates the whole workbook (F9).</summary>
    public void Recalculate()
    {
        int status = SheetNative.Recalculate(_handle);
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Recalculate", default);
    }

    // ───────────────────────────── save ─────────────────────────────

    /// <summary>The workbook as .xlsx bytes: the surgical save (parts of the sheets nobody touched are copied as they were, the
    /// calculation chain and the dependent parts follow the edits; a legacy .xls comes out converted).</summary>
    /// <exception cref="OperationCanceledException"><paramref name="ct"/> was cancelled (checked before the work and before the
    /// bytes come back)</exception>
    public byte[] Save(CancellationToken ct = default)
    {
        ct.ThrowIfCancellationRequested();
        using var cancel = ct.CanBeCanceled ? NativeMethods.CancelNew() : null;
        using var registration = cancel is null ? default : ct.Register(static c => NativeMethods.CancelRaise((CancelHandle)c!), cancel);
        int status = SheetNative.Save(_handle, cancel ?? NoCancel, out var buf);
        Changed();
        using (buf)
        {
            if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "Save", ct);
            return buf.ToArray();
        }
    }

    /// <summary>Clears <see cref="IsDirty"/> (after the host stored what <see cref="Save"/> returned).</summary>
    public void MarkSaved()
    {
        int status = SheetNative.MarkSaved(_handle);
        Changed();
        if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "MarkSaved", default);
    }

    public void Dispose() => _handle.Dispose();
}

/// <summary>Excel 97-2003 .xls to .xlsx.</summary>
public static class XlsConverter
{
    /// <summary>Converts the bytes of a .xls into a .xlsx (cells, styles, formulas, merges, drawings and charts the core's
    /// converter reads).</summary>
    /// <param name="xls">the file</param>
    /// <param name="trusted">the bytes come from a trusted source (the default conversion limits)</param>
    /// <exception cref="InvalidDataException">not an Excel 97-2003 file (a .xlsx is refused), a damaged or password-protected one</exception>
    public static unsafe byte[] ToXlsx(byte[] xls, bool trusted = false)
    {
        ArgumentNullException.ThrowIfNull(xls);
        int status;
        BufferHandle buf;
        fixed (byte* data = xls)
        {
            var o = XlsxWorkbook.Native(new XlsxOpenOptions { Trusted = trusted }, null, 0);
            status = SheetNative.XlsToXlsx(data, (nuint)xls.Length, &o, out buf);
        }
        using (buf)
        {
            if (status != NativeMethods.Ok) throw SheetNative.Fail(status, "ToXlsx", default);
            return buf.ToArray();
        }
    }
}
