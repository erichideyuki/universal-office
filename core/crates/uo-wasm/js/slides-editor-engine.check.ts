// Drift check of the editor's ambient declaration `slides/packages/slides/src/slides-editor-engine.d.ts` against the real module
// (`./slides-editor-engine.ts`): compiled with the core's strict tsc line together with that file; never imported. Assigning the
// real exports to the declared ones (and the declared messages to the real codec types) fails tsc when a shape drifts.
import type * as Declared from '@universal-office/slides-editor-engine';
import * as Real from './slides-editor-engine.ts';
import type * as Codec from './gen/slides-edit.ts';

export const functions: Pick<typeof Declared, 'openEditSession' | 'blankPresentation' | 'editSessions'> = Real;
export const types = {
  session: null as unknown as Real.SlidesEditSession satisfies Declared.SlidesEditSession,
  result: null as unknown as Codec.EditResult satisfies Declared.EditResult,
  op: null as unknown as Declared.Op satisfies Codec.Op,
  opBack: null as unknown as Codec.Op satisfies Declared.Op,
  clip: null as unknown as Codec.ShapeClip satisfies Declared.ShapeClip,
  clipBack: null as unknown as Declared.ShapeClip satisfies Codec.ShapeClip,
  query: null as unknown as Declared.QueryOp satisfies Codec.QueryOp,
  answer: null as unknown as Codec.QueryAnswer satisfies Declared.QueryAnswer,
  requests: null as unknown as Codec.FontRequests satisfies Declared.FontRequests,
  saved: null as unknown as Codec.SaveResult satisfies Declared.SaveResult,
  blank: null as unknown as Declared.BlankDeckOptions satisfies Codec.BlankDeckOptions,
};
