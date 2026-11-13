// Rotas da frente A2 da leva 7 do engine="rust" (listas, colar, tabelas, objetos e a página: o que as frentes da leva 5
// deixaram desligado). Só dados, como engine-routes.ts (que junta esta tabela à principal): nenhum import do motor. Os
// handlers ficam em rust-engine-completion.ts com o MESMO nome do método.
// Registro: core/docs/modules/uo-bindings.md "Wave 7 — A2: lists, paste, tables, objects and the page".
export const ROUTES_COMPLETION = {
  /** métodos públicos atendidos pela sessão (handler em rust-engine-completion.ts) */
  public: [
    // listas (ListCmd; as consultas de src/session/shell_completion.rs; markerPreview: o código TS da casca)
    'applyListLibrary', 'applyMultilevelLibrary', 'applyListLevels', 'applyListStyle', 'createListStyle', 'modifyListStyle',
    'defineListLevel', 'defineMultilevelList', 'setListLevel', 'restartNumbering', 'continueNumbering', 'setNumberingValue',
    'listInfo', 'documentLists', 'listStylesInDocument', 'numberingValueInfo', 'listLibrary', 'markerPreview',
    // colar
    'pasteCells', 'hasCellsClip', 'pasteData', 'changeImageFromClipboard',
    // tabelas
    'insertQuickTable', 'quickTables', 'insertSpreadsheet', 'canEmbedSpreadsheet', 'editEmbeddedObject', 'setTableTool', 'tableStyleList', 'tableStyleXml',
    'tableStyleBases', 'newTableStyleId', 'saveTableStyle', 'renderTableStyleXmlThumb',
    // objetos e a página
    'startEditWrapPoints', 'endEditWrapPoints', 'editingWrapPoints', 'startTransparentColorPick', 'editSelectedTextbox',
    'textboxTemplates', 'saveSelectionToTextboxGallery', 'removeTextboxGalleryItem', 'setSelectObjectsMode', 'isSelectObjectsMode', 'pageFillOptions',
    'shapeGradientVariations', 'resolveDmlColor',
    // as previas (o mini-documento da sessão: rust-completion-previews.ts)
    'paintCoverTemplate', 'paintWatermarkTemplate', 'headerFooterTemplateXml',
    // the page: the body section's properties (the session's sectPr as the TS object)
    'sectPrAt',
  ] as readonly string[],
  /** métodos privados do elemento que o motor assume */
  private: [] as readonly string[],
  /** ids de comando das barras cujo run só chega a métodos roteados ou da casca (engine-completion.test.ts confere) */
  commands: [
    'list:defineBullet', 'list:defineNumber', 'list:setValue', 'list:defineMultilevel', 'list:defineStyle', 'list:indents',
    'list:restart', 'list:continue',
    'paste:keep', 'paste:merge', 'paste:picture', 'paste:text', 'paste:cells:overwrite', 'paste:cells:rows', 'paste:cells:nest', 'paste:cells:text',
    'table:excel', 'table:style:new', 'table:style:modify', 'table:painter', 'table:draw', 'table:eraser', 'dlg:pageBorders',
    'pic:wrap:editPoints', 'pic:transparentColor', 'pic:change:clip', 'shp:editText', 'select:objects',
    ...Array.from({ length: 10 }, (_, i) => `toc:level${i}`),
  ] as readonly string[],
  /** consultas desligadas que respondem o neutro sem rodar o código TS */
  neutral: [] as readonly string[],
};
