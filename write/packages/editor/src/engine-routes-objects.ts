// Rotas da frente O da leva 5 do engine="rust" (imagens, formas/caixas de texto (objeto), configurar página, exibições, menu de contexto). Só dados, como engine-routes.ts (que junta esta tabela à
// principal): nenhum import do motor. Os handlers ficam em rust-engine-objects.ts com o MESMO nome do método.
// Registro: core/docs/modules/uo-bindings.md "Wave 5 — O: objects and the page".
export const ROUTES_OBJECTS = {
  /** métodos públicos atendidos pela sessão (handler em rust-engine-objects.ts) */
  public: [
    // a página: Configurar Página, colunas, quebras, números de linha, hifenização
    'setPageSetup', 'setColumns', 'insertPageBreak', 'insertColumnBreak', 'insertSectionBreak', 'insertBlankPage', 'setSectionStart',
    'setVerticalAlignment', 'setLineNumbering', 'setLineNumberingOptions', 'toggleLineNumberSuppression', 'lineNumbersSuppressed',
    'getLineNumbering', 'getSection', 'setPageBorders', 'getPageBorders', 'setHyphenation', 'setHyphenationOptions', 'getHyphenation',
    // Design: Cor da Página, marca-d'água, folha de rosto
    'setPageColor', 'setPageFill', 'pageBackground', 'setWatermark', 'removeWatermark', 'watermark', 'watermarkTexts', 'watermarkGallery',
    'insertCoverPage', 'removeCoverPage', 'hasCoverPage', 'coverPages',
    // imagens
    'insertImage', 'selectedImage', 'selectedImagePreview', 'deleteSelectedImage', 'setImageWrap', 'setImagePosition', 'setImageMoveWithText',
    // (setImageLockAnchor e setImageEffect: recuados com 4 espaços no fonte, fora de engine-public.ts — rodavam o corpo TS; ED-212)
    'setImageLockAnchor', 'alignImage', 'setImageSize', 'setImageRotation', 'rotateImage', 'flipImage', 'setImageAltText', 'arrangeImage',
    'setImageLayout', 'setImageBorder', 'setImageFormat', 'setImageEffect', 'cropImageFillFit', 'cropImageToAspect', 'cropImageToShape',
    'setImageTransparency', 'setImageRecolor', 'setImageTransparentColor', 'setImageCrop', 'resetImage', 'setImageAdjust', 'changeImage',
    'changeImageFromFile', 'compressImages', 'startImageCrop', 'endImageCrop', 'croppingImage',
    // formas e caixas de texto como objetos
    'selectedShape', 'setShapeFill', 'setShapePictureFill', 'setShapeOutline', 'setShapeFormat', 'setShapeStyle', 'changeShape',
    'setTextboxOptions', 'setShapeEffect', 'deleteSelectedShape', 'setShapeWrap', 'setShapePosition', 'setShapeMoveWithText',
    'setShapeLockAnchor', 'setShapeSize', 'setShapeRotation', 'setShapeAltText', 'setShapeLayout', 'alignShape', 'arrangeShape', 'flipShape',
    'rotateShape', 'alignObjects', 'insertTextbox', 'drawTextbox', 'cancelDrawTextbox', 'drawingTextbox', 'createTextboxLink',
    'cancelTextboxLink', 'linkingTextbox', 'linkTextboxTo', 'breakTextboxLink', 'setAsDefaultTextbox',
    // Painel de Seleção
    'selectionPaneItems', 'selectPaneItem', 'selectPaneItems', 'renamePaneItem', 'setPaneItemHidden', 'selectedObjectCount',
    // exibições (print e read; ED-209)
    'setView',
  ] as readonly string[],
  /** métodos privados do elemento que o motor assume */
  private: [] as readonly string[],
  /** ids de comando das barras cujo run só chega a métodos roteados ou da casca */
  // (computed from the bars by the run of each command against a recording editor, engine-objects.test.ts; not:
  // pic:wrap:editPoints, pic:transparentColor, pic:change:clip, shp:editText, textbox:save, textbox:remove, hyph:manual,
  // view:web/draft/outline — their methods stay disabled, ED-209/ED-216)
  commands: [
    'view:print', 'view:read', 'image', 'pagebreak', 'textbox', 'sectionbreak', 'sectionbreak:continuous',
    'sectionbreak:evenPage', 'sectionbreak:oddPage', 'columnbreak', 'columns:one', 'columns:two', 'columns:three', 'columns:left',
    'columns:right', 'dlg:columns', 'dlg:page', 'image:delete', 'pic:wrap:inline', 'pic:wrap:square', 'pic:wrap:tight',
    'pic:wrap:through', 'pic:wrap:topAndBottom', 'pic:wrap:behind', 'pic:wrap:front', 'pic:wrap:moveWithText',
    'pic:wrap:fixOnPage', 'pic:wrap:more', 'pic:pos:inline', 'pic:pos:top-left', 'pic:pos:top-center', 'pic:pos:top-right',
    'pic:pos:center-left', 'pic:pos:center-center', 'pic:pos:center-right', 'pic:pos:bottom-left', 'pic:pos:bottom-center',
    'pic:pos:bottom-right', 'pic:pos:more', 'pic:forward', 'pic:front', 'pic:frontOfText', 'pic:backward', 'pic:back',
    'pic:behindText', 'pic:rotate:right', 'pic:rotate:left', 'pic:flip:v', 'pic:flip:h', 'pic:rotate:more', 'pic:crop:mode',
    'pic:crop:fill', 'pic:crop:fit', 'pic:crop:ratio:1:1', 'pic:crop:ratio:2:3', 'pic:crop:ratio:3:4', 'pic:crop:ratio:3:5',
    'pic:crop:ratio:4:5', 'pic:crop:ratio:3:2', 'pic:crop:ratio:4:3', 'pic:crop:ratio:5:3', 'pic:crop:ratio:5:4',
    'pic:crop:ratio:16:9', 'pic:crop:ratio:16:10', 'pic:layout', 'pic:selectionPane', 'pic:align:left', 'pic:align:center',
    'pic:align:right', 'pic:align:top', 'pic:align:middle', 'pic:align:bottom', 'pic:align:to:page', 'pic:align:to:margin',
    'pic:format', 'pic:format:line', 'pic:format:effects', 'pic:format:layout', 'pic:format:picture', 'pic:fx:options',
    'pic:adjust:options', 'pic:alt', 'textbox:draw', 'textbox:drawVertical', 'shp:wrap:inline', 'shp:wrap:square',
    'shp:wrap:tight', 'shp:wrap:through', 'shp:wrap:topAndBottom', 'shp:wrap:behind', 'shp:wrap:front', 'shp:wrap:moveWithText',
    'shp:wrap:fixOnPage', 'shp:wrap:more', 'shp:pos:inline', 'shp:pos:top-left', 'shp:pos:top-center', 'shp:pos:top-right',
    'shp:pos:center-left', 'shp:pos:center-center', 'shp:pos:center-right', 'shp:pos:bottom-left', 'shp:pos:bottom-center',
    'shp:pos:bottom-right', 'shp:pos:more', 'shp:forward', 'shp:front', 'shp:frontOfText', 'shp:backward', 'shp:back',
    'shp:behindText', 'shp:rotate:right', 'shp:rotate:left', 'shp:flip:v', 'shp:flip:h', 'shp:rotate:more', 'shp:align:left',
    'shp:align:center', 'shp:align:right', 'shp:align:top', 'shp:align:middle', 'shp:align:bottom', 'shp:distribute:h',
    'shp:distribute:v', 'shp:layout', 'shp:dir:horz', 'shp:dir:vert', 'shp:dir:vert270', 'shp:anchor:t', 'shp:anchor:ctr',
    'shp:anchor:b', 'shp:textOptions', 'shp:link', 'shp:unlink', 'shp:setDefault', 'shp:fill:none', 'shp:outline:none',
    'shp:format', 'shp:delete', 'shp:alt', 'pic:compress', 'pic:change:file', 'pic:reset', 'pic:resetSize', 'page:noColor',
    'dlg:pageFill', 'dlg:watermark', 'watermark:remove', 'cover:remove', 'blankPage', 'hyph:none', 'hyph:auto', 'dlg:hyphenation',
    'ln:none', 'ln:continuous', 'ln:page', 'ln:section', 'ln:suppress', 'dlg:lineNumbers', 'hyphen:optional',
    'hyphen:nonBreaking',
  ] as readonly string[],
  /** consultas desligadas que respondem o neutro sem rodar o código TS */
  neutral: [] as readonly string[],
};
