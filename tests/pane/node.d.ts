// The tests run under `node --test`, but `typescript` is the only dev
// dependency (no @types/node) and tsconfig has `types: []`, so the two Node
// modules the tests import are declared here with just the shape they use.
declare module 'node:test' {
  type TestFn = (t: { name: string; diagnostic(message: string): void }) => void | Promise<void>;
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

declare module 'node:fs' {
  export function readFileSync(path: string, encoding: 'utf8'): string;
  // No encoding: the file's bytes (a Buffer in Node, typed as the Uint8Array it is).
  export function readFileSync(path: string): Uint8Array;
  export function readdirSync(path: string): string[];
  export function statSync(path: string): { isDirectory(): boolean };
}

declare module 'node:path' {
  export function join(...parts: string[]): string;
}

declare var __dirname: string;

// Bare timers for a test that has to let promise chains settle.
declare function setTimeout(fn: () => void, ms: number): unknown;

declare var Buffer: { from(data: string, encoding: 'base64'): Uint8Array };
declare var process: { env: Record<string, string | undefined> };
declare var console: { log(...a: unknown[]): void };
