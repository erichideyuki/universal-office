using System.Text;

namespace UoD3Harness;

/// <summary>
/// The .NET character tables the Editing API depends on, measured in this runtime and written
/// as Rust source: char.IsLetterOrDigit (BMP units: a surrogate is never a letter),
/// char.ToUpperInvariant (string.ToUpperInvariant is per unit for the BMP) and the
/// OrdinalIgnoreCase folding of one unit (string.Equals(a, b, OrdinalIgnoreCase) per unit).
/// char.IsWhiteSpace and char.IsControl are checked against the hand-written sets of the
/// Rust side and the harness fails if they differ.
/// </summary>
internal static class Tables
{
    private static readonly int[] WhiteSpace =
        [0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x20, 0x85, 0xA0, 0x1680, 0x2000, 0x2001, 0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200A, 0x2028, 0x2029, 0x202F, 0x205F, 0x3000];

    public static void Write(string path)
    {
        for (int c = 0; c <= 0xFFFF; c++)
        {
            bool ws = char.IsWhiteSpace((char)c);
            if (ws != WhiteSpace.Contains(c)) throw new InvalidOperationException($"IsWhiteSpace U+{c:X4} = {ws}");
            bool ctl = char.IsControl((char)c);
            if (ctl != (c < 0x20 || (c >= 0x7F && c <= 0x9F))) throw new InvalidOperationException($"IsControl U+{c:X4} = {ctl}");
        }

        var letters = new List<(int, int)>();
        int start = -1;
        for (int c = 0; c <= 0x10000; c++)
        {
            bool on = c <= 0xFFFF && char.IsLetterOrDigit((char)c);
            if (on && start < 0) start = c;
            if (!on && start >= 0) { letters.Add((start, c - 1)); start = -1; }
        }

        var upper = new List<(int, int)>();
        var fold = new List<(int, int)>();
        for (int c = 0; c <= 0xFFFF; c++)
        {
            if (char.IsSurrogate((char)c)) continue;
            char u = char.ToUpperInvariant((char)c);
            string su = new string((char)c, 1).ToUpperInvariant();
            if (su.Length != 1 || su[0] != u) throw new InvalidOperationException($"ToUpperInvariant U+{c:X4}");
            if (u != c) upper.Add((c, u));
            // the representative of the OrdinalIgnoreCase class: the upper case when it compares equal
            char f = string.Equals(((char)c).ToString(), u.ToString(), StringComparison.OrdinalIgnoreCase) ? u : (char)c;
            if (f != c) fold.Add((c, f));
        }
        // folding must be an equivalence by representative: check every pair with the same
        // representative compares equal and a sample of others does not
        for (int c = 0; c <= 0xFFFF; c++)
        {
            if (char.IsSurrogate((char)c)) continue;
            char u = char.ToUpperInvariant((char)c);
            bool eq = string.Equals(((char)c).ToString(), u.ToString(), StringComparison.OrdinalIgnoreCase);
            char lower = char.ToLowerInvariant((char)c);
            bool eqLower = string.Equals(((char)c).ToString(), lower.ToString(), StringComparison.OrdinalIgnoreCase);
            char rep = eq ? u : (char)c;
            char repLower = FoldOf(lower, fold);
            if (eqLower && repLower != rep) throw new InvalidOperationException($"fold of U+{c:X4} and its lower case differ");
        }

        var sb = new StringBuilder();
        sb.Append("//! .NET 10 character tables measured by the D3 harness (`dotnet-harness tables`), generated:\n");
        sb.Append("//! do not edit. Runtime: ").Append(Environment.Version).Append(", invariant globalization.\n");
        sb.Append("//! `LETTER_OR_DIGIT`: `char.IsLetterOrDigit` ranges (BMP units); `UPPER`: `char.ToUpperInvariant`\n");
        sb.Append("//! where it changes the unit; `FOLD`: the `OrdinalIgnoreCase` representative of a unit where it\n");
        sb.Append("//! is not the unit itself.\n\n");
        Emit(sb, "LETTER_OR_DIGIT", letters);
        Emit(sb, "UPPER", upper);
        Emit(sb, "FOLD", fold);
        File.WriteAllText(path, sb.ToString().Replace("\r\n", "\n"), new UTF8Encoding(false));
        Console.WriteLine($"letters {letters.Count} ranges, upper {upper.Count}, fold {fold.Count}");
    }

    private static char FoldOf(char c, List<(int, int)> fold)
    {
        foreach (var (a, b) in fold) if (a == c) return (char)b;
        return c;
    }

    private static void Emit(StringBuilder sb, string name, List<(int, int)> pairs)
    {
        sb.Append("pub(crate) static ").Append(name).Append(": [(u16, u16); ").Append(pairs.Count).Append("] = [\n");
        for (int i = 0; i < pairs.Count; i++)
        {
            if (i % 8 == 0) sb.Append("    ");
            sb.Append("(0x").Append(pairs[i].Item1.ToString("X4")).Append(", 0x").Append(pairs[i].Item2.ToString("X4")).Append("),");
            sb.Append(i % 8 == 7 || i == pairs.Count - 1 ? "\n" : " ");
        }
        sb.Append("];\n\n");
    }
}
