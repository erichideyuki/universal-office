// A tabela de rotas do `<universal-write engine="rust">` (registro core/docs/modules/uo-bindings.md
// "Milestone 2: the editor switch"). Só dados: nenhum import do motor — o caminho TS (o padrão) não carrega nada
// daqui além destes nomes, e o motor Rust (`rust-engine.ts`, carregado por import() dinâmico) os aplica.
//
// Desenho (decisão D1 do registro): sem refatorar as 23 mil linhas do elemento, o motor Rust SOMBREIA os métodos na
// INSTÂNCIA (Object.defineProperty): os de ROUTED vão à sessão wasm; os de SHELL continuam o código TS (só tocam o
// estado da casca: zoom, tema, plugins, avisos, barras); TODO o resto é fechado por padrão — um mutador vira no-op e
// uma consulta responde o valor do TS com o modelo TS vazio (o documento TS nunca é carregado com engine="rust").
// Método público novo fica desligado até ser classificado aqui (o teste engine-switch.test.ts imprime as contagens).

import { ROUTES_OBJECTS } from './engine-routes-objects.js';
import { ROUTES_REVIEW } from './engine-routes-review.js';
import { ROUTES_STORIES } from './engine-routes-stories.js';
import { ROUTES_FORMAT } from './engine-routes-format.js'; // wave 7 (A1)
import { ROUTES_COMPLETION } from './engine-routes-completion.js'; // wave 7 (A2)

/** As tabelas das frentes da leva 5 (cada uma no seu arquivo, para não disputarem este). */
const FRONTS = [ROUTES_STORIES, ROUTES_OBJECTS, ROUTES_REVIEW, ROUTES_FORMAT, ROUTES_COMPLETION]; // wave 7 (A2): ROUTES_COMPLETION

/** Métodos públicos que a sessão wasm atende (handler em rust-engine.ts ou rust-engine-<frente>.ts com o mesmo nome). */
export const ROUTED_PUBLIC: readonly string[] = [
  // abrir, novo, salvar, sujo
  'load', 'loadDocx', 'loadOdt', 'newDocument', 'saveDocx', 'saveDocxWithFonts', 'save', 'exportPdf', 'isDirty', 'setDirty',
  'isProtected', 'enableEditing', 'isEmpty',
  // os formatos que esta onda não exporta: um erro claro, não `undefined` (ED-18)
  'exportOdt', 'exportXps', 'exportHtml', 'exportMarkdown',
  // fontes no arquivo: as opções do DOCUMENTO (a sessão as lê do pacote e as grava)
  'getFontEmbedding', 'setFontEmbedding',
  // pintura, rolagem
  'refresh', 'repaint', 'paintPageThumbnail', 'layoutPageCount', 'currentPage', 'pageWidthZoom', 'stats',
  // seleção, cursor
  'setSelection', 'getSelectionState', 'caretRectAt', 'selectAll', 'selectedText',
  // desfazer
  'undo', 'redo',
  // formatação de caractere
  'toggleRunProp', 'toggleUnderline', 'setUnderline', 'underlineButtonStyle', 'setFontFamily', 'setFontSize', 'growFont', 'setColor',
  'setHighlight', 'clearFormatting', 'clearCharacterFormatting', 'changeCase', 'toggleVertAlign', 'setCharSpacing',
  // parágrafo
  'setAlignment', 'indent', 'setLineSpacing', 'setSpaceBefore', 'setSpaceAfter', 'setShading', 'clearParagraphFormatting',
  // listas, estilos ('currentStyles' é da frente R, engine-routes-review.ts)
  'toggleList', 'removeList', 'changeListLevel', 'applyStyle',
  // área de transferência ('copy', 'cut', 'paste', 'pastePlain' são da frente R, engine-routes-review.ts: os tratadores
  // das frentes ganham dos métodos do RustEditor em installRoutes; onda 7c, S: sem nomes repetidos)
  'pasteText', 'pasteOptions', 'repaste', 'pasteButtonRect',
  // a revisão de memória (engineStats) e o fim
  'focusEditor',
  ...FRONTS.flatMap((f) => f.public),
];

/** Métodos privados do elemento que o motor Rust assume (o código TS que os chama passa a usar a sessão). */
export const ROUTED_PRIVATE: readonly string[] = [
  'refreshView', 'refreshSelection', 'rebuildCanvases', 'paint', 'positionProxy', 'scrollCaretIntoView', 'onFontsResolved',
  'queueRefresh', 'type', 'moveCaret', 'moveWord', 'deleteWord', 'hitTest', 'hitTestRaw', 'popHistory', 'pressEnter',
  'updateTouchHandles', 'layout',
  ...FRONTS.flatMap((f) => f.private),
];

/** Métodos públicos que continuam o código TS (só estado da casca). */
export const SHELL: readonly string[] = [
  'constructor', 'connectedCallback', 'disconnectedCallback', 'attributeChangedCallback', 'initialize',
  'getSelection', 'setZoom', 'getZoom', 'getView', 'toggleMarks', 'setDarkPage', 'setUiColorMode',
  'use', 'unuse', 'pluginIds', 'setNotice', 'isSidePanelEvent', 'sidePanelWidths', 'spellCheckEnabled',
  'knownFontFamilies', 'listAvailableFonts', 'hasHostSystemFonts', 'requestSystemFonts',
  'ensureFontLoaded', 'canvasFont', 'resolvedFormattingPolicy', 'onClipboardChange',
];

/**
 * Mutadores (pelo nome): desligados com engine="rust" viram no-op. O resto dos públicos não classificados são
 * consultas: respondem o valor do código TS com o modelo TS vazio (null, [], false… — o que a barra já sabe ler),
 * e uma exceção responde o valor neutro de `neutralOf`.
 */
export const MUTATOR = /^(set|toggle|apply|insert|delete|remove|add|accept|reject|clear|change|create|modify|rename|update|paste|cut|copy|merge|split|move|promote|demote|restart|continue|convert|define|distribute|resize|rotate|flip|crop|align|arrange|reset|replace|mark|skip|end|start|cancel|edit|enter|exit|goTo|navigate|next|previous|select|save|export|print|load|draw|link|break|enable|ensure|ignore|grow|indent|sort|compress|repaste|cycle|show|highlight|find|search|open|repaint|render|paint|unuse|use|drop|fill|nudge|undo|redo|focus|enableEditing|initialize|refresh|toc|copyFormat|cancelFormatPainter|addToDictionary|replaceSpelling|autoFit|saveTableStyle|requestSystemFonts|breakTextboxLink|linkTextboxTo|createTextboxLink|cancelTextboxLink|drawTextbox|cancelDrawTextbox|paragraphBorderPreset)/;

/** Consultas cujo nome começa como um mutador (`editingTextbox`, `selectedImage`, `pasteOptions`…). */
export const QUERY = /^(editing|selected|selection|linking|drawing|cropping)|^(pasteOptions|pasteSpecialFormats|pasteButtonRect|sortInfo|nextCaptionNumber|markerPreview|tocEntrySuggestion|tocTemplates)$/;

/** O método público desligado é um mutador (no-op) e não uma consulta? */
export function isMutator(name: string): boolean {
  return MUTATOR.test(name) && !QUERY.test(name);
}

/**
 * Consultas desligadas que respondem SEMPRE o neutro (sem rodar o código TS): sobre o modelo TS vazio elas dariam um
 * valor falso do documento (a lista embutida de estilos do TS, o `tracking` do TS) — declarado (ED-24 do registro).
 */
export const NEUTRAL_QUERIES: readonly string[] = ['listStyles', 'paragraphStyles', 'listStylesInDocument', 'isTrackingChanges']
  .filter((n) => !FRONTS.some((f) => f.public.includes(n)))
  .concat(FRONTS.flatMap((f) => f.neutral));

/**
 * O valor neutro de uma consulta desligada cujo código TS lança com o modelo TS vazio (a barra lê estes): listas
 * vazias pelo nome (plural), os objetos que a barra desestrutura, null no resto.
 */
export function neutralOf(name: string): unknown {
  const fixed: Record<string, () => unknown> = {
    listStylesInDocument: () => [],
    getMarkupOptions: () => ({ insertionsDeletions: true, formatting: true, balloons: 'mixed', authors: null }),
    getTableState: () => null,
    getSection: () => null,
    listInfo: () => null,
    editingHeaderFooter: () => null,
    editingNote: () => null,
    editingTextbox: () => false,
    editingTableCell: () => false,
    isTrackingChanges: () => false,
    selectedObjectCount: () => 0,
  };
  const f = fixed[name];
  if (f) return f();
  return /^(get|list)[A-Z]\w*s$|(Items|Templates|Gallery|Texts|Kinds|Formats|Labels|Fonts|Styles|Spans|Anchors|Authors)$/.test(name) ? [] : null;
}

/**
 * Ids de comando das barras (chrome-*.ts) cujo `run` só chega a métodos ROUTED ou SHELL; com engine="rust" os outros
 * ficam visíveis e DESLIGADOS (decisão D7). Lidos um a um no `run` de cada comando.
 */
export const ROUTED_COMMAND_IDS: ReadonlySet<string> = new Set([
  ...FRONTS.flatMap((f) => f.commands),
  // Arquivo (app.newDocument/open/save chamam newDocument/load/save/saveDocxWithFonts; export:pdf chama save('pdf');
  // Salvar Como .odt chama exportOdt, que lança um erro claro: a barra conta como falha, nada é gravado — ED-18)
  'new', 'open', 'save', 'saveAs', 'export:pdf',
  // Editar (recortar/copiar/colar pelo menu ou botão chamam cut()/copy()/paste() SEM evento de área de transferência,
  // que esta onda não atende: desligados; Ctrl+X/C/V e Ctrl+Shift+V funcionam pelos eventos nativos — ED-16)
  'undo', 'redo', 'selectAll',
  // Exibir (casca)
  'marks', 'zoomin', 'zoomout', 'zoom100', 'toolbar:ribbon', 'toolbar:compact', 'statusbar', 'focus',
  // Fonte
  'bold', 'italic', 'underline', 'doubleStrike', 'hidden', 'strike', 'sup', 'sub', 'allCaps', 'smallCaps',
  'case:upper', 'case:lower', 'case:sentence', 'case:title', 'case:toggle', 'case:next', 'grow', 'shrink', 'highlight', 'clear',
  'underline:single', 'underline:double', 'underline:thick', 'underline:dotted', 'underline:dash', 'underline:wave', 'underline:words',
  // Parágrafo
  'align:left', 'align:center', 'align:right', 'align:both', 'outdent', 'indent', 'shading',
  'list:bullet', 'list:decimal',
  // Estilos
  'style:normal', 'style:clearChar', 'style:clearPara',
  // Ajuda
  'shortcuts', 'about',
]);
