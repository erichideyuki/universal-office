using System.Diagnostics;
using System.Globalization;
using System.Reflection;
using FontSpike;
using UniversalOffice.Fonts;

const int ParseIters = 10, MeasureIters = 20;
const double SizePt = 11;
string dir = Path.Combine(args[0], "data");
string[] names = File.ReadAllLines(Path.Combine(dir, "fonts.txt")).Where(l => l.Length > 0).ToArray();
var bytes = names.Select(n => File.ReadAllBytes(Path.Combine(dir, "fonts", n))).ToList();
int[] text = FontWorkload.CodePoints(File.ReadAllText(Path.Combine(dir, "text.txt")));

var fonts = FontWorkload.ParseAll(bytes, 1);
FontWorkload.MeasureAll(fonts, text, SizePt, 1);

var sw = Stopwatch.StartNew();
fonts = FontWorkload.ParseAll(bytes, ParseIters);
double parseMs = sw.Elapsed.TotalMilliseconds / ParseIters;
sw.Restart();
double total = FontWorkload.MeasureAll(fonts, text, SizePt, MeasureIters);
double measureMs = sw.Elapsed.TotalMilliseconds / MeasureIters;

var kernMap = typeof(SfntFont).GetField("_kernMap", BindingFlags.NonPublic | BindingFlags.Instance)!;
var class0 = typeof(SfntFont).GetField("_kernClass0", BindingFlags.NonPublic | BindingFlags.Instance)!;
var inv = CultureInfo.InvariantCulture;
var parity = names.Select((n, i) =>
{
    var f = fonts[i];
    int pairs = ((System.Collections.ICollection)kernMap.GetValue(f)!).Count;
    int c0 = ((System.Collections.ICollection)class0.GetValue(f)!).Count;
    return $"\"{n}\":[{pairs},{c0},{(f.KernInLatin ? 1 : 0)},{f.Metrics.UnitsPerEm.ToString(inv)},\"{FontWorkload.Measure(f, text, SizePt).ToString("F6", inv)}\"]";
});
Console.WriteLine($"{{\"impl\":\"csharp-native\",\"parseMs\":{parseMs.ToString("F2", inv)},\"measureMs\":{measureMs.ToString("F2", inv)},\"glyphs\":{text.Length * fonts.Length},\"total\":\"{total.ToString("F6", inv)}\",\"parity\":{{{string.Join(",", parity)}}}}}");
