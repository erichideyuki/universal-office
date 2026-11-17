// pptx/ppt -> PDF through the .NET facade of the Rust core, the way a server calls it.
using System.Diagnostics;
using UniversalOffice.Native;
using UniversalOffice.Pptx;

// convert <in.ppt> <out.pptx>
if (args.Length == 3 && args[0] == "convert")
{
    var swc = Stopwatch.StartNew();
    File.WriteAllBytes(args[2], PptConverter.ToPptx(File.ReadAllBytes(args[1])));
    Console.WriteLine($"uo-ffi {NativeCore.Version}: {args[1]} -> {args[2]} in {swc.ElapsedMilliseconds} ms");
    return 0;
}

if (args.Length < 2)
{
    Console.Error.WriteLine("usage: UniversalOffice.Pptx.Smoke <in.pptx|in.ppt> <out.pdf> [--font-dir <dir>]... [--no-system-fonts] | convert <in.ppt> <out.pptx>");
    return 2;
}
var dirs = new List<string>();
for (int i = 2; i < args.Length - 1; i++) if (args[i] == "--font-dir") dirs.Add(args[++i]);
bool system = !args.Contains("--no-system-fonts");
var sw = Stopwatch.StartNew();
using var conv = new SlidesPdfConverter(new SlidesPdfConverterOptions { SystemFonts = system, FontDirectories = dirs });
long indexMs = sw.ElapsedMilliseconds;
sw.Restart();
using var deck = PptxDeck.Open(File.ReadAllBytes(args[0]));
var pdf = deck.ToPdf(conv);
long convertMs = sw.ElapsedMilliseconds;
File.WriteAllBytes(args[1], pdf);
Console.WriteLine($"uo-ffi {NativeCore.Version}: {conv.Families.Count} font families indexed in {indexMs} ms; {args[0]} ({deck.SlideCount} slides, {deck.SlideWidthEmu / 914400:F2} x {deck.SlideHeightEmu / 914400:F2} in{(deck.IsLegacyPpt ? ", legacy .ppt" : "")}) -> {args[1]}: {pdf.Length} bytes in {convertMs} ms");
return 0;
