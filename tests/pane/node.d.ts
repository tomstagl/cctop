// The tests run under `node --test`, but `typescript` is the only dev
// dependency (no @types/node) and tsconfig has `types: []`, so the two Node
// modules the tests import are declared here with just the shape they use.
declare module 'node:test' {
  type TestFn = (t: { name: string }) => void | Promise<void>;
  export function test(name: string, fn: TestFn): void;
  export function describe(name: string, fn: () => void): void;
  export function it(name: string, fn: TestFn): void;
}

declare module 'node:assert/strict' {
  const assert: {
    (value: unknown, message?: string): asserts value;
    ok(value: unknown, message?: string): asserts value;
    equal(actual: unknown, expected: unknown, message?: string): void;
    notEqual(actual: unknown, expected: unknown, message?: string): void;
    deepEqual(actual: unknown, expected: unknown, message?: string): void;
    match(value: string, regex: RegExp, message?: string): void;
    doesNotMatch(value: string, regex: RegExp, message?: string): void;
    throws(fn: () => unknown, message?: string): void;
    rejects(promise: Promise<unknown> | (() => Promise<unknown>), message?: string): Promise<void>;
    fail(message?: string): never;
  };
  export default assert;
}
