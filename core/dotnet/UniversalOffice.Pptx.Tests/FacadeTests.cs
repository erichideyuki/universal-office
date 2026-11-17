using System.Runtime.CompilerServices;
using System.Security.Cryptography;
using UniversalOffice.Core;
using UniversalOffice.Export;
using UniversalOffice.Native;
using UniversalOffice.Pptx;
using Xunit;
using Xunit.Abstractions;

// the memory tests read the process-wide native counters: one test at a time (the soak runs its own threads)
[assembly: CollectionBehavior(DisableTestParallelization = true)]

namespace UniversalOffice.Pptx.Tests;

internal static class Fixtures
{
    internal static readonly string Core = typeof(Fixtures).Assembly.GetCustomAttributes(typeof(System.Reflection.AssemblyMetadataAttribute), false)
        .Cast<System.Reflection.AssemblyMetadataAttribute>().Single(a => a.Key == "CoreDir").Value!;
    internal static readonly string Repo = Path.GetFullPath(Path.Combine(Core, ".."));

    internal static string[] OpenFontDirs => [Path.Combine(Repo, "shared", "fonts", "files"), Path.Combine(Repo, "site", "assets", "fonts")];

    internal static SlidesPdfConverter OpenFonts(bool trusted = false) =>
        new(new SlidesPdfConverterOptions { SystemFonts = false, FontDirectories = OpenFontDirs, TrustedInput = trusted });

    internal static byte[] Read(string repoRelative) => File.ReadAllBytes(Path.Combine(Repo, repoRelative.Replace('/', Path.DirectorySeparatorChar)));

    /// <summary>`key → length:sha256-prefix` of the compressed native PDFs (uo-ffi tests/slides_abi.rs); keys are
    /// repository-relative paths.</summary>
    internal static List<(string Key, string Digest)> NativeDigests() =>
        File.ReadLines(Path.Combine(Core, "crates", "uo-ffi", "tests", "parity", "slides-native.tsv"))
            .Where(l => l.Length > 0).Select(l => l.Split('\t')).Select(p => (p[0], p[1])).ToList();

    internal static string Digest(byte[] pdf) => $"{pdf.Length}:{Convert.ToHexString(SHA256.HashData(pdf)).ToLowerInvariant()[..32]}";

    /// <summary>The smallest generated .pptx (loops run over it).</summary>
    internal static byte[] SmallDeck()
    {
        var dir = Path.Combine(Core, "crates", "uo-pptx", "tests", "captures", "decks", "b1a");
        return File.ReadAllBytes(new DirectoryInfo(dir).GetFiles("*.pptx").OrderBy(f => f.Length).ThenBy(f => f.Name).First().FullName);
    }
}

public class PdfParityTests(ITestOutputHelper log)
{
    [Fact]
    public void The_compressed_pdf_equals_the_rust_native_output_byte_for_byte()
    {
        using var conv = Fixtures.OpenFonts();
        var all = Fixtures.NativeDigests();
        var bad = new List<string>();
        foreach (var (key, digest) in all)
        {
            var pdf = conv.ToPdf(Fixtures.Read(key));
            if (Fixtures.Digest(pdf) != digest) bad.Add($"{key}: {Fixtures.Digest(pdf)} vs native {digest}");
        }
        log.WriteLine($"{all.Count} decks, {all.Count - bad.Count} equal to the Rust native PDF");
        Assert.True(all.Count > 60, $"{all.Count} decks in the digest file");
        Assert.Empty(bad);
    }

    [Fact]
    public void The_deck_route_equals_the_one_call_route_for_pptx_and_ppt()
    {
        using var conv = Fixtures.OpenFonts();
        foreach (var key in new[] { "slides/samples/reference.pptx", "slides/samples/legacy/lo-reference.ppt" })
        {
            var bytes = Fixtures.Read(key);
            using var deck = PptxDeck.Open(bytes);
            Assert.Equal(conv.ToPdf(bytes), deck.ToPdf(conv));
            Assert.Equal(conv.ToPdf(bytes), conv.ToPdf(deck));
            Assert.Equal(conv.ToPdf(bytes), PptxPdf.ToPdf(bytes, conv));
        }
    }

    [Fact]
    public void An_uncompressed_pdf_is_larger_and_still_a_pdf()
    {
        using var conv = Fixtures.OpenFonts();
        var bytes = Fixtures.Read("slides/samples/reference.pptx");
        var plain = conv.ToPdf(bytes, new SlidesPdfOptions { Compress = false });
        Assert.Equal("%PDF-"u8.ToArray(), plain[..5]);
        Assert.True(plain.Length > conv.ToPdf(bytes).Length);
    }

    [Fact]
    public void The_09_entry_point_compiles_and_converts_with_the_shared_converter()
    {
        var pdf = PptxPdf.ToPdf(Fixtures.Read("slides/samples/reference.pptx"));
        Assert.Equal("%PDF-"u8.ToArray(), pdf[..5]);
        log.WriteLine($"system index: {SlidesPdfConverter.Shared.Families.Count} families");
    }
}

public class DeckTests
{
    [Fact]
    public void A_deck_reports_its_size_notes_and_saves_a_deck_that_opens_again()
    {
        using var deck = PptxDeck.Open(Fixtures.Read("slides/samples/reference.pptx"), trusted: true);
        Assert.True(deck.SlideCount > 0);
        Assert.True(deck.SlideWidthEmu > 0 && deck.SlideHeightEmu > 0);
        Assert.False(deck.IsLegacyPpt);
        for (int i = 0; i < deck.SlideCount; i++) Assert.NotNull(deck.GetSlideNotes(i));
        Assert.Throws<ArgumentOutOfRangeException>(() => deck.GetSlideNotes(deck.SlideCount));
        Assert.Throws<ArgumentOutOfRangeException>(() => deck.GetSlideNotes(-1));
        using var again = PptxDeck.Open(deck.Save());
        Assert.Equal((deck.SlideCount, deck.SlideWidthEmu, deck.SlideHeightEmu), (again.SlideCount, again.SlideWidthEmu, again.SlideHeightEmu));
    }

    [Fact]
    public void A_ppt_opens_as_legacy_and_converts_to_a_pptx_with_the_same_slides()
    {
        var ppt = Fixtures.Read("slides/samples/legacy/lo-reference.ppt");
        using var deck = PptxDeck.Open(ppt);
        Assert.True(deck.IsLegacyPpt);
        var pptx = PptConverter.ToPptx(ppt);
        Assert.Equal("PK"u8.ToArray(), pptx[..2]);
        using var back = PptxDeck.Open(pptx);
        Assert.False(back.IsLegacyPpt);
        Assert.Equal((deck.SlideCount, deck.SlideWidthEmu, deck.SlideHeightEmu), (back.SlideCount, back.SlideWidthEmu, back.SlideHeightEmu));
        // a .pptx is not a .ppt for the converter
        Assert.Throws<InvalidDataException>(() => PptConverter.ToPptx(Fixtures.Read("slides/samples/reference.pptx")));
    }

    [Fact]
    public void A_disposed_deck_is_refused_not_a_crash()
    {
        var deck = PptxDeck.Open(Fixtures.SmallDeck());
        deck.Dispose();
        Assert.Throws<ObjectDisposedException>(() => deck.Save());
    }
}

public class ErrorTests
{
    [Fact]
    public void Garbage_is_invalid_data_with_a_message()
    {
        using var conv = Fixtures.OpenFonts();
        var e = Assert.Throws<InvalidDataException>(() => conv.ToPdf([1, 2, 3, 4]));
        Assert.Contains("ToPdf", e.Message);
        Assert.Throws<InvalidDataException>(() => PptxDeck.Open([]));
        Assert.Throws<InvalidDataException>(() => PptxDeck.Open(Fixtures.Read("core/crates/uo-docx-layout/tests/captures/f8/docs/f8__7__0.docx")));
        // a Compound File signature and nothing else (a damaged .ppt)
        Assert.Throws<InvalidDataException>(() => conv.ToPdf([0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]));
    }

    [Fact]
    public void Truncated_decks_are_refused_or_converted_never_a_crash()
    {
        using var conv = Fixtures.OpenFonts();
        var bytes = Fixtures.SmallDeck();
        int refused = 0;
        for (int i = 1; i < 20; i++)
        {
            try { conv.ToPdf(bytes[..(bytes.Length * i / 20)]); }
            catch (InvalidDataException) { refused++; }
            catch (UniversalOfficeException) { refused++; }
        }
        Assert.True(refused > 0);
    }

    [Fact]
    public void Null_arguments_are_argument_exceptions()
    {
        using var conv = Fixtures.OpenFonts();
        Assert.Throws<ArgumentNullException>(() => conv.ToPdf((byte[])null!));
        Assert.Throws<ArgumentNullException>(() => PptxDeck.Open(null!));
        Assert.Throws<ArgumentNullException>(() => PptConverter.ToPptx(null!));
        Assert.Throws<ArgumentNullException>(() => new SlidesPdfConverter(null!));
    }

    [Fact]
    public void A_cancelled_token_stops_the_conversion()
    {
        using var conv = Fixtures.OpenFonts();
        using var cts = new CancellationTokenSource();
        cts.Cancel();
        var bytes = Fixtures.Read("slides/samples/reference.pptx");
        Assert.ThrowsAny<OperationCanceledException>(() => conv.ToPdf(bytes, ct: cts.Token));
        using var deck = PptxDeck.Open(bytes);
        Assert.ThrowsAny<OperationCanceledException>(() => deck.ToPdf(conv, ct: cts.Token));
        // the deck is still usable after a cancelled call
        Assert.Equal("%PDF-"u8.ToArray(), deck.ToPdf(conv)[..5]);
        // a token that is never cancelled costs nothing and works
        using var live = new CancellationTokenSource();
        Assert.Equal("%PDF-"u8.ToArray(), conv.ToPdf(bytes, ct: live.Token)[..5]);
    }
}

/// <summary>Memory gate M4 at the .NET boundary: native handles created and released through Dispose() and through the
/// finalizer; the native live bytes must return exactly.</summary>
public class BoundaryMemoryTests(ITestOutputHelper log)
{
    private static readonly int Count = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_M4"), out var c) ? c : 10_000;
    private static readonly int Conversions = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_CONVERSIONS"), out var c2) ? c2 : 100;

    [Fact]
    public void The_native_library_counts_memory()
    {
        Assert.True(NativeCore.IsCountingMemory, "the tests build uo-ffi with --features count");
        Assert.StartsWith("0.10.0", NativeCore.Version);
    }

    [Fact]
    public void Dispose_releases_every_deck_and_converter()
    {
        var bytes = Fixtures.SmallDeck();
        using (var warm = PptxDeck.Open(bytes)) { _ = warm.Save(); }
        new SlidesPdfConverter(new SlidesPdfConverterOptions { SystemFonts = false }).Dispose();
        long before = Settle();
        for (int i = 0; i < Count; i++)
        {
            using var deck = PptxDeck.Open(bytes);
            if (i % 50 == 0) { Assert.True(deck.SlideCount > 0); Assert.NotEmpty(deck.Save()); }
        }
        for (int i = 0; i < Count; i++)
        {
            using var conv = new SlidesPdfConverter(new SlidesPdfConverterOptions { SystemFonts = false });
            Assert.Empty(conv.Families);
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count} decks (every 50th opened, saved) + {Count} converters through Dispose(): 0 B left");
    }

    [Fact]
    public void The_finalizer_releases_decks_that_were_not_disposed()
    {
        var bytes = Fixtures.SmallDeck();
        using (var warm = PptxDeck.Open(bytes)) { }
        long before = Settle();
        Abandon(bytes, Count / 5);
        Assert.True(NativeCore.LiveBytes > before, "the abandoned decks are counted while alive");
        Assert.Equal(before, Settle());
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static void Abandon(byte[] bytes, int n)
    {
        for (int i = 0; i < n; i++) _ = PptxDeck.Open(bytes);
    }

    [Fact]
    public void Conversions_leave_nothing_ok_failed_or_cancelled()
    {
        using var conv = Fixtures.OpenFonts();
        var bytes = Fixtures.Read("slides/samples/reference.pptx");
        var ppt = Fixtures.Read("slides/samples/legacy/lo-reference.ppt");
        conv.ToPdf(bytes); // warm-up: faces cached
        conv.ToPdf(ppt);
        long before = Settle();
        using var cts = new CancellationTokenSource();
        for (int i = 0; i < Conversions; i++)
        {
            Assert.True(conv.ToPdf(bytes, ct: cts.Token).Length > 1000);
        }
        for (int i = 0; i < 10; i++) Assert.True(conv.ToPdf(ppt).Length > 1000);
        // every failure path: a cancellable token (UoCancel handle + registration) with invalid data
        for (int i = 0; i < Count; i++)
        {
            Assert.Throws<InvalidDataException>(() => conv.ToPdf([1, 2, 3], ct: cts.Token));
        }
        using var already = new CancellationTokenSource();
        already.Cancel();
        using var deck = PptxDeck.Open(bytes);
        for (int i = 0; i < 100; i++) Assert.ThrowsAny<OperationCanceledException>(() => deck.ToPdf(conv, ct: already.Token));
        deck.Dispose();
        Assert.Equal(before, Settle());
        log.WriteLine($"{Conversions} conversions + 10 legacy + {Count} refused (with cancel handles) + 100 cancelled: 0 B left");
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
    private static readonly int PerThread = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_SOAK"), out var s) ? s : 3;

    [Fact]
    public void Parallel_conversions_give_the_sequential_bytes_and_leave_memory_flat()
    {
        using var conv = Fixtures.OpenFonts();
        string[] keys = ["slides/samples/reference.pptx", "slides/samples/stress-text.pptx", "slides/samples/gaps-smoke.pptx"];
        var decks = keys.Select(Fixtures.Read).ToArray();
        var expected = decks.Select(d => conv.ToPdf(d)).ToArray();
        using var shared = PptxDeck.Open(decks[0]);
        var lives = new List<long>();
        for (int round = 0; round < 3; round++)
        {
            Parallel.For(0, Threads, new ParallelOptions { MaxDegreeOfParallelism = Threads }, t =>
            {
                for (int i = 0; i < PerThread; i++)
                {
                    int k = (t + i) % decks.Length;
                    Assert.Equal(expected[k], conv.ToPdf(decks[k]));
                    // every thread also converts the ONE shared deck handle
                    Assert.Equal(expected[0], shared.ToPdf(conv));
                }
            });
            lives.Add(BoundaryMemoryTests.Settle());
        }
        log.WriteLine($"3 rounds x {Threads} threads x {PerThread} x 2 conversions; native live after each round: {string.Join(", ", lives)} B");
        Assert.True(lives.Skip(1).All(v => v == lives[1]), $"native live bytes grow: {string.Join(", ", lives)}");
    }
}
