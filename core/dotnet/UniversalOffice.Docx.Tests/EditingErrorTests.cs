using System.Xml;
using UniversalOffice.Core;
using UniversalOffice.Docx.Editing;
using UniversalOffice.Native;
using Xunit;
using static UniversalOffice.Docx.Tests.EditingFixtures;

namespace UniversalOffice.Docx.Tests;

/// <summary>The exception classes and properties of the 0.9 C# (the Rust ErrorKind through the facade).</summary>
public class EditingErrorTests
{
    [Fact]
    public void An_xml_depth_limit_carries_the_limit_the_maximum_and_the_part()
    {
        var e = Assert.Throws<OfficeLimitExceededException>(() => DocxDocument.Open(Fixture("deep-257")));
        Assert.Equal(OfficeLimit.XmlDepth, e.Limit);
        Assert.Equal(256, e.Maximum);
        Assert.Equal("word/document.xml", e.PartName);
        Assert.Equal(5, e.Code);
        // the boundary of the limit opens
        using var ok = DocxDocument.Open(Fixture("deep-256"));
        // a tighter limit throws earlier
        var tight = Assert.Throws<OfficeLimitExceededException>(() => DocxDocument.Open(Fixture("deep-256"), new DocxLimits { MaxXmlDepth = 10 }));
        Assert.Equal(10, tight.Maximum);
    }

    [Fact]
    public void Package_limits_name_the_cap_and_the_part()
    {
        var part = Assert.Throws<OfficeLimitExceededException>(() =>
            DocxDocument.Open(Fixture("fields"), new DocxLimits { Package = new OpcPackageLimits { MaxPartBytes = 100 } }));
        Assert.Equal(OfficeLimit.PartBytes, part.Limit);
        Assert.Equal(100, part.Maximum);
        Assert.NotNull(part.PartName);
        var total = Assert.Throws<OfficeLimitExceededException>(() =>
            DocxDocument.Open(Fixture("fields"), new DocxLimits { Package = new OpcPackageLimits { MaxTotalBytes = 100 } }));
        Assert.Equal(OfficeLimit.TotalBytes, total.Limit);
        Assert.Null(total.PartName);
        // no caps opens
        using var none = DocxDocument.Open(Fixture("fields"), new DocxLimits { Package = OpcPackageLimits.None });
        // a negative cap counts as 0 (D3A-11)
        Assert.Throws<OfficeLimitExceededException>(() => DocxDocument.Open(Fixture("fields"), new DocxLimits { Package = new OpcPackageLimits { MaxPartBytes = -5 } }));
    }

    [Fact]
    public void Field_limits_come_from_the_sanitizer()
    {
        using var open = DocxDocument.Open(Fixture("open-fields"));
        var e = Assert.Throws<OfficeLimitExceededException>(() => DocxSanitizer.Sanitize(open, DocxSanitizeOptions.Content));
        Assert.Equal(OfficeLimit.FieldNesting, e.Limit);
        using var many = DocxDocument.Open(Fixture("many-fields"), new DocxLimits { MaxFieldsPerStory = 10 });
        var m = Assert.Throws<OfficeLimitExceededException>(() => DocxSanitizer.Sanitize(many, DocxSanitizeOptions.Content));
        Assert.Equal(OfficeLimit.FieldsPerStory, m.Limit);
        Assert.Equal(10, m.Maximum);
    }

    [Fact]
    public void Corrupt_packages_throw_the_class_the_csharp_threw()
    {
        Assert.Throws<XmlException>(() => DocxDocument.Open(Fixture("bad-xml")));
        Assert.Throws<XmlException>(() => DocxDocument.Open(Fixture("doctype")));
        Assert.Throws<InvalidDataException>(() => DocxDocument.Open(Fixture("dtd")));
        Assert.Throws<InvalidDataException>(() => DocxDocument.Open(Fixture("not-word")));
        Assert.Throws<InvalidDataException>(() => DocxDocument.Open([1, 2, 3, 4]));
        Assert.Throws<InvalidDataException>(() => DocxDocument.Open([]));
    }

    [Fact]
    public void Argument_errors_and_nulls()
    {
        Assert.Throws<ArgumentNullException>(() => DocxDocument.Open(null!));
        Assert.Throws<ArgumentNullException>(() => DocxDocument.Open([], null!));
        using var doc = DocxDocument.Open(Small());
        Assert.Throws<ArgumentNullException>(() => DocxText.Replace(doc, null!, "x"));
        Assert.Throws<ArgumentNullException>(() => DocxText.Replace(null!, "x", "y"));
        Assert.Throws<ArgumentNullException>(() => DocxText.RemoveParagraphsContaining(doc, ["a", null!]));
        Assert.Throws<ArgumentException>(() => DocxText.Replace(doc, "", "x"));
        Assert.Throws<ArgumentException>(() => CustomInlineTag.Format("bad type", "{}"));
        Assert.Throws<System.Text.Json.JsonException>(() => CustomInlineTag.Format("t", "{bad"));
        Assert.Throws<ArgumentNullException>(() => DocxSanitizer.Sanitize(doc, null!));
        Assert.Throws<ArgumentNullException>(() => new DocxBodyWriter(null!));
    }

    [Fact]
    public void A_call_after_dispose_throws_and_dispose_is_idempotent()
    {
        var doc = DocxDocument.Open(Small());
        var writer = new DocxBodyWriter(doc);
        doc.Dispose();
        doc.Dispose();
        Assert.Throws<ObjectDisposedException>(() => doc.Save());
        Assert.Throws<ObjectDisposedException>(() => doc.Properties.Title);
        Assert.Throws<ObjectDisposedException>(() => DocxText.GetPlainText(doc));
        Assert.Throws<ObjectDisposedException>(() => writer.Finish());
        writer.Dispose();
    }

    [Fact]
    public void Writers_of_two_documents_and_detached_shapes_do_not_interfere()
    {
        using var a = DocxDocument.Open(Small());
        using var b = DocxDocument.Open(Small());
        using var writer = new DocxBodyWriter(a);
        writer.AddText("x", DocxRunFormat.None);
        // each writer works with its own document
        using var wb = new DocxBodyWriter(b);
        wb.AddText("y", DocxRunFormat.None);
        wb.Finish();
        // a shape removed from the document is a detached node: its operations do not crash
        using var shapes = DocxDocument.Open(WithShape());
        var shape = DocxShapes.FindContaining(shapes, "aaa")[0];
        shape.Remove();
        shape.ClearText();
    }

    [Fact]
    public void A_throwing_validator_propagates_its_exception_and_leaves_no_handle()
    {
        long before = BoundaryMemoryTests.Settle();
        for (int i = 0; i < 50; i++)
        {
            using var doc = DocxDocument.Open(Fixture("inlines"));
            var e = Assert.Throws<NotSupportedException>(() =>
                DocxSanitizer.Sanitize(doc, DocxSanitizeOptions.Content with { IsCustomInlineValid = (t, j) => throw new NotSupportedException("host") }));
            Assert.Equal("host", e.Message);
        }
        Assert.Equal(before, BoundaryMemoryTests.Settle());
    }

    [Fact]
    public void Import_body_of_a_document_into_itself_is_a_copy_import()
    {
        using var doc = DocxDocument.Open(Small());
        DocxTemplate.ImportBody(doc, doc);
        Assert.Equal(2, DocxText.GetPlainText(doc).Split("Hello").Length - 1);
    }

    [Fact]
    public void The_pdf_path_limit_is_the_same_class_with_the_conversion_limit()
    {
        var e = new OfficeLimitExceededException(OfficeLimit.XmlDepth, 5, "p");
        Assert.Equal("p", e.PartName);
        Assert.IsAssignableFrom<UniversalOfficeException>(e);
        _ = NativeCore.Version;
    }
}
