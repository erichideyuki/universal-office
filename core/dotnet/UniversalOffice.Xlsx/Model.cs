using System.Globalization;
using System.Text;

namespace UniversalOffice.Xlsx;

/// <summary>A rectangle of cells, 1-based and inclusive as in Excel (<c>A1</c> is column 1, row 1).</summary>
public readonly record struct XlsxRange(int FirstColumn, int FirstRow, int LastColumn, int LastRow)
{
    /// <summary>Excel's last column (XFD).</summary>
    public const int MaxColumns = 16_384;

    /// <summary>Excel's last row.</summary>
    public const int MaxRows = 1_048_576;

    /// <summary>One cell.</summary>
    public static XlsxRange Cell(int column, int row) => new(column, row, column, row);

    /// <summary>Parses <c>B2</c> or <c>B2:D8</c> (a sheet name before a <c>!</c> is not accepted).</summary>
    /// <exception cref="FormatException">not an A1 reference</exception>
    public static XlsxRange Parse(string reference)
    {
        ArgumentNullException.ThrowIfNull(reference);
        var parts = reference.Split(':');
        if (parts.Length is < 1 or > 2) throw new FormatException($"not an A1 reference: {reference}");
        var (c1, r1) = ParseCell(parts[0], reference);
        var (c2, r2) = parts.Length == 2 ? ParseCell(parts[1], reference) : (c1, r1);
        return new XlsxRange(Math.Min(c1, c2), Math.Min(r1, r2), Math.Max(c1, c2), Math.Max(r1, r2));
    }

    private static (int Col, int Row) ParseCell(string s, string whole)
    {
        int i = 0, col = 0;
        while (i < s.Length && s[i] is (>= 'A' and <= 'Z') or (>= 'a' and <= 'z'))
        {
            col = col * 26 + (char.ToUpperInvariant(s[i]) - 'A' + 1);
            if (col > MaxColumns) throw new FormatException($"column out of range: {whole}");
            i++;
        }
        if (i == 0 || i == s.Length || !int.TryParse(s.AsSpan(i), NumberStyles.None, CultureInfo.InvariantCulture, out int row) || row < 1 || row > MaxRows)
            throw new FormatException($"not an A1 reference: {whole}");
        return (col, row);
    }

    /// <summary>The column letters of a 1-based column (<c>1</c> is <c>A</c>, <c>27</c> is <c>AA</c>).</summary>
    public static string ColumnName(int column)
    {
        if (column < 1 || column > MaxColumns) throw new ArgumentOutOfRangeException(nameof(column));
        var sb = new StringBuilder();
        for (int n = column; n > 0; n = (n - 1) / 26) sb.Insert(0, (char)('A' + (n - 1) % 26));
        return sb.ToString();
    }

    /// <summary>The A1 text (<c>B2</c> or <c>B2:D8</c>).</summary>
    public override string ToString() =>
        FirstColumn == LastColumn && FirstRow == LastRow
            ? $"{ColumnName(FirstColumn)}{FirstRow}"
            : $"{ColumnName(FirstColumn)}{FirstRow}:{ColumnName(LastColumn)}{LastRow}";
}

/// <summary>What a cell holds.</summary>
public enum XlsxCellKind
{
    /// <summary>No value (a styled or cleared cell, a formula that gives nothing).</summary>
    Empty = 0,
    /// <summary>A number (dates and times are numbers: see <see cref="XlsxCell.Display"/>).</summary>
    Number = 1,
    /// <summary>Text.</summary>
    Text = 2,
    /// <summary>TRUE or FALSE.</summary>
    Boolean = 3,
    /// <summary>An Excel error such as <c>#DIV/0!</c> (shown in the workbook's language).</summary>
    Error = 4,
}

/// <summary>The horizontal alignment of a cell.</summary>
public enum XlsxHorizontalAlignment
{
    /// <summary>The style does not say.</summary>
    None = 0,
    General,
    Left,
    Center,
    Right,
    Fill,
    Justify,
    CenterContinuous,
    Distributed,
    Other,
}

/// <summary>The style essentials of a cell (<see cref="XlsxReadOptions.Styles"/>).</summary>
public sealed class XlsxCellStyle
{
    /// <summary>The index of the cell format in the workbook (<c>cellXfs</c>).</summary>
    public int FormatIndex { get; init; }
    /// <summary>The number format code (<c>General</c>, <c>0.00</c>, <c>dd/mm/yyyy</c>…), in the workbook's own notation.</summary>
    public string NumberFormat { get; init; } = "General";
    public bool Bold { get; init; }
    public bool Italic { get; init; }
    public bool Underline { get; init; }
    public bool Strikethrough { get; init; }
    public bool WrapText { get; init; }
    public XlsxHorizontalAlignment HorizontalAlignment { get; init; }
    /// <summary>The font size in points; null when the format has none.</summary>
    public double? FontSize { get; init; }
    public string FontName { get; init; } = "";
    /// <summary>The font colour as the workbook resolved it (<c>#RRGGBB</c> or empty).</summary>
    public string FontColor { get; init; } = "";
    /// <summary>The solid fill colour (empty: none).</summary>
    public string FillColor { get; init; } = "";
}

/// <summary>A cell that exists in the workbook.</summary>
public sealed class XlsxCell
{
    /// <summary>1-based row.</summary>
    public int Row { get; init; }
    /// <summary>1-based column.</summary>
    public int Column { get; init; }
    /// <summary>The A1 reference (<c>B2</c>).</summary>
    public string Reference => $"{XlsxRange.ColumnName(Column)}{Row}";
    public XlsxCellKind Kind { get; init; }
    /// <summary>The number when <see cref="Kind"/> is <see cref="XlsxCellKind.Number"/> (1 or 0 for a boolean).</summary>
    public double Number { get; init; }
    /// <summary>The text of a text cell, or the error as shown (<c>#DIV/0!</c>, <c>#NOME?</c>).</summary>
    public string Text { get; init; } = "";
    /// <summary>The cell as the sheet shows it, in a column wide enough: numbers and dates formatted by the cell's number format
    /// (the core formats with the pt-BR locale of Excel whatever the workbook's language), booleans and errors in the language.</summary>
    public string Display { get; init; } = "";
    /// <summary>The value as a .NET object: <see cref="double"/>, <see cref="string"/>, <see cref="bool"/>, or null for an empty
    /// cell; an error is its text.</summary>
    public object? Value => Kind switch
    {
        XlsxCellKind.Number => Number,
        XlsxCellKind.Boolean => Number != 0,
        XlsxCellKind.Text or XlsxCellKind.Error => Text,
        _ => null,
    };
    /// <summary>The cell holds a formula (<see cref="XlsxReadOptions.Formulas"/>).</summary>
    public bool HasFormula { get; init; }
    /// <summary>An array formula entered with Ctrl+Shift+Enter (shown in braces in <see cref="Formula"/>).</summary>
    public bool IsArrayFormula { get; init; }
    /// <summary>A cell filled by the dynamic array of another cell (<see cref="Formula"/> is the anchor's).</summary>
    public bool IsSpilled { get; init; }
    /// <summary>The formula in the workbook's language, as typed in Excel (<c>=SOMA(A1:A3)</c> in pt-BR).</summary>
    public string Formula { get; init; } = "";
    /// <summary>The formula in the canonical (English, comma) notation (<c>=SUM(A1:A3)</c>).</summary>
    public string CanonicalFormula { get; init; } = "";
    /// <summary>The style essentials (<see cref="XlsxReadOptions.Styles"/>), else null.</summary>
    public XlsxCellStyle? Style { get; init; }
}

/// <summary>What <see cref="XlsxWorkbook.ReadRange"/> reads besides the value and the displayed text.</summary>
[Flags]
public enum XlsxReadOptions
{
    None = 0,
    /// <summary>The formula of each cell (builds the calculation engine of the workbook on the first formula).</summary>
    Formulas = 1,
    /// <summary>The style essentials of each cell.</summary>
    Styles = 2,
}

/// <summary>Whether a sheet shows.</summary>
public enum XlsxSheetState
{
    Visible = 0,
    Hidden = 1,
    VeryHidden = 2,
}

/// <summary>A worksheet.</summary>
public sealed class XlsxSheetInfo
{
    /// <summary>0-based, in workbook order (chart sheets are not listed).</summary>
    public int Index { get; init; }
    public string Name { get; init; } = "";
    public XlsxSheetState State { get; init; }
    /// <summary>Rows up to the last one that has a cell (0: the sheet has no cell).</summary>
    public int UsedRows { get; init; }
    /// <summary>Columns up to the last one that has a cell.</summary>
    public int UsedColumns { get; init; }
    public int FrozenColumns { get; init; }
    public int FrozenRows { get; init; }
    public bool ShowGridlines { get; init; }
    public IReadOnlyList<XlsxRange> MergedRanges { get; init; } = [];

    /// <summary>The used area, or null for an empty sheet.</summary>
    public XlsxRange? UsedRange => UsedRows == 0 || UsedColumns == 0 ? null : new XlsxRange(1, 1, UsedColumns, UsedRows);
}

/// <summary>The outcome of entering a text in a cell: Excel refuses some entries (an invalid formula) and says why.</summary>
public readonly record struct XlsxEditResult(bool Accepted, string Message);

/// <summary>How Merge combines the cells.</summary>
public enum XlsxMergeKind
{
    /// <summary>Merge &amp; Center.</summary>
    Center = 0,
    /// <summary>One merge per row.</summary>
    Across = 1,
    /// <summary>Merge Cells.</summary>
    Cells = 2,
}

/// <summary>What Clear removes.</summary>
public enum XlsxClearKind
{
    Contents = 0,
    Formats = 1,
    All = 2,
}

/// <summary>Options of opening or creating a workbook.</summary>
public sealed class XlsxOpenOptions
{
    /// <summary>The language of the formulas typed and read back, and of the booleans and errors shown: <c>pt-BR</c> (the
    /// default, the data-fidelity locale) or <c>en-US</c>; other tags fall back to the closest table. Fixed for the life of the
    /// workbook.</summary>
    public string Language { get; set; } = "pt-BR";

    /// <summary>The workbook comes from a trusted source (the default container and reader limits). The default, false, holds an
    /// upload to 256 MiB per inflated part and 512 MiB in all (checked on the declared sizes before anything is inflated) and the
    /// reader's bounds of the browser build.</summary>
    public bool Trusted { get; set; }

    /// <summary>The local time <c>NOW()</c>/<c>TODAY()</c> and a date typed without a year take; null: the machine's.</summary>
    public DateTime? Now { get; set; }

    /// <summary>The seed of <c>RAND</c>/<c>RANDBETWEEN</c> (values differ from Excel's by nature).</summary>
    public ulong RandomSeed { get; set; } = 1;
}
