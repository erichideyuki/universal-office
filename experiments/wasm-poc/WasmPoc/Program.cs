using System.Diagnostics;
using System.Runtime.InteropServices.JavaScript;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using UniversalOffice.Docx;
using UniversalOffice.DocxLayout;
using UniversalOffice.Fonts;

namespace WasmPoc;

public static partial class Program
{
    public static void Main() => Console.WriteLine("UniversalOffice.Docx em WebAssembly: pronto");
}

/// <summary>Exportações para o JS: fontes, layout (árvore no formato do tree.ts), hit-test e PDF.</summary>
public static partial class Engine
{
    private static readonly MemoryFontProvider Fonts = new();
    private static DocxPackage? _pkg;
    private static FullDocument? _full;
    private static LayoutResult? _layout;
    private static readonly List<FontFace> FaceList = [];
    private static readonly Dictionary<FontFace, int> FaceIds = new(ReferenceEqualityComparer.Instance);
    private static readonly List<InlineImage> ImageList = [];
    private static readonly Dictionary<InlineImage, int> ImageIds = new(ReferenceEqualityComparer.Instance);

    [JSExport]
    public static void AddFont(string family, bool bold, bool italic, byte[] bytes) => Fonts.Add(family, bold, italic, bytes);

    /// <summary>Lê o .docx e diagrama; devolve os tempos (ms) e o número de páginas.</summary>
    [JSExport]
    public static string Load(byte[] docx)
    {
        FaceList.Clear(); FaceIds.Clear(); ImageList.Clear(); ImageIds.Clear();
        var sw = Stopwatch.StartNew();
        _pkg = DocxPackage.Load(docx);
        _full = FullDocReader.ReadFullDocument(_pkg);
        double read = sw.Elapsed.TotalMilliseconds;
        sw.Restart();
        _layout = LayoutCurrent();
        double layout = sw.Elapsed.TotalMilliseconds;
        return $"{{\"readMs\":{read:0.0},\"layoutMs\":{layout:0.0},\"pages\":{_layout.Pages.Count}}}";
    }

    /// <summary>Diagrama de novo o documento já lido (para medir o layout quente, sem a leitura).</summary>
    [JSExport]
    public static double Relayout()
    {
        var sw = Stopwatch.StartNew();
        _layout = LayoutCurrent();
        return sw.Elapsed.TotalMilliseconds;
    }

    private static LayoutResult LayoutCurrent()
    {
        List<EmbeddedFace> embedded;
        try
        {
            embedded = EmbeddedFonts.ReadEmbeddedFonts(_pkg!)
                .Select(e => new EmbeddedFace { Family = e.Family, Variant = e.Variant, Data = e.Data }).ToList();
        }
        catch { embedded = []; }
        var provider = UniversalOffice.DocxLayout.Fonts.WithEmbeddedFonts(new SubstituteFontProvider(Fonts), embedded, b => new SfntFont(b));
        return LayoutEngine.LayoutDocument(_full!, provider);
    }

    /// <summary>A árvore do layout em JSON com os nomes do tree.ts (camelCase); faces e imagens
    /// viram ids (os bytes saem por FaceBytes/ImageBytes, uma vez cada).</summary>
    [JSExport]
    public static string LayoutJson()
    {
        var res = _layout ?? throw new InvalidOperationException("nenhum documento");
        using var ms = new MemoryStream();
        using (var w = new Utf8JsonWriter(ms))
        {
            w.WriteStartObject();
            w.WriteStartArray("pages");
            foreach (var p in res.Pages)
            {
                w.WriteStartObject();
                w.WriteNumber("widthTw", p.WidthTw);
                w.WriteNumber("heightTw", p.HeightTw);
                w.WriteNumber("section", p.Section);
                w.WriteNumber("number", p.Number);
                if (p.Background is not null) w.WriteString("background", p.Background);
                w.WriteBoolean("backgroundShow", p.BackgroundShow);
                w.WriteStartArray("items");
                foreach (var it in p.Items) JsonSerializer.Serialize(w, it, LayoutJsonContext.Default.GetTypeInfo(it.GetType())!);
                w.WriteEndArray();
                w.WriteStartArray("lines");
                foreach (var l in p.Lines) JsonSerializer.Serialize(w, l, LayoutJsonContext.Default.LaidOutLine);
                w.WriteEndArray();
                w.WriteEndObject();
            }
            w.WriteEndArray();
            w.WriteStartArray("faces");
            foreach (var f in FaceList)
            {
                w.WriteStartObject();
                w.WriteString("key", f.Key);
                w.WriteString("family", f.Family);
                w.WriteBoolean("bold", f.Bold);
                w.WriteBoolean("italic", f.Italic);
                w.WriteBoolean("synthBold", f.SynthBold);
                w.WriteBoolean("synthItalic", f.SynthItalic);
                w.WriteEndObject();
            }
            w.WriteEndArray();
            w.WriteStartArray("images");
            foreach (var img in ImageList)
            {
                w.WriteStartObject();
                w.WriteString("ext", img.Ext);
                w.WriteNumber("widthTwips", img.WidthTwips);
                w.WriteNumber("heightTwips", img.HeightTwips);
                w.WriteEndObject();
            }
            w.WriteEndArray();
            w.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }

    [JSExport]
    public static byte[] FaceBytes(int id) => FaceList[id].Bytes ?? [];

    [JSExport]
    public static byte[] ImageBytes(int id) => ImageList[id].Data;

    /// <summary>Ponto (twips da página) → posição no texto e caret, pela geometria das linhas do motor.</summary>
    [JSExport]
    public static string HitTest(int page, double x, double y)
    {
        var p = _layout!.Pages[page];
        LaidOutLine? best = null;
        double bestD = double.MaxValue;
        foreach (var l in p.Lines)
        {
            double dy = y < l.Y ? l.Y - y : y > l.Y + l.Height ? y - l.Y - l.Height : 0;
            double dx = x < l.X ? l.X - x : x > l.X + l.Width ? x - l.X - l.Width : 0;
            double d = dy * 4 + dx;
            if (d < bestD) { bestD = d; best = l; }
        }
        if (best is null) return "null";
        double offset = best.Start, caretX = best.X;
        if (best.Chars is { Count: > 0 } chars)
        {
            caretX = chars[0].X;
            offset = chars[0].O;
            foreach (var c in chars)
            {
                if (x >= c.X + c.W / 2) { offset = c.O + 1; caretX = c.X + c.W; }
            }
        }
        using var ms = new MemoryStream();
        using (var w = new Utf8JsonWriter(ms))
        {
            w.WriteStartObject();
            w.WriteString("ref", best.Ref);
            w.WriteNumber("offset", offset);
            w.WriteNumber("x", caretX);
            w.WriteNumber("y", best.Y);
            w.WriteNumber("height", best.Height);
            w.WriteEndObject();
        }
        return Encoding.UTF8.GetString(ms.ToArray());
    }

    [JSExport]
    public static byte[] ToPdf()
    {
        return PdfRenderer.RenderPdf(_layout!, null, PdfNavigation.From(_full!, _layout!));
    }

    internal static int FaceId(FontFace f)
    {
        if (!FaceIds.TryGetValue(f, out int id)) { id = FaceList.Count; FaceList.Add(f); FaceIds[f] = id; }
        return id;
    }

    internal static int ImageId(InlineImage img)
    {
        if (!ImageIds.TryGetValue(img, out int id)) { id = ImageList.Count; ImageList.Add(img); ImageIds[img] = id; }
        return id;
    }
}
