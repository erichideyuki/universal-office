// Captures the C# font providers, the REFERENCE of the `server-substitute` policy (and of the system
// index): `SystemFontIndex` (UniversalOffice.Core/Fonts/SystemFonts.cs), `SystemFontProvider` and
// `SubstituteFontProvider` (UniversalOffice.Docx/DocxLayout), over the generated font folder
// (cargo run -p uo-fonts --example provider_corpus) plus shared/fonts/files, driven by the system
// scenarios (`S`) of tests/parity/provider_scenarios.txt. Same format as the other captures (digests
// only), replayed by tests/provider_parity.rs:
//
//   dotnet run provider_capture.cs -- <corpus folder> <shared/fonts/files> <scenarios file> <output cap> [trace file]
//
//   csharp-index/entries   every face the index read, sorted: family|typo|bold italic|weight|file#face
//   csharp-index/families  the family list (name ID 1), sorted
//   csharp-system/<id>     stream of answer lines, SystemFontProvider (one per scenario)
//   csharp-server/<id>     stream of answer lines, SubstituteFontProvider over a SystemFontProvider
//   csharp-catalog         FontCatalog.MetricSubstitute of every family the scenarios ask for
// An answer line is  key|family|sha256|bold italic synthBold synthItalic standIn|identity  with the
// path of the key reduced to the file name and identity the index of the first answer of the
// scenario that is the same object.
#:property PublishAot=false
#:project ../../../../../dotnet/src/UniversalOffice.Docx/UniversalOffice.Docx.csproj
using System.Security.Cryptography;
using System.Text;
using UniversalOffice.DocxLayout;
using UniversalOffice.Fonts;

var corpus = args[0];
var shared = args[1];
var scenariosFile = args[2];
var output = args[3];
var traceFile = args.Length > 4 ? args[4] : null;

// only the two folders count: the default roots point nowhere
Environment.SetEnvironmentVariable("WINDIR", "Z:/uo-fonts-nowhere");
Environment.SetEnvironmentVariable("LOCALAPPDATA", "Z:/uo-fonts-nowhere");
Environment.SetEnvironmentVariable("EXTRA_FONT_DIRS", null);

static string Sha(byte[]? b) => b is null ? "none" : Convert.ToHexString(SHA256.HashData(b)).ToLowerInvariant();
static string NormalizeKey(string key)
{
    int at = key.LastIndexOf('#');
    if (at < 0) return key;
    string head = key[..at], tail = key[(at + 1)..];
    var parts = head.Split('|', 3);
    if (parts.Length < 3) return key;
    int slash = parts[2].LastIndexOfAny(['/', '\\']);
    return $"{parts[0]}|{parts[1]}|{(slash >= 0 ? parts[2][(slash + 1)..] : parts[2])}#{tail}";
}
static string Line(FontFace? a, List<FontFace> seen)
{
    if (a is null) return "-";
    int at = seen.FindIndex(x => ReferenceEquals(x, a));
    if (at < 0) { seen.Add(a); at = seen.Count - 1; }
    static int F(bool x) => x ? 1 : 0;
    return $"{NormalizeKey(a.Key)}|{a.Family}|{Sha(a.Bytes)}|{F(a.Bold)}{F(a.Italic)}{F(a.SynthBold)}{F(a.SynthItalic)}{F(a.StandIn)}|{at}";
}
static string Digest(List<string> items)
{
    // the canonical stream of uo-testkit: tag 'q', then the string as u32 length + UTF-8 bytes; items = 2 per line
    using var sha = SHA256.Create();
    var ms = new MemoryStream();
    foreach (var s in items)
    {
        ms.WriteByte((byte)'q');
        var u8 = Encoding.UTF8.GetBytes(s);
        ms.Write(BitConverter.GetBytes((uint)u8.Length));
        ms.Write(u8);
    }
    return $"{items.Count * 2}:{Convert.ToHexString(sha.ComputeHash(ms.ToArray())).ToLowerInvariant()[..32]}";
}

var scenarios = new List<(string Id, List<(string Family, bool Bold, bool Italic)> Asks)>();
var rows = File.ReadAllLines(scenariosFile).Where(l => l.Length > 0).Select(l => l.Split('\t')).ToList();
for (int i = 0; i < rows.Count; i++)
{
    if (rows[i][0] != "S") continue;
    var asks = new List<(string, bool, bool)>();
    int j = i + 1;
    while (rows[j][0] != "E") { asks.Add((rows[j][1], rows[j][2] == "1", rows[j][3] == "1")); j++; }
    scenarios.Add((rows[i][1], asks));
    i = j;
}

var index = SystemFontIndex.Create([corpus, shared]);
var lines = new List<string>
{
    "# uo-fonts provider policies: the C# SystemFontIndex, SystemFontProvider and SubstituteFontProvider over the generated font folder; digests only.",
    "# Written by core/crates/uo-fonts/tests/parity_support/provider_capture.cs; replayed by core/crates/uo-fonts/tests/provider_parity.rs.",
    $"@dotnet {Environment.Version} {System.Runtime.InteropServices.RuntimeInformation.OSDescription.Replace(' ', '_')}",
    $"@inputs {scenarios.Count} scenarios",
};
var trace = new List<string>();

var entries = index.Entries
    .Select(e => $"{e.Family}|{e.Typo ?? "-"}|{(e.Bold ? "b" : "")}{(e.Italic ? "i" : "")}|{e.Weight}|{Path.GetFileName(e.Path)}#{e.Index}")
    .OrderBy(x => x, StringComparer.Ordinal).ToList();
lines.Add($"csharp-index/entries\tlist={entries.Count}:{Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(string.Join("\u0001", entries)))).ToLowerInvariant()[..32]}");
trace.Add("entries"); trace.AddRange(entries);
var families = index.Families().OrderBy(x => x, StringComparer.Ordinal).ToList();
lines.Add($"csharp-index/families\tlist={families.Count}:{Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(string.Join("\u0001", families)))).ToLowerInvariant()[..32]}");
trace.Add("families " + string.Join("|", families));

var asked = new SortedSet<string>(StringComparer.Ordinal);
foreach (var (id, asks) in scenarios)
{
    var sysLines = new List<string>();
    var subLines = new List<string>();
    var sys = new SystemFontProvider(index);
    var sub = new SubstituteFontProvider(new SystemFontProvider(index));
    var seenSys = new List<FontFace>();
    var seenSub = new List<FontFace>();
    trace.Add($"## {id}");
    foreach (var (family, bold, italic) in asks)
    {
        asked.Add(family);
        var a = Line(sys.Face(family, bold, italic), seenSys);
        var b = Line(sub.Face(family, bold, italic), seenSub);
        sysLines.Add(a);
        subLines.Add(b);
        trace.Add($"system {family} {(bold ? 1 : 0)}{(italic ? 1 : 0)} {a}");
        trace.Add($"server {family} {(bold ? 1 : 0)}{(italic ? 1 : 0)} {b}");
    }
    lines.Add($"csharp-system/{id}\tstream={Digest(sysLines)}");
    lines.Add($"csharp-server/{id}\tstream={Digest(subLines)}");
}

var catalog = asked.Select(f => $"{f}=>{FontCatalog.MetricSubstitute(f) ?? "-"}").ToList();
lines.Add($"csharp-catalog\tlist={catalog.Count}:{Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(string.Join("\u0001", catalog)))).ToLowerInvariant()[..32]}");
trace.Add("catalog"); trace.AddRange(catalog);

File.WriteAllText(output, string.Join("\n", lines) + "\n");
if (traceFile is not null) File.WriteAllText(traceFile, string.Join("\n", trace) + "\n");
Console.WriteLine($"{output}: {scenarios.Count} scenarios, {entries.Count} faces");
