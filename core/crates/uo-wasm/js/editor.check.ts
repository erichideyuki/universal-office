// Drift check of the editor's ambient declaration `write/packages/editor/src/write-session.d.ts` against the real session
// binding (`./session.mjs`, typed by `./session.d.ts`): compiled with the core's strict tsc line together with that file and
// the viewer's `write-engine.d.ts`; never imported (record uo-bindings.md "Milestone 2: the editor switch"). Assigning the real
// shapes to the declared ones fails tsc when a signature drifts.
import type * as Declared from '@universal-office/write-session';
import type * as Real from './session.d.ts';
import type { WriteLayout, WriteEnv } from './write.mjs';
import type * as Engine from '@universal-office/write-engine';

export const types = {
  binding: null as unknown as Real.SessionBinding satisfies Declared.SessionBinding,
  session: null as unknown as Real.WriteSession satisfies Declared.WriteSession,
  shell: null as unknown as Real.ShellEngine satisfies Declared.ShellEngine,
  report: null as unknown as Real.RelayoutReport satisfies Declared.RelayoutReport,
  layout: null as unknown as WriteLayout satisfies Engine.EngineLayout,
  env: null as unknown as WriteEnv satisfies Engine.EngineEnv,
};
// a method signature on the declared side: the binding parameter is opaque there (`unknown`, the viewer module's `binding`)
export const bind = null as unknown as { bindSession: typeof Real.bindSession } satisfies { bindSession(exports: WebAssembly.Exports, uo: unknown): Declared.SessionBinding };
export const paste = null as unknown as { encodePaste: typeof Real.encodePaste } satisfies { encodePaste: typeof Declared.encodePaste };
