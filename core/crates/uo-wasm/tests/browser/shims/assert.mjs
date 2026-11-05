// The subset of `node:assert/strict` the wasm tests use.
class AssertionError extends Error {}

function fail(message, fallback) {
  throw new AssertionError(message ?? fallback);
}

function assert(value, message) {
  if (!value) fail(message, `expected a truthy value, got ${value}`);
}
assert.ok = assert;
assert.equal = (actual, expected, message) => {
  if (!Object.is(actual, expected)) fail(message, `expected ${expected}, got ${actual}`);
};
assert.notEqual = (actual, expected, message) => {
  if (Object.is(actual, expected)) fail(message, `expected a value other than ${expected}`);
};
assert.deepEqual = (actual, expected, message) => {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) fail(message, `expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
};
// `throws(fn, expected?, message?)`: `expected` an object whose own properties the error must have
// (the Node form the render tests use), a RegExp for the message, or a constructor
assert.throws = (fn, expected, message) => {
  let error;
  try { fn(); } catch (e) { error = e; }
  if (error === undefined) fail(typeof expected === 'string' ? expected : message, 'expected the function to throw');
  if (expected && typeof expected === 'object' && !(expected instanceof RegExp)) {
    for (const [k, v] of Object.entries(expected)) if (!Object.is(error?.[k], v)) fail(message, `expected the error to have ${k} = ${v}, got ${error?.[k]}`);
  } else if (expected instanceof RegExp) {
    if (!expected.test(String(error?.message ?? error))) fail(message, `the error ${error} does not match ${expected}`);
  } else if (typeof expected === 'function' && !(error instanceof expected)) fail(message, `the error is not a ${expected.name}`);
};
assert.AssertionError = AssertionError;

export default assert;
