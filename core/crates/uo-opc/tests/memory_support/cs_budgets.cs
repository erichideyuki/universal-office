// M3 references of uo-opc in C#: the Core OpcPackage (eager: every part inflated at load) and
// MiniXml, on the same scenarios as package_budgets.rs and ts_budgets.mjs.
//
//   dotnet run cs_budgets.cs
//
// `allocated` = GC.GetAllocatedBytesForCurrentThread() around the operation; `retained` =
// GC.GetTotalMemory(true) with the outputs alive minus before. .NET has no cheap per-operation
// peak, so none is reported.
#:project ../../../../../dotnet/src/UniversalOffice.Core/UniversalOffice.Core.csproj
#:property PublishAot=false
using System.IO.Compression;
using System.Runtime.CompilerServices;
using UniversalOffice.Core;

var repo = Path.GetFullPath(Path.Combine(ScriptDir(), "../../../../.."));
byte[] File_(string rel) => File.ReadAllBytes(Path.Combine(repo, rel));
static bool IsXml(string n) => n.EndsWith(".xml", StringComparison.OrdinalIgnoreCase) || n.EndsWith(".rels", StringComparison.OrdinalIgnoreCase) || n.EndsWith(".vml", StringComparison.OrdinalIgnoreCase);

int ParseAll(OpcPackage p)
{
    int n = 0;
    foreach (var name in p.PartNames())
    {
        if (IsXml(name)) { MiniXml.Parse(p.GetPartText(name)!); n++; }
        else p.GetPart(name);
    }
    return n;
}
void Edit(OpcPackage p, string main)
{
    var t = p.GetPartText(main)!;
    int i = t.LastIndexOf("</", StringComparison.Ordinal);
    p.SetPartText(main, t[..i] + "<uo:soak xmlns:uo=\"urn:uo\" n=\"1\"/>" + t[i..]);
    p.SetPartText("customXml/soak.xml", "<soak n=\"1\"/>");
}
byte[] Deck()
{
    var ms = new MemoryStream();
    using (var src = new ZipArchive(new MemoryStream(File_("slides/samples/reference.pptx")), ZipArchiveMode.Read))
    using (var zip = new ZipArchive(ms, ZipArchiveMode.Create, leaveOpen: true))
    {
        foreach (var e in src.Entries)
        {
            using var r = e.Open();
            using var w = zip.CreateEntry(e.FullName, CompressionLevel.Optimal).Open();
            r.CopyTo(w);
        }
        using (var w = zip.CreateEntry("ppt/media/zeros.bin", CompressionLevel.Optimal).Open()) w.Write(new byte[64 << 20]);
        using (var w = zip.CreateEntry("ppt/media/video.mp4", CompressionLevel.NoCompression).Open()) w.Write(Noise(16 << 20, 7));
    }
    return ms.ToArray();
}
static byte[] Noise(int len, ulong seed)
{
    var o = new byte[len];
    ulong x = seed | 1;
    for (int i = 0; i < len; i += 8)
    {
        x ^= x << 13; x ^= x >> 7; x ^= x << 17;
        for (int k = 0; k < 8 && i + k < len; k++) o[i + k] = (byte)(x >> (8 * k));
    }
    return o;
}

var giant = File_("sheet/samples/giant-1m.xlsx");
var docx = File_("write/packages/docx-layout/test/oracle/justify4/sw-16-8.docx");
var deck = Deck();
// warm-up: JIT and the static tables of the code paths measured
ParseAll(OpcPackage.Load(docx));
Edit(OpcPackage.Load(docx), "word/document.xml");
OpcPackage.Load(docx).Save();

Measure("giant-open-read-parse", () => { var p = OpcPackage.Load(giant); ParseAll(p); return p; });
var gp = OpcPackage.Load(giant);
Measure("giant-save-untouched", () => gp.Save());
gp = null;
Measure("docx-open-read-parse", () => { var p = OpcPackage.Load(docx); return (p, MiniXml.Parse(p.GetPartText("word/document.xml")!)); });
var dp = OpcPackage.Load(docx);
Measure("docx-edit-save", () => { Edit(dp, "word/document.xml"); return dp.Save(); });
Measure("deck-open-parse", () => { var p = OpcPackage.Load(deck); ParseAll(p); return p; });
var kp = OpcPackage.Load(deck);
Measure("deck-edit-save", () => { Edit(kp, "ppt/slides/slide1.xml"); return kp.Save(); });

static void Measure<T>(string name, Func<T> op)
{
    long before = GC.GetTotalMemory(true);
    long a0 = GC.GetAllocatedBytesForCurrentThread();
    var r = op();
    long a1 = GC.GetAllocatedBytesForCurrentThread();
    long after = GC.GetTotalMemory(true);
    GC.KeepAlive(r);
    Console.WriteLine($"{{\"scenario\":\"{name}\",\"allocated\":{a1 - a0},\"retained\":{after - before}}}");
}
static string ScriptDir([CallerFilePath] string path = "") => Path.GetDirectoryName(path)!;
