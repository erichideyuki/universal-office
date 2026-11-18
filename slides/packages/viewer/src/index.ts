export { UniversalSlidesViewerElement } from './universal-slides-viewer.js';
export type { PlayOptions, PresentOptions } from './universal-slides-viewer.js';
// só tipos (apagados): a entrada principal não importa o motor; o editor carrega-o por loadRustEngine()
export type { RustSlides, RustSlidesHost } from './rust-engine.js';
export { CONTROL_ITEMS, type ControlItem } from './controls.js';
// resolvedor de fontes único dos apps: o host configura base/fontes do SO nele
export { FontResolver, type HostFontFace, type HostFontProvider } from '@universal-office/fonts';
