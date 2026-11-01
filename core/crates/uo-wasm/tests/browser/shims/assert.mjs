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
assert.AssertionError = AssertionError;

export default assert;
