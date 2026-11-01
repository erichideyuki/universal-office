using System.Diagnostics;
using UniversalOffice.Docx;
using UniversalOffice.DocxLayout;
using UniversalOffice.Fonts;
using WasmPoc;

string here = AppContext.GetData("PocDir") as string ?? args[0];
string docxPath = args.Length > 1 ? args[1] : Path.Combine(here, "../../site/assets/samples/demo.docx");
string fontsDir = Path.Combine(here, "../../site/assets/fonts");

var fonts = new MemoryFontProvider();
foreach (var (file, family) in new[] { ("carlito", "Carlito"), ("caladea", "Caladea"), ("liberationsans", "Liberation Sans"), ("liberationserif", "Liberation Serif"), ("liberationmono", "Liberation Mono") })
    foreach (var (v, b, i) in new[] { ("regular", false, false), ("bold", true, false), ("italic", false, true), ("bolditalic", true, true) })
        fonts.Add(family, b, i, File.ReadAllBytes(Path.Combine(fontsDir, $"{file}-{v}.ttf")));

byte[] docx = File.ReadAllBytes(docxPath);
var sw = Stopwatch.StartNew();
var pkg = DocxPackage.Load(docx);
var full = FullDocReader.ReadFullDocument(pkg);
double read = sw.Elapsed.TotalMilliseconds;

LayoutResult Layout()
{
    var embedded = EmbeddedFonts.ReadEmbeddedFonts(pkg).Select(e => new EmbeddedFace { Family = e.Family, Variant = e.Variant, Data = e.Data }).ToList();
    var provider = UniversalOffice.DocxLayout.Fonts.WithEmbeddedFonts(new SubstituteFontProvider(fonts), embedded, b => new SfntFont(b));
    return LayoutEngine.LayoutDocument(full, provider);
}

sw.Restart();
var layout = Layout();
double first = sw.Elapsed.TotalMilliseconds;
var warm = new List<double>();
for (int k = 0; k < 10; k++) { sw.Restart(); layout = Layout(); warm.Add(sw.Elapsed.TotalMilliseconds); }
sw.Restart();
var pdf = PdfRenderer.RenderPdf(layout, null, PdfNavigation.From(full, layout));
double pdfMs = sw.Elapsed.TotalMilliseconds;
Console.WriteLine($"{{\"build\":\"native-jit\",\"readMs\":{read:0.0},\"firstLayoutMs\":{first:0.0},\"pages\":{layout.Pages.Count},\"warmBestMs\":{warm.Min():0.0},\"warmMedianMs\":{warm.Order().ElementAt(5):0.0},\"pdfMs\":{pdfMs:0.0},\"pdfKB\":{pdf.Length / 1024}}}");
