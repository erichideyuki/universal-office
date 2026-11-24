# Converts every .xlsx of a source folder to .xls (BIFF8: FileFormat 56, the default; 39 = Excel 5.0/95 BIFF5) with Excel COM `SaveAs`: Excel invisible,
# DisplayAlerts off, workbooks opened read-only, Excel quit at the end. Nothing is activated or brought forward.
#
#   node tools/parallel/word-lock.mjs -- powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File core/crates/uo-xlsx/tools/xls-saveas.ps1 -Src <folder> -Dst <folder> [-Filter *.xlsx] [-Format 56|39]
# Prints one TSV line per file: name, ok|fail, message.
param(
  [Parameter(Mandatory = $true)][string]$Src,
  [Parameter(Mandatory = $true)][string]$Dst,
  [string]$Filter = '*.xlsx',
  [int]$Format = 56
)
$ErrorActionPreference = 'Stop'
# Excel resolves relative paths against its own working directory: make both absolute
$Src = [string][IO.Path]::GetFullPath((Join-Path $PWD.Path $Src))
$Dst = [string][IO.Path]::GetFullPath((Join-Path $PWD.Path $Dst))
[void](New-Item -ItemType Directory -Force -Path $Dst)
$xl = New-Object -ComObject Excel.Application
$xl.Visible = $false
$xl.DisplayAlerts = $false
$xl.ScreenUpdating = $false
$xl.EnableEvents = $false
$xl.AutomationSecurity = 3
$xl.AskToUpdateLinks = $false
$ok = 0
$bad = 0
try {
  foreach ($f in Get-ChildItem -LiteralPath $Src -Filter $Filter | Sort-Object Name) {
    $in = [string]$f.FullName
    $out = [string](Join-Path $Dst ([IO.Path]::GetFileNameWithoutExtension($f.Name) + '.xls'))
    $wb = $null
    try {
      # Open(FileName, UpdateLinks = 0, ReadOnly = true); FileFormat 56 = xlExcel8
      $wb = $xl.Workbooks.Open($in, 0, $true)
      if (Test-Path -LiteralPath $out) { Remove-Item -LiteralPath $out -Force }
      $wb.SaveAs($out, $Format)
      $ok++
      Write-Output "$($f.Name)`tok`t"
    } catch {
      $bad++
      Write-Output "$($f.Name)`tfail`t$($_.Exception.Message -replace '[\r\n\t]+', ' ')"
    } finally {
      if ($wb -ne $null) { try { $wb.Close($false) } catch { } }
    }
  }
} finally {
  try { $xl.Quit() } catch { }
  [void][Runtime.InteropServices.Marshal]::ReleaseComObject($xl)
}
Write-Output "done`t$ok ok`t$bad failed"
