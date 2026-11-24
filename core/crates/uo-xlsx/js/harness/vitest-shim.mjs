// The part of `vitest`'s `vi` that write/test-support/fake-dom.ts uses (stubGlobal, fn), so the fake DOM
// runs in plain Node inside the harness bundle (build-harness.mjs aliases `vitest` to this file).
const stubbed = new Map();

export const vi = {
  fn(impl = () => undefined) {
    const calls = [];
    const f = function (...a) { calls.push(a); return impl.apply(this, a); };
    f.mock = { calls };
    return f;
  },
  stubGlobal(name, value) {
    if (!stubbed.has(name)) stubbed.set(name, Object.getOwnPropertyDescriptor(globalThis, name));
    Object.defineProperty(globalThis, name, { value, writable: true, configurable: true, enumerable: true });
  },
  unstubAllGlobals() {
    for (const [name, d] of stubbed) {
      if (d) Object.defineProperty(globalThis, name, d);
      else delete globalThis[name];
    }
    stubbed.clear();
  },
};
