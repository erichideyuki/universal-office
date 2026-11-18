# PowerPoint PDF of each deck of a list (the M4 PowerPoint-PDF oracle; driver: pdf-oracle.mjs).
#
# Safe with the user's PowerPoint OPEN: COM attaches to the running instance, so no window (Open with
# WithWindow = msoFalse), no WindowState, no Activate and no Quit unless this script started PowerPoint;
# it only closes the presentations it opened. Paths go to COM as [string] (a PSObject path can hang it).
#
# Usage: powershell -NoProfile -ExecutionPolicy Bypass -File export-pdf.ps1 -List <tsv: id<TAB>path> -OutDir <dir>
# Writes <OutDir>\<id>.pdf (SaveAs ppSaveAsPDF; hidden slides shown in memory first, so that page i is slide i) and
# <OutDir>\<id>.info (slide count, hidden slides, slide size in pt, seconds; or failed=<message>).
param(
  [Parameter(Mandatory = $true)][string]$List,
  [Parameter(Mandatory = $true)][string]$OutDir
)
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$rows = Get-Content -LiteralPath $List -Encoding UTF8 | Where-Object { $_ -match "`t" }

$wasRunning = @(Get-Process POWERPNT -ErrorAction SilentlyContinue).Count -gt 0
$ppt = New-Object -ComObject PowerPoint.Application
try {
  foreach ($row in $rows) {
    $parts = $row.Split("`t")
    $id = [string]$parts[0]
    $src = [string]$parts[1]
    $pdf = [string](Join-Path $OutDir "$id.pdf")
    $info = [string](Join-Path $OutDir "$id.info")
    $pres = $null
    $t0 = Get-Date
    try {
      # ReadOnly = msoTrue, Untitled = msoFalse, WithWindow = msoFalse
      $pres = $ppt.Presentations.Open($src, -1, 0, 0)
      # embedded fonts load asynchronously: a first export right after opening may draw the substitute
      try { $pres.Slides.Item(1).Export([string](Join-Path $env:TEMP 'export-pdf-warm.png'), 'PNG', 160, 90) } catch {}
      Start-Sleep -Milliseconds 700
      # hidden slides: listed, then shown IN MEMORY (the deck is read-only, nothing is saved over it) so that the
      # PDF has every slide and page i is slide i (Save as PDF leaves hidden slides out by default)
      $hidden = @()
      for ($i = 1; $i -le $pres.Slides.Count; $i++) {
        $tr = $pres.Slides.Item($i).SlideShowTransition
        if ($tr.Hidden -eq -1) { $hidden += $i; $tr.Hidden = 0 }
      }
      if (Test-Path -LiteralPath $pdf) { Remove-Item -LiteralPath $pdf }
      # ppSaveAsPDF = 32 (ExportAsFixedFormat cannot be called with optional arguments from PowerShell's late binding)
      $pres.SaveAs($pdf, 32)
      $secs = [Math]::Round(((Get-Date) - $t0).TotalSeconds, 2)
      $w = $pres.PageSetup.SlideWidth
      $h = $pres.PageSetup.SlideHeight
      Set-Content -LiteralPath $info -Encoding UTF8 -Value ("slides=" + $pres.Slides.Count + "`thidden=" + ($hidden -join ',') + "`tw=" + $w + "`th=" + $h + "`tsecs=" + $secs)
      Write-Host "$id : $($pres.Slides.Count) slides, $secs s"
    } catch {
      Write-Host "$id : FAILED $($_.Exception.Message)"
      Set-Content -LiteralPath $info -Encoding UTF8 -Value ("failed=" + $_.Exception.Message)
    } finally {
      if ($pres) { try { $pres.Close() } catch {} }
    }
  }
} finally {
  if (-not $wasRunning) { try { $ppt.Quit() } catch {} }
  [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($ppt)
}
