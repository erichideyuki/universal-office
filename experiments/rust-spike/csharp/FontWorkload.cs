using UniversalOffice.Fonts;

namespace FontSpike;

/// <summary>A carga do spike sobre o SfntFont de verdade do UniversalOffice.Core: a mesma do
/// bench-ts.mjs e do Rust (parse das fontes; medição do texto com kerning).</summary>
public static class FontWorkload
{
    public static double Measure(SfntFont font, int[] text, double sizePt)
    {
        double total = 0;
        long prev = -1;
        foreach (int cp in text)
        {
            int g = font.GlyphId(cp);
            total += font.AdvanceTwips(cp, sizePt);
            if (prev >= 0) total += font.KernGlyphsTwips(prev, g, sizePt);
            prev = g;
        }
        return total;
    }

    public static SfntFont[] ParseAll(IReadOnlyList<byte[]> bytes, int iters)
    {
        SfntFont[] fonts = [];
        for (int i = 0; i < iters; i++) fonts = bytes.Select(b => new SfntFont((byte[])b.Clone())).ToArray();
        return fonts;
    }

    public static double MeasureAll(SfntFont[] fonts, int[] text, double sizePt, int iters)
    {
        double total = 0;
        for (int i = 0; i < iters; i++)
        {
            double s = 0;
            foreach (var f in fonts) s += Measure(f, text, sizePt);
            if (i == 0) total = s;
        }
        return total;
    }

    public static int[] CodePoints(string s)
    {
        var list = new List<int>(s.Length);
        for (int i = 0; i < s.Length; i += char.IsSurrogatePair(s, i) ? 2 : 1) list.Add(char.ConvertToUtf32(s, i));
        return [.. list];
    }
}
