using System.IO.Compression;
using System.Text;

namespace UniversalOffice.Docx.Tests;

/// <summary>Fixtures and documents of the Editing API tests.</summary>
internal static class EditingFixtures
{
    internal static string Path_(string name) => Path.Combine(Fixtures.Core, "crates", "uo-docx", "tests", "parity", "d3", "fixtures", name + ".docx");

    internal static byte[] Fixture(string name) => File.ReadAllBytes(Path_(name));

    internal static IEnumerable<string> FixtureNames() =>
        Directory.GetFiles(Path.GetDirectoryName(Path_("x"))!, "*.docx").Select(f => System.IO.Path.GetFileNameWithoutExtension(f)).OrderBy(n => n, StringComparer.Ordinal);

    internal const string W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";

    /// <summary>A package with the given body xml (paragraphs, tables…); no styles part.</summary>
    internal static byte[] Docx(string body, string extraNamespaces = "")
    {
        const string ct = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>";
        const string rels = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>";
        string doc = $"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><w:document xmlns:w=\"{W}\" {extraNamespaces}><w:body>{body}</w:body></w:document>";
        using var ms = new MemoryStream();
        using (var zip = new ZipArchive(ms, ZipArchiveMode.Create, leaveOpen: true))
        {
            foreach (var (name, text) in new[] { ("[Content_Types].xml", ct), ("_rels/.rels", rels), ("word/document.xml", doc) })
            {
                using var w = new StreamWriter(zip.CreateEntry(name).Open(), new UTF8Encoding(false));
                w.Write(text);
            }
        }
        return ms.ToArray();
    }

    /// <summary>A small template: a sample paragraph, a table row with a marker, a text with a parameter.</summary>
    internal static byte[] Small() => Docx(
        "<w:p><w:pPr><w:pStyle w:val=\"Sample\"/></w:pPr><w:r><w:t>#{Sample}</w:t></w:r></w:p>" +
        "<w:tbl><w:tr><w:tc><w:p><w:r><w:t>#{Row}</w:t></w:r><w:r><w:t xml:space=\"preserve\"> #{Other}</w:t></w:r></w:p></w:tc></w:tr></w:tbl>" +
        "<w:p><w:r><w:t>Hello #{Name}</w:t></w:r></w:p>");

    /// <summary>A paragraph holding a VML text box with the text <c>aaa</c>.</summary>
    internal static byte[] WithShape() => Docx(
        "<w:p><w:r><w:pict><v:rect><v:textbox><w:txbxContent><w:p><w:r><w:t>aaa</w:t></w:r></w:p></w:txbxContent></v:textbox></v:rect></w:pict></w:r></w:p>",
        "xmlns:v=\"urn:schemas-microsoft-com:vml\"");

    /// <summary>A 1x1 PNG.</summary>
    internal static readonly byte[] Png = Convert.FromBase64String("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==");
}
