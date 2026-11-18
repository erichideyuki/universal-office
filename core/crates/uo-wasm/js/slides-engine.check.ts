// Drift check of the viewer's ambient declaration `slides/packages/viewer/src/slides-engine.d.ts` against the real module
// (`./slides-engine.ts`): compiled with the core's strict tsc line together with that file; never imported. Assigning the real
// exports to the declared ones fails tsc when a signature or a shape drifts.
import type * as Declared from '@universal-office/slides-engine';
import * as Real from './slides-engine.ts';

export const functions: Pick<typeof Declared, 'loadSlidesModule' | 'slidesModuleLoaded' | 'slidesHandles' | 'slidesLiveBytes' | 'slidesMemoryBytes' | 'PENDING' | 'currentSlidesModule'> = Real;
export const slidesError: Declared.SlidesError = new Real.SlidesError(3, 'x');
export const types = {
  module: null as unknown as Real.SlidesModule satisfies Declared.SlidesModule,
  document: null as unknown as Real.SlidesDocument satisfies Declared.SlidesDocument,
  show: null as unknown as Real.SlidesShowHandle satisfies Declared.SlidesShowHandle,
  timeline: null as unknown as Real.SlidesTimelineHandle satisfies Declared.SlidesTimelineHandle,
  host: null as unknown as Declared.SlidesPaintHost satisfies Real.SlidesPaintHost,
  result: null as unknown as Real.SlidePaintResult satisfies Declared.SlidePaintResult,
};
