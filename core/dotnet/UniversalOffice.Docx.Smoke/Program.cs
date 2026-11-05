// docx/.doc -> PDF through the .NET facade of the Rust core, the way clickCompliance calls it.
using System.Diagnostics;
using UniversalOffice.DocxLayout;
using UniversalOffice.Native;

if (args.Length < 2)
{
    Console.Error.WriteLine("usage: UniversalOffice.Docx.Smoke <in.docx|in.doc> <out.pdf> [--font-dir <dir>]... [--no-system-fonts]");
    return 2;
}
var dirs = new List<string>();
for (int i = 2; i < args.Length - 1; i++) if (args[i] == "--font-dir") dirs.Add(args[++i]);
bool system = !args.Contains("--no-system-fonts");
var sw = Stopwatch.StartNew();
using var conv = new DocxPdfConverter(new DocxPdfConverterOptions { SystemFonts = system, FontDirectories = dirs });
long indexMs = sw.ElapsedMilliseconds;
var opts = new LayoutOptions { HyphenationLanguages = ["pt-BR", "en", "es"] };
sw.Restart();
var pdf = DocxLayoutPipeline.ToPdf(File.ReadAllBytes(args[0]), conv, opts);
long convertMs = sw.ElapsedMilliseconds;
File.WriteAllBytes(args[1], pdf);
Console.WriteLine($"uo-ffi {NativeCore.Version}: {conv.Families.Count} font families indexed in {indexMs} ms; {args[0]} -> {args[1]}: {pdf.Length} bytes in {convertMs} ms");
return 0;
