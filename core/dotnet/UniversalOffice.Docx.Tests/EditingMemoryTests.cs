using System.Runtime.CompilerServices;
using UniversalOffice.Docx.Editing;
using UniversalOffice.Native;
using Xunit;
using Xunit.Abstractions;
using static UniversalOffice.Docx.Tests.EditingFixtures;

namespace UniversalOffice.Docx.Tests;

/// <summary>Memory gate M4 for the Editing handles: created and released through Dispose() and through the
/// finalizer, the native live bytes come back exactly. <c>UO_DOTNET_M4</c> sets the count (default 10,000).</summary>
[Collection("memory")]
public class EditingMemoryTests(ITestOutputHelper log)
{
    private static readonly int Count = int.TryParse(Environment.GetEnvironmentVariable("UO_DOTNET_M4"), out var c) ? c : 10_000;
    private static readonly long Settle0 = BoundaryMemoryTests.Settle();

    private static long Settle() => BoundaryMemoryTests.Settle();

    /// <summary>Warm up the first-use state (the default lists, thread-local slots) so it is not counted.</summary>
    private static void Warm()
    {
        using var d = DocxDocument.Open(Small());
        _ = DocxFields.DefaultAllowedFields;
        _ = DocxSanitizeOptions.Content;
        d.Save();
        try { DocxDocument.Open(Fixture("deep-257")); } catch (UniversalOffice.Core.OfficeLimitExceededException) { }
        using var row = DocxTableRowTemplate.Take(d, "#{Row}");
    }

    [Fact]
    public void Ten_thousand_documents_open_save_dispose_leave_nothing()
    {
        Warm();
        var bytes = Small();
        long before = Settle();
        for (int i = 0; i < Count; i++)
        {
            using var doc = DocxDocument.Open(bytes);
            Assert.NotEmpty(doc.Save());
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count} documents Open -> Save -> Dispose: 0 B left ({Settle0} B at class start)");
    }

    [Fact]
    public void Ten_thousand_abandoned_documents_are_released_by_the_finalizer()
    {
        Warm();
        long before = Settle();
        Abandon(Count);
        Assert.True(NativeCore.LiveBytes > before, "the abandoned documents are counted while alive");
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count} documents abandoned, finalized: 0 B left");
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static void Abandon(int n)
    {
        var bytes = Small();
        for (int i = 0; i < n; i++)
        {
            var doc = DocxDocument.Open(bytes);
            _ = doc.MainPartPath;
        }
    }

    [Fact]
    public void Writers_row_templates_stages_samples_and_properties_leave_nothing()
    {
        Warm();
        var bytes = Small();
        long before = Settle();
        for (int i = 0; i < Count; i++)
        {
            using var doc = DocxDocument.Open(bytes);
            using (var w = new DocxBodyWriter(doc)) { w.AddText("x", DocxRunFormat.None); w.EndParagraph(DocxParagraphFormat.None); w.Finish(); }
            using (var row = DocxTableRowTemplate.Take(doc, "#{Row}")) { row!.Append([new("Row", "a")]); }
            var samples = DocxParagraphSamples.Extract(doc, ["Sample"]);
            foreach (var s in samples.Values) s.Dispose();
            // the two-phase sanitizer creates and frees a stage
            DocxSanitizer.Sanitize(doc, DocxSanitizeOptions.Output with { IsCustomInlineValid = (t, j) => true });
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count} x (document + writer + row template + sample set + paragraph props + sanitize stage) through Dispose(): 0 B left");
    }

    [Fact]
    public void Abandoned_writers_templates_and_properties_are_finalized()
    {
        Warm();
        long before = Settle();
        AbandonHandles(Count / 10);
        Assert.Equal(before, Settle());
        log.WriteLine($"{Count / 10} x (writer + row template + paragraph props) abandoned, finalized: 0 B left");
    }

    [MethodImpl(MethodImplOptions.NoInlining)]
    private static void AbandonHandles(int n)
    {
        var bytes = Small();
        for (int i = 0; i < n; i++)
        {
            var doc = DocxDocument.Open(bytes);
            _ = new DocxBodyWriter(doc);
            _ = DocxTableRowTemplate.Take(doc, "#{Row}");
            _ = DocxParagraphSamples.Extract(doc, ["Sample"]);
        }
    }

    [Fact]
    public void Failed_opens_and_failed_operations_leave_nothing()
    {
        Warm();
        var deep = Fixture("deep-257");
        var bad = Fixture("bad-xml");
        long before = Settle();
        for (int i = 0; i < Count; i++)
        {
            Assert.ThrowsAny<Exception>(() => DocxDocument.Open(deep));
            Assert.ThrowsAny<Exception>(() => DocxDocument.Open(bad));
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{2 * Count} failed opens (limit + xml): 0 B left");
    }

    [Fact]
    public void Operations_on_every_fixture_repeated_leave_nothing()
    {
        Warm();
        var names = FixtureNames().ToList();
        long before = Settle();
        int rounds = Math.Max(1, Count / 1000);
        for (int r = 0; r < rounds; r++)
        {
            foreach (var name in names)
            {
                try
                {
                    using var doc = DocxDocument.Open(Fixture(name));
                    DocxSanitizer.Sanitize(doc, DocxSanitizeOptions.Content with { IsCustomInlineValid = (t, j) => j.Length > 2 });
                    DocxText.ReplaceParameter(doc, "Name", "x");
                    _ = DocxFields.GetInstructions(doc);
                    _ = DocxCustomInlines.Find(doc);
                    _ = doc.Properties.CustomPropertyNames;
                    doc.Save();
                }
                catch (Exception e) when (e is InvalidDataException or System.Xml.XmlException or UniversalOffice.Core.OfficeLimitExceededException) { }
            }
        }
        Assert.Equal(before, Settle());
        log.WriteLine($"{rounds} rounds x {names.Count} fixtures (sanitize, text, fields, inlines, save): 0 B left");
    }
}
