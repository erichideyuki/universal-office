using System.Text;
using UniversalOffice.Tests.Editing;
using static UniversalOffice.Tests.Editing.DocxBuilder;

namespace UoD3Harness;

/// <summary>
/// Synthetic documents built with the frozen test builders (DocxBuilder, SampleTemplate): the
/// cases of the C# Editing suite (revisions, comments, sections, headers, fields, hyperlinks,
/// plugin inlines, customXml, OLE/controls/altChunk, linked images, macros, metadata, text
/// boxes, pictures, VML, deep and odd XML) as files, so that the Rust side replays the same
/// bytes. Written once into tests/parity/d3/fixtures (committed; no fonts).
/// </summary>
internal static class Fixtures
{
    private const string RelBase = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";

    private static string Nested(int depth)
    {
        var sb = new StringBuilder();
        for (int i = 0; i < depth; i++) sb.Append("<x:a>");
        for (int i = 0; i < depth; i++) sb.Append("</x:a>");
        return sb.ToString();
    }

    private static string Hdr(string tag, string body) =>
        $"<w:{tag} xmlns:w=\"{WordNs}\" xmlns:r=\"{RelNs}\">{body}</w:{tag}>";

    public static IEnumerable<(string Name, byte[] Data)> All()
    {
        yield return ("sample-template", SampleTemplate.Build());
        yield return ("sample-template-plain", SampleTemplate.Build(withSamples: false));
        yield return ("revisions", new DocxBuilder().WithBody(
            Paragraph(Run("antes ") + "<w:ins w:id=\"1\" w:author=\"A\"><w:r><w:t>novo</w:t></w:r></w:ins>" + "<w:del w:id=\"2\" w:author=\"A\"><w:r><w:delText>velho</w:delText></w:r></w:del>" + Run(" depois", "<w:b/><w:rPrChange w:id=\"3\" w:author=\"A\"><w:rPr/></w:rPrChange>")),
            Paragraph(Run("junta"), "<w:rPr><w:del w:id=\"4\" w:author=\"A\"/></w:rPr>"),
            Paragraph(Run("com o seguinte"), "<w:jc w:val=\"center\"/>"),
            Rows([Paragraph(Run("fica"))], ["<w:p><w:r><w:t>sai</w:t></w:r></w:p>"]).Replace("<w:tr><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>sai", "<w:tr><w:trPr><w:del w:id=\"5\" w:author=\"A\"/></w:trPr><w:tc><w:tcPr><w:tcW w:w=\"2000\" w:type=\"dxa\"/></w:tcPr><w:p><w:r><w:t>sai"),
            Paragraph("<w:moveFromRangeStart w:id=\"6\" w:name=\"m\"/><w:moveFrom w:id=\"7\" w:author=\"A\"><w:r><w:t>movido</w:t></w:r></w:moveFrom><w:moveFromRangeEnd w:id=\"6\"/><w:moveTo w:id=\"8\" w:author=\"A\"><w:r><w:t>aqui</w:t></w:r></w:moveTo>"))
            .WithSettings($"<w:settings xmlns:w=\"{WordNs}\"><w:trackRevisions/><w:documentProtection w:edit=\"readOnly\"/></w:settings>").Build());
        yield return ("comments", new DocxBuilder()
            .WithBody(Paragraph("<w:commentRangeStart w:id=\"0\"/>" + Run("comentado") + "<w:commentRangeEnd w:id=\"0\"/><w:r><w:commentReference w:id=\"0\"/></w:r>"))
            .WithComments($"<w:comments xmlns:w=\"{WordNs}\"><w:comment w:id=\"0\" w:author=\"A\"><w:p><w:r><w:t>nota</w:t></w:r></w:p></w:comment></w:comments>").Build());
        yield return ("sections", new DocxBuilder().WithBody(
            Paragraph(Run("seção 1"), "<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:cols w:num=\"2\" w:space=\"720\"/></w:sectPr>"),
            Paragraph(Run("seção 2 com texto bem comprido")),
            Paragraph(Run("seção 3"), "<w:sectPr><w:type w:val=\"continuous\"/><w:cols w:num=\"3\"/><w:titlePg/><w:pgNumType w:start=\"5\"/></w:sectPr>"),
            Paragraph(Run("última")))
            .WithSectionProperties("<w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/><w:cols w:num=\"2\" w:space=\"708\" w:equalWidth=\"0\"><w:col w:w=\"4000\" w:space=\"708\"/><w:col w:w=\"4652\"/></w:cols></w:sectPr>").Build());
        yield return ("headers", new DocxBuilder()
            .WithHeader("rId20", Hdr("hdr", Paragraph(Run("cabeçalho")) ))
            .WithFooter("rId21", Hdr("ftr", Paragraph(Run("rodapé #{Title}"), "<w:jc w:val=\"right\"/>")))
            .WithHeader("rId22", Hdr("hdr", ""), "header2.xml")
            .WithBody(Paragraph(Run("corpo #{Title} e #{title}")))
            .WithSectionProperties("<w:sectPr><w:headerReference w:type=\"default\" r:id=\"rId20\"/><w:headerReference w:type=\"first\" r:id=\"rId22\"/><w:footerReference w:type=\"default\" r:id=\"rId21\"/><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:titlePg/></w:sectPr>").Build());
        yield return ("fields", new DocxBuilder().WithBody(
            Paragraph(Field(" DATE \\@ \"dd/MM/yyyy\" ", "01/01/2020") + Field(" PAGE ", "1") + Run(" e ") + Field(" INCLUDETEXT \"C:\\\\Windows\\\\win.ini\" ", "resultado")),
            Paragraph("<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> IF </w:instrText></w:r>" + Field(" DATE ", "hoje") + "<w:r><w:instrText xml:space=\"preserve\"> = \"x\" \"sim\" </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>" + Run("pág. ") + Field(" PAGE ", "3") + "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>"),
            Paragraph("<w:fldSimple w:instr=\" AUTHOR \">" + Run("Fulano") + "</w:fldSimple><w:fldSimple w:instr=\" PAGE \">" + Run("3") + "</w:fldSimple><w:fldSimple w:instr=\"=2+2\">" + Run("4") + "</w:fldSimple>"),
            Paragraph(Run("Ver ") + Field(" HYPERLINK \"https://example.com/politica\" ", "política") + Run(" fim") + Field(" HYPERLINK \\l \"marca\" ", "interno") + Field(" HYPERLINK \"javascript:alert(1)\" ", "mau") + Field(" HYPERLINK \"file:///c:/x\" \\o \"dica\" ", "arquivo") + Field("HYPERLINK \"mailto:a@b.com\" \\l \"x\"", "mail")),
            Paragraph(Field(" REF marca \\h ", "ref") + Field(" TOC \\o \"1-3\" ", "sumário") + Field(" SEQ Figura ", "1") + Field(" = { NUMPAGES } - 1 ", "x") + Field(" NOTEREF n1 ", "1") + Field("SECTION", "2")),
            Paragraph("<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r><w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>" + Run("aberto") + "<w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r>")).Build());
        yield return ("hyperlinks", new DocxBuilder()
            .WithBody(Paragraph("<w:hyperlink r:id=\"rId30\">" + Run("ok") + "</w:hyperlink><w:hyperlink r:id=\"rId31\">" + Run("mau") + "</w:hyperlink><w:hyperlink w:anchor=\"x\">" + Run("âncora") + "</w:hyperlink>"))
            .WithDocumentRelationship("rId30", RelBase + "hyperlink", "https://example.com/a?b=1#c", external: true)
            .WithDocumentRelationship("rId31", RelBase + "hyperlink", "javascript:alert(1)", external: true)
            .WithDocumentRelationship("rId32", RelBase + "image", "http://example.com/img.png", external: true)
            .WithDocumentRelationship("rId33", RelBase + "attachedTemplate", "file:///c:/t.dotm", external: true).Build());
        yield return ("inlines", new DocxBuilder().WithBody(
            Paragraph(Run("Olá ") + TagInline("userDisplayName", "Fulano", "<w:b/>") + Run(" de ") + TagInline("bogus", "Depto") + Run(", veja ") + ItemLinkInline("document", 42, "Política", 3) + Run(" e ") + CustomInline("other.plugin", "{\"z\":true}", "Outro") + CustomInline("bad type!", "{}", "x") + "<w:sdt><w:sdtPr><w:tag w:val=\"wordnext:inline:t:%7Bbroken\"/></w:sdtPr><w:sdtContent>" + Run("quebrado") + "</w:sdtContent></w:sdt>"),
            Paragraph(Run("the theme of the day, The end. the")),
            "<w:sdt><w:sdtPr><w:tag w:val=\"wordnext:inline:block:%7B%7D\"/><w:id w:val=\"77\"/></w:sdtPr><w:sdtContent>" + Paragraph(Run("bloco")) + "</w:sdtContent></w:sdt>").Build());
        yield return ("customxml", SampleTemplate.Build().Length > 0 ? new DocxBuilder()
            .WithEntry("customXml/item1.xml", "<b:Sources xmlns:b=\"http://schemas.openxmlformats.org/officeDocument/2006/bibliography\"/>", "application/xml")
            .WithEntry("customXml/itemProps1.xml", "<ds:datastoreItem ds:itemID=\"{1}\" xmlns:ds=\"http://schemas.openxmlformats.org/officeDocument/2006/customXml\"/>", "application/vnd.openxmlformats-officedocument.customXmlProperties+xml")
            .WithPartRelationships("customXml/item1.xml", Rel("rId1", RelBase + "customXmlProps", "itemProps1.xml"))
            .WithDocumentRelationship("rId40", RelBase + "customXml", "../customXml/item1.xml").Build() : []);
        yield return ("unsafe", new DocxBuilder()
            .WithBody(
                Paragraph(Run("antes") + "<w:r><w:object w:dxaOrig=\"1\" w:dyaOrig=\"1\"><v:shape id=\"o1\" style=\"width:10pt;height:10pt\"><v:imagedata r:id=\"rId50\" o:title=\"\"/></v:shape><o:OLEObject Type=\"Embed\" ProgID=\"Package\" ShapeID=\"o1\" r:id=\"rId51\"/></w:object></w:r>" + Run("depois")),
                Paragraph("<w:r><w:pict><v:shape id=\"c1\" style=\"width:20pt;height:20pt\"><v:imagedata r:id=\"rId50\"/></v:shape><w:control r:id=\"rId52\" w:name=\"CheckBox1\"/></w:pict></w:r>" + Run("x")),
                "<w:altChunk r:id=\"rId53\"/>",
                Paragraph("<w:subDoc r:id=\"rId54\"/>" + Run("sub")),
                Paragraph(Picture("rId50", 914400, 914400, 3)))
            .WithImage("rId50", "image1.png", PngOfSize(10, 10))
            .WithEntry("word/embeddings/oleObject1.bin", new byte[] { 1, 2, 3 }, "application/vnd.openxmlformats-officedocument.oleObject")
            .WithEntry("word/activeX/activeX1.xml", "<ax:ocx xmlns:ax=\"http://schemas.microsoft.com/office/2006/activeX\" ax:classid=\"{X}\"/>", "application/vnd.ms-office.activeX+xml")
            .WithEntry("word/afchunk.htm", "<html><body>x</body></html>", "text/html")
            .WithDocumentRelationship("rId51", RelBase + "oleObject", "embeddings/oleObject1.bin")
            .WithDocumentRelationship("rId52", RelBase + "control", "activeX/activeX1.xml")
            .WithDocumentRelationship("rId53", RelBase + "aFChunk", "afchunk.htm")
            .WithDocumentRelationship("rId54", RelBase + "subDocument", "file:///c:/sub.docx", external: true)
            .WithDocumentRelationship("rId55", RelBase + "frame", "http://evil.example/", external: true).Build());
        yield return ("macros", new DocxBuilder()
            .WithMainContentType("application/vnd.ms-word.document.macroEnabled.main+xml")
            .WithEntry("word/vbaProject.bin", new byte[] { 0xD0, 0xCF, 0x11, 0xE0 }, "application/vnd.ms-office.vbaProject")
            .WithDocumentRelationship("rId60", "http://schemas.microsoft.com/office/2006/relationships/vbaProject", "vbaProject.bin")
            .WithSettings($"<w:settings xmlns:w=\"{WordNs}\" xmlns:r=\"{RelNs}\"><w:writeProtection w:recommended=\"1\"/><w:attachedTemplate r:id=\"rId1\"/><w:mailMerge><w:dataSource r:id=\"rId2\"/></w:mailMerge><w:trackRevisions/><w:documentProtection w:edit=\"forms\"/></w:settings>")
            .WithPartRelationships("word/settings.xml", Rel("rId1", RelBase + "attachedTemplate", "file:///C:/modelo.dotm", true), Rel("rId2", RelBase + "mailMergeSource", "file:///C:/dados.xlsx", true)).Build());
        yield return ("metadata", new DocxBuilder()
            .WithEntry("docProps/core.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:dcterms=\"http://purl.org/dc/terms/\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\"><dc:title>Título</dc:title><dc:creator>Fulano</dc:creator><cp:lastModifiedBy>Beltrano</cp:lastModifiedBy><dcterms:created xsi:type=\"dcterms:W3CDTF\">2021-03-04T05:06:07Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2022-12-31T23:59:59Z</dcterms:modified><cp:keywords>a, b</cp:keywords></cp:coreProperties>", "application/vnd.openxmlformats-package.core-properties+xml")
            .WithEntry("docProps/app.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><Application>Microsoft Office Word</Application><Company>ACME</Company><Manager>Chefe</Manager></Properties>", "application/vnd.openxmlformats-officedocument.extended-properties+xml")
            .WithEntry("docProps/custom.xml", "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?><Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/custom-properties\" xmlns:vt=\"http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes\"><property fmtid=\"{D5CDD505-2E9C-101B-9397-08002B2CF9AE}\" pid=\"2\" name=\"Cliente\"><vt:lpwstr>X</vt:lpwstr></property></Properties>", "application/vnd.openxmlformats-officedocument.custom-properties+xml")
            .WithPackageRelationship("rId2", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml")
            .WithPackageRelationship("rId3", RelBase + "extended-properties", "docProps/app.xml")
            .WithPackageRelationship("rId4", RelBase + "custom-properties", "docProps/custom.xml").Build());
        yield return ("pictures", new DocxBuilder()
            .WithBody(
                Paragraph(Picture("rId70", 9_000_000, 4_000_000, 1), "<w:ind w:left=\"720\"/>"),
                Paragraph("<w:r><w:pict><v:shape id=\"i1\" style=\"position:absolute;width:600pt;height:3in;margin-left:1cm\"><v:imagedata r:id=\"rId70\"/></v:shape></w:pict></w:r>"),
                Paragraph(Picture("rId70", 100_000, 100_000, 2)),
                Paragraph(Run("a imagem #{Logo} aqui e #{logo}")))
            .WithImage("rId70", "image1.png", PngOfSize(1200, 600))
            .WithSectionProperties("<w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1417\" w:right=\"1701\" w:bottom=\"1417\" w:left=\"1701\" w:gutter=\"0\"/><w:cols w:num=\"2\" w:space=\"708\"/></w:sectPr>").Build());
        yield return ("textboxes", new DocxBuilder()
            .WithBody(Paragraph(TextBox(Paragraph(Run("#{Logo} caixa a")) + Paragraph(Run("segunda linha a"))) + Run("fora")), Paragraph(TextBox(Paragraph(Run("outra a")), 7, null)))
            .WithFooter("rId80", Hdr("ftr", Paragraph(TextBox(Paragraph(Run("rodapé a")), 9))))
            .WithSectionProperties("<w:sectPr><w:footerReference w:type=\"default\" r:id=\"rId80\"/></w:sectPr>").Build());
        yield return ("tables", new DocxBuilder().WithBody(
            Rows([Paragraph(Run("Versão"))], [Paragraph(Run("#{Version} the"))], [Paragraph(Run("#{Date}"))]),
            Table(Paragraph(Run("#{A} the a")), Paragraph(Run("#{B}"))),
            "<w:tbl><w:tr></w:tr></w:tbl><w:tbl><w:tblPr/></w:tbl>",
            Table("<w:tbl><w:tr><w:tc><w:p/></w:tc></w:tr></w:tbl>")).Build());
        yield return ("styles", new DocxBuilder()
            .WithStyles($"<w:styles xmlns:w=\"{WordNs}\"><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:pPr><w:numPr><w:numId w:val=\"3\"/></w:numPr><w:ind w:left=\"400\"/></w:pPr></w:style><w:style w:type=\"character\" w:default=\"1\" w:styleId=\"DefaultParagraphFont\"><w:name w:val=\"Default Paragraph Font\"/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Heading1\"><w:name w:val=\"heading 1\"/><w:basedOn w:val=\"Normal\"/><w:link w:val=\"Heading1Char\"/><w:rPr><w:rFonts w:ascii=\"Caladea\"/></w:rPr></w:style><w:style w:type=\"character\" w:styleId=\"Heading1Char\"><w:name w:val=\"Heading 1 Char\"/><w:link w:val=\"Heading1\"/></w:style><w:style w:type=\"paragraph\" w:styleId=\"Cyc\"><w:name w:val=\"Cyc\"/><w:basedOn w:val=\"Cyc\"/></w:style><w:style w:type=\"character\" w:styleId=\"Hyperlink\"><w:name w:val=\"Hyperlink\"/></w:style></w:styles>")
            .WithNumbering($"<w:numbering xmlns:w=\"{WordNs}\"><w:numPicBullet w:numPicBulletId=\"0\"><w:pict/></w:numPicBullet><w:abstractNum w:abstractNumId=\"4\"><w:nsid w:val=\"ABCDEF01\"/><w:lvl w:ilvl=\"0\"><w:start w:val=\"3\"/><w:lvlPicBulletId w:val=\"0\"/><w:pPr><w:ind w:start=\"1080\"/></w:pPr></w:lvl></w:abstractNum><w:num w:numId=\"3\"><w:abstractNumId w:val=\"4\"/><w:lvlOverride w:ilvl=\"0\"><w:lvl w:ilvl=\"0\"><w:pPr><w:ind w:left=\"1500\"/></w:pPr></w:lvl></w:lvlOverride></w:num><w:numIdMacAtCleanup w:val=\"1\"/></w:numbering>")
            .WithFontTable($"<w:fonts xmlns:w=\"{WordNs}\" xmlns:r=\"{RelNs}\"><w:font w:name=\"Caladea\"><w:embedRegular r:id=\"rId1\" w:fontKey=\"{{00000000-0000-0000-0000-000000000000}}\"/></w:font><w:font w:name=\"Other\"/></w:fonts>", ("rId1", "font1.odttf", new byte[64]))
            .WithSettings($"<w:settings xmlns:w=\"{WordNs}\"><w:zoom w:percent=\"100\"/><w:proofState w:spelling=\"clean\"/></w:settings>")
            .WithBody(Paragraph(Run("estilo #{Heading1}", "<w:rStyle w:val=\"Heading1Char\"/><w:rFonts w:ascii=\"Caladea\" w:hAnsi=\"Other\"/>"), "<w:pStyle w:val=\"Heading1\"/>"), Paragraph(Run("lista #{BulletList}"), "<w:pStyle w:val=\"Cyc\"/><w:numPr><w:ilvl w:val=\"0\"/><w:numId w:val=\"3\"/></w:numPr>"), Paragraph(Run("sem definição"), "<w:pStyle w:val=\"Heading2\"/>"), Paragraph(Run("toc"), "<w:pStyle w:val=\"TOC1\"/>")).Build());
        yield return ("notes", new DocxBuilder()
            .WithBody(Paragraph(Run("texto") + "<w:r><w:footnoteReference w:id=\"1\"/></w:r>" + "<w:bookmarkStart w:id=\"0\" w:name=\"marca\"/>" + Run("marcado") + "<w:bookmarkEnd w:id=\"0\"/>" + Field(" REF marca ", "marcado") + "<w:hyperlink w:anchor=\"marca\">" + Run("ir") + "</w:hyperlink>"))
            .WithFootnotes($"<w:footnotes xmlns:w=\"{WordNs}\"><w:footnote w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote><w:footnote w:id=\"1\"><w:p><w:r><w:t>nota um</w:t></w:r></w:p></w:footnote><w:footnote w:id=\"2\"><w:p><w:r><w:t>órfã</w:t></w:r></w:p></w:footnote></w:footnotes>").Build());
        yield return ("odd-xml", new DocxBuilder().WithBody(
            "<w:p xmlns:x=\"urn:x\"><w:r><w:t>a</w:t></w:r><x:vazio/><x:aberto></x:aberto><!--comentário--><?pi dados?><x:c><![CDATA[<cdata>]]]]><![CDATA[>]]></x:c><w:r><w:t xml:space=\"preserve\">  dois  espaços\ttab&#xD;cr&#xA;lf &amp; &lt;&gt; &quot;'</w:t></w:r></w:p>",
            "<w:p><w:r w:rsidR=\"00A\" xmlns:q=\"urn:q\" q:attr=\"v&#9;t&#10;n&#13;r&lt;&gt;&amp;&quot;'\"><w:t>x</w:t></w:r></w:p>",
            "<p xmlns=\"urn:default\"><w:r><w:t>default ns</w:t></w:r><inner xmlns=\"\"><w:p/></inner></p>",
            "<w:p><w:r><w:t>emoji 😀 e ﬀ e \u00A0nbsp\u2028</w:t></w:r></w:p>").Build());
        yield return ("deep-256", new DocxBuilder().WithBody($"<w:p xmlns:x=\"urn:x\">{Nested(254)}</w:p>").WithSectionProperties(null).Build());
        yield return ("deep-257", new DocxBuilder().WithBody($"<w:p xmlns:x=\"urn:x\">{Nested(255)}</w:p>").WithSectionProperties(null).Build());
        yield return ("many-fields", new DocxBuilder().WithBody(Paragraph(string.Concat(Enumerable.Range(0, 300).Select(i => Field(" AUTHOR ", "autor") + Field($" HYPERLINK \"https://example.com/{i}\" ", "link"))))).Build());
        yield return ("open-fields", new DocxBuilder().WithBody(Paragraph(Run("a") + string.Concat(Enumerable.Repeat("<w:r><w:fldChar w:fldCharType=\"begin\"/></w:r>", 70)) + Run("x"))).Build());
        yield return ("dtd", Encoding.UTF8.GetBytes("not a zip"));
        yield return ("no-main", ReplaceEntry(new DocxBuilder().Build(), "_rels/.rels", "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>"));
        yield return ("doctype", ReplaceEntry(new DocxBuilder().Build(), "word/document.xml", $"<?xml version=\"1.0\"?><!DOCTYPE x [<!ENTITY e \"x\">]><w:document xmlns:w=\"{WordNs}\"><w:body><w:p/></w:body></w:document>"));
        yield return ("not-word", ReplaceEntry(new DocxBuilder().Build(), "word/document.xml", "<?xml version=\"1.0\"?><x:document xmlns:x=\"urn:x\"><x:body/></x:document>"));
        yield return ("bad-xml", ReplaceEntry(new DocxBuilder().Build(), "word/document.xml", $"<w:document xmlns:w=\"{WordNs}\"><w:body><w:p></w:body></w:document>"));
        yield return ("empty-body", new DocxBuilder().WithBody("").WithSectionProperties(null).Build());
        yield return ("double-escape", new DocxBuilder()
            .WithBody(Paragraph(Run("texto") + "<w:r><w:footnoteReference w:id=\"1\"/></w:r>"))
            .WithEntry("word/fn1.xml", $"<?xml version=\"1.0\"?><w:footnotes xmlns:w=\"{WordNs}\"><w:footnote w:id=\"1\"><w:p><w:r><w:t>nota</w:t></w:r></w:p></w:footnote></w:footnotes>", "application/vnd.openxmlformats-officedocument.obfuscatedFont")
            .WithEntry("word/fn%31.xml", $"<?xml version=\"1.0\"?><w:footnotes xmlns:w=\"{WordNs}\"><w:footnote w:id=\"1\"><w:p><w:r><w:t>escapada</w:t></w:r></w:p></w:footnote></w:footnotes>", "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml")
            .WithDocumentRelationship("rId7", RelBase + "footnotes", "fn%2531.xml").Build());
        yield return ("case-names", ReplaceEntry(ReplaceEntry(new DocxBuilder().WithBody(Paragraph(Run("caixa"))).Build(), "_rels/.rels", "<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"/WORD/Document.XML\"/></Relationships>"), "[Content_Types].xml", "<?xml version=\"1.0\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"XML\" ContentType=\"application/xml\"/><Override PartName=\"/word/DOCUMENT.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>"));
        yield return ("ns-noncanonical", new DocxBuilder().WithBody(
            "<w:p><w:r><w:t>x</w:t></w:r></w:p>").Build() is var b ? ReplaceEntry(b, "word/document.xml", $"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"no\"?>\r\n<!-- antes -->\r\n<ww:document xmlns:ww=\"{WordNs}\" xmlns:rr=\"{RelNs}\"><ww:body><ww:p><ww:r><ww:t>prefixos</ww:t></ww:r></ww:p><ww:sectPr/></ww:body></ww:document>\r\n<?fim x?>\r\n") : []);
        yield return ("utf16", ReplaceEntryBytes(new DocxBuilder().Build(), "word/document.xml", Encoding.Unicode.GetPreamble().Concat(Encoding.Unicode.GetBytes($"<?xml version=\"1.0\" encoding=\"UTF-16\"?><w:document xmlns:w=\"{WordNs}\"><w:body><w:p><w:r><w:t>dezesseis</w:t></w:r></w:p></w:body></w:document>")).ToArray()));
        // the security review's probes (record "Security review of the sanitizer"): a hyperlink
        // relationship WITHOUT TargetMode="External" to javascript:, a VML href, a package-level
        // relationship to javascript:, a glossary part with an OLE object
        yield return ("sanitizer-gaps", new DocxBuilder()
            .WithBody(
                Paragraph("<w:hyperlink r:id=\"rId90\">" + Run("internal js") + "</w:hyperlink>"),
                Paragraph("<w:r><w:pict><v:shape id=\"s1\" href=\"javascript:alert(1)\" style=\"width:10pt;height:10pt\"/></w:pict></w:r>"))
            .WithDocumentRelationship("rId90", RelBase + "hyperlink", "javascript:alert(1)")
            .WithPackageRelationship("rId91", RelBase + "hyperlink", "javascript:alert(2)")
            .WithEntry("word/glossary/document.xml", $"<?xml version=\"1.0\"?><w:glossaryDocument xmlns:w=\"{WordNs}\"><w:docParts><w:docPart><w:docPartBody><w:p><w:r><w:object/></w:r><w:fldSimple w:instr=\" DDEAUTO x y \"><w:r><w:t>dde</w:t></w:r></w:fldSimple></w:p></w:docPartBody></w:docPart></w:docParts></w:glossaryDocument>", "application/vnd.openxmlformats-officedocument.wordprocessingml.document.glossary+xml")
            .WithDocumentRelationship("rId92", RelBase + "glossaryDocument", "glossary/document.xml").Build());
        yield return ("bom", ReplaceEntryBytes(new DocxBuilder().Build(), "word/document.xml", new byte[] { 0xEF, 0xBB, 0xBF }.Concat(Encoding.UTF8.GetBytes($"<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{WordNs}\"><w:body><w:p><w:r><w:t>bom</w:t></w:r></w:p></w:body></w:document>")).ToArray()));
    }

    public static void Write(string dir)
    {
        Directory.CreateDirectory(dir);
        foreach (var (name, data) in All()) File.WriteAllBytes(Path.Combine(dir, name + ".docx"), data);
    }

    private static byte[] ReplaceEntry(byte[] docx, string name, string text) => ReplaceEntryBytes(docx, name, Encoding.UTF8.GetBytes(text));

    private static byte[] ReplaceEntryBytes(byte[] docx, string name, byte[] data)
    {
        var entries = new List<(string, byte[])>();
        using (var zip = new System.IO.Compression.ZipArchive(new MemoryStream(docx), System.IO.Compression.ZipArchiveMode.Read))
        {
            foreach (var e in zip.Entries)
            {
                using var ms = new MemoryStream();
                using (var s = e.Open()) s.CopyTo(ms);
                entries.Add((e.FullName, e.FullName == name ? data : ms.ToArray()));
            }
        }
        using var outStream = new MemoryStream();
        using (var zip = new System.IO.Compression.ZipArchive(outStream, System.IO.Compression.ZipArchiveMode.Create, true))
        {
            foreach (var (n, d) in entries)
            {
                using var s = zip.CreateEntry(n).Open();
                s.Write(d);
            }
        }
        return outStream.ToArray();
    }
}
