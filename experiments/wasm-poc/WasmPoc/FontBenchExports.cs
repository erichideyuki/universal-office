using System.Diagnostics;
using System.Runtime.InteropServices.JavaScript;
using FontSpike;
using UniversalOffice.Fonts;

namespace WasmPoc;

/// <summary>A carga do spike de Rust (experiments/rust-spike) no SfntFont do C# em WebAssembly:
/// parse das fontes e medição do texto com kerning; os tempos saem do Stopwatch do C#.</summary>
public static partial class FontBench
{
    private static readonly List<byte[]> Bytes = [];
    private static SfntFont[] _fonts = [];
    private static int[] _text = [];

    [JSExport]
    public static void AddFont(byte[] bytes) => Bytes.Add(bytes);

    [JSExport]
    public static void SetText(string text) => _text = FontWorkload.CodePoints(text);

    [JSExport]
    public static double ParseAll(int iters)
    {
        var sw = Stopwatch.StartNew();
        _fonts = FontWorkload.ParseAll(Bytes, iters);
        return sw.Elapsed.TotalMilliseconds / iters;
    }

    [JSExport]
    public static double MeasureAll(double sizePt, int iters)
    {
        var sw = Stopwatch.StartNew();
        FontWorkload.MeasureAll(_fonts, _text, sizePt, iters);
        return sw.Elapsed.TotalMilliseconds / iters;
    }

    [JSExport]
    public static double Total(double sizePt) => FontWorkload.MeasureAll(_fonts, _text, sizePt, 1);
}
