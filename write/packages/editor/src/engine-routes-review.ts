// Rotas da frente R da leva 5 do engine="rust" (comentários, revisões, campos/referências, localizar, ortografia, estilos, área de transferência sem evento, teclas Alt). Só dados, como engine-routes.ts (que junta esta tabela à
// principal): nenhum import do motor. Os handlers ficam em rust-engine-review.ts com o MESMO nome do método.
// Registro: core/docs/modules/uo-bindings.md "Wave 5 — R" (as rotas, ED-301..ED-3xx, o que fica de fora).
export const ROUTES_REVIEW = {
  /** métodos públicos atendidos pela sessão (handler em rust-engine-review.ts) */
  public: [
    // controle de alterações
    'toggleTrackChanges', 'isTrackingChanges', 'setMarkupMode', 'getMarkupMode', 'setMarkupOptions', 'getMarkupOptions',
    'revisionAuthors', 'authorColor', 'getRevisions', 'revisionSpans', 'acceptRevisionAt', 'rejectRevisionAt', 'acceptChange',
    'rejectChange', 'acceptAllChanges', 'rejectAllChanges', 'nextChange', 'previousChange', 'selectRevision', 'revisionAtCaret',
    'revisionAnchors',
    // comentários
    'getComments', 'addComment', 'removeComment', 'goToComment', 'commentAnchorPx',
    // localizar, substituir, ir para, painel de navegação
    'findNextMatch', 'replaceMatch', 'replaceAllMatches', 'highlightMatches', 'selectAllMatches', 'goTo', 'goToPage',
    'getSelectionRanges', 'setSelectionRanges', 'search', 'clearSearch', 'goToSearchResult', 'getHeadings', 'currentHeading',
    'goToHeading', 'selectHeading', 'promoteHeading', 'demoteHeading', 'moveHeading', 'insertHeading', 'deleteHeading',
    // campos, sumário, legendas, referência cruzada, índice de ilustrações
    'insertField', 'insertDateTime', 'dateTimeFormats', 'insertDocumentInfo', 'insertAlignmentTab', 'updateFields', 'canInsertField',
    'insertTableOfContents', 'insertTocTemplate', 'updateToc', 'removeTableOfContents', 'setTocLevel', 'markTocEntry', 'getTocLevel',
    'hasTableOfContents', 'inTableOfContents', 'getTableOfContentsOptions', 'tocTemplates',
    'getCaptionLabels', 'addCaptionLabel', 'deleteCaptionLabel', 'getCaptionNumbering', 'setCaptionNumbering', 'getAutoCaption',
    'setAutoCaption', 'captionTarget', 'nextCaptionNumber', 'insertCaption', 'crossReferenceKinds', 'getCrossReferenceItems',
    'insertCrossReference', 'getTableOfFiguresOptions', 'insertTableOfFigures', 'updateTableOfFigures',
    // indicadores, hiperlinks
    'getBookmarks', 'hasBookmark', 'insertBookmark', 'deleteBookmark', 'goToBookmark',
    'insertHyperlink', 'setLink', 'removeLink', 'getLink', 'getLinkTargets', 'navigateTo',
    // estilos
    'listStyles', 'paragraphStyles', 'styleFormatting', 'stylesKey', 'currentStyles', 'inspectStyle',
    'selectionStyleFormat', 'createStyle', 'modifyStyle', 'setDefaultFont', 'updateStyleFromSelection', 'renameStyle', 'deleteStyle',
    'setStyleInGallery', 'clearParagraphStyle', 'clearCharacterStyle', 'knownFontFamilies',
    // ortografia
    'toggleSpellCheck', 'spellingAtCaret', 'spellingSuggestions', 'replaceSpelling', 'ignoreSpelling', 'addToDictionary', 'nextMisspelling',
    // área de transferência sem evento
    'copy', 'cut', 'paste', 'pastePlain', 'pasteSpecialFormats', 'pasteSpecial', 'clipboardItems', 'pasteClipboardItem',
    'pasteAllClipboardItems', 'removeClipboardItem', 'clearClipboardItems', 'onClipboardChange',
  ] as readonly string[],
  /** métodos privados do elemento que o motor assume (os adornos de tela: `el[m] = () => {}` desliga os dois motores) */
  private: ['paintEngineMarks', 'paintEngineSpelling', 'paintSearchHits'] as readonly string[],
  /** ids de comando das barras cujo run só chega a métodos roteados ou da casca */
  commands: [
    // Editar (sem evento de área de transferência), Localizar
    'cut', 'copy', 'paste', 'pastePlain', 'find', 'replace', 'find:advanced', 'goto', 'goto:f5', 'hl:all', 'hl:clear', 'in:sel', 'in:main',
    'paste:strip', 'paste:special', 'paste:defaults', 'clipboard:pane', 'clip:paste', 'clip:delete',
    // Exibir: painel de navegação, comentários
    'view:navpane', 'navigation', 'nav:replace', 'nav:clear', 'nav:levels:all', 'nav:promote', 'nav:demote', 'nav:newBefore', 'nav:newAfter',
    'nav:newSub', 'nav:delete', 'nav:select', 'nav:expandAll', 'nav:collapseAll', 'nav:levels', 'comments',
    // Inserir: link, comentário, campos, indicador, referências
    'link', 'comment', 'field:page', 'field:numpages', 'field:date', 'bookmark', 'crossref', 'caption', 'tof', 'tof:update',
    'toc', 'toc:custom', 'toc:remove', 'toc:mark', 'toc:addText',
    // Estilos
    'styles', 'styles:pane', 'style:inspector', 'style:new', 'style:modifyCurrent', 'style:updateCurrent', 'style:h1', 'style:h2', 'style:h3',
    'style:create', 'style:update', 'style:modify', 'style:rename', 'style:ungallery', 'style:togallery', 'style:delete',
    // Revisão
    'comment:prev', 'comment:next', 'comment:delete', 'comment:deleteAll', 'spelling', 'spellAsYouType', 'trackChanges',
    'markup:simple', 'markup:all', 'markup:none', 'markup:original', 'show:insdel', 'show:formatting',
    'balloons:revisions', 'balloons:inline', 'balloons:mixed', 'people:all', 'reviewPane', 'reviewPane:vertical', 'reviewPane:horizontal',
    'change:accept', 'change:acceptNext', 'change:acceptOne', 'change:acceptShown', 'change:acceptAll', 'change:acceptAllStop',
    'change:reject', 'change:rejectNext', 'change:rejectOne', 'change:rejectShown', 'change:rejectAll', 'change:rejectAllStop',
    'change:prev', 'change:next',
  ] as readonly string[],
  /** consultas desligadas que respondem o neutro sem rodar o código TS */
  neutral: [] as readonly string[],
};
