// Strict-mode oracle: the reading of the C# Editing API (DocxDocument.Parse: XmlReader with DTD
// prohibited, depth scan with DocxLimits.MaxXmlDepth = 256, then XDocument with whitespace kept)
// over the inputs of strict_inputs.txt (one JSON string per line, from gen_strict_inputs.mjs).
//
//   dotnet run strict_oracle.cs -- strict_inputs.txt strict_oracle.txt
//
// Output line: JSON(input) \t dump | ERR | DEPTH
#:property PublishAot=false
using System.Text;
using System.Text.Json;
using System.Xml;
using System.Xml.Linq;

var inputs = File.ReadAllLines(args[0]).Where(l => l.Length > 0).Select(l => JsonSerializer.Deserialize<string>(l)!).ToList();
var settings = new XmlReaderSettings
{
    DtdProcessing = DtdProcessing.Prohibit,
    XmlResolver = null,
    IgnoreWhitespace = false,
    IgnoreComments = false,
    IgnoreProcessingInstructions = false,
};
var outLines = new List<string>();
foreach (var input in inputs)
{
    string result;
    try
    {
        var bytes = new UTF8Encoding(false).GetBytes(input);
        bool deep = false;
        using (var scan = XmlReader.Create(new MemoryStream(bytes), settings))
            while (scan.Read())
                if (scan.Depth > 256) { deep = true; break; }
        if (deep) result = "DEPTH";
        else
        {
            using var reader = XmlReader.Create(new MemoryStream(bytes), settings);
            var doc = XDocument.Load(reader, LoadOptions.PreserveWhitespace);
            result = Dump(doc.Root!);
        }
    }
    catch (XmlException) { result = "ERR"; }
    outLines.Add(Q(input) + "\t" + result);
}
File.WriteAllText(args[1], string.Join("\n", outLines) + "\n", new UTF8Encoding(false));
Console.WriteLine($"{outLines.Count} lines");

static string Dump(XElement e)
{
    var sb = new StringBuilder();
    sb.Append('{').Append(N(e.Name.NamespaceName)).Append('}').Append(e.Name.LocalName).Append('[');
    sb.Append(string.Join(",", e.Attributes().Where(a => !a.IsNamespaceDeclaration)
        .Select(a => "{" + N(a.Name.NamespaceName) + "}" + a.Name.LocalName + "=" + Q(a.Value))));
    sb.Append("]{");
    sb.Append(string.Join(",", e.Nodes().Select(n => n switch
    {
        XElement c => Dump(c),
        XText t => Q(t.Value),
        XComment c => "!" + Q(c.Value),
        XProcessingInstruction p => "?" + p.Target,
        _ => "#",
    })));
    sb.Append('}');
    return sb.ToString();
}

static string N(string ns) => Q(ns)[1..^1];

static string Q(string s)
{
    var sb = new StringBuilder("\"");
    foreach (var c in s)
    {
        switch (c)
        {
            case '"': sb.Append("\\\""); break;
            case '\\': sb.Append("\\\\"); break;
            case '\b': sb.Append("\\b"); break;
            case '\f': sb.Append("\\f"); break;
            case '\n': sb.Append("\\n"); break;
            case '\r': sb.Append("\\r"); break;
            case '\t': sb.Append("\\t"); break;
            default:
                if (c < 0x20) sb.Append("\\u").Append(((int)c).ToString("x4"));
                else sb.Append(c);
                break;
        }
    }
    return sb.Append('"').ToString();
}
