using System.Text.Json;
using System.Text.Json.Serialization;
using UniversalOffice.Docx;
using UniversalOffice.DocxLayout;

namespace WasmPoc;

/// <summary>JSON da árvore gerado em tempo de compilação (sem reflexão): o trimmer pode podar
/// tudo, e o formato é o mesmo do tree.ts (campos, camelCase, nulos omitidos).</summary>
[JsonSourceGenerationOptions(
    IncludeFields = true,
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull,
    NumberHandling = JsonNumberHandling.AllowNamedFloatingPointLiterals,
    Converters = new[] { typeof(FaceIdConverter), typeof(ImageIdConverter), typeof(GlyphPosConverter) })]
[JsonSerializable(typeof(GlyphRunItem))]
[JsonSerializable(typeof(RectItem))]
[JsonSerializable(typeof(LineItem))]
[JsonSerializable(typeof(ImageItem))]
[JsonSerializable(typeof(ShapeItem))]
[JsonSerializable(typeof(PathItem))]
[JsonSerializable(typeof(ClipItem))]
[JsonSerializable(typeof(XformItem))]
[JsonSerializable(typeof(LaidOutLine))]
internal partial class LayoutJsonContext : JsonSerializerContext;

/// <summary>Face → id (os bytes saem por Engine.FaceBytes).</summary>
internal sealed class FaceIdConverter : JsonConverter<FontFace>
{
    public override FontFace Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o) => throw new NotSupportedException();
    public override void Write(Utf8JsonWriter w, FontFace f, JsonSerializerOptions o) => w.WriteNumberValue(Engine.FaceId(f));
}

/// <summary>Imagem → id (os bytes saem por Engine.ImageBytes).</summary>
internal sealed class ImageIdConverter : JsonConverter<InlineImage>
{
    public override InlineImage Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o) => throw new NotSupportedException();
    public override void Write(Utf8JsonWriter w, InlineImage img, JsonSerializerOptions o) => w.WriteNumberValue(Engine.ImageId(img));
}

/// <summary>GlyphPos é record struct com propriedades: { cp, x, gid? } como no tree.ts.</summary>
internal sealed class GlyphPosConverter : JsonConverter<GlyphPos>
{
    public override GlyphPos Read(ref Utf8JsonReader r, Type t, JsonSerializerOptions o) => throw new NotSupportedException();
    public override void Write(Utf8JsonWriter w, GlyphPos g, JsonSerializerOptions o)
    {
        w.WriteStartObject();
        w.WriteNumber("cp", g.Cp);
        w.WriteNumber("x", g.X);
        if (g.Gid is int gid) w.WriteNumber("gid", gid);
        w.WriteEndObject();
    }
}
