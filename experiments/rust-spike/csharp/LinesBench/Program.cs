using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using UniversalOffice.Docx;
using UniversalOffice.DocxLayout;

// Réplica do spike 2 no C#: reconstrói os parágrafos da captura do TS com os tipos do motor
// .NET e as faces do SystemFontProvider, repete new / next / reset no LineBreaker de verdade,
// confere as linhas e mede CPU e memória da quebra.
int iters = args.Length > 1 ? int.Parse(args[1], CultureInfo.InvariantCulture) : 20;
using var doc = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(args[0], "data", "lines.json")));
var root = doc.RootElement;
var provider = SystemFontProvider.Shared;

double? Opt(JsonElement e, string name) => e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.Number ? v.GetDouble() : null;
bool Flag(JsonElement e, string name) => e.TryGetProperty(name, out var v) && (v.ValueKind == JsonValueKind.True || (v.ValueKind == JsonValueKind.Number && v.GetDouble() != 0));
string? Str(JsonElement e, string name) => e.TryGetProperty(name, out var v) && v.ValueKind == JsonValueKind.String ? v.GetString() : null;

var faces = root.GetProperty("faces").EnumerateArray().Select(f =>
    provider.Face(f.GetProperty("family").GetString()!, f.GetProperty("bold").GetBoolean(), f.GetProperty("italic").GetBoolean())
    ?? throw new InvalidOperationException($"face ausente: {f.GetProperty("family").GetString()}")).ToList();

var kinds = new Dictionary<string, GlyphKind>
{
    ["char"] = GlyphKind.Char, ["space"] = GlyphKind.Space, ["nbsp"] = GlyphKind.Nbsp, ["tab"] = GlyphKind.Tab, ["lb"] = GlyphKind.Lb,
    ["pb"] = GlyphKind.Pb, ["cb"] = GlyphKind.Cb, ["obj"] = GlyphKind.Obj, ["shy"] = GlyphKind.Shy, ["box"] = GlyphKind.Box,
};
var plainStyle = new RunStyle { Rpr = new RPr() };
var ulStyle = new RunStyle { Rpr = new RPr(), Underline = new UnderlineStyle { Val = "single" } };
Glyph ToGlyph(JsonElement o)
{
    var g = new Glyph
    {
        Kind = kinds[o.GetProperty("k").GetString()!], Ch = o.GetProperty("ch").GetString()!, Cp = o.GetProperty("cp").GetInt32(),
        Adv = o.GetProperty("adv").GetDouble(), Asc = o.GetProperty("asc").GetDouble(), Desc = o.GetProperty("desc").GetDouble(),
        Offset = o.GetProperty("off").GetDouble(), BreakAfter = o.GetProperty("br").GetBoolean(), SizePt = o.GetProperty("sz").GetDouble(),
        Face = faces[o.GetProperty("f").GetInt32()], Style = Flag(o, "ul") ? ulStyle : plainStyle,
        NAsc = Opt(o, "nAsc"), NDesc = Opt(o, "nDesc"), Glue = Flag(o, "glue"), Clear = Flag(o, "clear"), Hr = Opt(o, "hr"),
        FeShrink = Opt(o, "fe"), Fixed = Flag(o, "fixed"), Hang = Flag(o, "hang"), Hyph = Flag(o, "hyph"),
    };
    if (o.TryGetProperty("ptab", out var pt)) g.Ptab = new PtabInfo { Alignment = pt.GetProperty("a").GetString()!, Leader = Str(pt, "l") };
    return g;
}

var ctxs = new Dictionary<string, DocContext>();
DocContext CtxOf(JsonElement s)
{
    string key = s.GetRawText();
    if (ctxs.TryGetValue(key, out var c)) return c;
    var fmt = DocContext.EmptyFormatting();
    fmt.Settings.DefaultTabStop = s.GetProperty("defaultTabStop").GetDouble();
    fmt.Settings.CompatibilityMode = s.GetProperty("compatibilityMode").GetDouble();
    if (Flag(s, "doNotUseIndentAsNumberingTabStop")) fmt.Settings.Compat["doNotUseIndentAsNumberingTabStop"] = true;
    if (Flag(s, "splitPgBreakAndParaMark")) fmt.Settings.Compat["splitPgBreakAndParaMark"] = true;
    double limit = s.GetProperty("consecutiveHyphenLimit").GetDouble();
    if (limit != 0) fmt.Settings.ConsecutiveHyphenLimit = limit;
    fmt.Settings.HyphenationZone = Opt(s, "hyphenationZone");
    c = new DocContext(fmt, provider);
    ctxs[key] = c;
    return c;
}

var paras = root.GetProperty("paras").EnumerateArray().Select(p =>
{
    var ms = p.GetProperty("markStyle");
    var rpr = new RPr { VertAlign = Str(ms, "vertAlign"), Sz = Opt(ms, "sz") };
    if (ms.TryGetProperty("textShadow", out var ts) && ts.ValueKind == JsonValueKind.Object)
        rpr.TextShadow = new TextShadowSpec { Blur = ts.GetProperty("blur").GetDouble(), Dist = ts.GetProperty("dist").GetDouble(), Dir = ts.GetProperty("dir").GetDouble() };
    var pp = new PreparedPara
    {
        Glyphs = p.GetProperty("glyphs").EnumerateArray().Select(ToGlyph).ToList(),
        Tabs = p.GetProperty("tabs").EnumerateArray().Select(t => new TabSpec { Val = t.GetProperty("val").GetString()!, Pos = t.GetProperty("pos").GetDouble(), Leader = Str(t, "leader") }).ToList(),
        FirstLine = p.GetProperty("firstLine").GetDouble(), IndLeft = p.GetProperty("indLeft").GetDouble(), IndRight = p.GetProperty("indRight").GetDouble(),
        Jc = p.GetProperty("jc").GetString()!, LineRule = p.GetProperty("lineRule").GetString()!, Line = p.GetProperty("line").GetDouble(),
        MarkFace = faces[p.GetProperty("markFace").GetInt32()],
        MarkStyle = new RunStyle { Rpr = rpr, SizePt = ms.GetProperty("sizePt").GetDouble(), Shift = ms.GetProperty("shift").GetDouble() },
    };
    if (p.GetProperty("marker") is { ValueKind: JsonValueKind.Object } mk)
        pp.Marker = new ParaMarker { Jc = mk.GetProperty("jc").GetString()!, Width = mk.GetProperty("width").GetDouble(), Suff = mk.GetProperty("suff").GetString()!, Glyphs = mk.GetProperty("glyphs").EnumerateArray().Select(ToGlyph).ToList() };
    return (Ctx: CtxOf(p.GetProperty("settings")), Grid: Opt(p, "grid"), P: pp);
}).ToList();

var events = root.GetProperty("events").EnumerateArray().Select(e =>
{
    string op = e[0].GetString()!;
    int id = e[1].GetInt32();
    if (op == "next")
    {
        var f = e[2];
        var fr = new LineFrame(f.GetProperty("left").GetDouble(), f.GetProperty("right").GetDouble()) { Gap = Flag(f, "gap") };
        if (f.TryGetProperty("next", out var n)) fr.Next = (n.GetProperty("left").GetDouble(), n.GetProperty("right").GetDouble());
        return (Op: 1, Id: id, Frame: fr, Mark: default((int, int, bool, int)));
    }
    if (op == "reset")
    {
        var m = e[2];
        return (Op: 2, Id: id, Frame: default(LineFrame), Mark: (m.GetProperty("pos").GetInt32(), m.GetProperty("lineIdx").GetInt32(), m.GetProperty("trailingBreak").GetBoolean(), m.GetProperty("hyphenRun").GetInt32()));
    }
    return (Op: 0, Id: id, Frame: default(LineFrame), Mark: default((int, int, bool, int)));
}).ToList();
var expected = root.GetProperty("lines").EnumerateArray().ToList();
long glyphs = paras.Sum(p => (long)p.P.Glyphs.Count);

List<Line> Replay()
{
    var brs = new LineBreaker[paras.Count];
    var outp = new List<Line>(expected.Count);
    foreach (var e in events)
    {
        if (e.Op == 0) { var q = paras[e.Id]; brs[e.Id] = new LineBreaker(q.Ctx, q.P, q.Grid); }
        else if (e.Op == 1) outp.Add(brs[e.Id].Next(e.Frame));
        else brs[e.Id].Reset(e.Mark);
    }
    return outp;
}

int Mismatches(List<Line> got)
{
    var nextParas = events.Where(e => e.Op == 1).Select(e => paras[e.Id].P).ToList();
    int Ref(PlacedGlyph q, PreparedPara p)
    {
        int i = p.Glyphs.FindIndex(g => ReferenceEquals(g, q.G));
        if (i >= 0) return i;
        int m = p.Marker?.Glyphs.FindIndex(g => ReferenceEquals(g, q.G)) ?? -1;
        return m >= 0 ? -2 - m : -1;
    }
    bool SameList(List<PlacedGlyph> a, JsonElement b, PreparedPara p)
    {
        if (a.Count != b.GetArrayLength()) return false;
        int k = 0;
        foreach (var c in b.EnumerateArray())
        {
            var q = a[k++];
            string? leader = c[3].ValueKind == JsonValueKind.String ? c[3].GetString() : null;
            if (Ref(q, p) != c[0].GetInt32() || q.X != c[1].GetDouble() || q.W != c[2].GetDouble() || (q.Leader is "none" ? null : q.Leader) != leader) return false;
        }
        return true;
    }
    int bad = Math.Abs(got.Count - expected.Count);
    for (int n = 0; n < Math.Min(got.Count, expected.Count); n++)
    {
        var l = got[n];
        var c = expected[n];
        var p = nextParas[n];
        bool same = l.Start == c.GetProperty("start").GetInt32() && l.End == c.GetProperty("end").GetInt32() && l.Width == c.GetProperty("width").GetDouble()
            && l.XStart == c.GetProperty("xStart").GetDouble() && l.Asc == c.GetProperty("asc").GetDouble() && l.Desc == c.GetProperty("desc").GetDouble()
            && l.Height == c.GetProperty("height").GetDouble() && l.Baseline == c.GetProperty("baseline").GetDouble() && l.Last == c.GetProperty("last").GetBoolean()
            && l.BreakAfter == Str(c, "breakAfter") && l.Clear == c.GetProperty("clear").GetBoolean() && l.GapTab == Str(c, "gapTab")
            && l.Spanned == c.GetProperty("spanned").GetBoolean() && SameList(l.Placed, c.GetProperty("placed"), p)
            && (l.Marker is null ? c.GetProperty("marker").ValueKind == JsonValueKind.Null : c.GetProperty("marker").ValueKind == JsonValueKind.Array && SameList(l.Marker, c.GetProperty("marker"), p));
        if (!same) bad++;
    }
    return bad;
}

int bad = Mismatches(Replay());
Replay();

var proc = Process.GetCurrentProcess();
var times = new List<double>();
var cpu0 = proc.TotalProcessorTime;
for (int i = 0; i < iters; i++)
{
    var sw = Stopwatch.StartNew();
    GC.KeepAlive(Replay());
    times.Add(sw.Elapsed.TotalMilliseconds);
}
proc.Refresh();
double cpu = (proc.TotalProcessorTime - cpu0).TotalMilliseconds / iters;
times.Sort();

long without = GC.GetTotalMemory(true);
long a0 = GC.GetAllocatedBytesForCurrentThread();
var kept = Replay();
long allocated = GC.GetAllocatedBytesForCurrentThread() - a0;
long withLines = GC.GetTotalMemory(true);
int keptCount = kept.Count;
GC.KeepAlive(kept);
proc.Refresh();

var inv = CultureInfo.InvariantCulture;
string F(double v) => v.ToString("F2", inv);
Console.WriteLine($"{{\"impl\":\"csharp-native\",\"lines\":{keptCount},\"glyphs\":{glyphs},\"mismatches\":{bad},\"bestMs\":{F(times[0])},\"medianMs\":{F(times[times.Count / 2])},\"cpuMsPerIter\":{F(cpu)},\"allocatedMB\":{F(allocated / 1048576.0)},\"retainedMB\":{F((withLines - without) / 1048576.0)},\"peakRssMB\":{F(proc.PeakWorkingSet64 / 1048576.0)},\"heapMB\":{F(without / 1048576.0)}}}");
