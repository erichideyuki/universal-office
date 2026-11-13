// Rotas da frente A1 da leva 7 do engine="rust" (Arquivo, Exibir, Fonte e Parágrafo: o que a leva 6 deixou desligado).
// Só dados, como engine-routes.ts (que junta esta tabela à principal): nenhum import do motor. Os handlers ficam em
// rust-engine-format.ts com o MESMO nome do método. Registro: core/docs/modules/uo-bindings.md "Wave 7 — A1".
export const ROUTES_FORMAT = {
  /** métodos públicos atendidos pela sessão ou pela casca (handler em rust-engine-format.ts) */
  public: [
    // Arquivo: imprimir (save, load, loadOdt, exportOdt/Html/Markdown/Xps já estão na tabela principal e setView na da frente O:
    // os handlers desta frente os substituem em installRoutes, sem repetir o nome)
    'print', 'printPages',
    // Exibir: régua, linhas de grade, indicadores, os modos de exibição
    'getRulerInfo', 'showGridlines', 'gridlinesShown', 'showBookmarks', 'bookmarksShown',
    // Fonte: diálogo, efeitos, OpenType, cor do sublinhado, degradê, cores do tema, Pincel
    'setFontFormat', 'setOpenType', 'setUnderlineColor', 'setTextEffects', 'applyTextEffectPreset', 'setTextGradient', 'textEffects',
    'documentThemeColors', 'copyFormat', 'cancelFormatPainter', 'painterActive', 'painterSticky',
    // Parágrafo: diálogos Parágrafo/Tabulação/Bordas, botão Bordas, linha horizontal
    'setParagraphIndents', 'setSpecialIndent', 'setIndentRight', 'setTabStops', 'setDefaultTabStop', 'setParagraphFlow', 'setParagraphLayout',
    'setParagraphBorders', 'paragraphBorderPreset', 'toggleBorder', 'insertHorizontalLine', 'getParagraphIndents', 'getTabStops',
    'getDefaultTabStop', 'getParagraphFlow', 'getParagraphLayout', 'getParagraphBorders', 'paragraphBorderSides',
    // Selecionar (Selecionar Objetos — o modo, o laço e os vários objetos — é da frente A2), hifenização manual
    'selectSimilarFormatting',
    'nextManualHyphenation', 'applyManualHyphenation', 'skipManualHyphenation', 'endManualHyphenation',
    // localizar simples, contagem, links, inlines de plugin, campos externos, estilos
    'findNext', 'replaceAll', 'countMatches', 'getLinks', 'getCustomInlines', 'insertCustomInline', 'hasExternalFields', 'styleKeyOf',
    'ensureBuiltinStyle', 'paintTocTemplate', 'tocEntrySuggestion', 'hasAuthor', 'validBookmarkName',
  ] as readonly string[],
  /** métodos privados do elemento que o motor assume (os adornos de tela e a impressão) */
  private: [
    'ensureImagesDecoded', 'paintBookmarkMarks', 'paintTocControl', 'paintEngineInlines', 'paintDocGrid', 'tocTabMouseDown',
  ] as readonly string[],
  /** ids de comando das barras cujo run só chega a métodos roteados ou da casca */
  commands: [
    // Arquivo
    'fontsInFile', 'export:odt', 'export:html', 'export:md', 'export:xps', 'print', 'compare', 'enableEditing', 'wordCount',
    'mailmerge', 'mergefield', 'fontAccess',
    // Exibir
    'view:web', 'view:draft', 'view:outline', 'view:ruler', 'view:gridlines', 'view:bookmarks',
    // Fonte
    'color', 'color:gradient', 'dlg:font', 'underline:more', 'painter', 'fx:pane',
    // Parágrafo
    'borders', 'dlg:paragraph', 'dlg:tabs', 'dlg:borders', 'home:border:bottom', 'home:border:top', 'home:border:left',
    'home:border:right', 'home:border:none', 'home:border:all', 'home:border:outside', 'home:border:inside', 'home:border:insideH',
    'home:border:insideV', 'home:border:tl2br', 'home:border:tr2bl', 'home:borders', 'home:hrule', 'select:similar', 'hyph:manual',
    // (select:objects e toc:level0..9 estão na tabela da frente A2)
  ] as readonly string[],
  /** consultas desligadas que respondem o neutro sem rodar o código TS */
  neutral: [] as readonly string[],
};
