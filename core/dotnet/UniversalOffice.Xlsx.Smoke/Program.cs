using UniversalOffice.Xlsx;

// usage: Smoke <in.xlsx|in.xls>
//        Smoke set <in> <out.xlsx> <sheet-index> <A1> <text> [en-US|pt-BR]
//        Smoke convert <in.xls> <out.xlsx>
if (args.Length == 3 && args[0] == "convert")
{
    File.WriteAllBytes(args[2], XlsConverter.ToXlsx(File.ReadAllBytes(args[1])));
    Console.WriteLine($"{args[1]} -> {args[2]}");
    return 0;
}

if (args.Length is 6 or 7 && args[0] == "set")
{
    using var wb = XlsxWorkbook.Open(File.ReadAllBytes(args[1]), new XlsxOpenOptions { Language = args.Length == 7 ? args[6] : "pt-BR", Trusted = true });
    var r = wb.SetCellText(int.Parse(args[3]), args[4], args[5]);
    Console.WriteLine(r.Accepted ? "accepted" : $"refused: {r.Message}");
    File.WriteAllBytes(args[2], wb.Save());
    Console.WriteLine($"saved {args[2]}");
    return r.Accepted ? 0 : 1;
}

if (args.Length != 1)
{
    Console.Error.WriteLine("usage: Smoke <in.xlsx|in.xls> | set <in> <out.xlsx> <sheet> <A1> <text> [lang] | convert <in.xls> <out.xlsx>");
    return 2;
}

using var w = XlsxWorkbook.Open(File.ReadAllBytes(args[0]), new XlsxOpenOptions { Trusted = true });
Console.WriteLine($"{(w.IsLegacyXls ? ".xls" : ".xlsx")}, {w.Sheets.Count} sheets");
foreach (var s in w.Sheets)
{
    Console.WriteLine($"- {s.Index} {s.Name} [{s.State}] {s.UsedRows} rows x {s.UsedColumns} columns, {s.MergedRanges.Count} merges");
    foreach (var c in w.ReadSheet(s.Index, XlsxReadOptions.Formulas).Take(5))
        Console.WriteLine($"    {c.Reference} = {c.Display}{(c.HasFormula ? $"   {c.Formula}" : "")}");
}
return 0;
