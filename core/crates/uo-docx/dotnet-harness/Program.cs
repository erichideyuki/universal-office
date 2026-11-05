// The C# differential harness of uo-docx D3 (record core/docs/modules/uo-docx-d3.md,
// "Parity harness"). It runs the FROZEN C# Editing API (dotnet/src, identical to main) and
// writes what the Rust tests replay:
//
//   tables    <out.rs>                     .NET 10 character tables (IsLetterOrDigit,
//                                          ToUpperInvariant, OrdinalIgnoreCase folding) as the
//                                          generated Rust source src/api/net_tables.rs
//   fixtures  <dir>                        the synthetic documents (frozen DocxBuilder and
//                                          SampleTemplate) into tests/parity/d3/fixtures
//   semantics <out.capture> <tags.txt>     unit-level differential of the .NET semantics
//                                          (Uri, Uri escapes, CustomInlineTag, the regex sites,
//                                          number parsing/formatting, ImageInfo, …), internals
//                                          reached by reflection
//   corpus    <out.capture> <root> <list>  the scripts (Scripts.cs) over every document named
//                                          in <list> (paths relative to <root>, one per line);
//                                          `--dump <dir>` writes every saved part for diffing
//
// Build and run (output outside the repository):
//   dotnet run -c Release --project core/crates/uo-docx/dotnet-harness \
//     --artifacts-path core/target/agent-d3/dotnet -- <mode> <args>
using System.Reflection;
using System.Security.Cryptography;
using System.Text;

namespace UoD3Harness;

internal static class Program
{
    private static int Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.Error.WriteLine("modes: tables | fixtures | semantics | corpus");
            return 2;
        }
        switch (args[0])
        {
            case "tables": Tables.Write(args[1]); return 0;
            case "fixtures": Fixtures.Write(args[1]); return 0;
            case "semantics": Semantics.Run(args[1], args[2]); return 0;
            case "corpus":
                string? dump = null;
                for (int i = 4; i + 1 < args.Length; i++) if (args[i] == "--dump") dump = args[i + 1];
                Scripts.RunCorpus(args[1], args[2], args[3], dump);
                return 0;
            default:
                Console.Error.WriteLine("unknown mode " + args[0]);
                return 2;
        }
    }
}

/// <summary>Capture writing: the uo-testkit format (key TAB facet=value…).</summary>
internal sealed class CaptureWriter : IDisposable
{
    private readonly StreamWriter _w;

    public CaptureWriter(string path, string comment)
    {
        _w = new StreamWriter(path, false, new UTF8Encoding(false)) { NewLine = "\n" };
        _w.WriteLine("# " + comment);
        _w.WriteLine($"@platform dotnet={Environment.Version} os={Environment.OSVersion.Platform} arch={System.Runtime.InteropServices.RuntimeInformation.ProcessArchitecture} invariant={AppContext.TryGetSwitch("System.Globalization.Invariant", out var inv) && inv}");
    }

    public void Meta(string name, string value) => _w.WriteLine("@" + name + " " + value);

    public void Record(string key, params (string Facet, string Value)[] facets)
    {
        var sb = new StringBuilder(Esc(key));
        foreach (var (f, v) in facets) sb.Append('\t').Append(f).Append('=').Append(v);
        _w.WriteLine(sb.ToString());
    }

    public void Dispose() => _w.Dispose();

    /// <summary>Escapes a value for one capture field: backslash, TAB, LF, CR and other controls.</summary>
    public static string Esc(string s)
    {
        var sb = new StringBuilder(s.Length);
        foreach (char c in s)
        {
            switch (c)
            {
                case '\\': sb.Append("\\\\"); break;
                case '\t': sb.Append("\\t"); break;
                case '\n': sb.Append("\\n"); break;
                case '\r': sb.Append("\\r"); break;
                default:
                    if (c < 0x20 || c == 0x7F || char.IsSurrogate(c)) sb.Append("\\u").Append(((int)c).ToString("X4"));
                    else sb.Append(c);
                    break;
            }
        }
        return sb.ToString();
    }

    /// <summary>SHA-256 of the bytes, the first 16 bytes in lowercase hex (uo-testkit `Digest::short`).</summary>
    public static string Digest(byte[] data) => Convert.ToHexStringLower(SHA256.HashData(data)[..16]);

    public static string Digest(string s) => Digest(Encoding.UTF8.GetBytes(s));

    /// <summary>A result value: `=` + the escaped text when short, else `#` + its digest.</summary>
    public static string Ret(string s)
    {
        string e = Esc(s);
        return e.Length <= 96 ? "=" + e : "#" + Digest(s);
    }
}

/// <summary>Reflection into the frozen assembly (internal helpers compared one by one).</summary>
internal static class Frozen
{
    public static readonly Assembly Docx = typeof(UniversalOffice.Docx.Editing.DocxDocument).Assembly;

    public static object? Call(string type, string method, params object?[] args)
    {
        var t = Docx.GetType(type, throwOnError: true)!;
        var m = t.GetMethods(BindingFlags.Static | BindingFlags.Public | BindingFlags.NonPublic)
            .First(x => x.Name == method && x.GetParameters().Length == args.Length);
        try
        {
            return m.Invoke(null, args);
        }
        catch (TargetInvocationException e) when (e.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(e.InnerException).Throw();
            throw;
        }
    }

    public static object? CallInstance(object target, string method, params object?[] args)
    {
        var m = target.GetType().GetMethods(BindingFlags.Instance | BindingFlags.Public | BindingFlags.NonPublic)
            .First(x => x.Name == method && x.GetParameters().Length == args.Length);
        try
        {
            return m.Invoke(target, args);
        }
        catch (TargetInvocationException e) when (e.InnerException is not null)
        {
            System.Runtime.ExceptionServices.ExceptionDispatchInfo.Capture(e.InnerException).Throw();
            throw;
        }
    }
}
