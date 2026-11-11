// The D3 differential harness through the .NET facade (record core/docs/modules/uo-docx-d3.md, sections 4 and 5).
//
//   corpus-check <expected.capture> <root> <list> <out.capture> [--dump <dir>]
//        runs the 18 scripts (Scripts.cs) over every document of <list> (paths relative to <root>) through the
//        facade and compares line by line with what the frozen C# captured
//   semantics-check <expected.capture> <tags.txt> <out.capture>
//        the .NET-semantics sites that go through public API or pure .NET, compared with the capture
//   api-dump <frozen UniversalOffice.Docx.dll>
//        the public API of the frozen 0.9 assembly against this facade's
//
// Build and run (output outside the repository):
//   dotnet run -c Release --project core/dotnet/UniversalOffice.Docx.Parity \
//     --artifacts-path core/target/m2-d3abi/dotnet -- corpus-check ...
using System.Diagnostics;
using System.Reflection;
using System.Runtime.Loader;
using System.Text;
using System.Text.RegularExpressions;

namespace UoD3Harness.Facade;

internal static class ParityMain
{
    private static int Main(string[] args)
    {
        if (args.Length == 0)
        {
            Console.Error.WriteLine("modes: corpus-check | semantics-check | api-dump");
            return 2;
        }
        try
        {
            return args[0] switch
            {
                "corpus-check" => CorpusCheck(args),
                "semantics-check" => SemanticsCheck(args),
                "api-dump" => ApiDump(args[1]),
                _ => Fail("unknown mode " + args[0]),
            };
        }
        catch (Exception e)
        {
            Console.Error.WriteLine(e);
            return 3;
        }
    }

    private static int Fail(string message)
    {
        Console.Error.WriteLine(message);
        return 2;
    }

    /// <summary>The records of a capture: key to the raw facets (tab-separated tail), meta and comment lines apart.</summary>
    private static (Dictionary<string, string> Records, List<string> Order, List<string> Meta) Read(string path)
    {
        var records = new Dictionary<string, string>(StringComparer.Ordinal);
        var order = new List<string>();
        var meta = new List<string>();
        foreach (var line in File.ReadLines(path))
        {
            if (line.Length == 0 || line[0] == '#') continue;
            if (line[0] == '@')
            {
                if (!line.StartsWith("@platform", StringComparison.Ordinal)) meta.Add(line);
                continue;
            }
            int tab = line.IndexOf('\t');
            string key = tab < 0 ? line : line[..tab];
            if (records.TryAdd(key, tab < 0 ? "" : line[(tab + 1)..])) order.Add(key);
        }
        return (records, order, meta);
    }

    private static int CorpusCheck(string[] args)
    {
        string expectedPath = args[1], root = args[2], list = args[3], outPath = args[4];
        string? dump = null;
        for (int i = 5; i + 1 < args.Length; i++) if (args[i] == "--dump") dump = args[i + 1];
        var sw = Stopwatch.StartNew();
        Scripts.RunCorpus(outPath, root, list, null);
        long runMs = sw.ElapsedMilliseconds;
        var (want, order, wantMeta) = Read(expectedPath);
        var (got, gotOrder, gotMeta) = Read(outPath);
        int records = 0, facets = 0, equal = 0;
        var bad = new List<string>();
        foreach (var key in order)
        {
            records++;
            if (!got.TryGetValue(key, out var g)) { bad.Add($"{key}: missing"); continue; }
            var wf = want[key].Split('\t');
            var gf = g.Split('\t');
            facets += wf.Length;
            bool same = true;
            foreach (var f in wf)
            {
                if (gf.Contains(f, StringComparer.Ordinal)) equal++;
                else { same = false; bad.Add($"{key}: expected {Cut(f)}  got {Cut(string.Join(" ", gf.Where(x => x[..Math.Min(x.IndexOf('='), x.Length)] == f[..Math.Min(f.IndexOf('='), f.Length)])))}"); }
            }
            if (gf.Length != wf.Length && same) bad.Add($"{key}: facet count {wf.Length} vs {gf.Length}");
        }
        foreach (var k in gotOrder) if (!want.ContainsKey(k)) bad.Add($"{k}: unexpected record");
        foreach (var (a, b) in wantMeta.Zip(gotMeta)) if (a != b) bad.Add($"meta differs: {a} vs {b}");
        int mismatches = bad.Count;
        Console.WriteLine($"records {records}, facets equal {equal} of {facets}, mismatches {mismatches}, run {runMs / 1000.0:F1} s, total {sw.ElapsedMilliseconds / 1000.0:F1} s");
        foreach (var b in bad.Take(20)) Console.WriteLine("  " + b);
        if (mismatches > 0 && dump is not null)
        {
            Console.WriteLine($"re-running with --dump {dump}");
            Scripts.RunCorpus(outPath + ".dump", root, list, dump);
        }
        return mismatches == 0 ? 0 : 1;
    }

    private static string Cut(string s) => s.Length <= 120 ? s : s[..120] + "…";

    // the sites that go through public API or pure .NET; the others reach internals by reflection into the frozen assembly
    private static readonly HashSet<string> Public = ["uri", "tag", "tagfmt", "hyperlink", "image", "fit", "unescape", "escape", "ints", "trim", "ignorecase", "fmt"];

    private static int SemanticsCheck(string[] args)
    {
        string expectedPath = args[1], tags = args[2], outPath = args[3];
        var sw = Stopwatch.StartNew();
        Semantics.Run(outPath, tags);
        var (want, order, _) = Read(expectedPath);
        var (got, _, _) = Read(outPath);
        var perSite = new SortedDictionary<string, (int Checked, int Mismatch, int Accepted)>(StringComparer.Ordinal);
        int skipped = 0, json5 = 0;
        var skippedSites = new SortedDictionary<string, int>(StringComparer.Ordinal);
        var bad = new List<string>();
        foreach (var key in order)
        {
            string site = key[..key.IndexOf(':')];
            if (!Public.Contains(site)) { skipped++; skippedSites[site] = skippedSites.GetValueOrDefault(site) + 1; continue; }
            var s = perSite.GetValueOrDefault(site);
            string w = want[key];
            // A1: a lone surrogate in the input cannot cross to Rust as UTF-8 (it becomes U+FFFD): accepted
            if (HasLoneSurrogate(Facet(w, "in"))) { perSite[site] = (s.Checked + 1, s.Mismatch, s.Accepted + 1); continue; }
            got.TryGetValue(key, out var g);
            bool same = g == w;
            // D3A-5: the 0.9 C# threw the internal JsonReaderException; the facade throws the public JsonException (its base class)
            if (!same && g is not null && g.Replace("!JsonException", "!JsonReaderException") == w) { perSite[site] = (s.Checked + 1, s.Mismatch, s.Accepted + 1); json5++; continue; }
            perSite[site] = (s.Checked + 1, s.Mismatch + (same ? 0 : 1), s.Accepted);
            if (!same) bad.Add($"{key}: expected {Cut(w)}  got {Cut(g ?? "<missing>")}");
        }
        foreach (var (site, s) in perSite) Console.WriteLine($"{site}: {s.Checked} cases, {s.Mismatch} mismatches, {s.Accepted} accepted (lone surrogate A1, or JsonReaderException as JsonException D3A-5)");
        Console.WriteLine($"skipped {skipped} reflection-site records ({string.Join(", ", skippedSites.Select(kv => kv.Key + " " + kv.Value))})");
        Console.WriteLine($"semantics-check: {perSite.Values.Sum(s => s.Checked)} checked, {perSite.Values.Sum(s => s.Accepted)} accepted ({json5} by D3A-5), {bad.Count} mismatches, {sw.ElapsedMilliseconds / 1000.0:F1} s");
        foreach (var b in bad.Take(20)) Console.WriteLine("  " + b);
        return bad.Count == 0 ? 0 : 1;
    }

    private static string Facet(string facets, string name)
    {
        foreach (var f in facets.Split('\t')) if (f.StartsWith(name + "=", StringComparison.Ordinal)) return f[(name.Length + 1)..];
        return "";
    }

    /// <summary>The capture's escape back to UTF-16 units: is there a lone surrogate?</summary>
    private static bool HasLoneSurrogate(string escaped)
    {
        var units = new List<char>();
        for (int i = 0; i < escaped.Length; i++)
        {
            char c = escaped[i];
            if (c != '\\' || i + 1 >= escaped.Length) { units.Add(c); continue; }
            char d = escaped[++i];
            switch (d)
            {
                case 'n': units.Add('\n'); break;
                case 'r': units.Add('\r'); break;
                case 't': units.Add('\t'); break;
                case 'u' when i + 4 < escaped.Length + 0 && int.TryParse(escaped.AsSpan(i + 1, 4), System.Globalization.NumberStyles.HexNumber, null, out int u):
                    units.Add((char)u);
                    i += 4;
                    break;
                default: units.Add(d); break;
            }
        }
        for (int i = 0; i < units.Count; i++)
        {
            if (char.IsHighSurrogate(units[i]) && i + 1 < units.Count && char.IsLowSurrogate(units[i + 1])) { i++; continue; }
            if (char.IsSurrogate(units[i])) return true;
        }
        return false;
    }

    // ───────────────────────────── api-dump ─────────────────────────────

    private static readonly Regex AssemblyInfo = new(@", [A-Za-z0-9_.]+, Version=[0-9.]+, Culture=[a-z]+, PublicKeyToken=[A-Za-z0-9]+", RegexOptions.CultureInvariant);

    // generic arguments carry the assembly version: drop it, so 0.9.3 and 0.10.0 spell the same type the same way
    private static string Simple(Type t) => AssemblyInfo.Replace(t.FullName ?? t.Name, "");

    private static string Sig(MemberInfo m) => m switch
    {
        MethodBase mb => (mb is MethodInfo mi ? Simple(mi.ReturnType) + " " : "") + m.Name + "(" + string.Join(", ", mb.GetParameters().Select(p => Simple(p.ParameterType) + " " + p.Name + (p.HasDefaultValue ? "=" + (p.DefaultValue ?? "null") : ""))) + ")" + (mb.IsStatic ? " static" : ""),
        PropertyInfo p => Simple(p.PropertyType) + " " + p.Name + " {" + (p.GetMethod?.IsPublic == true ? " get;" : "") + (p.SetMethod?.IsPublic == true ? " set;" : "") + " }" + ((p.GetMethod ?? p.SetMethod)?.IsStatic == true ? " static" : ""),
        FieldInfo f => Simple(f.FieldType) + " " + f.Name + (f.IsStatic ? " static" : "") + (f.IsInitOnly ? " readonly" : ""),
        _ => m.Name,
    };

    private static SortedDictionary<string, SortedSet<string>> Surface(Assembly asm, Func<Type, bool> include)
    {
        var d = new SortedDictionary<string, SortedSet<string>>(StringComparer.Ordinal);
        foreach (var t in asm.GetExportedTypes().Where(include))
        {
            string kind = (t.IsEnum ? "enum" : t.IsInterface ? "interface" : t.IsValueType ? "struct" : t.IsAbstract && t.IsSealed ? "static class" : t.IsSealed ? "sealed class" : "class");
            var members = new SortedSet<string>(StringComparer.Ordinal) { "[" + kind + "]" + (t.BaseType is { } b && b != typeof(object) && b != typeof(ValueType) && b != typeof(Enum) ? " : " + Simple(b) : "") };
            foreach (var m in t.GetMembers(BindingFlags.Public | BindingFlags.Instance | BindingFlags.Static | BindingFlags.DeclaredOnly))
            {
                if (m is MethodBase mb && (mb.IsSpecialName && m is MethodInfo) ) continue;
                if (m is MethodInfo mi && (mi.Name is "<Clone>$" or "PrintMembers" or "Deconstruct" or "Equals" or "GetHashCode" or "ToString" or "GetType")) continue;
                if (m is Type) continue;
                members.Add(Sig(m));
            }
            d[t.FullName ?? t.Name] = members;
        }
        return d;
    }

    private static int ApiDump(string frozenPath)
    {
        var ctx = new AssemblyLoadContext("frozen", isCollectible: false);
        string dir = Path.GetDirectoryName(Path.GetFullPath(frozenPath))!;
        ctx.Resolving += (c, name) => File.Exists(Path.Combine(dir, name.Name + ".dll")) ? c.LoadFromAssemblyPath(Path.Combine(dir, name.Name + ".dll")) : null;
        var frozenDocx = ctx.LoadFromAssemblyPath(Path.GetFullPath(frozenPath));
        var frozenCore = ctx.LoadFromAssemblyPath(Path.Combine(dir, "UniversalOffice.Core.dll"));
        var facade = typeof(UniversalOffice.Docx.Editing.DocxDocument).Assembly;
        // the Editing API (the namespace and CustomInlineTag) and the four Core types it throws, takes or that the host validator calls (OpcPartName); the rest of the
        // 0.9 assembly (the C# document model and layout reader) is not part of the Editing API
        bool InScope(Type t) => t.Namespace == "UniversalOffice.Docx.Editing"
            || (t.Namespace == "UniversalOffice.Docx" && t.Name == "CustomInlineTag")
            || (t.Namespace == "UniversalOffice.Core" && t.Name is "OfficeLimit" or "OfficeLimitExceededException" or "OpcPackageLimits" or "OpcPartName");
        var old = Surface(frozenDocx, InScope);
        foreach (var (k, v) in Surface(frozenCore, InScope)) old[k] = v;
        var now = Surface(facade, InScope);
        int diffs = 0;
        foreach (var (type, members) in old)
        {
            if (!now.TryGetValue(type, out var nm)) { Console.WriteLine($"REMOVED type {type}"); diffs++; continue; }
            foreach (var m in members.Except(nm)) { Console.WriteLine($"REMOVED/CHANGED {type}: {m}"); diffs++; }
            foreach (var m in nm.Except(members)) { Console.WriteLine($"ADDED {type}: {m}"); diffs++; }
        }
        foreach (var type in now.Keys.Except(old.Keys)) { Console.WriteLine($"ADDED type {type}"); diffs++; }
        // everything else public in the two frozen assemblies is listed, so the scope is a visible choice
        var outOfScope = frozenDocx.GetExportedTypes().Concat(frozenCore.GetExportedTypes()).Where(t => !InScope(t)).Select(t => t.FullName ?? t.Name).OrderBy(n => n, StringComparer.Ordinal).ToList();
        Console.WriteLine($"api-dump: out of scope: {outOfScope.Count} public types of the frozen Docx and Core assemblies (the C# document model, layout reader, PDF writer, exporters), by namespace: " + string.Join(", ", outOfScope.GroupBy(n => n[..n.LastIndexOf('.')]).Select(g => $"{g.Key} {g.Count()}")) + ". Core types in that set: " + string.Join(", ", outOfScope.Where(n => n.StartsWith("UniversalOffice.Core.", StringComparison.Ordinal))));
        Console.WriteLine($"api-dump: {old.Count} frozen public types in scope, {now.Count} in the facade, {diffs} differences");
        return 0;
    }
}
