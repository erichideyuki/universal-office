# Opens every .xlsx of a folder in Excel (COM, invisible, read-only) and prints one TSV line per
# workbook. With DisplayAlerts off, a workbook Excel would repair FAILS Workbooks.Open (measured in
# the calc lab: a stale calcChain), so "open" = no repair prompt. For the sample workbooks of
# tests/support/samples.rs it also reads back what the writer meant (values, formats, styles,
# outline, panes, names, links, properties) and prints them for the record.
#
#   node tools/parallel/word-lock.mjs -- powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File core/crates/uo-xlsx/tools/excel-open-check.ps1 -Dir <folder>
param([Parameter(Mandatory = $true)][string]$Dir)
$ErrorActionPreference = 'Stop'
$xl = New-Object -ComObject Excel.Application
$xl.Visible = $false
$xl.DisplayAlerts = $false
$xl.ScreenUpdating = $false
$xl.EnableEvents = $false
$xl.AutomationSecurity = 3
$failed = 0
try {
  foreach ($f in Get-ChildItem -LiteralPath $Dir -Filter *.xlsx | Sort-Object Name) {
    $path = [string]$f.FullName
    $wb = $null
    try {
      # Open(FileName, UpdateLinks = 0, ReadOnly = true); CorruptLoad stays xlNormalLoad (passing the
      # 15 positional arguments with [Type]::Missing makes every Open fail)
      $wb = $xl.Workbooks.Open($path, 0, $true)
      Write-Output "$($f.Name)`topen`tsheets=$($wb.Worksheets.Count)"
      $out = @()
      try {
      foreach ($ws in $wb.Worksheets) {
        $used = $ws.UsedRange
        $out += "  sheet=$($ws.Name) rtl=$($ws.DisplayRightToLeft) used=$($used.Address($false, $false)) summaryBelow=$($ws.Outline.SummaryRow) printTitles=$($ws.PageSetup.PrintTitleRows) links=$($ws.Hyperlinks.Count)"
      }
      foreach ($ws in $wb.Worksheets) {
        [void]$ws.Activate()
        $win = $wb.Windows.Item(1)
        $out += "  panes $($ws.Name): freeze=$($win.FreezePanes) splitRow=$($win.SplitRow) splitCol=$($win.SplitColumn)"
      }
      foreach ($n in $wb.Names) { $out += "  name=$($n.Name) refersTo=$($n.RefersTo)" }
      $out += "  title=$($wb.Title) author=$($wb.Author)"
      if ($f.Name -like 'features*') {
        $ws = $wb.Worksheets.Item(1)
        foreach ($a in 'A1','B1','A2','B4','D4','E4','F4','A5','B5','A6','B6','A7','B7','B8','C8','D8','E8','A9','B9','A10','C10') {
          $c = $ws.Range($a)
          $out += "  $a value=[$($c.Value2)] text=[$($c.Text)] fmt=[$($c.NumberFormat)] bold=$($c.Font.Bold) italic=$($c.Font.Italic) font=$($c.Font.Color) fill=$($c.Interior.Color)/$($c.Interior.Pattern) h=$($c.HorizontalAlignment) wrap=$($c.WrapText) bTop=$($c.Borders.Item(8).LineStyle)/$($c.Borders.Item(8).Weight) bBottom=$($c.Borders.Item(9).LineStyle)/$($c.Borders.Item(9).Weight) link=$($c.Hyperlinks.Count) merged=$($c.MergeCells)"
        }
        foreach ($r in 1..11) { $out += "  row$r outline=$($ws.Rows.Item($r).OutlineLevel) hidden=$($ws.Rows.Item($r).Hidden) height=$($ws.Rows.Item($r).RowHeight)" }
        foreach ($k in 1..6) { $out += "  col$k width=$($ws.Columns.Item($k).ColumnWidth)" }
        foreach ($h in $ws.Hyperlinks) { $out += "  link $($h.Range.Address($false, $false)) address=[$($h.Address)] sub=[$($h.SubAddress)]" }
        $ws2 = $wb.Worksheets.Item(2)
        $out += "  sheet2 A2 h=$($ws2.Range('A2').HorizontalAlignment) colB hidden=$($ws2.Columns.Item(2).Hidden) C2=$($ws2.Range('C2').Text)"
      }
      } catch { $out += "  inspect error after line $($out.Count): $($_.Exception.Message) at $($_.InvocationInfo.ScriptLineNumber)" }
      $out | ForEach-Object { Write-Output $_ }
    } catch {
      $failed++
      Write-Output "$($f.Name)`tFAILED`t$($_.Exception.Message)"
    } finally {
      if ($wb) { $wb.Close($false) | Out-Null; [void][Runtime.InteropServices.Marshal]::ReleaseComObject($wb) }
    }
  }
} finally {
  $xl.Quit()
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($xl)
}
if ($failed) { exit 1 }
