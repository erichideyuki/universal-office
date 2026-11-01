using UniversalOffice.DocxLayout;
using UniversalOffice.Fonts;

namespace WasmPoc;

/// <summary>Fontes passadas pelo JS (bytes .ttf): no WebAssembly não há pasta de fontes do sistema.
/// Variante ausente cai na regular com negrito/itálico sintético, como o SystemFontIndex.</summary>
public sealed class MemoryFontProvider : IFontProvider
{
    private sealed record Entry(string Family, bool Bold, bool Italic, byte[] Bytes)
    {
        public SfntFont? Font;
    }

    private readonly Dictionary<string, List<Entry>> _families = new(StringComparer.OrdinalIgnoreCase);
    private readonly Dictionary<(string, bool, bool), FontFace?> _faces = new();

    public void Add(string family, bool bold, bool italic, byte[] bytes)
    {
        if (!_families.TryGetValue(family, out var list)) _families[family] = list = [];
        list.Add(new Entry(family, bold, italic, bytes));
        _faces.Clear();
    }

    public FontFace? Face(string family, bool bold, bool italic)
    {
        var key = (family.ToLowerInvariant(), bold, italic);
        if (_faces.TryGetValue(key, out var cached)) return cached;
        FontFace? face = null;
        if (_families.TryGetValue(family, out var list) && list.Count > 0)
        {
            var best = list.MinBy(e => (e.Bold == bold ? 0 : 4) + (e.Italic == italic ? 0 : 2))!;
            best.Font ??= new SfntFont(best.Bytes);
            face = new FontFace
            {
                Key = $"{best.Family}|{(best.Bold ? "b" : "")}{(best.Italic ? "i" : "")}",
                Family = best.Family,
                Font = best.Font,
                Bytes = best.Bytes,
                Bold = bold,
                Italic = italic,
                SynthBold = bold && !best.Bold,
                SynthItalic = italic && !best.Italic,
            };
        }
        _faces[key] = face;
        return face;
    }
}
