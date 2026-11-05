using System.Globalization;
using System.IO.Compression;
using System.Text;
using UniversalOffice.Core;
using UniversalOffice.Docx;
using UniversalOffice.Docx.Editing;
using static UoD3Harness.CaptureWriter;

namespace UoD3Harness;

/// <summary>
/// The scripted operation sequences of the D3 parity capture. Each script opens one corpus
/// document (and, for the import scripts, the next one in the list as content), runs public
/// API calls and records: `ret` (the API results joined by `|`, see <see cref="CaptureWriter.Ret"/>),
/// `pkg` (the digest of the saved package: every part's name and decompressed bytes in zip
/// order, <see cref="Pkg"/>) and `err` (the exception class, `OfficeLimitExceededException`
/// with its limit, when the script stopped). The Rust test (`tests/parity_d3.rs`) runs the same
/// sequences; they are kept in the same order there.
/// </summary>
internal static class Scripts
{
    public static readonly string[] Names =
    [
        "open", "content", "output", "valid", "empty", "apply", "applysec", "import", "text", "fields", "unsafe",
        "props", "writer", "shapes", "inlines", "parts", "rowtpl", "samples",
    ];

    /// <summary>The script's verdict of a plugin inline (deterministic; the Rust test has the same).</summary>
    public static bool Verdict(string type, string json) => json.Length % 3 != 0 && !type.Contains("bogus", StringComparison.Ordinal);

    public static void RunCorpus(string outPath, string root, string listFile, string? dump)
    {
        var files = File.ReadAllLines(listFile).Where(l => l.Length > 0 && !l.StartsWith('#')).ToList();
        using var w = new CaptureWriter(outPath, "uo-docx D3 Editing API scripts over the corpus (dotnet-harness corpus)");
        w.Meta("scripts", string.Join(",", Names));
        w.Meta("files", files.Count.ToString(CultureInfo.InvariantCulture));
        for (int i = 0; i < files.Count; i++)
        {
            byte[] bytes = File.ReadAllBytes(Path.Combine(root, files[i]));
            byte[] next = File.ReadAllBytes(Path.Combine(root, files[(i + 1) % files.Count]));
            foreach (var script in Names)
            {
                string key = files[i] + "|" + script;
                var log = new List<string>();
                string? err = null;
                byte[]? saved = null;
                try
                {
                    saved = Run(script, bytes, next, log);
                }
                catch (Exception e)
                {
                    err = ErrorClass(e);
                }
                var facets = new List<(string, string)> { ("ret", Ret(string.Join("|", log))) };
                if (saved is not null)
                {
                    bool mask = script is "apply" or "applysec" or "import";
                    facets.Add(("pkg", Pkg(saved, mask, dump is null ? null : Path.Combine(dump, Sanitize(key)))));
                }
                if (err is not null) facets.Add(("err", err));
                w.Record(key, facets.ToArray());
            }
            if (i % 200 == 0) Console.WriteLine($"{i}/{files.Count}");
        }
    }

    public static string ErrorClass(Exception e) =>
        e is OfficeLimitExceededException l ? "OfficeLimitExceededException:" + l.Limit : e.GetType().Name;

    private static string Sanitize(string key) => new(key.Select(c => char.IsLetterOrDigit(c) || c is '-' or '.' ? c : '_').ToArray());

    /// <summary>
    /// The digest of a saved package: for each part in zip order, its name (UTF-8), a zero
    /// byte, the length in decimal, a zero byte and the bytes, masked (import scripts, see
    /// <see cref="Mask"/>) when asked.
    /// </summary>
    public static string Pkg(byte[] docx, bool mask, string? dumpDir)
    {
        using var zip = new ZipArchive(new MemoryStream(docx), ZipArchiveMode.Read);
        using var sha = System.Security.Cryptography.IncrementalHash.CreateHash(System.Security.Cryptography.HashAlgorithmName.SHA256);
        foreach (var e in zip.Entries)
        {
            using var ms = new MemoryStream();
            using (var s = e.Open()) s.CopyTo(ms);
            byte[] data = ms.ToArray();
            if (mask) data = Mask(data);
            if (dumpDir is not null)
            {
                string p = Path.Combine(dumpDir, e.FullName.Replace('/', Path.DirectorySeparatorChar));
                Directory.CreateDirectory(Path.GetDirectoryName(p)!);
                File.WriteAllBytes(p, data);
            }
            sha.AppendData(Encoding.UTF8.GetBytes(e.FullName));
            sha.AppendData([0]);
            sha.AppendData(Encoding.ASCII.GetBytes(data.Length.ToString(CultureInfo.InvariantCulture)));
            sha.AppendData([0]);
            sha.AppendData(data);
        }
        return Convert.ToHexStringLower(sha.GetHashAndReset()[..16]);
    }

    /// <summary>
    /// The import scripts draw random ids (RandomNumberGenerator: w14:paraId and w:sdt ids on a
    /// collision, every new w:nsid): the value after each of `paraId="`, `nsid w:val="` and
    /// `<w:id w:val="` up to the next `"` becomes `#`, on both sides.
    /// </summary>
    public static byte[] Mask(byte[] data)
    {
        byte[][] keys = [Encoding.ASCII.GetBytes("paraId=\""), Encoding.ASCII.GetBytes("nsid w:val=\""), Encoding.ASCII.GetBytes("<w:id w:val=\"")];
        var o = new List<byte>(data.Length);
        int i = 0;
        while (i < data.Length)
        {
            byte[]? hit = null;
            foreach (var k in keys)
                if (i + k.Length <= data.Length && data.AsSpan(i, k.Length).SequenceEqual(k)) { hit = k; break; }
            if (hit is null) { o.Add(data[i++]); continue; }
            o.AddRange(hit);
            i += hit.Length;
            int end = Array.IndexOf(data, (byte)'"', i);
            if (end < 0) end = data.Length;
            o.Add((byte)'#');
            i = end;
        }
        return o.ToArray();
    }

    private static string B(bool v) => v ? "1" : "0";
    private static string N(int v) => v.ToString(CultureInfo.InvariantCulture);
    private static string S(string? v) => v ?? "<null>";

    private static readonly byte[] Png = UniversalOffice.Tests.Editing.DocxBuilder.PngOfSize(2000, 1000);

    private static byte[] Run(string script, byte[] bytes, byte[] next, List<string> log)
    {
        var doc = DocxDocument.Open(bytes);
        switch (script)
        {
            case "open":
                log.Add(doc.MainPartPath);
                log.Add(N(doc.PartNames.Count));
                break;
            case "content":
                DocxSanitizer.Sanitize(doc, DocxSanitizeOptions.Content);
                break;
            case "output":
                DocxSanitizer.Sanitize(doc, DocxSanitizeOptions.Output);
                break;
            case "valid":
                DocxSanitizer.Sanitize(doc, DocxSanitizeOptions.Content with
                {
                    IsCustomInlineValid = (t, j) =>
                    {
                        log.Add(t + "\u0001" + j);
                        return Verdict(t, j);
                    },
                });
                break;
            case "empty":
                DocxTemplate.Empty(doc);
                break;
            case "apply":
                DocxTemplate.ApplyContent(doc, next, new DocxApplyContentOptions { SampleMarkers = ["#{Heading1}", "#{Normal}"] });
                break;
            case "applysec":
                DocxTemplate.ApplyContent(doc, next, new DocxApplyContentOptions { PreserveSections = true, FitPictures = true });
                break;
            case "import":
                DocxTemplate.ImportBody(doc, DocxDocument.Open(next));
                break;
            case "text":
                log.Add(Digest(DocxText.GetPlainText(doc, includeHeadersAndFooters: true)));
                log.Add(Digest(string.Join("\u0001", DocxFields.GetInstructions(doc).Select(f => f.Name + "\u0002" + f.Text))));
                log.Add(B(DocxText.Contains(doc, "the")));
                log.Add(N(DocxText.Replace(doc, "the", "THE·X", caseSensitive: false, wholeWord: true)));
                log.Add(N(DocxText.ReplaceParameters(doc, [new("Title", "T\tx"), new("Missing", null), new("Version", "1\r\n2")])));
                log.Add(N(DocxText.Replace(doc, "a", "b\nc", caseSensitive: true, wholeWord: false, scope: DocxTextScope.Body)));
                log.Add(N(DocxText.RemoveParagraphsContaining(doc, ["zz", "#{Normal}", "Lorem"])));
                break;
            case "fields":
                log.Add(N(DocxFields.ReplaceDisallowedWithResult(doc, ["PAGE", "REF", "HYPERLINK"], ["https"])));
                log.Add(N(DocxFields.ConvertHyperlinkFields(doc)));
                log.Add(N(DocxFields.GetInstructions(doc).Count));
                break;
            case "unsafe":
                DocxSanitizer.RemoveUnsafeContent(doc, ["http", "https", "mailto", "file"]);
                DocxSanitizer.RemoveCustomXml(doc);
                DocxSanitizer.MergeSections(doc);
                DocxSanitizer.RemoveHeadersAndFooters(doc);
                DocxReview.DisableTrackChanges(doc);
                DocxReview.RemoveProtection(doc);
                break;
            case "props":
            {
                var p = doc.Properties;
                log.Add(S(p.Title)); log.Add(S(p.Subject)); log.Add(S(p.Author)); log.Add(S(p.Keywords)); log.Add(S(p.Comments));
                log.Add(S(p.LastAuthor)); log.Add(S(p.Category)); log.Add(S(p.Company)); log.Add(S(p.Manager)); log.Add(S(p.ApplicationName));
                log.Add(p.LastSaveDate?.ToString("o", CultureInfo.InvariantCulture) ?? "<null>");
                log.Add(p.CreatedDate?.ToString("o", CultureInfo.InvariantCulture) ?? "<null>");
                log.Add(string.Join(",", p.CustomPropertyNames));
                p.Title = "Título\u0001 x";
                p.Author = null;
                p.Company = "ACME";
                p.Subject = "";
                p.SetCustomProperty("k", "v");
                p.SetCustomProperty("k", "w");
                p.SetCustomProperty("z", "");
                p.LastSaveDate = new DateTime(2024, 5, 6, 7, 8, 9, DateTimeKind.Utc);
                p.CreatedDate = null;
                log.Add(S(p.Title)); log.Add(S(p.Company)); log.Add(string.Join(",", p.CustomPropertyNames));
                log.Add(p.LastSaveDate?.ToString("o", CultureInfo.InvariantCulture) ?? "<null>");
                byte[] mid = doc.Save();
                log.Add(Pkg(mid, false, null));
                p.RemovePersonalMetadata();
                log.Add(string.Join(",", p.CustomPropertyNames));
                break;
            }
            case "writer":
            {
                var wr = new DocxBodyWriter(doc, new DocxBodyWriterOptions { LinkColor = bytes.Length % 2 == 0 ? null : "#0563c1" });
                log.Add(B(wr.LineIsEmpty));
                wr.AddText("Hello\tworld\nnext\r\nline\u000Bv\u0001x", new DocxRunFormat { Bold = true, Color = "red", Background = "#ff0" });
                wr.AddText("link", new DocxRunFormat { Link = "www.example.com", Underline = true });
                wr.AddText("bad", new DocxRunFormat { Link = "javascript:alert(1)", Italic = true, Strike = true, VerticalAlign = DocxVerticalAlign.Subscript });
                wr.AddText("", DocxRunFormat.None);
                wr.AddBreak();
                log.Add(B(wr.AddImage(Png)));
                log.Add(B(wr.AddImage([1, 2, 3])));
                log.Add(B(wr.LineIsEmpty));
                wr.EndParagraph(new DocxParagraphFormat { StyleId = "Heading1", Alignment = "center", ListNumId = "1", ListLevel = 12, LeftIndentTwips = 360, KeepWithNext = true });
                wr.AddText("sup", new DocxRunFormat { VerticalAlign = DocxVerticalAlign.Superscript, Link = " https://example.com/x ", Color = "rgb(1, 2, 300)" });
                wr.EndParagraph(DocxParagraphFormat.None);
                try { wr.EndParagraph(new DocxParagraphFormat { Alignment = "middle" }); }
                catch (ArgumentException) { log.Add("bad-align"); }
                wr.AddText("  ", DocxRunFormat.None);
                wr.Finish();
                break;
            }
            case "shapes":
            {
                log.Add(N(DocxPictures.FitToPage(doc)));
                var shapes = DocxShapes.FindContaining(doc, "a");
                log.Add(N(shapes.Count));
                if (shapes.Count > 0)
                {
                    log.Add(Digest(shapes[0].Text));
                    shapes[0].SetFillColor("#FF0000");
                    shapes[0].SetTextColor("00ff00", lastParagraphOnly: false);
                }
                if (shapes.Count > 1)
                {
                    shapes[1].SetFillTransparent();
                    shapes[1].ClearText();
                    shapes[1].SetTextColor("blue");
                }
                try { shapes.FirstOrDefault()?.SetFillColor("nope"); }
                catch (ArgumentException) { log.Add("bad-color"); }
                log.Add(N(DocxShapes.ReplaceTextWithImage(doc, "the", Png, 100f, 50.5f)));
                var footerShape = DocxShapes.FirstShapeIn(doc, footer: true);
                log.Add(footerShape is null ? "<null>" : Digest(footerShape.Text));
                footerShape?.Remove();
                log.Add(B(DocxShapes.ClearHeaderFooter(doc, footer: false)));
                log.Add(B(DocxShapes.ClearHeaderFooter(doc, footer: true, DocxHeaderFooterType.First)));
                log.Add(N(DocxShapes.RemoveContaining(doc, "zzz")));
                log.Add(S(DocxNumbering.AddRestartedInstance(doc, "1")));
                break;
            }
            case "inlines":
            {
                log.Add(N(DocxCustomInlines.ReplaceText(doc, "the", "x.y", "{\"a\":[1,2]}", "F\u0007b")));
                var list = DocxCustomInlines.Find(doc);
                foreach (var c in list) log.Add(c.Type + "\u0001" + c.Json + "\u0001" + c.PartPath + "\u0001" + S(c.Title) + "\u0001" + c.Text + "\u0001" + B(c.IsAttached));
                for (int k = 0; k < list.Count; k++)
                {
                    try
                    {
                        switch (k % 4)
                        {
                            case 0: log.Add(B(list[k].ReplaceWithHyperlink("https://e.com/x", "L"))); break;
                            case 1: list[k].ReplaceWithText(k % 8 == 1 ? "" : "T"); break;
                            case 2: list[k].Unwrap(); break;
                            default: log.Add(B(list[k].ReplaceWithHyperlink("ftp://e.com", "F", applyHyperlinkStyle: false))); break;
                        }
                    }
                    catch (InvalidOperationException) { log.Add("detached"); }
                }
                if (list.Count > 0)
                {
                    try { list[0].Unwrap(); } catch (InvalidOperationException) { log.Add("again"); }
                }
                log.Add(N(DocxCustomInlines.UnwrapInvalid(doc, Verdict)));
                break;
            }
            case "parts":
            {
                DocxReview.RemoveComments(doc);
                DocxReview.AcceptAllRevisions(doc);
                foreach (var part in doc.PartNames.ToList())
                {
                    var b = doc.GetPartBytes(part);
                    log.Add(part + ":" + (b is null ? "-" : Digest(b)) + ":" + S(doc.GetContentType(part)));
                }
                log.Add(B(doc.HasPart("WORD/DOCUMENT.XML")) + B(doc.HasPart("nope")));
                foreach (var r in doc.GetRelationships(doc.MainPartPath)) log.Add(r.Id + "\u0001" + r.Type + "\u0001" + r.Target + "\u0001" + B(r.External));
                log.Add(S(doc.GetRelationship("", "rId1")?.Target));
                doc = doc.Clone();
                break;
            }
            case "rowtpl":
            {
                var t = DocxTableRowTemplate.Take(doc, "the") ?? DocxTableRowTemplate.Take(doc, "#{Version}");
                log.Add(t is null ? "<null>" : "row");
                if (t is not null)
                {
                    t.Append([new("Version", "1.0"), new("y", null), new("the", "x")]);
                    t.Append([], removeRemainingParameters: false);
                    log.Add(N(t.AppendedRows));
                }
                break;
            }
            case "samples":
            {
                log.Add(B(DocxParagraphSamples.Contains(doc, "Heading1")));
                var m = DocxParagraphSamples.Extract(doc, ["Heading1", "Normal", "BulletList", "Heading1", "Logo"]);
                foreach (var (k, v) in m.OrderBy(x => x.Key, StringComparer.Ordinal)) log.Add(k + "\u0001" + S(v.StyleId) + "\u0001" + S(v.NumId));
                var wr = new DocxBodyWriter(doc, new DocxBodyWriterOptions { ReuseEmptyLastParagraph = false, TrimTrailingEmptyParagraph = false, FitPictures = false });
                foreach (var (_, v) in m.OrderBy(x => x.Key, StringComparer.Ordinal))
                {
                    wr.AddText("from sample", DocxRunFormat.None);
                    wr.EndParagraph(new DocxParagraphFormat { Base = v, KeepWithNext = true });
                }
                wr.Finish();
                break;
            }
            default:
                throw new ArgumentException(script);
        }
        return doc.Save();
    }
}
