// G16: capture of the C# legacy `kern` reader (UniversalOffice.Core/Pdf/TtfProgram.cs: `KernAdjust`,
// `HasKerning`) in the stream format of uo-testkit, for `FontProgram::legacy_kern`. The corpus is
// written by `cargo run --release -p uo-fonts --example legacy_kern_corpus -- <dir>`; the pairs asked
// come from the scanner below, repeated in tests/fonts_parity_support/legacy.rs. Output:
// tests/parity/legacy_kern.cap (digests only), replayed by tests/legacy_kern_parity.rs.
//
//   dotnet run legacy_kern.cs -- <corpus dir> <output cap>
#:property PublishAot=false
#:project ../../../../../dotnet/src/UniversalOffice.Core/UniversalOffice.Core.csproj
using System.Security.Cryptography;
using System.Text;
using UniversalOffice.Pdf;

var dir = args[0];
var output = args[1];
var lines = new List<string>
{
    "# uo-fonts G16: the C# TtfProgram legacy kern (KernAdjust, HasKerning) over tests/fonts_parity_support/legacy.rs; digests only.",
    "# Written by core/crates/uo-fonts/tests/parity_support/legacy_kern.cs; replayed by core/crates/uo-fonts/tests/legacy_kern_parity.rs.",
    $"@dotnet {Environment.Version} {System.Runtime.InteropServices.RuntimeInformation.OSDescription.Replace(' ', '_')}",
};
int count = 0;
foreach (var row in File.ReadAllLines(Path.Combine(dir, "manifest.tsv")))
{
    if (row.Length == 0) continue;
    var parts = row.Split('\t');
    var bytes = File.ReadAllBytes(Path.Combine(dir, parts[1]));
    lines.Add($"{parts[0]}\tlegacykern={LegacyFacet(bytes)}");
    count++;
}
lines.Insert(3, $"@inputs {count} fonts");
File.WriteAllText(output, string.Join("\n", lines) + "\n");
Console.WriteLine($"{output}: {count} fonts");

static int U16(byte[] b, int o) => o >= 0 && o + 2 <= b.Length ? (b[o] << 8) | b[o + 1] : -1;

static List<(uint, uint)> KernPairs(byte[] b)
{
    var outList = new List<(uint, uint)>();
    int n = U16(b, 4);
    if (n < 0) return outList;
    long kern = -1;
    for (int i = 0; i < n; i++)
    {
        int p = 12 + i * 16;
        if (p + 16 > b.Length && p + 12 > b.Length) return outList;
        if (b[p] == 'k' && b[p + 1] == 'e' && b[p + 2] == 'r' && b[p + 3] == 'n')
            kern = ((long)b[p + 8] << 24) | ((long)b[p + 9] << 16) | ((long)b[p + 10] << 8) | b[p + 11];
    }
    if (kern < 0) return outList;
    int nTables = U16(b, (int)kern + 2);
    if (nTables < 0) return outList;
    long pos = kern + 4;
    var seen = new HashSet<(int, int)>();
    for (int t = 0; t < nTables; t++)
    {
        int length = U16(b, (int)pos + 2), coverage = U16(b, (int)pos + 4);
        if (length < 0 || coverage < 0) break;
        if (coverage >> 8 == 0)
        {
            int nPairs = U16(b, (int)pos + 6);
            if (nPairs < 0) break;
            for (int i = 0; i < nPairs; i++)
            {
                int r = (int)pos + 14 + i * 6;
                int l = U16(b, r), rt = U16(b, r + 2);
                if (l < 0 || rt < 0) break;
                if (seen.Add((l, rt)))
                {
                    outList.Add(((uint)l, (uint)rt));
                    if (outList.Count >= 100000) return outList;
                }
            }
        }
        pos += length;
    }
    return outList;
}

static uint NumGlyphs(byte[] b)
{
    int n = U16(b, 4);
    if (n < 0) return 0;
    for (int i = 0; i < n; i++)
    {
        int p = 12 + i * 16;
        if (p + 4 > b.Length) return 0;
        if (b[p] == 'm' && b[p + 1] == 'a' && b[p + 2] == 'x' && b[p + 3] == 'p')
        {
            if (p + 12 > b.Length) return 0;
            long off = ((long)b[p + 8] << 24) | ((long)b[p + 9] << 16) | ((long)b[p + 10] << 8) | b[p + 11];
            int v = off > int.MaxValue - 8 ? -1 : U16(b, (int)off + 4);
            return v < 0 ? 0 : (uint)v;
        }
    }
    return 0;
}

static string LegacyFacet(byte[] bytes)
{
    TtfProgram program;
    try { program = new TtfProgram(bytes); }
    catch { return "E"; }
    var hash = IncrementalHash.CreateHash(HashAlgorithmName.SHA256);
    long items = 0;
    void Tag(char c) { hash.AppendData(new[] { (byte)c }); items++; }
    void Bool(bool v) { hash.AppendData(new[] { (byte)(v ? 1 : 0) }); items++; }
    void U32(uint v) { hash.AppendData(BitConverter.GetBytes(v)); items++; }
    void I64(long v) { hash.AppendData(BitConverter.GetBytes(v)); items++; }
    void Ask(uint l, uint r) { U32(l); U32(r); I64(program.KernAdjust((int)l, (int)r)); }
    Tag('K');
    Bool(program.HasKerning);
    foreach (var (l, r) in KernPairs(bytes)) Ask(l, r);
    uint n = NumGlyphs(bytes);
    uint s = 0x4C4B4552;
    uint Next() { s ^= s << 13; s ^= s >> 17; s ^= s << 5; return s; }
    uint Below(uint m) => m == 0 ? 0 : Next() % m;
    for (int i = 0; i < 10000; i++)
    {
        uint l = Below(n + 2);
        uint r = Below(n + 2);
        Ask(l, r);
    }
    foreach (var (l, r) in new (uint, uint)[] { (0, 0x10000), (1, 0x10001), (0xFFFF, 0xFFFF), (0, 0), (0xFFFF, 0x10000), (0x10000, 0), (0x10000, 1), (0x7FFFFFFF, 0x7FFFFFFF) })
        Ask(l, r);
    var digest = hash.GetHashAndReset();
    return $"{items}:{Convert.ToHexStringLower(digest)[..32]}";
}
