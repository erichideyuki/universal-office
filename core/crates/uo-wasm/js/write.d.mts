// TS7016: a `.ts` file importing `./write.mjs` finds no types, because `write.d.ts` types `./write.js`, not the `.mjs`.
// This re-export gives the module the same types.
export * from './write.js';
