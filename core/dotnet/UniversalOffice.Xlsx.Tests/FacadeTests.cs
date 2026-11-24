using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using System.Text;
using UniversalOffice.Core;
using UniversalOffice.Native;
using UniversalOffice.Xlsx;
using Xunit;
using Xunit.Abstractions;

// the memory tests read the process-wide native counters: one test at a time (the soak runs its own threads)
[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace UniversalOffice.Xlsx.Tests;

internal static class Fixtures
{
    internal static readonly string Core = typeof(Fixtures).Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyMetadataAttribute), false)
        .Cast<System.Reflection.AssemblyMetadataAttribute>().Single(a => a.Key == "CoreDir").Value!;
    internal static readonly string Repo = Path.GetFullPath(Path.Combine(Core, ".."));

    /// <summary>The clock and seed of the native digests (uo-ffi tests/sheet_abi.rs: NOW, YEAR, SEED).</summary>
    internal static XlsxOpenOptions Opts(string lang = "pt-BR", bool trusted = false) =>
        new() { Language = lang, Trusted = trusted, Now = DateTime.FromOADate(45672.5), RandomSeed = 1 };

    internal static byte[] Read(string repoRelative) => File.ReadAllBytes(Path.Combine(Repo, repoRelative.Replace('/', Path.DirectorySeparatorChar)));

    internal static string Book(int n) => $"core/crates/uo-xlsx/tests/captures/workbooks/x1/x1-{n:0000}.xlsx";

    internal static byte[] SmallBook() => Read(Book(0));

    /// <summary>`key → read, save, edit digests` of the native handles (uo-ffi tests/sheet_abi.rs); keys are repository-relative.</summary>
    internal static List<(string Key, string[] Digests)> NativeDigests() =>
        File.ReadLines(Path.Combine(Core, "crates", "uo-ffi", "tests", "parity", "sheet-native.tsv"))
            .Where(l => l.Length > 0).Select(l => l.Split('\t')).Select(p => (p[0], p[1..])).ToList();

    internal static string Digest(byte[] b) => $"{b.Length}:{Convert.ToHexString(SHA256.HashData(b)).ToLowerInvariant()[..32]}";
}

/// <summary>Writes what the facade parsed back in the wire format of the native buffers: equal bytes prove the parse lost
/// nothing and the P/Invoke layer moved nothing wrongly.</summary>
internal sealed class Encoder
{
    private readonly MemoryStream _ms = new();
    internal void U8(int v) => _ms.WriteByte((byte)v);
    internal void U32(uint v) { Span<byte> b = stackalloc byte[4]; System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(b, v); _ms.Write(b); }
    internal void F64(double v) { Span<byte> b = stackalloc byte[8]; System.Buffers.Binary.BinaryPrimitives.WriteInt64LittleEndian(b, BitConverter.DoubleToInt64Bits(v)); _ms.Write(b); }
    internal void Str(string? s)
    {
        if (s is null) { U32(uint.MaxValue); return; }
        var b = Encoding.UTF8.GetBytes(s);
        U32((uint)b.Length);
        _ms.Write(b);
    }
    internal void Raw(byte[] b) => _ms.Write(b);
    internal byte[] ToArray() => _ms.ToArray();

    internal void Info(XlsxWorkbook w)
    {
        U32(1);
        U32((uint)w.ActiveSheetIndex);
        U8(w.Date1904 ? 1 : 0);
        U8(w.IsDirty ? 1 : 0);
        U8(w.CanUndo ? 1 : 0);
        U8(w.CanRedo ? 1 : 0);
        Str(w.UndoLabel);
        Str(w.RedoLabel);
        U32((uint)w.Sheets.Count);
        foreach (var s in w.Sheets)
        {
            Str(s.Name);
            U8((int)s.State);
            U32((uint)s.UsedRows);
            U32((uint)s.UsedColumns);
            U32((uint)s.FrozenColumns);
            U32((uint)s.FrozenRows);
            U8(s.ShowGridlines ? 1 : 0);
            U32((uint)s.MergedRanges.Count);
            foreach (var m in s.MergedRanges) Str(m.ToString());
        }
    }

    internal void Cells(IReadOnlyList<XlsxCell> cells)
    {
        U32((uint)cells.Count);
        foreach (var c in cells)
        {
            U32((uint)c.Row);
            U32((uint)c.Column);
            U8((int)c.Kind);
            U8((c.HasFormula ? 1 : 0) | (c.IsArrayFormula ? 2 : 0) | (c.IsSpilled ? 4 : 0));
            F64(c.Number);
            Str(c.Text);
            Str(c.Display);
            Str(c.Formula);
            Str(c.CanonicalFormula);
            var s = c.Style!;
            U32((uint)s.FormatIndex);
            Str(s.NumberFormat);
            U8((s.Bold ? 1 : 0) | (s.Italic ? 2 : 0) | (s.Underline ? 4 : 0) | (s.Strikethrough ? 8 : 0) | (s.WrapText ? 16 : 0));
            U8((int)s.HorizontalAlignment);
            F64(s.FontSize ?? BitConverter.Int64BitsToDouble(0x7FF8000000000000));
            Str(s.FontName);
            Str(s.FontColor);
            Str(s.FillColor);
        }
    }

    /// <summary>The dump of uo-ffi tests/sheet_abi.rs: the info and every sheet's used range, 500 rows at a time.</summary>
    internal static byte[] Dump(XlsxWorkbook w)
    {
        var e = new Encoder();
        e.Info(w);
        foreach (var s in w.Sheets)
        {
            if (s.UsedRows == 0 || s.UsedColumns == 0) continue;
            for (int r1 = 1; r1 <= s.UsedRows; r1 += 500)
                e.Cells(w.ReadRange(s.Index, new XlsxRange(1, r1, s.UsedColumns, Math.Min(r1 + 499, s.UsedRows)), XlsxReadOptions.Formulas | XlsxReadOptions.Styles));
        }
        return e.ToArray();
    }

    /// <summary>The scripted edit session of uo-ffi tests/sheet_abi.rs (`edit_script`).</summary>
    internal static byte[] EditScript(XlsxWorkbook w)
    {
        var e = new Encoder();
        void Set(int col, int row, string text) => e.Result(() => w.SetCellText(0, col, row, text));
        Set(1, 1, "=SOMA(2;3)");
        Set(2, 2, "12,5");
        Set(3, 3, "=SOMA(");
        Set(4, 4, "15/03/2025");
        Ignore(() => w.SetNumberFormat(0, new XlsxRange(2, 2, 2, 3), "0.00"));
        Ignore(() => w.Merge(0, new XlsxRange(6, 6, 7, 6), XlsxMergeKind.Cells, allowDataLoss: true));
        Ignore(() => w.Clear(0, XlsxRange.Cell(2, 2), XlsxClearKind.Contents));
        try { e.Raw(Encoding.UTF8.GetBytes(w.AddSheet("Parity"))); } catch (Exception x) { e.Raw(Encoding.ASCII.GetBytes($"err{CodeOf(x)}")); }
        e.U8(w.Undo() ? 1 : 0);
        e.U8(w.Redo() ? 1 : 0);
        Ignore(w.Recalculate);
        e.Raw(Dump(w));
        try { e.Raw(w.Save()); } catch (Exception x) { e.Raw(Encoding.ASCII.GetBytes($"err{CodeOf(x)}")); }
        return e.ToArray();
    }

    private void Result(Func<XlsxEditResult> f)
    {
        try
        {
            var r = f();
            U8(r.Accepted ? 1 : 0);
            Str(r.Message);
        }
        catch (Exception x) { Raw(Encoding.ASCII.GetBytes($"err{CodeOf(x)}")); }
    }

    private static void Ignore(Action a)
    {
        try { a(); } catch (Exception) { /* the native script ignores these statuses too */ }
    }

    internal static int CodeOf(Exception x) => x switch
    {
        ArgumentOutOfRangeException => 9,
        ArgumentException => 1,
        InvalidDataException => 3,
        InvalidOperationException => 2,
        OfficeLimitExceededException => 5,
        UniversalOfficeException u => u.Code,
        _ => -1,
    };
}

public class ParityTests(ITestOutputHelper log)
{
    [Fact]
    public void Reads_saves_and_edits_equal_the_rust_native_output_byte_for_byte()
    {
        var all = Fixtures.NativeDigests();
        var bad = new List<string>();
        int checkedBooks = 0, cells = 0;
        foreach (var (key, d) in all)
        {
            var bytes = Fixtures.Read(key);
            if (d[0] == "refused") { Assert.ThrowsAny<Exception>(() => XlsxWorkbook.Open(bytes, Fixtures.Opts())); continue; }
            using (var w = XlsxWorkbook.Open(bytes, Fixtures.Opts()))
            {
                var dump = Encoder.Dump(w);
                cells += w.Sheets.Sum(s => s.UsedRows);
                if (Fixtures.Digest(dump) != d[0]) bad.Add($"{key}: read {Fixtures.Digest(dump)} vs native {d[0]}");
                var saved = w.Save();
                if (Fixtures.Digest(saved) != d[1]) bad.Add($"{key}: save {Fixtures.Digest(saved)} vs native {d[1]}");
            }
            using (var w = XlsxWorkbook.Open(bytes, Fixtures.Opts()))
            {
                string got = w.Sheets.Count == 0 ? "none" : Fixtures.Digest(Encoder.EditScript(w));
                if (got != d[2]) bad.Add($"{key}: edit {got} vs native {d[2]}");
            }
            checkedBooks++;
        }
        log.WriteLine($"{all.Count} workbooks ({checkedBooks} opened), read + save + edit digests: {all.Count * 3 - bad.Count} of {all.Count * 3} equal to the Rust native output");
        Assert.True(all.Count >= 40, $"{all.Count} workbooks in the digest file");
        Assert.Empty(bad);
    }
}

public class ApiTests
{
    [Fact]
    public void A_workbook_lists_its_sheets_and_reads_values_display_formulas_and_styles()
    {
        using var w = XlsxWorkbook.Open(Fixtures.SmallBook(), Fixtures.Opts());
        Assert.NotEmpty(w.Sheets);
        Assert.False(w.IsLegacyXls);
        Assert.False(w.IsDirty);
        var sheet = w.Sheets.First(s => s.UsedRange is not null);
        var cells = w.ReadSheet(sheet.Index, XlsxReadOptions.Formulas | XlsxReadOptions.Styles).ToList();
        Assert.NotEmpty(cells);
        Assert.All(cells, c => Assert.NotNull(c.Style));
        Assert.Equal(cells.Select(c => (c.Row, c.Column)).OrderBy(x => x).ToList(), cells.Select(c => (c.Row, c.Column)).ToList());
        Assert.Equal(cells[0].Reference, XlsxRange.Cell(cells[0].Column, cells[0].Row).ToString());
        Assert.Equal(sheet.Index, w.IndexOfSheet(sheet.Name.ToUpperInvariant()));
        Assert.Equal(-1, w.IndexOfSheet("no such sheet"));
        Assert.Null(w.GetCell(sheet.Index, XlsxRange.MaxColumns, XlsxRange.MaxRows));
    }

    [Fact]
    public void The_entry_rules_follow_the_language_and_a_refusal_is_a_result()
    {
        using var w = XlsxWorkbook.Create(Fixtures.Opts("pt-BR"));
        Assert.Equal("Planilha1", w.Sheets[0].Name);
        Assert.True(w.SetCellText(0, 1, 1, "2").Accepted);
        Assert.True(w.SetCellText(0, 1, 2, "3,5").Accepted);
        Assert.True(w.SetCellText(0, "A3", "=SOMA(A1:A2)").Accepted);
        var refused = w.SetCellText(0, 4, 1, "=SOMA(");
        Assert.False(refused.Accepted);
        Assert.NotEmpty(refused.Message);
        var a3 = w.GetCell(0, 1, 3, XlsxReadOptions.Formulas)!;
        Assert.Equal(XlsxCellKind.Number, a3.Kind);
        Assert.Equal(5.5, a3.Number);
        Assert.Equal("5,5", a3.Display);
        Assert.Equal("=SOMA(A1:A2)", a3.Formula);
        Assert.Equal("=SUM(A1:A2)", a3.CanonicalFormula);
        Assert.True(a3.HasFormula);
        Assert.Equal(5.5, a3.Value);
        Assert.True(w.IsDirty);
        // text, boolean, error
        w.SetCellText(0, 2, 1, "texto");
        w.SetCellText(0, 2, 2, "=1>0");
        w.SetCellText(0, 2, 3, "=1/0");
        Assert.Equal(("texto", XlsxCellKind.Text), (w.GetCell(0, 2, 1)!.Text, w.GetCell(0, 2, 1)!.Kind));
        Assert.Equal("VERDADEIRO", w.GetCell(0, 2, 2)!.Display);
        Assert.Equal(true, w.GetCell(0, 2, 2)!.Value);
        Assert.Equal("#DIV/0!", w.GetCell(0, 2, 3)!.Text);
        Assert.Equal(XlsxCellKind.Error, w.GetCell(0, 2, 3)!.Kind);

        using var en = XlsxWorkbook.Create(Fixtures.Opts("en-US"));
        Assert.Equal("Sheet1", en.Sheets[0].Name);
        en.SetCellText(0, 1, 1, "2.5");
        en.SetCellText(0, 1, 2, "=SUM(A1,1)");
        en.SetCellText(0, 1, 3, "=1>0");
        Assert.Equal(3.5, en.GetCell(0, 1, 2)!.Number);
        Assert.Equal("TRUE", en.GetCell(0, 1, 3)!.Display);
        // the saved workbook read back in the other language: the same formula, in that language
        using var back = XlsxWorkbook.Open(w.Save(), Fixtures.Opts("en-US"));
        Assert.Equal("=SUM(A1:A2)", back.GetCell(0, 1, 3, XlsxReadOptions.Formulas)!.Formula);
    }

    [Fact]
    public void Number_format_merge_clear_undo_redo_and_add_sheet()
    {
        using var w = XlsxWorkbook.Create(Fixtures.Opts("en-US"));
        w.SetCellText(0, 1, 1, "1234.5");
        w.SetCellText(0, 2, 1, "x");
        w.SetNumberFormat(0, XlsxRange.Cell(1, 1), "#,##0.00");
        Assert.Equal("#,##0.00", w.GetCell(0, 1, 1, XlsxReadOptions.Styles)!.Style!.NumberFormat);
        Assert.Throws<InvalidOperationException>(() => w.Merge(0, XlsxRange.Parse("A1:B1"), XlsxMergeKind.Cells));
        w.Merge(0, XlsxRange.Parse("A1:B1"), XlsxMergeKind.Cells, allowDataLoss: true);
        Assert.Equal(XlsxRange.Parse("A1:B1"), Assert.Single(w.Sheets[0].MergedRanges));
        w.Unmerge(0, XlsxRange.Cell(1, 1));
        Assert.Empty(w.Sheets[0].MergedRanges);
        w.Clear(0, XlsxRange.Parse("A1:A1"), XlsxClearKind.Contents);
        Assert.Equal(XlsxCellKind.Empty, w.GetCell(0, 1, 1)?.Kind ?? XlsxCellKind.Empty);
        Assert.True(w.CanUndo);
        Assert.True(w.Undo());
        Assert.True(w.CanRedo);
        Assert.True(w.Redo());
        Assert.NotNull(w.UndoLabel);
        Assert.Equal("Sheet2", w.AddSheet());
        Assert.Equal("Totals", w.AddSheet("  Totals "));
        Assert.Equal(["Sheet1", "Sheet2", "Totals"], w.Sheets.Select(s => s.Name));
        Assert.Throws<ArgumentException>(() => w.AddSheet("totals"));
        Assert.Throws<ArgumentException>(() => w.AddSheet("a/b"));
        Assert.Throws<ArgumentException>(() => w.AddSheet(new string('x', 32)));
        Assert.False(w.CanUndo, "inserting a sheet rebuilds the workbook: the history is empty");
        // the save carries it all
        using var again = XlsxWorkbook.Open(w.Save(), Fixtures.Opts("en-US"));
        Assert.Equal(["Sheet1", "Sheet2", "Totals"], again.Sheets.Select(s => s.Name));
        w.MarkSaved();
        Assert.False(w.IsDirty);
    }

    [Fact]
    public void A_xls_opens_as_legacy_converts_and_saves_as_xlsx()
    {
        var xls = Fixtures.Read("core/crates/uo-xlsx/tests/captures/workbooks/xls/a-cells.xls");
        using var w = XlsxWorkbook.Open(xls, Fixtures.Opts());
        Assert.True(w.IsLegacyXls);
        var xlsx = XlsConverter.ToXlsx(xls);
        Assert.Equal((byte)'P', xlsx[0]);
        using var x = XlsxWorkbook.Open(xlsx, Fixtures.Opts());
        Assert.False(x.IsLegacyXls);
        Assert.Equal(w.Sheets.Select(s => (s.Name, s.UsedRows, s.UsedColumns)), x.Sheets.Select(s => (s.Name, s.UsedRows, s.UsedColumns)));
        using var s = XlsxWorkbook.Open(w.Save(), Fixtures.Opts());
        Assert.Equal(w.Sheets.Select(q => q.Name), s.Sheets.Select(q => q.Name));
        Assert.Throws<InvalidDataException>(() => XlsConverter.ToXlsx(Fixtures.SmallBook()));
    }

    [Fact]
    public void Ranges_parse_and_print()
    {
        Assert.Equal(new XlsxRange(2, 2, 4, 8), XlsxRange.Parse("B2:D8"));
        Assert.Equal(new XlsxRange(2, 2, 4, 8), XlsxRange.Parse("d8:b2"));
        Assert.Equal("AA10", XlsxRange.Cell(27, 10).ToString());
        Assert.Equal("XFD1048576", XlsxRange.Cell(16_384, 1_048_576).ToString());
        foreach (var bad in new[] { "", "A", "1", "A0", "XFE1", "A1:B2:C3", "A1B", "A1048577" })
            Assert.Throws<FormatException>(() => XlsxRange.Parse(bad));
    }

    [Fact]
    public void Errors_are_the_exceptions_of_their_class()
    {
        Assert.Throws<ArgumentNullException>(() => XlsxWorkbook.Open(null!));
        Assert.Throws<InvalidDataException>(() => XlsxWorkbook.Open([]));
        Assert.Throws<InvalidDataException>(() => XlsxWorkbook.Open([1, 2, 3, 4]));
        Assert.Throws<InvalidDataException>(() => XlsxWorkbook.Open(Fixtures.Read("core/crates/uo-docx-layout/tests/captures/f8/docs/f8__7__0.docx")));
        using var w = XlsxWorkbook.Open(Fixtures.SmallBook(), Fixtures.Opts());
        Assert.Throws<ArgumentOutOfRangeException>(() => w.ReadRange(99, XlsxRange.Cell(1, 1)));
        Assert.Throws<ArgumentOutOfRangeException>(() => w.ReadRange(-1, XlsxRange.Cell(1, 1)));
        Assert.Throws<ArgumentOutOfRangeException>(() => w.ReadRange(0, new XlsxRange(0, 1, 1, 1)));
        Assert.Throws<ArgumentOutOfRangeException>(() => w.ReadRange(0, new XlsxRange(3, 1, 2, 1)));
        Assert.Throws<ArgumentOutOfRangeException>(() => w.ReadRange(0, new XlsxRange(1, 1, 16_385, 1)));
        Assert.Throws<ArgumentOutOfRangeException>(() => w.SetCellText(7, 1, 1, "x"));
        Assert.Throws<ArgumentOutOfRangeException>(() => w.SetCellText(0, 1, 1, new string('a', 32_768)));
        Assert.Throws<ArgumentNullException>(() => w.SetCellText(0, 1, 1, null!));
        Assert.Throws<ArgumentOutOfRangeException>(() => w.SetNumberFormat(0, XlsxRange.Cell(1, 1), ""));
        Assert.Throws<ArgumentException>(() => w.SetCellText(0, "A1:B2", "x"));
        // a lone surrogate becomes U+FFFD as in every UTF-8 boundary of the facade
        Assert.True(w.SetCellText(0, 1, 1, "\ud800x").Accepted);
        w.Dispose();
        Assert.Throws<ObjectDisposedException>(() => w.Save());
    }

    [Fact]
    public void Hostile_input_is_refused_by_class_never_a_crash()
    {
        var book = Fixtures.SmallBook();
        var rng = new Random(7);
        int refused = 0, opened = 0;
        for (int i = 0; i < 120; i++)
        {
            var m = i % 2 == 0 ? book[..rng.Next(book.Length)] : (byte[])book.Clone();
            if (i % 2 == 1) for (int k = 0; k <= rng.Next(6); k++) m[rng.Next(m.Length)] ^= (byte)(1 << rng.Next(8));
            try
            {
                using var w = XlsxWorkbook.Open(m, Fixtures.Opts());
                foreach (var s in w.Sheets.Take(2)) _ = w.ReadRange(s.Index, new XlsxRange(1, 1, 20, 20), XlsxReadOptions.Formulas).Count;
                _ = w.Save();
                opened++;
            }
            catch (Exception x) when (x is InvalidDataException or OfficeLimitExceededException or InvalidOperationException) { refused++; }
        }
        Assert.True(refused > 0);
        Assert.Equal(120, refused + opened);
    }

    [Fact]
    public void A_lying_central_directory_is_a_limit_for_untrusted_workbooks()
    {
        var zip = Fixtures.SmallBook();
        // every central header's uncompressed size (offset 24) becomes 300 MiB
        int patched = 0;
        for (int i = 0; i + 46 <= zip.Length; i++)
        {
            if (zip[i] == 0x50 && zip[i + 1] == 0x4B && zip[i + 2] == 1 && zip[i + 3] == 2)
            {
                BitConverter.GetBytes(300u << 20).CopyTo(zip, i + 24);
                patched++;
                i += 45;
            }
        }
        Assert.True(patched > 3);
        var e = Assert.Throws<OfficeLimitExceededException>(() => XlsxWorkbook.Open(zip));
        Assert.Equal(256L << 20, e.Maximum);
    }

    [Fact]
    public void A_cancelled_token_cancels_open_read_and_save()
    {
        using var cts = new CancellationTokenSource();
        using var live = XlsxWorkbook.Open(Fixtures.SmallBook(), Fixtures.Opts(), cts.Token);
        Assert.True(live.Save(cts.Token).Length > 0);
        cts.Cancel();
        Assert.ThrowsAny<OperationCanceledException>(() => XlsxWorkbook.Open(Fixtures.SmallBook(), Fixtures.Opts(), cts.Token));
        Assert.ThrowsAny<OperationCanceledException>(() => live.Save(cts.Token));
        Assert.ThrowsAny<OperationCanceledException>(() => live.ReadRange(0, XlsxRange.Cell(1, 1), ct: cts.Token));
        // the handle is intact
        Assert.True(live.Save().Length > 0);
    }

    [Fact]
    public void An_untouched_workbook_saves_the_same_bytes_twice()
    {
        using var w = XlsxWorkbook.Open(Fixtures.SmallBook(), Fixtures.Opts());
        Assert.Equal(w.Save(), w.Save());
    }
}

/// <summary>Memory gate M4 at the .NET boundary: native handles created and released through Dispose() and through the
/// finalizer; the native live bytes must return exactly.</summary>
public class BoundaryMemoryTests(ITestOutputHelper log)
{
    private static readonly int Count = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_M4"), out var c) ? c : 10_000;

    [Fact]
    public void The_native_library_counts_memory()
    {
        Assert.True(NativeCore.IsCountingMemory, "the tests build uo-ffi with --features count");
        Assert.StartsWith("0.10.0", NativeCore.Version);
    }

    [Fact]
    public void Dispose_releases_every_workbook()
    {
        var bytes = Fixtures.SmallBook();
        using (var warm = XlsxWorkbook.Open(bytes, Fixtures.Opts())) { _ = warm.SetCellText(0, 1, 1, "=SOMA(1;2)"); _ = Encoder.Dump(warm); _ = warm.Save(); }
        using (var warm = XlsxWorkbook.Create()) { _ = warm.Save(); }
        long before = Settle();
        for (int i = 0; i < Count; i++)
        {
            using var w = XlsxWorkbook.Open(bytes, Fixtures.Opts());
            if (i % 50 == 0)
            {
                Assert.NotEmpty(w.Sheets);
                Assert.True(w.SetCellText(0, 1, 1, "=SOMA(1;2)").Accepted);
                Assert.NotEmpty(Encoder.Dump(w));
                Assert.NotEmpty(w.Save());
            }
        }
        for (int i = 0; i < Count; i++)
        {
            using var w = XlsxWorkbook.Create();
            if (i % 100 == 0) Assert.NotEmpty(w.Save());
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count} workbooks (every 50th edited, read, saved) + {Count} blank ones through Dispose(): 0 B left");
    }

    [Fact]
    public void The_finalizer_releases_workbooks_that_were_not_disposed()
    {
        var bytes = Fixtures.SmallBook();
        using (var warm = XlsxWorkbook.Open(bytes, Fixtures.Opts())) { }
        long before = Settle();
        Abandon(bytes, Count / 5);
        Assert.True(NativeCore.LiveBytes > before, "the abandoned workbooks are counted while alive");
        Assert.Equal(before, Settle());
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static void Abandon(byte[] bytes, int n)
    {
        for (int i = 0; i < n; i++) _ = XlsxWorkbook.Open(bytes, Fixtures.Opts());
    }

    [Fact]
    public void Failures_and_cancellations_leave_nothing()
    {
        var bytes = Fixtures.SmallBook();
        using (var warm = XlsxWorkbook.Open(bytes, Fixtures.Opts())) { _ = warm.Save(); }
        long before = Settle();
        using var cts = new CancellationTokenSource();
        for (int i = 0; i < Count; i++)
        {
            // a cancellable token (UoCancel handle + registration) with invalid data
            Assert.Throws<InvalidDataException>(() => XlsxWorkbook.Open([1, 2, 3], ct: cts.Token));
        }
        using var already = new CancellationTokenSource();
        already.Cancel();
        using var w = XlsxWorkbook.Open(bytes, Fixtures.Opts());
        for (int i = 0; i < 100; i++)
        {
            Assert.ThrowsAny<OperationCanceledException>(() => w.Save(already.Token));
            Assert.Throws<ArgumentOutOfRangeException>(() => w.ReadRange(5, XlsxRange.Cell(1, 1)));
            Assert.False(w.SetCellText(0, 1, 1, "=SOMA(").Accepted);
        }
        w.Dispose();
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count} refused opens (with cancel handles) + 100 x (cancelled save, bad range, refused entry): 0 B left");
    }

    internal static long Settle()
    {
        for (int i = 0; i < 3; i++)
        {
            GC.Collect();
            GC.WaitForPendingFinalizers();
        }
        return NativeCore.LiveBytes;
    }
}

public class ThreadSoakTests(ITestOutputHelper log)
{
    private static readonly int Threads = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_THREADS"), out var t) ? t : 8;
    private static readonly int PerThread = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_SOAK"), out var s) ? s : 5;

    [Fact]
    public void Parallel_calls_give_the_sequential_bytes_and_leave_memory_flat()
    {
        var bytes = Fixtures.SmallBook();
        using var shared = XlsxWorkbook.Open(bytes, Fixtures.Opts());
        var expected = Encoder.Dump(shared);
        var lives = new List<long>();
        for (int round = 0; round < 3; round++)
        {
            Parallel.For(0, Threads, new ParallelOptions { MaxDegreeOfParallelism = Threads }, t =>
            {
                for (int i = 0; i < PerThread; i++)
                {
                    // every thread reads the ONE shared handle and opens its own
                    Assert.Equal(expected, Encoder.Dump(shared));
                    using var own = XlsxWorkbook.Open(bytes, Fixtures.Opts());
                    Assert.Equal(expected, Encoder.Dump(own));
                    Assert.NotEmpty(own.Save());
                }
            });
            lives.Add(BoundaryMemoryTests.Settle());
        }
        log.WriteLine($"3 rounds x {Threads} threads x {PerThread} x (shared read + own open/read/save); native live after each round: {string.Join(", ", lives)} B");
        Assert.True(lives.Skip(1).All(v => v == lives[1]), $"native live bytes grow: {string.Join(", ", lives)}");
    }
}
