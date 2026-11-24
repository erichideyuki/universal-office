using System.Buffers.Binary;
using System.Text;
using UniversalOffice.Xlsx;

namespace UniversalOffice.Native;

/// <summary>Reads the buffers of core/crates/uo-ffi/src/sheet.rs (little-endian; a string is a u32 byte length, 0xFFFFFFFF = null,
/// then UTF-8). A malformed buffer is an <see cref="InvalidDataException"/>.</summary>
internal sealed class SheetWire
{
    private readonly byte[] _b;
    private int _at;

    internal SheetWire(byte[] b) => _b = b;

    private ReadOnlySpan<byte> Take(int n)
    {
        if (n < 0 || _at + n > _b.Length) throw new InvalidDataException("malformed native buffer");
        var s = new ReadOnlySpan<byte>(_b, _at, n);
        _at += n;
        return s;
    }

    internal byte U8() => Take(1)[0];

    internal uint U32() => BinaryPrimitives.ReadUInt32LittleEndian(Take(4));

    internal double F64() => BitConverter.Int64BitsToDouble(BinaryPrimitives.ReadInt64LittleEndian(Take(8)));

    internal string? StrOrNull()
    {
        uint n = U32();
        return n == uint.MaxValue ? null : Encoding.UTF8.GetString(Take(checked((int)n)));
    }

    internal string Str() => StrOrNull() ?? throw new InvalidDataException("malformed native buffer");

    internal bool AtEnd => _at == _b.Length;

    /// <summary>The state buffer of <c>uo_xlsx_info</c>.</summary>
    internal static WorkbookState ParseInfo(byte[] bytes)
    {
        var r = new SheetWire(bytes);
        if (r.U32() != 1) throw new InvalidDataException("unknown native info version");
        int active = checked((int)r.U32());
        bool date1904 = r.U8() != 0, dirty = r.U8() != 0, canUndo = r.U8() != 0, canRedo = r.U8() != 0;
        string? undo = r.StrOrNull(), redo = r.StrOrNull();
        int n = checked((int)r.U32());
        var sheets = new List<XlsxSheetInfo>(Math.Min(n, 1024));
        for (int i = 0; i < n; i++)
        {
            string name = r.Str();
            var state = (XlsxSheetState)r.U8();
            int rows = checked((int)r.U32()), cols = checked((int)r.U32());
            int fc = checked((int)r.U32()), fr = checked((int)r.U32());
            bool grid = r.U8() != 0;
            int m = checked((int)r.U32());
            var merges = new List<XlsxRange>(Math.Min(m, 4096));
            for (int k = 0; k < m; k++) merges.Add(XlsxRange.Parse(r.Str()));
            sheets.Add(new XlsxSheetInfo
            {
                Index = i, Name = name, State = state, UsedRows = rows, UsedColumns = cols,
                FrozenColumns = fc, FrozenRows = fr, ShowGridlines = grid, MergedRanges = merges,
            });
        }
        if (!r.AtEnd) throw new InvalidDataException("malformed native buffer");
        return new WorkbookState(active, date1904, dirty, canUndo, canRedo, undo, redo, sheets);
    }

    /// <summary>The cell buffer of <c>uo_xlsx_read_range</c> read with the same flags.</summary>
    internal static List<XlsxCell> ParseCells(byte[] bytes, uint flags)
    {
        var r = new SheetWire(bytes);
        int n = checked((int)r.U32());
        var cells = new List<XlsxCell>(Math.Min(n, 1 << 16));
        for (int i = 0; i < n; i++)
        {
            int row = checked((int)r.U32()), col = checked((int)r.U32());
            var kind = (XlsxCellKind)r.U8();
            byte bits = r.U8();
            double number = r.F64();
            string text = r.Str(), display = r.Str();
            string local = "", canon = "";
            XlsxCellStyle? style = null;
            if ((flags & SheetNative.ReadFormulas) != 0)
            {
                local = r.Str();
                canon = r.Str();
            }
            if ((flags & SheetNative.ReadStyles) != 0)
            {
                int xf = checked((int)r.U32());
                string code = r.Str();
                byte sb = r.U8();
                var align = (XlsxHorizontalAlignment)r.U8();
                double size = r.F64();
                string font = r.Str(), color = r.Str(), fill = r.Str();
                style = new XlsxCellStyle
                {
                    FormatIndex = xf, NumberFormat = code,
                    Bold = (sb & 1) != 0, Italic = (sb & 2) != 0, Underline = (sb & 4) != 0, Strikethrough = (sb & 8) != 0, WrapText = (sb & 16) != 0,
                    HorizontalAlignment = align, FontSize = double.IsNaN(size) ? null : size, FontName = font, FontColor = color, FillColor = fill,
                };
            }
            cells.Add(new XlsxCell
            {
                Row = row, Column = col, Kind = kind, Number = number, Text = text, Display = display,
                HasFormula = (bits & 1) != 0, IsArrayFormula = (bits & 2) != 0, IsSpilled = (bits & 4) != 0,
                Formula = local, CanonicalFormula = canon, Style = style,
            });
        }
        if (!r.AtEnd) throw new InvalidDataException("malformed native buffer");
        return cells;
    }
}

/// <summary>The decoded <c>uo_xlsx_info</c> buffer.</summary>
internal sealed record WorkbookState(
    int Active, bool Date1904, bool Dirty, bool CanUndo, bool CanRedo, string? UndoLabel, string? RedoLabel, IReadOnlyList<XlsxSheetInfo> Sheets);
