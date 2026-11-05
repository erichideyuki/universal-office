using System.Globalization;
using System.Text;
using System.Xml.Linq;
using UniversalOffice.Docx;
using UniversalOffice.Docx.Editing;
using static UoD3Harness.CaptureWriter;

namespace UoD3Harness;

/// <summary>
/// Unit-level differential of the .NET semantics the Editing API depends on (record table
/// U13): each site is called on generated inputs and the result is captured with the input,
/// so the Rust test replays the same inputs from the capture.
/// </summary>
internal static class Semantics
{
    private static readonly string[] DefaultSchemes = ["http", "https", "mailto"];
    private static readonly string[] WideSchemes = ["http", "https", "mailto", "ftp", "tel", "news", "file", "ldap"];

    public static void Run(string outPath, string tagInputs)
    {
        using var w = new CaptureWriter(outPath, "uo-docx D3 .NET-semantics differential (dotnet-harness semantics)");
        int n = 0;
        void Rec(string site, string input, string output) => w.Record(site + ":" + (n++).ToString(CultureInfo.InvariantCulture), ("in", Esc(input)), ("out", Esc(output)));
        void RecT(string site, string input, Func<string> output)
        {
            string o;
            try { o = output(); }
            catch (Exception e) { o = "!" + e.GetType().Name; }
            Rec(site, input, o);
        }

        foreach (var u in Urls())
        {
            bool a = DocxFields.IsAllowedUrl(u, DefaultSchemes);
            bool b = DocxFields.IsAllowedUrl(u, WideSchemes);
            Rec("uri", u, (a ? "1" : "0") + (b ? "1" : "0"));
        }
        foreach (var line in File.ReadAllLines(tagInputs))
        {
            string tag = Unescape(line);
            Rec("tag", tag, CustomInlineTag.TryParse(tag, out var t, out var j) ? t + "\u0001" + j : "-");
        }
        foreach (var (type, json) in TagFormats())
        {
            string o;
            try { o = CustomInlineTag.Format(type, json); }
            catch (Exception e) { o = "!" + e.GetType().Name; }
            Rec("tagfmt", type + "\u0001" + json, o);
        }
        foreach (var s in Strings())
        {
            RecT("unescape", s, () => Uri.UnescapeDataString(s));
            string esc;
            try { esc = Uri.EscapeDataString(s); } catch (Exception e) { esc = "!" + e.GetType().Name; }
            Rec("escape", s, esc);
            RecT("color", s, () => (string?)Frozen.Call("UniversalOffice.Docx.Editing.Colors", "NormalizeHex", s) ?? "-");
            RecT("firstword", s, () => (string)Frozen.Call("UniversalOffice.Docx.Editing.DocxFields", "FirstWord", s)!);
            RecT("removeinvalid", s, () => (string)Frozen.Call("UniversalOffice.Docx.Editing.XmlText", "RemoveInvalidChars", s)!);
            RecT("builtin", s, () => (string?)Frozen.Call("UniversalOffice.Docx.Editing.DocxStyles", "BuiltInName", s) ?? "-");
            RecT("hyperlink", s, () => DocxFields.TryParseHyperlink(s, DefaultSchemes, out var url, out var anchor) ? "1" + (url ?? "-") + "\u0001" + (anchor ?? "-") : "0");
            RecT("ints", s, () => Ints(s));
            RecT("trim", s, () => s.Trim() + "\u0001" + s.TrimStart() + "\u0001" + s.ToUpperInvariant());
            RecT("date", s, () => Date(s));
            RecT("vml", s, () => Vml(s));
        }
        foreach (var (text, start, len) in WholeWords())
            RecT("wholeword", text + "\u0001" + start + "\u0001" + len, () => (bool)Frozen.Call("UniversalOffice.Docx.Editing.ParagraphText", "IsWholeWord", text, start, len)! ? "1" : "0");
        foreach (var (a, b) in Pairs())
        {
            RecT("relative", a + "\u0001" + b, () => (string)Frozen.Call("UniversalOffice.Docx.Editing.DocxDocument", "RelativeTarget", a, b)!);
            Rec("ignorecase", a + "\u0001" + b, (string.Equals(a, b, StringComparison.OrdinalIgnoreCase) ? "1" : "0") + a.IndexOf(b, StringComparison.OrdinalIgnoreCase).ToString(CultureInfo.InvariantCulture) + "," + a.IndexOf(b, StringComparison.Ordinal).ToString(CultureInfo.InvariantCulture));
        }
        foreach (var ids in RelIdSets())
            RecT("nextrelid", string.Join("\u0001", ids), () => (string)Frozen.Call("UniversalOffice.Docx.Editing.DocxDocument", "NextRelId", (IEnumerable<string>)ids)!);
        foreach (var bytes in Images())
        {
            RecT("image", Convert.ToHexString(bytes), () => ImageInfo.TryRead(bytes) is not { } info ? "-" : $"{info.Extension}|{info.ContentType}|{info.WidthPx}|{info.HeightPx}|{R(info.DpiX)}|{R(info.DpiY)}|{info.WidthEmu}|{info.HeightEmu}");
        }
        foreach (var (cx, cy, mx, my) in Fits())
        {
            var (a, b) = DocxPictures.Fit(cx, cy, mx, my);
            Rec("fit", $"{cx}|{cy}|{mx?.ToString(CultureInfo.InvariantCulture) ?? "-"}|{my?.ToString(CultureInfo.InvariantCulture) ?? "-"}", $"{a}|{b}");
        }
        foreach (var d in Doubles())
            Rec("fmt", R(d), d.ToString("0.##", CultureInfo.InvariantCulture));
        foreach (var desired in Desired())
            Rec("unique", desired, Unique(desired));
        w.Meta("count", n.ToString(CultureInfo.InvariantCulture));
        Console.WriteLine($"semantics: {n} records");
    }

    /// <summary>A double as the exact round-trip text ("R").</summary>
    private static string R(double d) => d.ToString("R", CultureInfo.InvariantCulture);

    private static string Ints(string s)
    {
        string a = int.TryParse(s, out var x) ? x.ToString(CultureInfo.InvariantCulture) : "-";
        string b = int.TryParse(s, NumberStyles.Integer, CultureInfo.InvariantCulture, out var y) ? y.ToString(CultureInfo.InvariantCulture) : "-";
        string c = int.TryParse(s.AsSpan(), NumberStyles.None, CultureInfo.InvariantCulture, out var z) ? z.ToString(CultureInfo.InvariantCulture) : "-";
        string d = long.TryParse(s, out var l) ? l.ToString(CultureInfo.InvariantCulture) : "-";
        return a + "|" + b + "|" + c + "|" + d;
    }

    private static string Date(string s)
    {
        object? r = Frozen.Call("UniversalOffice.Docx.Editing.DocxProperties", "ParseDate", s);
        return r is DateTime d ? d.ToString("yyyy-MM-dd'T'HH:mm:ss.fffffff", CultureInfo.InvariantCulture) + "|" + d.Kind : "-";
    }

    private static string Vml(string style)
    {
        var shape = new XElement(XName.Get("shape", "urn:schemas-microsoft-com:vml"), new XAttribute("style", style));
        try
        {
            bool r = (bool)Frozen.Call("UniversalOffice.Docx.Editing.DocxPictures", "FitVml", shape, 100.0, 50.0)!;
            return (r ? "1" : "0") + (string?)shape.Attribute("style");
        }
        catch (Exception e)
        {
            return "!" + e.GetType().Name;
        }
    }

    private static string Unique(string desired)
    {
        var doc = DocxDocument.Open(new UniversalOffice.Tests.Editing.DocxBuilder()
            .WithEntry("word/media/image1.png", [1]).WithEntry("word/media/image.png", [1]).WithEntry("word/media/image2.png", [1])
            .WithEntry("word/media/x9.png", [1]).WithEntry("word/media/x1.png", [1]).WithEntry("word/a", [1]).WithEntry("a1", [1]).Build());
        try { return (string)Frozen.CallInstance(doc, "UniquePartPath", desired)!; }
        catch (Exception e) { return "!" + e.GetType().Name; }
    }

    public static string Unescape(string line)
    {
        var sb = new StringBuilder();
        for (int i = 0; i < line.Length; i++)
        {
            char c = line[i];
            if (c != '\\' || i + 1 >= line.Length) { sb.Append(c); continue; }
            char d = line[++i];
            switch (d)
            {
                case 'n': sb.Append('\n'); break;
                case 'r': sb.Append('\r'); break;
                case 't': sb.Append('\t'); break;
                default: sb.Append(d); break;
            }
        }
        return sb.ToString();
    }

    // ─────────────────────────────── inputs ───────────────────────────────

    private static IEnumerable<string> Urls()
    {
        string[] bases =
        [
            "http://example.com", "https://example.com/a/b?c=d#e", "HTTP://EXAMPLE.COM/", "mailto:a@b.com", "mailto:a@b.com?subject=x y",
            "mailto:", "mailto:a", "mailto:@", "mailto:a@", "mailto:%20", "http://", "http:", "http:/x", "http:x", "http:///x", "https:////x",
            "http://a b.com", "http://a/b c", "http://example.com:80", "http://example.com:65535", "http://example.com:65536", "http://example.com:",
            "http://example.com:x", "http://[::1]/", "http://[::1", "http://[fe80::1%25eth0]/", "http://1.2.3.4/", "http://1.2.3.256/", "http://999999999/",
            "http://0x7f.1/", "http://user:pass@host/", "http://user@/", "http://@host/", "http://ex%41mple.com/", "http://exa_mple.com/", "http://-a.com/",
            "http://a..b/", "http://a.b./", "http://ä.com/", "http://xn--4ca.com/", "http://例え.テスト/", "http://ex ample/", "http://ex\u00ADample.com/",
            " http://example.com ", "\thttp://example.com\n", "http://exa\tmple.com", "http://exa\nmple.com/", "http://example.com/\u0000", "http://example.com/%",
            "http://example.com/%zz", "http://example.com/%E9", "http://example.com/é", "http://example.com/\\path", "http:\\\\example.com\\x", "javascript:alert(1)",
            "JAVASCRIPT:alert(1)", "vbscript:x", "data:text/html,<b>", "file:///c:/x", "file://server/share", "\\\\server\\share", "c:\\x.docx", "C:/x", "/x/y",
            "x/y", "#anchor", "www.example.com", "//example.com", "ftp://ftp.example.com/x", "ftp://", "tel:+123", "news:comp.lang", "ldap://[::1]/x", "urn:isbn:1",
            "h ttp://x", "ht~tp://x", "1http://x", "+http://x", "http+x://y", "a:", "a:b", ":", "", " ", "http://example.com/" + new string('a', 70000),
            "http://" + new string('a', 64) + ".com/", "http://" + new string('a', 63) + ".com/", "http://" + string.Join(".", Enumerable.Repeat("a", 130)) + "/",
            "https://example.com/?q=<script>", "https://example.com/'\"", "http://example.com/{x}|^`", "http://example.com\u3000/", "http://exa%20mple.com/",
            "mailto:a@b.com,c@d.com", "mailto:\"a b\"@c.com", "mailto:a@[1.2.3.4]", "mailto:a@b.c?d", "mailto:a@b..c", "MAILTO:A@B.COM", "mailto://a@b.com",
            "https://example.com:0/", "https://example.com:00080/", "https://[::ffff:1.2.3.4]/", "https://[1:2:3:4:5:6:7:8:9]/", "https://[g::1]/",
            "http://a\u200Bb.com/", "http://a\uFEFFb.com/", "http://\uD83D\uDE00.com/", "http://example.com/\uD83D\uDE00", "https://example.com#frag ment",
            "https://example.com/?a=1&b=2", "http://localhost", "http://localhost:8080/x", "http://127.0.0.1:8080", "http://[::]:1/", "http://a.b-c.d/",
            "http://a.-b/", "http://a.b-/", "https://ex*ample.com", "https://ex!ample.com", "https://ex$ample.com", "https://ex'ample.com", "https://ex(ample).com",
            "https://ex,ample.com", "https://ex;ample.com", "https://ex=ample.com", "https://ex&ample.com", "https://ex+ample.com", "https://ex~ample.com",
        ];
        foreach (var b in bases) yield return b;
        // generated: scheme × authority × path × suffix combinations
        string[] schemes = ["http", "https", "mailto", "HtTpS", "ftp", "x"];
        string[] seps = [":", "://", ":/", ":\\\\", ": //"];
        string[] hosts = ["example.com", "a", "1.2.3.4", "[::1]", "a b", "a%2Fb", "", "ex_ample", "é.com", "a:1", "a:99999", "u@h", "@", "a.b.c.d.e"];
        string[] paths = ["", "/", "/x y", "/%41", "?q", "#f", "/a/../b", "/\u00e9", "/%", "\\x"];
        foreach (var s in schemes) foreach (var sep in seps) foreach (var h in hosts) foreach (var p in paths) yield return s + sep + h + p;
    }

    private static IEnumerable<(string, string)> TagFormats()
    {
        string[] types = ["a", "x.y", "my-app_1.tag", "", "a b", "á", "a:b", "a\n", "ok"];
        string[] jsons = ["{}", "[]", "1", "\"s\"", "{\"a\":\"é ü 😀 %\"}", "{", "", " {} ", "{\"a\":1,}", "nul", "{\"a\":[1,2]}", "\"\\ud800\""];
        foreach (var t in types) foreach (var j in jsons) yield return (t, j);
    }

    private static IEnumerable<string> Strings()
    {
        string[] fixedInputs =
        [
            "", " ", "abc", " PAGE ", "PAGE", "page \\* Arabic", "=SUM(A1)", " = 1", "HYPERLINK \"https://example.com\"", "HYPERLINK \\l \"x\"",
            "hyperlink \"http://a\" \\l \"y\"", "HYPERLINK", "HYPERLINK \"javascript:x\"", "HYPERLINK \\o \"tip\" \"https://a\"", "HYPERLINK \\t \"_blank\" \"mailto:a@b\"",
            "HYPERLINK \"https://a", "HYPERLINK\t\"https://a\"", "HYPERLINK \\L \"X\"", "HYPERLINK \\l", "HYPERLINK \\l \"\"", "\u00A0PAGE", "\u2003TOC\\o", "toc\\o \"1-3\"",
            "red", "RED", "Gray", "grey", "#fff", "#FfF", "fff", "#ffffff", "ABCDEF", "#abcdeg", "rgb(1,2,3)", "rgb( 255 , 0 , 300 )", "RGBA(1,2,3,0.5)",
            "rgb(1,2)", "rgb(1,2,3,)", "rgba(1,2,3, .5 )", "rgb(1000,2,3)", "rgb(٣,2,3)", " #123 ", "\t blue \n", "#12", "#1234567", "transparent", "rgb(1,2,3)\n",
            "Heading1", "heading1", "HEADING9", "Heading10", "TOC1", "toc9", "TOCA", "Heading٣", "Normal", "normal", "ListParagraph", "Caption", "TableGrid",
            "0", "1", "-1", "+1", " 1", "1 ", " 1 ", "00012", "2147483647", "2147483648", "-2147483648", "-2147483649", "9223372036854775807", "9223372036854775808",
            "1e3", "1.0", "0x10", "١٢", "1,000", "", "-", "+", "--1", "1-", "\u00A01", "1\u00A0", "\t1\r\n", "٣", "⁵",
            "2021-03-04T05:06:07Z", "2021-03-04T05:06:07", "2021-03-04", "2021-03-04T05:06:07.123Z", "2021-03-04T05:06:07+02:00", "2021-03-04 05:06",
            "03/04/2021", "4 Mar 2021", "2021-13-01T00:00:00Z", "2021-02-29T00:00:00Z", "2020-02-29T23:59:60Z", "  2021-03-04T05:06:07Z  ", "2021-03-04T05:06:07.1234567Z",
            "2021-03-04T05:06Z", "20210304T050607Z", "0001-01-01T00:00:00Z", "9999-12-31T23:59:59Z", "2021-03-04T24:00:00Z", "Thu, 04 Mar 2021 05:06:07 GMT",
            "width:100pt;height:50pt", "width:200pt;height:50pt", "position:absolute;width:600pt;height:3in;margin-left:1cm", "WIDTH : 10cm ; HEIGHT : 5cm",
            "width:1000px;height:10px", "width:2in;height:2in;width:9in", "height:300mm;width:10mm", "width:300;height:200", "width:1.2.3pt;height:5pt",
            "width:.pt;height:5pt", "width:0pt;height:50pt", "width:500pt", "width:120.5pt;height:60.25pt", "mso-width-percent:0;width:300pt;height:400pt",
            "z-index:1;margin-top:0;width:333.333pt;height:77.7777pt", "width : 150 pt ; height:100pt", "widthx:300pt;height:300pt", "Width:300Pt;Height:300PT",
            "%", "%41", "%4", "%%41", "a%20b", "%C3%A9", "%C3", "%E9", "%F0%9F%98%80", "%ED%A0%80", "%FF", "%2F", "%00", "%7B%22a%22%3A1%7D", "a+b", "%c3%a9",
            "😀", "é", "\u00ADsoft", "ﬀ", "ß", "ǅ", "ı", "İ", "ſ", "K", "Ω", "Σσς", "ẞ", "ǰ", "ﬃ", "ǈ", "\uD83D", "a\uDE00b", "\u0001\u0008\u000B\u000C\u000E\u001F",
            "\uFFFE\uFFFF\uFFFD", "\t\n\r", "\u0085\u2028", "a\u0000b", "x\u007Fy", "\uE000\uF8FF",
        ];
        foreach (var s in fixedInputs) yield return s;
        var rng = new Random(20261105);
        string alphabet = "aAzZ09 \t#{}()%,.:;-+=\\\"'/ \u00A0\u00E9\u00C9\u0131\u0130\u017F\u212A\u03A3\u03C3\u03C2\u1E9E\u00DF😀\u200B\u3000";
        for (int i = 0; i < 400; i++)
        {
            int len = rng.Next(0, 12);
            var sb = new StringBuilder();
            for (int k = 0; k < len; k++)
            {
                int idx = rng.Next(alphabet.Length);
                char c = alphabet[idx];
                if (char.IsHighSurrogate(c) && idx + 1 < alphabet.Length) { sb.Append(c).Append(alphabet[idx + 1]); continue; }
                if (char.IsLowSurrogate(c)) continue;
                sb.Append(c);
            }
            yield return sb.ToString();
        }
    }

    private static IEnumerable<(string, int, int)> WholeWords()
    {
        string[] texts = ["the theme", "a_b c", "x#{Title}y", "é#{a}", "1st 2nd", "αβγ δ", "😀a😀", "a\u00A0b", "#{T}", "x-y z", "the", "ab", "a\uFFFFb"];
        foreach (var t in texts)
            for (int s = 0; s < t.Length; s++)
                for (int l = 1; s + l <= t.Length && l <= 4; l++)
                    yield return (t, s, l);
    }

    private static IEnumerable<(string, string)> Pairs()
    {
        string[] a = ["word/document.xml", "word/glossary/document.xml", "document.xml", "", "Word/Document.xml", "WORD/DOCUMENT.XML", "word/media/image1.png", "a/b/c/d.xml",
            "Título", "TÍTULO", "título", "straße", "STRASSE", "ǅ", "ǆ", "Ǆ", "ı", "I", "i", "İ", "k", "K", "ſ", "s", "ΣΑΣ", "σας", "the end", "THE", "é", "É", "😀", "x😀y"];
        foreach (var x in a) foreach (var y in a) yield return (x, y);
        yield return ("word/document.xml", "word/my image%.png");
        yield return ("word/document.xml", "word/media/a b é.png");
        yield return ("word/sub/document.xml", "word/media/x.png");
        yield return ("customXml/item1.xml", "word/x.xml");
        yield return ("", "docProps/core.xml");
    }

    private static IEnumerable<string[]> RelIdSets()
    {
        yield return [];
        yield return ["rId1"];
        yield return ["rId1", "rId2", "rId9"];
        yield return ["rId01", "rId+5", "rId 7", "rid9", "rId2147483646"];
        yield return ["rId3", "rId4", "x", "rId٣"];
        yield return ["rId-1", "rId0"];
        yield return ["rId1", "rId2", "rId3"];
    }

    private static IEnumerable<byte[]> Images()
    {
        var png = UniversalOffice.Tests.Editing.DocxBuilder.PngOfSize(300, 200);
        yield return png;
        yield return png[..23];
        yield return png[..24];
        var phys = png.Concat(new byte[0]).ToArray();
        // pHYs chunk before IDAT: 2835 px/m (72 dpi) and 3780 (96 dpi)
        yield return Insert(png, 33, Chunk("pHYs", [0, 0, 0x0B, 0x13, 0, 0, 0x0E, 0xC4, 1]));
        yield return Insert(png, 33, Chunk("pHYs", [0, 0, 0x0B, 0x13, 0, 0, 0x0E, 0xC4, 0]));
        yield return Insert(png, 33, Chunk("tEXt", [1, 2, 3]));
        var neg = (byte[])png.Clone(); neg[16] = 0x80; yield return neg;
        var zero = (byte[])png.Clone(); zero[19] = 0; zero[18] = 0; zero[17] = 0; zero[16] = 0; yield return zero;
        var bigLen = (byte[])png.Clone(); bigLen[8] = 0x7F; bigLen[9] = 0xFF; yield return bigLen;
        yield return [0xFF, 0xD8, 0xFF, 0xE0, 0, 16, (byte)'J', (byte)'F', (byte)'I', (byte)'F', 0, 1, 1, 1, 0, 72, 0, 72, 0, 0, 0xFF, 0xC0, 0, 17, 8, 1, 0, 2, 0, 3, 1, 1, 0x11, 0];
        yield return [0xFF, 0xD8, 0xFF, 0xE0, 0, 16, (byte)'J', (byte)'F', (byte)'I', (byte)'F', 0, 1, 1, 2, 0, 100, 0, 50, 0, 0, 0xFF, 0xC2, 0, 17, 8, 0, 10, 0, 20];
        yield return [0xFF, 0xD8, 0xFF, 0xFF, 0xD8, 0xFF, 0x01, 0xFF, 0xD0, 0xFF, 0xC4, 0, 4, 0, 0, 0xFF, 0xC1, 0, 11, 8, 0, 5, 0, 6];
        yield return [0xFF, 0xD8, 0xFF, 0xDB, 0, 1];
        yield return [0xFF, 0xD8, 0, 0];
        yield return [(byte)'G', (byte)'I', (byte)'F', (byte)'8', (byte)'9', (byte)'a', 10, 0, 20, 0];
        yield return [(byte)'G', (byte)'I', (byte)'F', (byte)'8', (byte)'9', (byte)'a', 0, 0, 20, 0];
        var bmp = new byte[54]; bmp[0] = (byte)'B'; bmp[1] = (byte)'M'; bmp[14] = 40; bmp[18] = 100; bmp[22] = 0xF6; bmp[23] = 0xFF; bmp[24] = 0xFF; bmp[25] = 0xFF;
        bmp[38] = 0x13; bmp[39] = 0x0B; bmp[42] = 0x13; bmp[43] = 0x0B; yield return bmp;
        var bmpCore = new byte[26]; bmpCore[0] = (byte)'B'; bmpCore[1] = (byte)'M'; bmpCore[14] = 12; bmpCore[18] = 7; bmpCore[20] = 9; yield return bmpCore;
        var bmpMin = new byte[54]; bmpMin[0] = (byte)'B'; bmpMin[1] = (byte)'M'; bmpMin[14] = 40; bmpMin[18] = 1; bmpMin[22] = 0; bmpMin[23] = 0; bmpMin[24] = 0; bmpMin[25] = 0x80; yield return bmpMin;
        yield return [1, 2, 3];
        yield return [];
        yield return phys[..8];
    }

    private static byte[] Chunk(string type, byte[] data)
    {
        var c = new byte[12 + data.Length];
        c[0] = (byte)(data.Length >> 24); c[1] = (byte)(data.Length >> 16); c[2] = (byte)(data.Length >> 8); c[3] = (byte)data.Length;
        Encoding.ASCII.GetBytes(type).CopyTo(c, 4);
        data.CopyTo(c, 8);
        return c;
    }

    private static byte[] Insert(byte[] a, int at, byte[] b) => a[..at].Concat(b).Concat(a[at..]).ToArray();

    private static IEnumerable<(long, long, long?, long?)> Fits()
    {
        long[] v = [0, 1, 3, 635, 914400, 9_000_000, long.MaxValue / 2];
        long?[] m = [null, 0, -5, 1, 500_000, 5_400_000];
        foreach (var a in v) foreach (var b in v) foreach (var x in m) foreach (var y in m) yield return (a, b, x, y);
    }

    private static IEnumerable<double> Doubles()
    {
        double[] d = [0, -0.0, 1, 0.005, 0.015, 0.025, 0.125, 1.005, 2.675, 33.333333, 77.7777, 99.995, 99.994999, 1e-7, 123456789.125, 1e15, 1e16, 1e21, double.Epsilon, 0.3 - 0.1, 213.75 * 0.468, 480.0 / 7];
        foreach (var x in d) yield return x;
        var rng = new Random(7);
        for (int i = 0; i < 300; i++) yield return Math.Round(rng.NextDouble() * 1000, rng.Next(1, 9));
    }

    private static IEnumerable<string> Desired() =>
        ["word/media/image1.png", "word/media/image.png", "word/media/image5.png", "word/media/x9.png", "word/a", "a1", "a", "word/media/new.png",
         "word/media/image١.png", "word/media/a1b2.png", "word/media/.png", "word/media/image1", "word/media/image1.png.bak", "WORD/MEDIA/IMAGE1.PNG"];
}
