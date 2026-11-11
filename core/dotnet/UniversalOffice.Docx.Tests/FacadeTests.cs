using System.Runtime.CompilerServices;
using UniversalOffice.Core;
using System.Security.Cryptography;
using UniversalOffice.DocxLayout;
using UniversalOffice.Native;
using Xunit;
using Xunit.Abstractions;

// the memory tests read the process-wide native counters: one test at a time (the soak runs its own threads)
[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace UniversalOffice.Docx.Tests;

/// <summary>The fixture documents (uo-docx-layout tests/captures, open fonts only) and what the
/// Rust side committed about them.</summary>
internal static class Fixtures
{
    internal static readonly string Core = typeof(Fixtures).Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyMetadataAttribute), false)
        .Cast<System.Reflection.AssemblyMetadataAttribute>().Single(a => a.Key == "CoreDir").Value!;
    internal static readonly string Repo = Path.GetFullPath(Path.Combine(Core, ".."));
    internal static readonly string[] Families = ["f7a", "f7b", "f7c", "f7d", "f8"];

    internal static string[] OpenFontDirs => [Path.Combine(Repo, "shared", "fonts", "files"), Path.Combine(Repo, "site", "assets", "fonts")];

    internal static byte[] Docx(string family, string key) =>
        File.ReadAllBytes(Path.Combine(Core, "crates", "uo-docx-layout", "tests", "captures", family, "docs", key + ".docx"));

    /// <summary>`key → raw` of the TS capture (`length:sha256-prefix` of the uncompressed PDF).</summary>
    internal static IEnumerable<(string Family, string Key, string Raw)> TsCaptures()
    {
        foreach (var fam in Families)
            foreach (var line in File.ReadLines(Path.Combine(Core, "crates", "uo-ffi", "tests", "parity", $"server-{fam}.cap")))
            {
                if (line.Length == 0 || line[0] is '#' or '@') continue;
                var f = line.Split('\t');
                var raw = f.FirstOrDefault(x => x.StartsWith("raw=", StringComparison.Ordinal));
                if (raw is not null) yield return (fam, f[0], raw[4..]);
            }
    }

    /// <summary>`family/key → digest` of the compressed native PDFs (tests/server_parity.rs).</summary>
    internal static Dictionary<string, string> NativeCompressed() =>
        File.ReadLines(Path.Combine(Core, "crates", "uo-ffi", "tests", "parity", "server-native.tsv"))
            .Where(l => l.Length > 0).Select(l => l.Split('\t')).ToDictionary(p => p[0], p => p[1]);

    internal static string Digest(byte[] pdf) => $"{pdf.Length}:{Convert.ToHexString(SHA256.HashData(pdf)).ToLowerInvariant()[..32]}";

    /// <summary>The options of the captures: 2026-01-15T12:00Z, UTC, every pattern set.</summary>
    internal static LayoutOptions TsOptions() => new() { Now = new DateTime(2026, 1, 15, 12, 0, 0, DateTimeKind.Utc), UtcOffset = TimeSpan.Zero };

    internal static DocxPdfConverter OpenFonts(bool trusted) =>
        new(new DocxPdfConverterOptions { SystemFonts = false, FontDirectories = OpenFontDirs, TrustedInput = trusted });

    /// <summary>A .docx of <paramref name="paragraphs"/> paragraphs of 80-120 words in Liberation Serif (650 take
    /// the native layout several hundred ms in release), built in memory.</summary>
    internal static byte[] BigDocx(int paragraphs)
    {
        string[] ws = ["o", "motor", "de", "layout", "quebra", "linhas", "colunas", "secoes", "informação", "parágrafo", "página", "responsabilidade"];
        var body = new System.Text.StringBuilder();
        for (int i = 0; i < paragraphs; i++)
        {
            int n = 80 + (i * 7) % 41;
            var text = string.Join(' ', Enumerable.Range(0, n).Select(k => ws[(k * 7 + i * 3) % ws.Length]));
            body.Append("<w:p><w:r><w:rPr><w:rFonts w:ascii=\"Liberation Serif\" w:hAnsi=\"Liberation Serif\"/></w:rPr><w:t xml:space=\"preserve\">").Append(text).Append("</w:t></w:r></w:p>");
        }
        const string W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
        string doc = $"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"{W}\"><w:body>{body}<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>";
        const string ct = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
        const string rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
        using var ms = new MemoryStream();
        using (var zip = new System.IO.Compression.ZipArchive(ms, System.IO.Compression.ZipArchiveMode.Create, leaveOpen: true))
        {
            foreach (var (name, text) in new[] { ("[Content_Types].xml", ct), ("_rels/.rels", rels), ("word/document.xml", doc) })
            {
                using var w = new StreamWriter(zip.CreateEntry(name).Open(), new System.Text.UTF8Encoding(false));
                w.Write(text);
            }
        }
        return ms.ToArray();
    }
}

public class PdfParityTests(ITestOutputHelper log)
{
    [Fact]
    public void The_uncompressed_pdf_equals_the_ts_render_pdf_byte_for_byte()
    {
        using var conv = Fixtures.OpenFonts(trusted: true);
        int n = 0;
        var bad = new List<string>();
        foreach (var (fam, key, raw) in Fixtures.TsCaptures())
        {
            var pdf = conv.ToPdf(Fixtures.Docx(fam, key), Fixtures.TsOptions(), new PdfRenderOptions { Compress = false });
            n++;
            if (Fixtures.Digest(pdf) != raw) bad.Add($"{fam}/{key}: {Fixtures.Digest(pdf)} vs TS {raw}");
        }
        log.WriteLine($"{n} documents, {n - bad.Count} equal to the TS renderPdf");
        Assert.Equal(68, n);
        Assert.Empty(bad);
    }

    [Fact]
    public void The_compressed_pdf_equals_the_rust_native_output_byte_for_byte()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        var native = Fixtures.NativeCompressed();
        int n = 0;
        foreach (var (fam, key, _) in Fixtures.TsCaptures())
        {
            var pdf = conv.ToPdf(Fixtures.Docx(fam, key), Fixtures.TsOptions());
            Assert.Equal(native[$"{fam}/{key}"], Fixtures.Digest(pdf));
            n++;
        }
        Assert.Equal(68, n);
    }

    [Fact]
    public void The_09_entry_points_still_compile_and_convert()
    {
        // the clickCompliance call, unchanged
        var opts = new LayoutOptions { HyphenationLanguages = ["pt-BR", "en", "es"] };
        var pdf = DocxLayoutPipeline.ToPdf(Fixtures.Docx("f8", "f8__7__0"), SubstituteFontProvider.Shared, opts);
        Assert.Equal("%PDF-"u8.ToArray(), pdf[..5]);
        Assert.Equal("%PDF-"u8.ToArray(), Export.DocxPdf.ToPdf(Fixtures.Docx("f8", "f8__7__3"))[..5]);
        log.WriteLine($"system index: {SubstituteFontProvider.Shared.Families.Count} families");
    }
}

public class ErrorTests(ITestOutputHelper log)
{
    [Fact]
    public void Garbage_is_invalid_data_with_a_message()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        var e = Assert.Throws<InvalidDataException>(() => conv.ToPdf([1, 2, 3, 4]));
        Assert.Contains("ToPdf", e.Message);
    }

    [Fact]
    public void A_page_limit_is_an_office_limit_exception_of_the_layout()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        var e = Assert.Throws<OfficeLimitExceededException>(() => conv.ToPdf(Fixtures.Docx("f8", "f8__7__1"), new LayoutOptions { MaxPages = 1 }));
        Assert.Equal("layout", e.Stage);
    }

    [Fact]
    public void Bad_options_are_argument_errors()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        Assert.Throws<ArgumentException>(() => conv.ToPdf(Fixtures.Docx("f8", "f8__7__0"), new LayoutOptions { Revisions = new RevisionView { Mode = "nope" } }));
        Assert.Throws<ArgumentNullException>(() => conv.ToPdf(null!));
    }

    [Fact]
    public void A_cancelled_token_stops_the_conversion()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        using var cts = new CancellationTokenSource();
        cts.Cancel();
        Assert.ThrowsAny<OperationCanceledException>(() => conv.ToPdf(Fixtures.Docx("f8", "f8__7__0"), ct: cts.Token));
        // a token cancelled while the native core works: the native flag stops it (or it finished first)
        var docx = Fixtures.Docx("f8", "f8__7__1");
        int cancelled = 0;
        for (int i = 0; i < 20 && cancelled == 0; i++)
        {
            using var late = new CancellationTokenSource();
            late.CancelAfter(TimeSpan.FromMilliseconds(1 + i));
            try { conv.ToPdf(docx, ct: late.Token); }
            catch (OperationCanceledException) { cancelled++; }
        }
        Assert.True(cancelled > 0, "no conversion was cancelled in 20 tries");
    }

    [Fact]
    public void A_token_cancelled_during_the_layout_stops_it_promptly()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        var big = Fixtures.BigDocx(650);
        conv.ToPdf(Fixtures.Docx("f8", "f8__7__0")); // warm-up: faces cached
        using var cts = new CancellationTokenSource();
        var sw = System.Diagnostics.Stopwatch.StartNew();
        TimeSpan raisedAt = TimeSpan.Zero;
        cts.Token.Register(() => raisedAt = sw.Elapsed);
        cts.CancelAfter(TimeSpan.FromMilliseconds(50));
        Assert.ThrowsAny<OperationCanceledException>(() => conv.ToPdf(big, ct: cts.Token));
        var thrownAt = sw.Elapsed;
        var latency = thrownAt - raisedAt;
        log.WriteLine($"G4 facade: raised at {raisedAt.TotalMilliseconds:F1} ms, thrown at {thrownAt.TotalMilliseconds:F1} ms, latency {latency.TotalMilliseconds:F1} ms");
        Assert.True(raisedAt > TimeSpan.Zero, "the token was never cancelled");
        Assert.True(latency < TimeSpan.FromMilliseconds(250), $"latency {latency.TotalMilliseconds:F1} ms");
    }
}

/// <summary>Memory gate M4 at the .NET boundary: native handles created and released through
/// Dispose() and through the finalizer; the native live bytes must return exactly.</summary>
[Collection("memory")]
public class BoundaryMemoryTests(ITestOutputHelper log)
{
    private static readonly int Count = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_M4"), out var c) ? c : 10_000;
    private static readonly int Conversions = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_CONVERSIONS"), out var c2) ? c2 : 200;

    [Fact]
    public void The_native_library_counts_memory()
    {
        Assert.True(NativeCore.IsCountingMemory, "the tests build uo-ffi with --features count");
        Assert.StartsWith("0.10.0", NativeCore.Version);
    }

    [Fact]
    public void Dispose_releases_every_converter()
    {
        new DocxPdfConverter(new DocxPdfConverterOptions { SystemFonts = false }).Dispose(); // warm-up
        long before = Settle();
        for (int i = 0; i < Count; i++)
        {
            using var conv = new DocxPdfConverter(new DocxPdfConverterOptions { SystemFonts = false });
            Assert.Empty(conv.Families);
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count} converters + {Count} family buffers through Dispose(): 0 B left");
    }

    [Fact]
    public void The_finalizer_releases_converters_that_were_not_disposed()
    {
        new DocxPdfConverter(new DocxPdfConverterOptions { SystemFonts = false }).Dispose();
        long before = Settle();
        Abandon(Count);
        Assert.True(NativeCore.LiveBytes > before, "the abandoned converters are counted while alive");
        Assert.Equal(before, Settle());
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static void Abandon(int n)
    {
        for (int i = 0; i < n; i++) _ = new DocxPdfConverter(new DocxPdfConverterOptions { SystemFonts = false });
    }

    [Fact]
    public void Conversions_leave_nothing_ok_failed_or_cancelled()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        var docx = Fixtures.Docx("f8", "f8__7__0");
        conv.ToPdf(docx); // warm-up: faces cached, patterns parsed
        var big = Fixtures.BigDocx(650);
        using (var warm = new CancellationTokenSource())
        {
            warm.CancelAfter(TimeSpan.FromMilliseconds(30)); // the faces of the big document
            Assert.ThrowsAny<OperationCanceledException>(() => conv.ToPdf(big, ct: warm.Token));
        }
        long before = Settle();
        using var cts = new CancellationTokenSource();
        for (int i = 0; i < Conversions; i++)
        {
            Assert.True(conv.ToPdf(docx, ct: cts.Token).Length > 1000);
        }
        // every failure path: a cancellable token (UoCancel handle + registration) with invalid data
        for (int i = 0; i < Count; i++)
        {
            Assert.Throws<InvalidDataException>(() => conv.ToPdf([1, 2, 3], ct: cts.Token));
        }
        // G4: cancelled in the middle of the layout of a big document
        for (int i = 0; i < 5; i++)
        {
            using var mid = new CancellationTokenSource();
            mid.CancelAfter(TimeSpan.FromMilliseconds(30));
            Assert.ThrowsAny<OperationCanceledException>(() => conv.ToPdf(big, ct: mid.Token));
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{Conversions} conversions + {Count} refused (with cancel handles) + 5 cancelled inside the layout: 0 B left");
    }

    /// <summary>Runs the finalizers until no native memory is pending, then reads the counter.</summary>
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

[Collection("memory")]
public class ThreadSoakTests(ITestOutputHelper log)
{
    private static readonly int Threads = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_THREADS"), out var t) ? t : 8;
    private static readonly int PerThread = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_SOAK"), out var s) ? s : 4;

    [Fact]
    public void Parallel_conversions_give_the_sequential_bytes_and_leave_memory_flat()
    {
        using var conv = Fixtures.OpenFonts(trusted: false);
        string[] keys = ["f8__7__0", "f8__7__3", "f8__7__4", "f8__7__11", "f8__7__10"];
        var docs = keys.Select(k => Fixtures.Docx("f8", k)).ToArray();
        var expected = docs.Select(d => conv.ToPdf(d, Fixtures.TsOptions())).ToArray();
        var lives = new List<long>();
        var sw = System.Diagnostics.Stopwatch.StartNew();
        for (int round = 0; round < 3; round++)
        {
            Parallel.For(0, Threads, new ParallelOptions { MaxDegreeOfParallelism = Threads }, t =>
            {
                for (int i = 0; i < PerThread; i++)
                {
                    int k = (t + i) % docs.Length;
                    Assert.Equal(expected[k], conv.ToPdf(docs[k], Fixtures.TsOptions()));
                }
            });
            lives.Add(BoundaryMemoryTests.Settle());
        }
        log.WriteLine($"3 rounds x {Threads} threads x {PerThread} conversions in {sw.ElapsedMilliseconds} ms; native live after each round: {string.Join(", ", lives)} B");
        Assert.True(lives.Skip(1).All(v => v == lives[1]), $"native live bytes grow: {string.Join(", ", lives)}");
    }
}

/// <summary>Opt-in (local, installed fonts): <c>UO_SERVER_ORACLE_DIR</c> holds the <c>native.tsv</c> that
/// uo-ffi's <c>the_oracle_documents_with_installed_fonts_equal_the_ts</c> wrote; the facade over the
/// machine's fonts must give the same compressed bytes for every oracle document listed.</summary>
public class OracleFacadeTests(ITestOutputHelper log)
{
    [Fact]
    public void The_facade_gives_the_native_bytes_for_the_oracle_documents()
    {
        var dir = Environment.GetEnvironmentVariable("UO_SERVER_ORACLE_DIR");
        if (string.IsNullOrEmpty(dir) || !File.Exists(Path.Combine(dir, "native.tsv")))
        {
            log.WriteLine("SKIPPED: set UO_SERVER_ORACLE_DIR (uo-ffi tests/server_parity.rs)");
            return;
        }
        using var conv = new DocxPdfConverter(new DocxPdfConverterOptions());
        var root = Path.Combine(Fixtures.Repo, "write", "packages", "docx-layout", "test", "oracle");
        int n = 0;
        foreach (var line in File.ReadLines(Path.Combine(dir, "native.tsv")).Where(l => l.Length > 0))
        {
            var p = line.Split('\t');
            var docx = File.ReadAllBytes(Path.Combine(root, p[0] + ".docx"));
            Assert.Equal(p[1], Fixtures.Digest(conv.ToPdf(docx, Fixtures.TsOptions())));
            n++;
        }
        log.WriteLine($"{n} oracle documents: the facade's bytes equal the native ones");
    }
}
