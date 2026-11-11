using System.Xml.Linq;
using UniversalOffice.Core;
using UniversalOffice.Docx.Editing;
using Xunit;
using Xunit.Abstractions;
using static UniversalOffice.Docx.Tests.EditingFixtures;

namespace UniversalOffice.Docx.Tests;

/// <summary>Every call shape clickCompliance uses (and the rest of the 0.9 Editing surface), compiled and run
/// over the facade: a failure to compile here is an API difference.</summary>
public class EditingContractTests(ITestOutputHelper log)
{
    [Fact]
    public void The_clickcompliance_flow_compiles_and_runs()
    {
        var limits = new DocxLimits { Package = new OpcPackageLimits { MaxPartBytes = 64 << 20, MaxTotalBytes = 256 << 20 }, MaxXmlDepth = 128 };
        var seen = new List<(string, string)>();
        using var content = DocxDocument.Open(Fixture("inlines"), limits);
        DocxSanitizer.Sanitize(content, DocxSanitizeOptions.Content with { MergeSections = false, IsCustomInlineValid = (t, j) => { seen.Add((t, j)); return t == "mention"; } });
        Assert.NotEmpty(seen);
        using var template = DocxDocument.Open(Fixture("sample-template"));
        DocxTemplate.ApplyContent(template, content.Save(), new DocxApplyContentOptions { SampleMarkers = ["#{Normal}"], PreserveSections = true });
        template.Properties.Title += " (generated)";
        template.Properties.Author = "me";
        template.Properties.Company = "ACME";
        template.Properties.Category = "cat";
        template.Properties.Subject = "sub";
        template.Properties.LastAuthor = "me";
        template.Properties.ApplicationName = "app";
        template.Properties.LastSaveDate = DateTime.UtcNow;
        DocxSanitizer.Sanitize(template, DocxSanitizeOptions.Output);
        var saved = template.Save();
        using var again = DocxDocument.Open(saved);
        Assert.Equal("app", again.Properties.ApplicationName);
        Assert.EndsWith(" (generated)", again.Properties.Title);
        log.WriteLine($"flow: {seen.Count} inlines judged, {saved.Length} bytes");
    }

    [Fact]
    public void Dates_are_kept_in_utc_and_a_local_date_is_converted()
    {
        using var doc = DocxDocument.Open(Fixture("metadata"));
        var utc = new DateTime(2026, 11, 11, 10, 20, 30, DateTimeKind.Utc).AddTicks(1234567);
        doc.Properties.LastSaveDate = utc;
        Assert.Equal(new DateTime(2026, 11, 11, 10, 20, 30, DateTimeKind.Utc), doc.Properties.LastSaveDate);
        Assert.Equal(DateTimeKind.Utc, doc.Properties.LastSaveDate!.Value.Kind);
        var local = new DateTime(2026, 6, 1, 12, 0, 0, DateTimeKind.Local);
        doc.Properties.CreatedDate = local;
        Assert.Equal(new DateTime(local.ToUniversalTime().Ticks, DateTimeKind.Utc), doc.Properties.CreatedDate);
        doc.Properties.LastSaveDate = null;
        Assert.Null(doc.Properties.LastSaveDate);
        doc.Properties.Keywords = null;
        Assert.Null(doc.Properties.Keywords);
        doc.Properties.SetCustomProperty("Nome", "a\nb");
        Assert.Contains("Nome", doc.Properties.CustomPropertyNames);
    }

    [Fact]
    public void Text_parameters_and_plain_text()
    {
        using var doc = DocxDocument.Open(Small());
        Assert.Equal("#{Name}", DocxText.Parameter("Name"));
        Assert.True(DocxText.Contains(doc, "#{Name}"));
        Assert.Equal(1, DocxText.ReplaceParameter(doc, "name", "Ana\nMaria"));
        Assert.Contains("Ana", DocxText.GetPlainText(doc));
        Assert.Equal(1, DocxText.ReplaceParameters(doc, [new("Other", "x")]));
        Assert.Matches(DocxText.AnyParameter, "#{Row}");
        Assert.Equal(1, DocxText.Replace(doc, "hello", "Bye", caseSensitive: false));
        Assert.Equal(0, DocxText.RemoveParagraphsContaining(doc, ["nothing here"]));
        Assert.True(DocxParagraphSamples.Contains(doc, "Sample"));
        var samples = DocxParagraphSamples.Extract(doc, ["Sample", "Missing"]);
        Assert.Equal(["Sample"], samples.Keys);
        Assert.Equal("Sample", samples["Sample"].StyleId);
        Assert.Null(samples["Sample"].NumId);
        Assert.False(DocxParagraphSamples.Contains(doc, "Sample"));
    }

    [Fact]
    public void Row_templates_and_the_body_writer_with_formats()
    {
        using var doc = DocxDocument.Open(Small());
        using var row = DocxTableRowTemplate.Take(doc, "#{Row}");
        Assert.NotNull(row);
        row!.Append([new("Row", "r1"), new("Other", null)]);
        row.Append([new("Row", "r2")], removeRemainingParameters: false);
        Assert.Equal(2, row.AppendedRows);
        Assert.Null(DocxTableRowTemplate.Take(doc, "#{NoSuchMarker}"));
        var text = DocxText.GetPlainText(doc);
        Assert.Contains("r1", text);
        Assert.Contains("r2", text);

        var samples = DocxParagraphSamples.Extract(doc, ["Sample"]);
        using var writer = new DocxBodyWriter(doc, new DocxBodyWriterOptions { LinkColor = "0000FF" });
        Assert.True(writer.LineIsEmpty);
        var bold = DocxRunFormat.None with { Bold = true, VerticalAlign = DocxVerticalAlign.Superscript, Color = "#FF0000", Link = "https://example.com/a" };
        writer.AddText("Title ", bold);
        writer.AddText("line\ttab", DocxRunFormat.None);
        writer.AddBreak();
        Assert.False(writer.LineIsEmpty);
        Assert.True(writer.AddImage(Png));
        writer.EndParagraph(new DocxParagraphFormat { Base = samples["Sample"], Alignment = "center", KeepWithNext = true, LeftIndentTwips = 720 });
        writer.AddText("second", DocxRunFormat.None with { Italic = true });
        writer.EndParagraph(DocxParagraphFormat.None);
        writer.Finish();
        var after = DocxText.GetPlainText(doc);
        Assert.Contains("Title", after);
        Assert.Contains("second", after);
        Assert.Throws<ArgumentException>(() =>
        {
            writer.AddText("x", DocxRunFormat.None);
            writer.EndParagraph(DocxParagraphFormat.None with { Alignment = "middle" });
        });
        using var saved = DocxDocument.Open(doc.Save());
        Assert.Contains("second", DocxText.GetPlainText(saved));
    }

    [Fact]
    public void Custom_inlines_find_unwrap_replace_and_the_tag_contract()
    {
        using var doc = DocxDocument.Open(Fixture("inlines"));
        var all = DocxCustomInlines.Find(doc);
        Assert.NotEmpty(all);
        var first = all[0];
        Assert.False(string.IsNullOrEmpty(first.Type));
        Assert.True(CustomInlineTag.TryParse(CustomInlineTag.Format(first.Type, first.Json), out var type, out var json));
        Assert.Equal(first.Type, type);
        Assert.Equal(first.Json, json);
        Assert.True(first.IsAttached);
        Assert.NotNull(first.Text);
        _ = first.Title;
        Assert.Equal(all.Count(i => i.Type == first.Type), DocxCustomInlines.Find(doc, first.Type).Count);
        Assert.Empty(DocxCustomInlines.Find(doc, "no-such-type"));
        if (all.Count > 1)
        {
            Assert.False(all[1].ReplaceWithHyperlink("javascript:alert(1)", "text"));
            Assert.False(all[1].IsAttached);
        }
        first.Unwrap();
        Assert.False(first.IsAttached);
        Assert.Throws<InvalidOperationException>(() => first.Unwrap());
        int left = DocxCustomInlines.UnwrapInvalid(doc, (t, j) => false);
        Assert.Empty(DocxCustomInlines.Find(doc));
        log.WriteLine($"unwrapped {left} more");
        using var plain = DocxDocument.Open(Small());
        Assert.Equal(1, DocxCustomInlines.ReplaceText(plain, "#{Name}", "mention", "{\"id\":1}", "Name"));
        var made = DocxCustomInlines.Find(plain, "mention");
        Assert.Single(made);
        Assert.True(made[0].ReplaceWithHyperlink("https://example.com", "Link"));
        Assert.False(CustomInlineTag.TryParse("not a tag", out _, out _));
        Assert.False(CustomInlineTag.TryParse(null, out _, out _));
        Assert.StartsWith(CustomInlineTag.Prefix, CustomInlineTag.Format("a.b-c_d", "{}"));
    }

    [Fact]
    public void Shapes_headers_and_pictures()
    {
        using var doc = DocxDocument.Open(WithShape());
        var shapes = DocxShapes.FindContaining(doc, "aaa");
        var shape = Assert.Single(shapes);
        Assert.Equal("aaa", shape.Text);
        shape.SetFillColor("#00FF00");
        shape.SetFillTransparent();
        shape.SetTextColor("blue", lastParagraphOnly: false);
        shape.ClearText();
        Assert.Equal("", shape.Text);
        Assert.Equal(0, DocxShapes.RemoveContaining(doc, "aaa"));
        Assert.Throws<ArgumentException>(() => shape.SetFillColor("nope"));
        shape.Remove();
        // as on the detached XElement of 0.9, the operations on a removed shape do not throw
        _ = shape.Text;
        Assert.Null(DocxShapes.FirstShapeIn(doc, footer: true));
        Assert.False(DocxShapes.ClearHeaderFooter(doc, footer: false, DocxHeaderFooterType.First));
        Assert.Equal(0, DocxShapes.ReplaceTextWithImage(doc, "#{Logo}", Png, 100f, 50.5f));
        Assert.Equal(0, DocxPictures.FitToPage(doc));
        Assert.Equal((100L, 50L), DocxPictures.Fit(1000, 500, 100, null));
        Assert.Equal((10L, 5L), DocxPictures.Fit(10, 5, null, null));
        Assert.Null(DocxNumbering.AddRestartedInstance(doc, "99"));
    }

    [Fact]
    public void Hyperlink_fields_defaults_and_pure_helpers()
    {
        Assert.Contains("HYPERLINK", DocxFields.DefaultAllowedFields);
        Assert.Contains("DATE", DocxFields.DefaultOutputAllowedFields);
        Assert.Equal(["http", "https", "mailto"], DocxFields.DefaultAllowedSchemes);
        Assert.True(DocxFields.IsAllowedUrl("HTTPS://x.com/a", DocxFields.DefaultAllowedSchemes));
        Assert.False(DocxFields.IsAllowedUrl("javascript:alert(1)", DocxFields.DefaultAllowedSchemes));
        Assert.False(DocxFields.IsAllowedUrl("relative/path", DocxFields.DefaultAllowedSchemes));
        Assert.True(DocxFields.TryParseHyperlink("HYPERLINK \"https://a.com/x\" \\l \"top\"", DocxFields.DefaultAllowedSchemes, out var url, out var anchor));
        Assert.Equal("https://a.com/x#top", url);
        Assert.Null(anchor);
        Assert.True(DocxFields.TryParseHyperlink("HYPERLINK \\l \"mark\"", DocxFields.DefaultAllowedSchemes, out url, out anchor));
        Assert.Equal("mark", anchor);
        Assert.False(DocxFields.TryParseHyperlink("PAGE", DocxFields.DefaultAllowedSchemes, out _, out _));
        using var doc = DocxDocument.Open(Fixture("hyperlinks"));
        int converted = DocxFields.ConvertHyperlinkFields(doc);
        Assert.True(converted >= 0);
        Assert.NotNull(DocxFields.GetInstructions(doc));
        DocxFields.ReplaceDisallowedWithResult(doc, ["PAGE"], ["https"]);
        var info = ImageInfo.TryRead(Png);
        Assert.Equal(new ImageInfo("png", "image/png", 1, 1, 96.0, 96.0).WidthPx, info!.WidthPx);
        Assert.Equal("png", info.Extension);
        Assert.Null(ImageInfo.TryRead(new byte[] { 1, 2, 3 }));
    }

    [Fact]
    public void Parts_relationships_and_xelement_helpers()
    {
        using var doc = DocxDocument.Open(Fixture("hyperlinks"));
        Assert.Equal("word/document.xml", doc.MainPartPath);
        Assert.Contains("word/document.xml", doc.PartNames);
        Assert.True(doc.HasPart("word/document.xml"));
        Assert.False(doc.HasPart("word/nothing.xml"));
        Assert.NotNull(doc.GetPartBytes("word/document.xml"));
        Assert.Null(doc.GetPartBytes("word/nothing.xml"));
        var rels = doc.GetRelationships("word/document.xml");
        foreach (var r in rels) Assert.Equal(r, doc.GetRelationship("word/document.xml", r.Id));
        Assert.Null(doc.GetRelationship("word/document.xml", "rId9999"));
        Assert.NotNull(doc.GetContentType("word/document.xml"));
        Assert.Equal("application/xml", doc.GetContentType("word/nothing.xml")); // the default of the extension
        Assert.Null(doc.GetContentType("word/nothing.unknownext"));
        using var clone = doc.Clone();
        Assert.Equal(doc.Save(), clone.Save());
        // DocxPageArea.From / ParagraphStyleName take the host's XElement
        XNamespace w = W;
        var sect = new XElement(w + "sectPr", new XElement(w + "pgSz", new XAttribute(w + "w", "11906"), new XAttribute(w + "h", "16838")), new XElement(w + "pgMar", new XAttribute(w + "left", "1000")));
        var area = DocxPageArea.From(sect);
        Assert.Equal(11906, area.PageWidth);
        Assert.Equal(1000, area.MarginLeft);
        Assert.Equal(1440, area.MarginRight);
        Assert.Equal(11906 - 1000 - 1440, area.ClientWidth);
        Assert.Equal(12240, DocxPageArea.From(null).PageWidth);
        using var styled = DocxDocument.Open(Fixture("styles"));
        var p = new XElement(w + "p", new XElement(w + "pPr", new XElement(w + "pStyle", new XAttribute(w + "val", "Heading1"))));
        Assert.NotNull(DocxStyles.ParagraphStyleName(styled, p));
        _ = DocxStyles.ParagraphStyleName(styled, new XElement(w + "p"));
    }

    [Fact]
    public void Review_and_structure_operations_run_on_every_fixture_that_opens()
    {
        int opened = 0;
        foreach (var name in FixtureNames())
        {
            DocxDocument doc;
            try { doc = DocxDocument.Open(Fixture(name)); }
            catch (Exception e) when (e is InvalidDataException or System.Xml.XmlException or OfficeLimitExceededException) { continue; }
            using (doc)
            {
                opened++;
                try
                {
                    DocxReview.AcceptAllRevisions(doc);
                    DocxReview.RemoveComments(doc);
                    DocxReview.DisableTrackChanges(doc);
                    DocxReview.RemoveProtection(doc);
                    DocxReview.RemoveAttachedTemplate(doc);
                    DocxReview.RemoveMacros(doc);
                    DocxSanitizer.RemoveUnsafeContent(doc);
                    DocxSanitizer.RemoveCustomXml(doc);
                    DocxSanitizer.MergeSections(doc);
                    DocxSanitizer.RemoveHeadersAndFooters(doc);
                    doc.Properties.RemovePersonalMetadata();
                    doc.Properties.RemoveCustomProperties();
                    DocxTemplate.Empty(doc);
                    Assert.NotEmpty(doc.Save());
                }
                catch (Exception e) when (e is InvalidDataException or System.Xml.XmlException or OfficeLimitExceededException) { }
            }
        }
        Assert.True(opened >= 25, $"{opened} fixtures opened");
        log.WriteLine($"{opened} fixtures through every no-argument operation");
    }

    [Fact]
    public void OpcPartName_resolves_relationship_targets_like_the_clickcompliance_validator_calls_it()
    {
        // the call shape of DocxPackageValidationService / OpcPartNameContractTests: a public static in UniversalOffice.Core
        Assert.Equal("word/fn%31.xml", OpcPartName.ResolveRelationshipTarget("word/document.xml", "fn%2531.xml"));
        Assert.Equal("word/media/a.png", OpcPartName.ResolveRelationshipTarget("/word/document.xml", "media/a.png#frag"));
        Assert.Equal("customXml/item1.xml", OpcPartName.ResolveRelationshipTarget("word/document.xml", "../customXml/item1.xml"));
        Assert.Equal("word/document.xml", OpcPartName.ResolveRelationshipTarget("", "/word/document.xml"));
        Assert.Null(OpcPartName.ResolveRelationshipTarget("word/document.xml", "http://example.com/a"));
        Assert.Null(OpcPartName.ResolveRelationshipTarget("word/document.xml", "../../x.xml"));
        Assert.Null(OpcPartName.ResolveRelationshipTarget("word/document.xml", "a%2Fb.xml"));
        Assert.Null(OpcPartName.ResolveRelationshipTarget("word/document.xml", ""));
        Assert.Throws<ArgumentNullException>(() => OpcPartName.ResolveRelationshipTarget("word/document.xml", null!));
        Assert.Equal("a%20b%2531.xml", OpcPartName.EscapeSegment("a b%31.xml"));
        Assert.Equal("word/a b%31.xml", OpcPartName.ResolveRelationshipTarget("word/document.xml", OpcPartName.EscapeSegment("a b%31.xml")));
    }
}
