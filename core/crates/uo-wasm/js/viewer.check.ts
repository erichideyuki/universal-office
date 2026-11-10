// Drift check of the viewer's ambient declaration `write/packages/viewer/src/write-engine.d.ts` against the real module
// (`./viewer.ts`): compiled with the core's strict tsc line together with that file; never imported. Assigning the real exports
// to the declared ones fails tsc when a signature or a shape drifts.
import type * as Declared from '@universal-office/write-engine';
import * as Real from './viewer.ts';
import { WriteError } from './write.mjs';

export const functions: Pick<typeof Declared, 'loadViewerModule' | 'viewerModuleLoaded'> = Real;
export const writeError: typeof Declared.WriteError = WriteError;
export const types = {
  module: null as unknown as Real.ViewerModule satisfies Declared.ViewerModule,
  env: null as unknown as Real.ViewerEnv satisfies Declared.ViewerEnv,
  document: null as unknown as Real.ViewerDocument satisfies Declared.ViewerDocument,
  layout: null as unknown as Real.DecodedLayout satisfies Declared.DecodedLayout,
  page: null as unknown as Real.ViewerPage satisfies Declared.ViewerPage,
  options: null as unknown as Declared.LayoutOptions satisfies Real.LayoutOptions,
  paintEnv: null as unknown as Declared.PaintEnv satisfies Real.PaintEnv,
};
