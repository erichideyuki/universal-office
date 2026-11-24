# Builds the feature workbooks of the `.xls` parity corpus with Excel COM and saves each as .xls (BIFF8, FileFormat 56): cells and
# formulas of every kind, styles, names, merges, panes, comments, hyperlinks, data validation, conditional formats, autofilters, charts
# (every type), shapes (WordArt, freeforms, groups, connectors, text boxes), pictures. Excel runs invisible, alerts off; nothing is
# brought to the foreground. The files are NOT committed (the capture of the committed subset is `tests/captures/xls.capture`).
#
#   node tools/parallel/word-lock.mjs -- powershell -NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File core/crates/uo-xlsx/tools/xls-features.ps1 -Out core/target/sheet-cap/xls-out/features
param([Parameter(Mandatory = $true)][string]$Out)
$ErrorActionPreference = 'Stop'
$Out = [string][IO.Path]::GetFullPath((Join-Path $PWD.Path $Out))
[void](New-Item -ItemType Directory -Force -Path $Out)
$tmp = [string](Join-Path $Out '_img')
[void](New-Item -ItemType Directory -Force -Path $tmp)

# --- pictures (generated, no proprietary content)
Add-Type -AssemblyName System.Drawing
function New-Picture([string]$path, [string]$fmt, [int]$w, [int]$h) {
  $bmp = New-Object System.Drawing.Bitmap($w, $h)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.Clear([System.Drawing.Color]::FromArgb(255, 30, 120, 200))
  $b = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 250, 200, 40))
  $g.FillEllipse($b, 4, 4, $w - 8, $h - 8)
  $g.Dispose()
  $f = switch ($fmt) { 'png' { [System.Drawing.Imaging.ImageFormat]::Png } 'jpg' { [System.Drawing.Imaging.ImageFormat]::Jpeg } 'bmp' { [System.Drawing.Imaging.ImageFormat]::Bmp } 'gif' { [System.Drawing.Imaging.ImageFormat]::Gif } 'tif' { [System.Drawing.Imaging.ImageFormat]::Tiff } }
  $bmp.Save($path, $f)
  $bmp.Dispose()
}
New-Picture ([string](Join-Path $tmp 'a.png')) 'png' 64 48
New-Picture ([string](Join-Path $tmp 'b.jpg')) 'jpg' 80 60
New-Picture ([string](Join-Path $tmp 'c.bmp')) 'bmp' 40 40
New-Picture ([string](Join-Path $tmp 'd.gif')) 'gif' 32 32
New-Picture ([string](Join-Path $tmp 'e.tif')) 'tif' 32 24

$xl = New-Object -ComObject Excel.Application
$xl.Visible = $false
$xl.DisplayAlerts = $false
$xl.ScreenUpdating = $false
$xl.EnableEvents = $false
$xl.AutomationSecurity = 3
$log = @()
function Save-Xls($wb, [string]$name) {
  $p = [string](Join-Path $Out ($name + '.xls'))
  if (Test-Path -LiteralPath $p) { Remove-Item -LiteralPath $p -Force }
  $wb.SaveAs($p, 56)
  $wb.Close($false)
  $script:log += "$name`tok"
}
function Try-Step([string]$what, [scriptblock]$sb) {
  try { & $sb } catch { $script:log += "step`t$what`t$($_.Exception.Message -replace '[\r\n\t]+', ' ')" }
}

try {
  # ============ A: cells, formulas, names =============================================================================
  $wb = $xl.Workbooks.Add()
  $ws = $wb.Worksheets.Item(1); $ws.Name = 'Valores'
  $ws2 = $wb.Worksheets.Add([Type]::Missing, $ws); $ws2.Name = 'Dados 2'
  $ws3 = $wb.Worksheets.Add([Type]::Missing, $ws2); $ws3.Name = "O'Brien Ação"
  $ws.Range('A1').Value2 = 'Id'; $ws.Range('B1').Value2 = 'Valor'; $ws.Range('C1').Value2 = 'Nome'; $ws.Range('D1').Value2 = 'Data'
  $names = @('alfa', 'Beta', 'gamma delta', 'São Paulo', 'straße', 'ÇÃO', '😀 smile', '𝒳 astral', 'x  y', ('a' * 300))
  for ($i = 0; $i -lt 40; $i++) {
    $r = $i + 2
    $ws.Cells.Item($r, 1).Value2 = $i + 1
    $ws.Cells.Item($r, 2).Value2 = [double](($i * 37 % 101) / 8.0 - 3.5)
    $ws.Cells.Item($r, 3).Value2 = [string]$names[$i % $names.Count]
    $ws.Cells.Item($r, 4).Value2 = [double](45000 + $i * 3.25)
  }
  $ws.Range('D2:D41').NumberFormat = 'dd/mm/yyyy hh:mm'
  $ws.Range('F1').Value2 = 1e15; $ws.Range('F2').Value2 = 1.5e-9; $ws.Range('F3').Value2 = -0.0000123; $ws.Range('F4').Value2 = 123456789012; $ws.Range('F5').Value2 = 0.1; $ws.Range('F6').Value2 = 1073741823; $ws.Range('F7').Value2 = 536870911.5; $ws.Range('F8').Value2 = 0.07
  $ws.Range('G1').Value2 = $true; $ws.Range('G2').Value2 = $false
  $ws.Range('G3').Formula = '=1/0'; $ws.Range('G4').Formula = '=NA()'; $ws.Range('G5').Formula = '=SQRT(-1)'; $ws.Range('G6').Formula = '=VALUE("x")'; $ws.Range('G7').Formula = '=NOSUCH(1)'; $ws.Range('G8').Formula = '=A1:A2+B1:B2'
  # formulas of many kinds (direct, shared by fill, array, names, 3D, strings)
  $ws.Range('H1').Formula = '=SUM(B2:B41)'; $ws.Range('H2').Formula = '=AVERAGE(B2:B41)*2+MAX(A2:A41)'
  $ws.Range('H3').Formula = '=IF(B2>0,"pos","neg")&TEXT(B2,"0.00")'
  $ws.Range('H4').Formula = '=VLOOKUP(3,A2:C41,3,FALSE)'; $ws.Range('H5').Formula = '=INDEX(A2:D41,MATCH(5,A2:A41,0),3)'
  $ws.Range('H6').Formula = '=SUMIF(A2:A41,">10",B2:B41)'; $ws.Range('H7').Formula = '=COUNTIF(C2:C41,"alfa")'
  $ws.Range('H8').Formula = '=ROUND(PI()*B2,3)+POWER(2,10)+MOD(A3,7)'; $ws.Range('H9').Formula = '=CONCATENATE(C2,"-",C3)'
  $ws.Range('H10').Formula = '=DATE(2024,2,29)+EDATE(D2,1)'; $ws.Range('H11').Formula = '=PMT(0.05/12,36,10000)'
  $ws.Range('H12').Formula = '=SUM(Valores:''Dados 2''!A1)'; $ws.Range('H13').Formula = "='O''Brien Ação'!A1&""x"""
  $ws.Range('H14').Formula = '=RAND()'; $ws.Range('H15').Formula = '=NOW()'; $ws.Range('H16').Formula = '=ROW()+COLUMN()'
  $ws.Range('H17').Formula = '=-B2%+(1+2)*3^2'; $ws.Range('H18').Formula = '=SUM({1,2;3,4})'; $ws.Range('H19').Formula = '=CHOOSE(2,"a","b","c")'
  $ws.Range('H20').Formula = '=SUBTOTAL(9,B2:B41)'; $ws.Range('H21').Formula = '=IFERROR(1/0,"err")'; $ws.Range('H22').Formula = '=AND(TRUE,B2>0)'
  $ws.Range('H23').Formula = '=LEFT(C2,2)&RIGHT(C3,2)&MID(C4,2,3)'; $ws.Range('H24').Formula = '=YEARFRAC(D2,D3)'; $ws.Range('H25').Formula = '=ISEVEN(A2)'
  # shared formulas by fill
  $ws.Range('J2').Formula = '=B2*2+A2'; [void]$ws.Range('J2:J41').FillDown()
  $ws.Range('K2').Formula = '=IF(A2>5,C2,"")'; [void]$ws.Range('K2:K41').FillDown()
  $ws.Range('L2:M2').Formula = '=A2+$B$2'; [void]$ws.Range('L2:M41').FillDown()
  $ws.Range('N2').Formula = '=C2&"|"&C3'; [void]$ws.Range('N2:N41').FillDown()
  # array formulas (CSE)
  Try-Step 'array1' { $ws.Range('P1:P3').FormulaArray = '=A2:A4*10' }
  Try-Step 'array2' { $ws.Range('Q1:R2').FormulaArray = '=TRANSPOSE(A2:B3)' }
  Try-Step 'array3' { $ws.Range('S1').FormulaArray = '=SUM(A2:A41*B2:B41)' }
  # a data table (what-if)
  $ws2.Range('A1').Value2 = 'x'; $ws2.Range('B1').Formula = '=A1^2+1'
  for ($i = 2; $i -le 6; $i++) { $ws2.Cells.Item($i, 1).Value2 = $i }
  Try-Step 'data table' { [void]$ws2.Range('A1:B6').Table([Type]::Missing, $ws2.Range('A1')) }
  # sheet 3: formulas that cross sheets
  $ws3.Range('A1').Formula = '=Valores!B2+''Dados 2''!A3'; $ws3.Range('A2').Formula = '=SUM(Valores!A2:D6)'; $ws3.Range('A3').Value2 = 'text'
  # names: global, local, built-in, relative, constant, 3D
  Try-Step 'name' { [void]$wb.Names.Add('Taxas', '=Valores!$B$2:$B$41') }
  Try-Step 'name' { [void]$wb.Names.Add('Dobro', '=Valores!$B$2*2') }
  Try-Step 'name' { [void]$wb.Names.Add('Rel', '=Valores!A2') }
  Try-Step 'name' { [void]$wb.Names.Add('Const', '=314159/100000') }
  Try-Step 'name' { [void]$wb.Names.Add('Txt', '="hello ""world"""') }
  Try-Step 'name' { [void]$wb.Names.Add('SHH20', '=Valores!$B$1') }
  Try-Step 'name' { [void]$ws.Names.Add('Local1', '=Valores!$A$2') }
  Try-Step 'print area' { $ws.PageSetup.PrintArea = '$A$1:$D$20'; $ws.PageSetup.PrintTitleRows = '$1:$1' }
  $ws.Range('T1').Formula = '=SUM(Taxas)+Dobro+Const'; $ws.Range('T2').Formula = '=Local1*Rel'
  # rich text in a cell
  Try-Step 'rich' { $c = $ws.Range('U1'); $c.Value2 = 'Hello rich world'; $c.Characters(1, 5).Font.Bold = $true; $c.Characters(7, 4).Font.Italic = $true; $c.Characters(7, 4).Font.Color = 255; $c.Characters(12, 5).Font.Size = 16 }
  $ws.Range('V1').Value2 = ('long text ' * 1200)
  # date 1904 sheet
  Save-Xls $wb 'a-cells'

}
catch { $log += "FATAL-SEC`t$($_.InvocationInfo.ScriptLineNumber)`t$($_.Exception.Message)" }
try {
  # ============ B: date1904, calc options, hidden sheets ==============================================================
  $wb = $xl.Workbooks.Add()
  $wb.Date1904 = $true
  $ws = $wb.Worksheets.Item(1); $ws.Name = 'D1904'
  $ws.Range('A1').Value2 = [double]100.5; $ws.Range('A1').NumberFormat = 'yyyy-mm-dd'
  $ws.Range('A2').Formula = '=A1+1'
  $s2 = $wb.Worksheets.Add([Type]::Missing, $ws); $s2.Name = 'Oculta'; $s2.Visible = 0
  $s3 = $wb.Worksheets.Add([Type]::Missing, $s2); $s3.Name = 'MuitoOculta'; $s3.Visible = 2
  $s4 = $wb.Worksheets.Add([Type]::Missing, $s3); $s4.Name = ('N' * 31)
  $s5 = $wb.Worksheets.Add([Type]::Missing, $s4); $s5.Name = 'Visible 5'
  Try-Step 'calc' { $xl.Calculation = -4135; $xl.Iteration = $true; $xl.MaxIterations = 50; $xl.MaxChange = 0.01 }
  $ws.Range('B1').Formula = '=B2+1'; $ws.Range('B2').Formula = '=B1*0.5'
  $wb.Worksheets.Item(1).Activate() | Out-Null
  Save-Xls $wb 'b-book'
  Try-Step 'calc back' { $xl.Calculation = -4105; $xl.Iteration = $false }

}
catch { $log += "FATAL-SEC`t$($_.InvocationInfo.ScriptLineNumber)`t$($_.Exception.Message)" }
try {
  # ============ C: styles ==============================================================================================
  $wb = $xl.Workbooks.Add()
  $ws = $wb.Worksheets.Item(1); $ws.Name = 'Estilos'
  $r = 1
  foreach ($fn in @('Arial', 'Times New Roman', 'Courier New', 'Verdana', 'Calibri', 'Cambria', 'Tahoma', 'Consolas', 'Comic Sans MS', 'Georgia')) {
    $c = $ws.Cells.Item($r, 1); $c.Value2 = "Text $fn"; $c.Font.Name = $fn; $c.Font.Size = 8 + ($r % 5) * 2; $r++
  }
  $c = $ws.Range('B1'); $c.Value2 = 'bold'; $c.Font.Bold = $true
  $c = $ws.Range('B2'); $c.Value2 = 'italic'; $c.Font.Italic = $true
  $c = $ws.Range('B3'); $c.Value2 = 'underline'; $c.Font.Underline = 2
  $c = $ws.Range('B4'); $c.Value2 = 'double'; $c.Font.Underline = -4119
  $c = $ws.Range('B5'); $c.Value2 = 'acct'; $c.Font.Underline = 4
  $c = $ws.Range('B6'); $c.Value2 = 'strike'; $c.Font.Strikethrough = $true
  $c = $ws.Range('B7'); $c.Value2 = 'super'; $c.Font.Superscript = $true
  $c = $ws.Range('B8'); $c.Value2 = 'sub'; $c.Font.Subscript = $true
  $c = $ws.Range('B9'); $c.Value2 = 'color'; $c.Font.Color = 0x2040FF
  $c = $ws.Range('B10'); $c.Value2 = 'theme'; $c.Font.ThemeColor = 5; $c.Font.TintAndShade = 0.4
  $c = $ws.Range('B11'); $c.Value2 = 'theme2'; $c.Font.ThemeColor = 7; $c.Font.TintAndShade = -0.25
  $c = $ws.Range('B12'); $c.Value2 = 'outline'; try { $c.Font.OutlineFont = $true } catch { }
  $c = $ws.Range('B13'); $c.Value2 = 'shadow'; try { $c.Font.Shadow = $true } catch { }
  $pat = @(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18)
  for ($i = 0; $i -lt $pat.Count; $i++) { $c = $ws.Cells.Item($i + 1, 4); $c.Value2 = "p$i"; $c.Interior.Pattern = -4142 + 0; try { $c.Interior.Pattern = @(-4105, -4125, -4126, -4128, -4124, -4127, -4166, -4162, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18)[$i] } catch { }; $c.Interior.Color = (0x30 * $i) -band 0xFFFFFF; try { $c.Interior.PatternColor = 0xFF8800 } catch { } }
  $c = $ws.Range('E1'); $c.Value2 = 'theme fill'; $c.Interior.ThemeColor = 6; $c.Interior.TintAndShade = 0.6
  $c = $ws.Range('E2'); $c.Value2 = 'rgb fill'; $c.Interior.Color = 0x70B0E0
  $bs = @(1, 2, 3, 4, 5, 6, 7, -4115, -4118, -4119, 4, 5)
  $bst = @(1, 2, -4115, -4118, 4, 5, -4119, 13, 9, 1, 2, 1)
  for ($i = 0; $i -lt 12; $i++) {
    $c = $ws.Cells.Item($i + 1, 6); $c.Value2 = "b$i"
    foreach ($e in 7, 8, 9, 10) { try { $b = $c.Borders.Item($e); $b.LineStyle = @(1, -4115, -4118, -4119, 4, 5, 13, 9, 1, 1, 1, 1)[$i]; $b.Weight = @(2, 4, 2, 4, -4138, 1, 2, 4, 2, 2, 2, 2)[$i]; $b.Color = (0x10F0 * ($i + 1)) -band 0xFFFFFF } catch { } }
  }
  $c = $ws.Range('G1'); $c.Value2 = 'diag'; $c.Borders.Item(5).LineStyle = 1; $c.Borders.Item(6).LineStyle = 1; $c.Borders.Item(5).Color = 255
  $al = @(1, -4131, -4108, -4152, 5, -4130, 7, -4117)
  for ($i = 0; $i -lt $al.Count; $i++) { $c = $ws.Cells.Item($i + 1, 8); $c.Value2 = "align $i"; $c.HorizontalAlignment = $al[$i] }
  $va = @(-4160, -4108, -4107, -4130, -4117)
  for ($i = 0; $i -lt $va.Count; $i++) { $c = $ws.Cells.Item($i + 1, 9); $c.Value2 = "valign $i"; $c.VerticalAlignment = $va[$i]; $ws.Rows.Item($i + 1).RowHeight = 30 }
  $c = $ws.Range('J1'); $c.Value2 = 'wrap this text that is long enough'; $c.WrapText = $true
  $c = $ws.Range('J2'); $c.Value2 = 'indent'; $c.HorizontalAlignment = -4131; $c.IndentLevel = 3
  $c = $ws.Range('J3'); $c.Value2 = 'rot45'; $c.Orientation = 45
  $c = $ws.Range('J4'); $c.Value2 = 'rot-60'; $c.Orientation = -60
  $c = $ws.Range('J5'); $c.Value2 = 'vertical'; $c.Orientation = -4166
  $c = $ws.Range('J6'); $c.Value2 = 'shrink to fit text'; $c.ShrinkToFit = $true
  $fm = @('0', '0.00', '#,##0', '#,##0.00', '0%', '0.00%', '0.00E+00', '# ?/?', 'dd/mm/yyyy', 'd-mmm-yy', 'h:mm', 'h:mm:ss AM/PM', 'm/d/yyyy h:mm', '@', '"R$" #,##0.00', '[Red]-0.00;0.00', '_-* #,##0.00_-;-* #,##0.00_-;_-* "-"??_-;_-@_-', '[h]:mm:ss', 'General', '0;-0;"zero";@', '[$-409]d-mmm-yy', '[>100]"big";[<0]"neg";"small"', '#,##0,"k"', '"x"0.0 "y"', 'GENERAL', '[Blue]General', '#.##0,00')
  for ($i = 0; $i -lt $fm.Count; $i++) { $c = $ws.Cells.Item($i + 1, 11); $c.Value2 = [double](1234.5678 + $i); try { $c.NumberFormat = $fm[$i] } catch { } }
  $ws.Columns.Item(1).ColumnWidth = 22; $ws.Columns.Item(2).ColumnWidth = 12.7; $ws.Columns.Item(3).ColumnWidth = 3; $ws.Columns.Item(4).Hidden = $true
  $ws.Columns.Item(10).ColumnWidth = 40; $ws.Columns.Item(12).ColumnWidth = 0.5
  $ws.Rows.Item(20).RowHeight = 45; $ws.Rows.Item(21).Hidden = $true; $ws.Rows.Item(22).RowHeight = 5
  $ws.Range('A30:C32').Merge(); $ws.Range('A30').Value2 = 'merged'
  $ws.Range('E30:E33').Merge(); $ws.Range('G30:H30').Merge()
  Try-Step 'style' { $st = $wb.Styles.Add('MeuEstilo'); $st.Font.Bold = $true; $st.Interior.Color = 0x99FFFF; $ws.Range('A35').Style = 'MeuEstilo'; $ws.Range('A35').Value2 = 'styled' }
  Try-Step 'theme colors' { $wb.Theme.ThemeColorScheme.Colors(5).RGB = 0x3070C0 }
  $ws.Range('A36').Value2 = 'after theme'; $ws.Range('A36').Interior.ThemeColor = 5
  Save-Xls $wb 'c-styles'

}
catch { $log += "FATAL-SEC`t$($_.InvocationInfo.ScriptLineNumber)`t$($_.Exception.Message)" }
try {
  # ============ D: sheet features =====================================================================================
  $wb = $xl.Workbooks.Add()
  $ws = $wb.Worksheets.Item(1); $ws.Name = 'Recursos'
  for ($i = 1; $i -le 30; $i++) { $ws.Cells.Item($i, 1).Value2 = $i; $ws.Cells.Item($i, 2).Value2 = [double]($i * 3 % 17); $ws.Cells.Item($i, 3).Value2 = "item $($i % 7)"; $ws.Cells.Item($i, 4).Value2 = [double](40000 + $i * 11) }
  $ws.Range('D1:D30').NumberFormat = 'dd/mm/yyyy'
  # comments
  Try-Step 'comments' { $x = $ws.Range('A1').AddComment('First note, with author'); $x.Shape.Width = 150 }
  Try-Step 'comments' { $ws.Range('B3').AddComment("Line 1`nLine 2 çã 😀") | Out-Null }
  Try-Step 'comments' { $ws.Range('C5').AddComment(('x' * 400)) | Out-Null }
  Try-Step 'comments' { $ws.Range('D7').AddComment('visible one') | Out-Null; $ws.Range('D7').Comment.Visible = $true }
  Try-Step 'comments' { $ws.Range('A1').Comment.Text('Author line: ', 1, $false) | Out-Null }
  # hyperlinks
  Try-Step 'links' { [void]$ws.Hyperlinks.Add($ws.Range('F1'), 'https://example.com/a?b=1&c=ç', [Type]::Missing, 'Tip <1>', 'Link text') }
  Try-Step 'links' { [void]$ws.Hyperlinks.Add($ws.Range('F2'), 'mailto:someone@example.com', [Type]::Missing, 'mail') }
  Try-Step 'links' { [void]$ws.Hyperlinks.Add($ws.Range('F3'), '', 'Recursos!A10', 'jump', 'Go to A10') }
  Try-Step 'links' { [void]$ws.Hyperlinks.Add($ws.Range('F4:F5'), 'C:\data\file one.xlsx', [Type]::Missing, 'file') }
  Try-Step 'links' { [void]$ws.Hyperlinks.Add($ws.Range('F6'), '..\other\file.xlsx', [Type]::Missing, 'relative') }
  Try-Step 'links' { [void]$ws.Hyperlinks.Add($ws.Range('F7'), '\\server\share\x.xls', 'Sheet1!B2', 'unc') }
  Try-Step 'links' { [void]$ws.Hyperlinks.Add($ws.Range('F8'), 'file:///C:/x/y.pdf') }
  # data validation
  Try-Step 'dv' { $v = $ws.Range('G1:G10').Validation; $v.Delete(); [void]$v.Add(1, 1, 1, '1', '100'); $v.InputTitle = 'Whole'; $v.InputMessage = 'between 1 and 100'; $v.ErrorTitle = 'Bad'; $v.ErrorMessage = 'not ok'; $v.IgnoreBlank = $false }
  Try-Step 'dv' { $v = $ws.Range('G11').Validation; [void]$v.Add(2, 2, 5, '0.5'); $v.ShowError = $false }
  Try-Step 'dv' { $v = $ws.Range('G12').Validation; [void]$v.Add(3, 1, 1, 'a;b;c;d e'); $v.InCellDropdown = $false }
  Try-Step 'dv' { $v = $ws.Range('G13').Validation; [void]$v.Add(3, 1, 1, '=$C$1:$C$7') }
  Try-Step 'dv' { $v = $ws.Range('G14').Validation; [void]$v.Add(4, 2, 1, '1/1/2020', '12/31/2030'); $v.ErrorTitle = 'dt' }
  Try-Step 'dv' { $v = $ws.Range('G15').Validation; [void]$v.Add(5, 1, 7, '0:00'); }
  Try-Step 'dv' { $v = $ws.Range('G16').Validation; [void]$v.Add(6, 2, 8, '5'); $v.AlertStyle = 2 }
  Try-Step 'dv' { $v = $ws.Range('G17:G18').Validation; [void]$v.Add(7, 3, 1, '=ISNUMBER(G17)'); $v.AlertStyle = 3 }
  Try-Step 'dv' { $v = $ws.Range('H1,H5:H6').Validation; [void]$v.Add(1, 1, 3, '5') }
  # conditional formatting
  Try-Step 'cf' { $rg = $ws.Range('B1:B30') }
  Try-Step 'cf' { $f = $rg.FormatConditions.Add(1, 5, '=10'); $f.Interior.Color = 0x9999FF; $f.Font.Bold = $true; $f.Font.Color = 0x0000C0 }
  Try-Step 'cf' { $f = $rg.FormatConditions.Add(1, 1, '=3', '=6'); $f.Interior.Color = 0x99FF99; $f.Borders.Item(7).LineStyle = 1; $f.Borders.Item(7).Color = 0xFF0000 }
  Try-Step 'cf' { $f = $ws.Range('A1:A30').FormatConditions.Add(2, [Type]::Missing, '=MOD(A1,2)=0'); $f.Interior.Color = 0xFFCC99; $f.StopIfTrue = $true; $f.Font.Italic = $true; $f.Font.Strikethrough = $true }
  Try-Step 'cf' { [void]$ws.Range('D1:D30').FormatConditions.AddColorScale(3) }
  Try-Step 'cf' { [void]$ws.Range('B1:B30').FormatConditions.AddColorScale(2) }
  Try-Step 'cf' { $db = $ws.Range('A1:A30').FormatConditions.AddDatabar(); $db.BarColor.Color = 0x00AA00 }
  Try-Step 'cf' { [void]$ws.Range('B1:B30').FormatConditions.AddIconSetCondition() }
  Try-Step 'cf' { $t = $ws.Range('B1:B30').FormatConditions.AddTop10(); $t.TopBottom = 1; $t.Rank = 3; $t.Interior.Color = 0xAAAAFF }
  Try-Step 'cf' { $t = $ws.Range('B1:B30').FormatConditions.AddTop10(); $t.TopBottom = 2; $t.Rank = 20; $t.Percent = $true; $t.Interior.Color = 0xFFAAAA }
  Try-Step 'cf' { $a = $ws.Range('B1:B30').FormatConditions.AddAboveAverage(); $a.AboveBelow = 1; $a.Interior.Color = 0xCCFFFF }
  Try-Step 'cf' { $a = $ws.Range('B1:B30').FormatConditions.AddAboveAverage(); $a.AboveBelow = 4; $a.NumStdDev = 2 }
  Try-Step 'cf' { $u = $ws.Range('A1:A30').FormatConditions.AddUniqueValues(); $u.DupeUnique = 0; $u.Interior.Color = 0xDDDDDD }
  Try-Step 'cf' { $u = $ws.Range('C1:C30').FormatConditions.AddUniqueValues(); $u.DupeUnique = 1; $u.Interior.Color = 0xDDFFDD }
  Try-Step 'cf' { $tx = $ws.Range('C1:C30').FormatConditions.Add(9, [Type]::Missing, [Type]::Missing); }
  Try-Step 'cf2' { $f = $ws.Range('C1:C30').FormatConditions.Add(2, [Type]::Missing, '=ISNUMBER(SEARCH("item 3",C1))'); $f.Interior.Color = 0xFF99CC }
  Try-Step 'cf2' { $f = $ws.Range('J1:K10,L5:L9').FormatConditions.Add(1, 3, '=5'); $f.Font.Color = 0xFF0000 }
  # auto filter
  Try-Step 'filter' { [void]$ws.Range('A1:D30').AutoFilter(3, 'item 3') }
  # panes, zoom, view
  $ws.Activate() | Out-Null
  Try-Step 'view' { $win = $xl.ActiveWindow; $win.Zoom = 85; $win.DisplayGridlines = $false; $win.DisplayZeros = $false; $ws.Range('C3').Select() | Out-Null; $win.FreezePanes = $true }
  Try-Step 'rtl' { $ws.DisplayRightToLeft = $false }
  $s2 = $wb.Worksheets.Add([Type]::Missing, $ws); $s2.Name = 'Split'
  $s2.Range('A1:E20').Value2 = 7
  $s2.Activate() | Out-Null
  Try-Step 'split' { $win = $xl.ActiveWindow; $win.SplitColumn = 2; $win.SplitRow = 4; $win.ScrollColumn = 1; $win.ScrollRow = 1 }
  $s3 = $wb.Worksheets.Add([Type]::Missing, $s2); $s3.Name = 'RTL'; $s3.DisplayRightToLeft = $true; $s3.Range('A1').Value2 = 'rtl'
  $s3.Range('A2').Formula = '=A1&"!"'
  $ws.Activate() | Out-Null
  Save-Xls $wb 'd-sheet'
}
catch { $log += "FATAL`t$($_.InvocationInfo.ScriptLineNumber)`t$($_.Exception.Message -replace '[\r\n\t]+', ' ')" }

# ============ E: charts ===============================================================================================
try {
  $wb = $xl.Workbooks.Add()
  $ws = $wb.Worksheets.Item(1); $ws.Name = 'Graficos'
  $ws.Range('A1').Value2 = 'Cat'; $ws.Range('B1').Value2 = 'S1'; $ws.Range('C1').Value2 = 'S2'; $ws.Range('D1').Value2 = 'S3'
  $cats = @('Jan', 'Fev', 'Mar', 'Abr', 'Mai', 'Jun')
  for ($i = 0; $i -lt 6; $i++) { $ws.Cells.Item($i + 2, 1).Value2 = [string]$cats[$i]; $ws.Cells.Item($i + 2, 2).Value2 = [double](10 + $i * 3); $ws.Cells.Item($i + 2, 3).Value2 = [double](20 - $i * 2); $ws.Cells.Item($i + 2, 4).Value2 = [double](5 + ($i * 7 % 11)) }
  $ws.Range('B2:D7').NumberFormat = '#,##0.0'
  $types = @(51, 52, 53, 57, 58, 4, 65, 5, 70, -4169, 74, 1, 76, 75, 78, 81, 82, 83, -4100, 85, 86, 87, -4151, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100)
  $k = 0
  foreach ($t in $types) {
    $sh = $ws.ChartObjects().Add(10 + 20 * ($k % 6), 150 + 30 * $k, 300, 200)
    try {
      $ch = $sh.Chart
      $ch.ChartType = $t
      $ch.SetSourceData($ws.Range('A1:D7'))
      if ($k % 3 -eq 0) { $ch.HasTitle = $true; $ch.ChartTitle.Text = "Chart $t" }
      if ($k % 4 -eq 1) { $ch.HasLegend = $false } else { $ch.HasLegend = $true; $ch.Legend.Position = @(-4107, -4160, -4152, -4131, 2)[$k % 5] }
      if ($k % 5 -eq 0) { try { $ch.Axes(1).HasTitle = $true; $ch.Axes(1).AxisTitle.Text = 'Eixo X'; $ch.Axes(2).HasTitle = $true; $ch.Axes(2).AxisTitle.Text = 'Eixo Y' } catch { } }
      if ($k % 6 -eq 2) { try { $ch.SeriesCollection(1).HasDataLabels = $true } catch { } }
      if ($k % 7 -eq 3) { try { $ch.Axes(2).MinimumScale = 0; $ch.Axes(2).MaximumScale = 40; $ch.Axes(2).MajorUnit = 10; $ch.Axes(2).HasMajorGridlines = $true; $ch.Axes(2).HasMinorGridlines = $true } catch { } }
      if ($k % 8 -eq 4) { try { $ch.SeriesCollection(2).AxisGroup = 2 } catch { } }
      if ($k % 9 -eq 5) { try { $ch.Axes(2).ScaleType = -4133; $ch.Axes(1).ReversePlotOrder = $true } catch { } }
      if ($k % 10 -eq 6) { try { $ch.SeriesCollection(1).Format.Fill.ForeColor.RGB = 0x2060E0; $ch.ChartArea.Format.Fill.ForeColor.RGB = 0xEEEEEE; $ch.PlotArea.Format.Fill.ForeColor.RGB = 0xFFFFCC } catch { } }
      if ($k % 11 -eq 7) { try { $ch.ChartArea.Format.Fill.TwoColorGradient(1, 1); $ch.ChartArea.Format.Fill.ForeColor.RGB = 0xFF8800; $ch.ChartArea.Format.Fill.BackColor.RGB = 0xFFFFFF } catch { } }
      if ($k % 12 -eq 8) { try { $ch.SeriesCollection(1).Smooth = $true; $ch.SeriesCollection(1).MarkerStyle = 8; $ch.SeriesCollection(1).MarkerSize = 9 } catch { } }
    } catch { $log += "chart`t$t`t$($_.Exception.Message -replace '[\r\n\t]+', ' ')" }
    $k++
  }
  # a date axis chart
  $ws.Range('H1').Value2 = 'Data'; $ws.Range('I1').Value2 = 'V'
  $dts = @(45000, 45002, 45010, 45030, 45031, 45100)
  for ($i = 0; $i -lt 6; $i++) { $ws.Cells.Item($i + 2, 8).Value2 = [double]$dts[$i]; $ws.Cells.Item($i + 2, 9).Value2 = [double](3 + $i * $i) }
  $ws.Range('H2:H7').NumberFormat = 'dd/mm/yyyy'
  $sh = $ws.ChartObjects().Add(500, 20, 300, 200); $sh.Chart.ChartType = 65; $sh.Chart.SetSourceData($ws.Range('H1:I7'))
  try { $sh.Chart.Axes(1).CategoryType = 3; $sh.Chart.Axes(1).BaseUnit = 0 } catch { }
  # chart sheets
  $cs = $wb.Charts.Add(); $cs.ChartType = 51; $cs.SetSourceData($ws.Range('A1:C7')); $cs.Name = 'FolhaGrafico'
  $cs2 = $wb.Charts.Add(); $cs2.ChartType = 5; $cs2.SetSourceData($ws.Range('A1:B7'))
  Save-Xls $wb 'e-charts'
}
catch { $log += "FATAL-E`t$($_.InvocationInfo.ScriptLineNumber)`t$($_.Exception.Message -replace '[\r\n\t]+', ' ')" }

# ============ F: shapes, pictures =====================================================================================
try {
  $wb = $xl.Workbooks.Add()
  $ws = $wb.Worksheets.Item(1); $ws.Name = 'Formas'
  $ws.Range('A1:H20').Value2 = 1
  $ws.Columns.Item(2).ColumnWidth = 20; $ws.Rows.Item(3).RowHeight = 40
  $ids = @(1, 5, 9, 4, 3, 7, 6, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 52, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127, 128, 129, 130, 131, 132, 133, 134, 135, 136, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146, 147, 148, 149, 150, 151, 152, 153, 154, 155, 156, 157, 158, 159, 160, 161, 162, 163, 164, 165, 166, 167, 168, 169, 170, 171, 172, 173, 174, 175, 176, 177, 178, 179, 180, 181, 182, 183, 184, 185, 186, 187, 188, 189, 190, 191, 192)
  $k = 0
  foreach ($id in $ids) {
    try {
      $s = $ws.Shapes.AddShape($id, 20 + ($k % 12) * 55, 100 + [math]::Floor($k / 12) * 45, 50, 40)
      if ($k % 3 -eq 0) { $s.Fill.ForeColor.RGB = (0x103060 * ($k + 1)) -band 0xFFFFFF }
      if ($k % 5 -eq 1) { $s.Line.ForeColor.RGB = 0x0000FF; $s.Line.Weight = 3; $s.Line.DashStyle = (($k % 8) + 1) }
      if ($k % 7 -eq 2) { $s.Rotation = 33 }
      if ($k % 9 -eq 3) { $s.Flip(0) }
      if ($k % 11 -eq 4) { $s.Shadow.Visible = $true; $s.Shadow.OffsetX = 4; $s.Shadow.OffsetY = 3 }
      if ($k % 13 -eq 5) { try { $s.Fill.TwoColorGradient(($k % 7) + 1, 1); $s.Fill.ForeColor.RGB = 0xFF0000; $s.Fill.BackColor.RGB = 0x00FF00 } catch { } }
      if ($k % 14 -eq 6) { try { $s.Fill.Patterned(($k % 40) + 1) } catch { } }
      if ($k % 4 -eq 0) { try { $s.TextFrame2.TextRange.Text = "T$k"; } catch { } }
    } catch { $log += "shape`t$id`t$($_.Exception.Message -replace '[\r\n\t]+', ' ')" }
    $k++
  }
  # lines, arrows, connectors
  Try-Step 'lines' {
    $l = $ws.Shapes.AddLine(30, 400, 200, 440); $l.Line.EndArrowheadStyle = 2; $l.Line.BeginArrowheadStyle = 5; $l.Line.Weight = 2
    $c = $ws.Shapes.AddConnector(2, 220, 400, 300, 460); $c.Line.ForeColor.RGB = 0x00AA00
    $c = $ws.Shapes.AddConnector(3, 320, 400, 400, 460)
    $c = $ws.Shapes.AddConnector(1, 420, 400, 500, 460)
  }
  # text boxes with rich text
  Try-Step 'textboxes' {
    $t = $ws.Shapes.AddTextbox(1, 20, 480, 160, 60); $t.TextFrame2.TextRange.Text = "Line one`rLine two bold"; $t.TextFrame2.TextRange.Paragraphs(2).Font.Bold = -1; $t.TextFrame2.TextRange.Characters(1, 4).Font.Fill.ForeColor.RGB = 255
    $t.TextFrame2.VerticalAnchor = 3; $t.TextFrame2.TextRange.ParagraphFormat.Alignment = 2
    $t = $ws.Shapes.AddTextbox(5, 200, 480, 100, 80); $t.TextFrame2.TextRange.Text = 'vertical text'; $t.TextFrame2.Orientation = 5
    $t = $ws.Shapes.AddTextbox(1, 320, 480, 120, 50); $t.TextFrame2.TextRange.Text = 'wrapped text off ' * 3; $t.TextFrame2.WordWrap = 0; $t.TextFrame2.MarginLeft = 2
    $t = $ws.Shapes.AddTextbox(1, 460, 480, 120, 50); $t.TextFrame2.TextRange.Text = ('emoji 😀 ç ' * 2); $t.Fill.ForeColor.RGB = 0xCCFFFF; $t.Line.Visible = 0
  }
  # WordArt
  Try-Step 'wordart' {
    for ($i = 0; $i -lt 12; $i++) { $w = $ws.Shapes.AddTextEffect($i, "WordArt $i", 'Arial', 24, 0, 0, 20 + ($i % 6) * 110, 560 + [math]::Floor($i / 6) * 60); $w.Fill.ForeColor.RGB = (0x30A060 * ($i + 1)) -band 0xFFFFFF }
  }
  # freeform
  Try-Step 'freeform' {
    $fb = $ws.Shapes.BuildFreeform(0, 20, 700); $fb.AddNodes(0, 0, 80, 650); $fb.AddNodes(1, 0, 140, 720, 180, 700, 200, 660); $fb.AddNodes(0, 0, 100, 740); [void]$fb.ConvertToShape()
    $fb = $ws.Shapes.BuildFreeform(1, 260, 700); $fb.AddNodes(0, 0, 300, 650); $fb.AddNodes(0, 0, 340, 700); $fb.AddNodes(0, 0, 260, 700); [void]$fb.ConvertToShape()
  }
  # groups
  Try-Step 'group' {
    $a = $ws.Shapes.AddShape(1, 420, 700, 40, 30); $a.Name = 'g1'
    $b = $ws.Shapes.AddShape(9, 470, 710, 40, 30); $b.Name = 'g2'
    $c = $ws.Shapes.AddShape(5, 440, 740, 60, 20); $c.Name = 'g3'
    $grp = $ws.Shapes.Range(@('g1', 'g2', 'g3')).Group(); $grp.Name = 'GroupOne'
    $a = $ws.Shapes.AddShape(1, 530, 700, 40, 30); $a.Name = 'h1'; $a.Rotation = 45
    $b = $ws.Shapes.AddShape(9, 580, 710, 40, 30); $b.Name = 'h2'
    $g1 = $ws.Shapes.Range(@('h1', 'h2')).Group(); $g1.Name = 'inner'
    $d = $ws.Shapes.AddShape(1, 640, 700, 40, 30); $d.Name = 'h3'
    $g2 = $ws.Shapes.Range(@('inner', 'h3')).Group(); $g2.Name = 'nested'
  }
  # pictures
  Try-Step 'pictures' {
    $p = $ws.Shapes.AddPicture([string](Join-Path $tmp 'a.png'), 0, -1, 700, 100, 64, 48)
    $p2 = $ws.Shapes.AddPicture([string](Join-Path $tmp 'b.jpg'), 0, -1, 780, 100, -1, -1)
    $p3 = $ws.Shapes.AddPicture([string](Join-Path $tmp 'c.bmp'), 0, -1, 700, 170, -1, -1); $p3.PictureFormat.CropLeft = 5; $p3.PictureFormat.CropTop = 3
    $p4 = $ws.Shapes.AddPicture([string](Join-Path $tmp 'd.gif'), 0, -1, 780, 170, -1, -1); $p4.Rotation = 20
    $p5 = $ws.Shapes.AddPicture([string](Join-Path $tmp 'e.tif'), 0, -1, 700, 240, -1, -1)
    $p6 = $ws.Shapes.AddPicture([string](Join-Path $tmp 'a.png'), 0, -1, 780, 240, 80, 60); $p6.PictureFormat.Brightness = 0.7; $p6.PictureFormat.Contrast = 0.3
    $p7 = $ws.Shapes.AddPicture([string](Join-Path $tmp 'a.png'), 0, -1, 880, 240, 80, 60); $p7.PictureFormat.ColorType = 2; $p7.Line.Visible = -1; $p7.Line.Weight = 2
    $p8 = $ws.Shapes.AddShape(1, 880, 100, 90, 70); $p8.Fill.UserPicture([string](Join-Path $tmp 'a.png'))
    $p9 = $ws.Shapes.AddShape(1, 880, 180, 90, 50); $p9.Fill.UserTextured([string](Join-Path $tmp 'c.bmp'))
    $p10 = $ws.Shapes.AddShape(1, 700, 320, 60, 60); $p10.Fill.Patterned(7); $p10.Fill.ForeColor.RGB = 0x0000FF; $p10.Fill.BackColor.RGB = 0xFFFFFF
  }
  # a form control and a chart in a group
  Try-Step 'button' { $b = $ws.Buttons().Add(700, 400, 90, 28); $b.Text = 'Button' }
  Try-Step 'chartgroup' {
    $co = $ws.ChartObjects().Add(700, 440, 200, 120); $co.Chart.ChartType = 51; $co.Chart.SetSourceData($ws.Range('A1:B6')); $co.Name = 'cx'
    $r = $ws.Shapes.AddShape(1, 920, 440, 40, 30); $r.Name = 'rr'
    [void]$ws.Shapes.Range(@('cx', 'rr')).Group()
  }
  Save-Xls $wb 'f-shapes'
}
catch { $log += "FATAL-F`t$($_.Exception.Message -replace '[\r\n\t]+', ' ')" }

try { $xl.Quit() } catch { }
[void][Runtime.InteropServices.Marshal]::ReleaseComObject($xl)
$log | ForEach-Object { Write-Output $_ }
Write-Output "done`t$($log.Count) lines"
