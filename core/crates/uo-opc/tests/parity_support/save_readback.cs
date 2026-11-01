// .NET read-back of saved packages (opt-in part of tests/opc_parity.rs, `save_cross_readers`,
// with UO_DOTNET_PARITY=1): System.IO.Compression.ZipArchive reads every *.zip of a directory
// and prints `<file>\t<count>:<digest>`, the digest being the uo-testkit canonical stream of
// `str(entry name) digest_of(entry bytes)` per entry in central directory order (the encoding of
// core/crates/uo-testkit/src/stream.rs). .NET exposes no flags or methods, so names and bytes only.
//
//   dotnet run save_readback.cs -- <dir>
#:property PublishAot=false
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;

var output = new List<string>();
foreach (var file in Directory.GetFiles(args[0], "*.zip").OrderBy(f => f, StringComparer.Ordinal))
{
    string value;
    try
    {
        using var zip = ZipFile.OpenRead(file);
        using var stream = new MemoryStream();
        foreach (var entry in zip.Entries)
        {
            var name = Encoding.UTF8.GetBytes(entry.FullName);
            stream.Write(BitConverter.GetBytes((uint)name.Length));
            stream.Write(name);
            using var data = new MemoryStream();
            using (var s = entry.Open()) s.CopyTo(data);
            stream.Write(BitConverter.GetBytes((ulong)data.Length));
            stream.Write(SHA256.HashData(data.ToArray()));
        }
        value = $"{zip.Entries.Count}:{Convert.ToHexStringLower(SHA256.HashData(stream.ToArray()))[..32]}";
    }
    catch (Exception e)
    {
        value = $"err:{e.GetType().Name}";
    }
    output.Add($"{Path.GetFileName(file)}\t{value}");
}
Console.Out.Write(string.Join("\n", output) + "\n");
