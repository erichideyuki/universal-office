// Rotas da frente S da leva 5 do engine="rust" (tabelas, cabeçalho/rodapé, notas, cursor na caixa de texto). Só dados, como engine-routes.ts (que junta esta tabela à
// principal): nenhum import do motor. Os handlers ficam em rust-engine-stories.ts com o MESMO nome do método.
// Registro: core/docs/modules/uo-bindings.md "Wave 5 — S: stories".
export const ROUTES_STORIES = {
  /** métodos públicos atendidos pela sessão (handler em rust-engine-stories.ts) */
  public: [
    // tabelas
    'insertTable', 'insertRowAbove', 'insertRowBelow', 'insertColumnLeft', 'insertColumnRight', 'insertCells', 'deleteCells',
    'deleteRow', 'deleteColumn', 'deleteTable', 'mergeCells', 'splitCell', 'splitTable', 'selectCell', 'selectRow', 'selectColumn',
    'selectTable', 'distributeRows', 'distributeColumns', 'setRowHeight', 'setColumnWidth', 'autoFitTable', 'setCellAlignment',
    'setTextDirection', 'cycleTextDirection', 'toggleRepeatHeaderRows', 'setTableCellMargins', 'sort', 'sortInfo',
    'convertTableToText', 'convertTextToTable', 'textToTableGuess', 'setTableStyle', 'setTableStyleOptions', 'setCellShading',
    'setTableBorderSides', 'setCellBorders', 'setTableProperties', 'insertFormula', 'suggestFormula', 'previewFormula',
    'resizeTableColumn', 'resizeTableRow', 'editingTableCell', 'getTableState',
    // tabelas: estado da casca pelo código TS (ED-105)
    'getBorderPen', 'setBorderPen', 'getTableTool', 'tableGridlinesShown', 'showTableGridlines', 'tableStyleListNow',
    'renderTableStyleThumb', 'stylesVersion',
    // cabeçalho e rodapé
    'enterHeaderFooter', 'exitHeaderFooter', 'editingHeaderFooter', 'getHeaderFooterOptions', 'getPageNumberFormat',
    'setDifferentFirstPage', 'setDifferentOddEven', 'setHeaderFooterDistance', 'setLinkToPrevious', 'removeHeaderFooter',
    'removePageNumbers', 'setPageNumberFormat', 'headerFooterTemplates', 'insertHeaderFooterTemplate', 'paintHeaderFooterTemplate',
    'goToHeaderFooter', 'nextHeaderFooter', 'previousHeaderFooter', 'showDocumentText', 'documentTextShown',
    // notas
    'insertFootnote', 'insertEndnote', 'openNote', 'goToNoteRef', 'nextNote', 'showNotes', 'setNoteOptions', 'getNoteOptions',
    'convertNotes', 'noteCount', 'canInsertNote', 'editingNote',
    // o texto da caixa de texto (o objeto é da frente O)
    'exitTextbox', 'editingTextbox',
  ] as readonly string[],
  /** métodos privados do elemento que o motor assume */
  private: [] as readonly string[],
  /** ids de comando das barras cujo run só chega a métodos roteados ou da casca */
  commands: [
    // Inserir / Referências: cabeçalho, rodapé, notas (chrome-commands.ts; os diálogos de notas de chrome-dialogs.ts)
    'header', 'footer', 'hf:close', 'footnote', 'endnote', 'note:next', 'note:prev', 'note:nextEnd', 'note:prevEnd', 'note:show',
    'dlg:notes', 'note:convert', 'note:goRef', 'note:toEndnote', 'note:toFootnote',
    // tabelas (chrome-table.ts; os diálogos de chrome-table-dialogs.ts)
    'table', 'table:insertDlg', 'table:fromText', 'table:select:cell', 'table:select:row', 'table:select:column', 'table:select:table',
    'table:gridlines', 'table:properties', 'table:propertiesDlg', 'row:above', 'row:below', 'col:left', 'col:right',
    'cells:insertDlg', 'cells:deleteDlg', 'col:delete', 'row:delete', 'table:delete', 'cells:merge', 'cells:split', 'table:split',
    'table:autofit:content', 'table:autofit:window', 'table:autofit:fixed', 'rows:distribute', 'cols:distribute', 'cell:textdir',
    'cell:margins', 'table:headerRows', 'table:toText', 'sort', 'table:formula', 'table:style:clear', 'table:styles',
    'table:shading', 'table:borders', 'table:borderStyles', 'table:select', 'table:autofit', 'table:deleteMenu',
    'tableFmt', 'tableFmt:select', 'tableFmt:autofit', 'tableFmt:align', 'ctxTableInsert', 'ctxTableDelete', 'ctxTableAlign',
    'ctxTableAutofit',
    // os ids de modelo de chrome-table.ts (ALIGN, LOOK, BORDERS): Alinhamento da célula (setCellAlignment), Opções de Estilo
    // de Tabela (setTableStyleOptions), os itens do menu Bordas (setCellBorders); o teste confere com as listas de lá
    ...['tl', 'tc', 'tr', 'cl', 'cc', 'cr', 'bl', 'bc', 'br'].map((k) => `cell:align:${k}`),
    ...['header', 'first', 'total', 'last', 'bandedRows', 'bandedCols'].map((k) => `table:look:${k}`),
    ...['bottom', 'top', 'left', 'right', 'none', 'all', 'outside', 'inside', 'insideH', 'insideV', 'tl2br', 'tr2bl'].map((k) => `table:border:${k}`),
    // Cabeçalho e Rodapé (chrome-hf.ts)
    'hf:editHeader', 'hf:editFooter', 'hf:removeHeader', 'hf:removeFooter', 'hf:removePageNumbers', 'dlg:pageNumberFormat',
    'dlg:hfOptions', 'hf:gotoHeader', 'hf:gotoFooter', 'hf:prev', 'hf:next', 'hf:link', 'hf:firstPage', 'hf:oddEven', 'hf:showText',
    'hf:headerMenu', 'hf:footerMenu', 'hf:pgnumMenu',
    // ids de chrome-hf.ts / chrome-table.ts cujos métodos a frente R roteia (campos, data e hora, informações do documento,
    // tabulação de alinhamento): o comando só chega a eles (o teste (b3) os acha; registro ED-109)
    'fields:update', 'dlg:dateTime', 'dlg:alignTab', 'docinfo:author', 'docinfo:title', 'docinfo:fileName', 'docinfo:filePath',
  ] as readonly string[],
  /** consultas desligadas que respondem o neutro sem rodar o código TS */
  neutral: [] as readonly string[],
};
