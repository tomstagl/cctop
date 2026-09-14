// Headless stand-ins for what Claude Code hands a hooks module: a fake `$`
// (`fakeEngine`) whose every side effect lands in an inspectable field, and a
// fake `on` (`fakeOn`) that records registrations and dispatches an event
// through them with a working `next`. Nothing here talks to Claude Code.
//
// Importing this module also installs the two JSX globals a hooks module
// compiles against (`h`, `Fragment`): the engine provides them in its own
// environment, Node does not, so they must exist before a render hook runs.
import type {
  CommandSpec,
  ElementTable,
  EngineInterface,
  HookFailure,
  On,
  PaneCloseArgs,
  PaneOpenArgs,
  ProcessRunResult,
  RenderElement,
  RenderInput,
  RenderNode,
  SessionUsage,
  Timer,
} from 'claude-code';

type Props = Record<string, unknown>;

// JSX children as `h` receives them, flattened to what an element holds:
// nested lists flattened, numbers drawn as their string, `false`/`null`/
// `undefined` (and `true`, as React) dropped.
function normalizeChildren(children: unknown[]): RenderNode[] {
  const out: RenderNode[] = [];
  const visit = (c: unknown): void => {
    if (c === null || c === undefined || typeof c === 'boolean') return;
    if (Array.isArray(c)) {
      c.forEach(visit);
      return;
    }
    out.push(typeof c === 'number' ? String(c) : (c as RenderNode));
  };
  children.forEach(visit);
  return out;
}

// The JSX factory: a function tag is called with its props (children among
// them, as the engine's `h` does); a string tag becomes a plain element.
function h(tag: unknown, props: Props | null | undefined, ...children: unknown[]): unknown {
  const kids = normalizeChildren(children);
  const all: Props = { ...(props ?? {}) };
  if (kids.length > 0) all.children = kids;
  if (typeof tag === 'function') return (tag as (p: Props) => unknown)(all);
  return { type: tag, props: props ?? {}, children: kids };
}

// `<>...</>`: a column Box around the children.
function Fragment(props: { children?: RenderNode[] }): RenderElement {
  return { type: 'Box', props: { flexDirection: 'column' }, children: props.children ?? [] };
}

const globals = globalThis as unknown as Record<string, unknown>;
globals.h = h;
globals.Fragment = Fragment;

// The text of a Text-like subtree: strings and nested inline elements joined.
export function textOf(node: RenderNode | RenderNode[] | undefined): string {
  if (node === undefined) return '';
  if (typeof node === 'string') return node;
  if (Array.isArray(node)) return node.map(textOf).join('');
  if ('children' in node && Array.isArray(node.children)) return textOf(node.children);
  return '';
}

// The element table `$.ui.resolve(e)` answers: tagged factories usable through
// `h`. Buttons keep their `onPress` in `presses` (by key) so a test can press.
export function fakeElements(presses: Map<string, () => void>): ElementTable<'terminal'> {
  let handle = 0;
  const plain = (type: string) => (props: Props): RenderElement => {
    const { children, ...rest } = props;
    return { type, props: rest, children: (children as RenderNode[] | undefined) ?? [] } as unknown as RenderElement;
  };
  const leaf = (type: string) => (props: Props): RenderElement => {
    const { children: _children, onInput: _i, onSubmit: _s, onSelect: _o, ...rest } = props;
    return { type, props: rest, press: { plugin: 'cctop', handle: ++handle } } as unknown as RenderElement;
  };
  const Button = (props: Props): RenderElement => {
    const label = typeof props.label === 'string' ? props.label : textOf(props.children as RenderNode[] | undefined);
    const key = typeof props.key === 'string' ? props.key : label;
    if (typeof props.onPress === 'function') presses.set(key, props.onPress as () => void);
    const p: Props = { key, label };
    if (props.hotkey !== undefined) p.hotkey = props.hotkey;
    if (props.plain) p.plain = true;
    return { type: 'Button', props: p, press: { plugin: 'cctop', handle: ++handle } } as unknown as RenderElement;
  };
  const Code = (props: Props): RenderElement => {
    const { children: _children, ...rest } = props;
    return { type: 'Code', props: rest } as unknown as RenderElement;
  };
  return {
    Box: plain('Box'),
    Text: plain('Text'),
    Button,
    Input: leaf('Input'),
    Select: leaf('Select'),
    Link: plain('Link'),
    Code,
    Client: leaf('Client'),
  } as unknown as ElementTable<'terminal'>;
}

// One `process.run` answer: a result to resolve with, an Error to reject
// with, or a function for anything else (a never-settling promise, a counter).
export type ProcessScript = ProcessRunResult | Error | (() => Promise<ProcessRunResult>);

export type ScriptedSession = {
  id: string;
  model: string;
  turns: number;
  cwd: string;
  usage: SessionUsage;
};

export type FakeEngineOptions = {
  /** Clock start in ms since the epoch; default 2026-09-12T12:00:00Z. */
  now?: number;
  env?: Record<string, string>;
  session?: Partial<ScriptedSession>;
  /** `process.run` answers keyed by `argv.join(' ')`; anything else rejects. */
  process?: Record<string, ProcessScript>;
  store?: Record<string, unknown>;
  files?: Record<string, string>;
  plugin?: { name?: string; root?: string };
};

// What the fake adds on top of `EngineInterface`: the inspection points.
export type FakeExtras = {
  prompt: {
    /** Every `$.prompt.fill` text, in order. */
    fills: string[];
    /** What the next fills answer (`isFilled`). */
    filled: boolean;
  };
  ui: {
    /** `$.ui.invalidate` calls, counted per event name. */
    invalidates: Record<string, number>;
    logs: string[];
    toasts: string[];
    statuses: (string | undefined)[];
    opens: PaneOpenArgs[];
    closes: PaneCloseArgs[];
    /** Pane ids open right now. */
    openIds: Set<string>;
    /** `onPress` of every Button drawn, by key; `press(key)` runs one. */
    presses: Map<string, () => void>;
    press: (key: string) => void;
    /** Run on every `$.ui.invalidate("ui.render")`: how `fakeOn`'s surface learns to draw. */
    renderListeners: (() => void)[];
  };
  clock: {
    /** Advances the clock by `ms`, firing every due timer in due order. */
    tick: (ms: number) => void;
    /** Timers armed and not cancelled. */
    pending: () => number;
  };
  store: { map: Map<string, unknown> };
  fs: { files: Map<string, string> };
  process: { calls: string[][]; script: Record<string, ProcessScript> };
  session: { scripted: ScriptedSession };
  command: { registered: CommandSpec[] };
  env: { values: Record<string, string> };
};

export type FakeEngine = EngineInterface & FakeExtras;

type FakeTimer = { due: number; ms: number; fn: () => void; repeat: boolean; cancelled: boolean };

export function fakeEngine(opts: FakeEngineOptions = {}): FakeEngine {
  let now = opts.now ?? Date.UTC(2026, 8, 12, 12, 0, 0);
  const timers: FakeTimer[] = [];
  const arm = (ms: number, fn: () => void, repeat: boolean): Timer => {
    if (!(ms >= 1)) throw new Error(`fakeEngine: timer of ${ms} ms`);
    const t: FakeTimer = { due: now + ms, ms, fn, repeat, cancelled: false };
    timers.push(t);
    return {
      cancel: () => {
        t.cancelled = true;
      },
    };
  };
  const tick = (ms: number): void => {
    const target = now + ms;
    for (;;) {
      let due: FakeTimer | undefined;
      for (const t of timers) {
        if (!t.cancelled && t.due <= target && (due === undefined || t.due < due.due)) due = t;
      }
      if (due === undefined) break;
      now = due.due;
      if (due.repeat) due.due += due.ms;
      else due.cancelled = true;
      due.fn();
    }
    now = target;
    for (let i = timers.length - 1; i >= 0; i--) if (timers[i].cancelled) timers.splice(i, 1);
  };

  const presses = new Map<string, () => void>();
  const elements = fakeElements(presses);
  const invalidates: Record<string, number> = {};
  const openIds = new Set<string>();
  const storeMap = new Map<string, unknown>(
    Object.entries(opts.store ?? {}).map(([k, v]) => [k, JSON.parse(JSON.stringify(v))]),
  );
  const files = new Map<string, string>(Object.entries(opts.files ?? {}));
  const script = { ...(opts.process ?? {}) };
  const calls: string[][] = [];
  const scripted: ScriptedSession = {
    id: 'fake-session',
    model: 'claude-sonnet-5',
    turns: 0,
    cwd: '/home/user/project',
    usage: { context: { window: 200000 }, rateLimits: [] },
    ...opts.session,
  };
  const env = { ...(opts.env ?? {}) };
  const registered: CommandSpec[] = [];

  const engine = {
    plugin: { name: opts.plugin?.name ?? 'cctop', root: opts.plugin?.root ?? '/plugins/cctop' },
    ui: {
      invalidates,
      logs: [] as string[],
      toasts: [] as string[],
      statuses: [] as (string | undefined)[],
      opens: [] as PaneOpenArgs[],
      closes: [] as PaneCloseArgs[],
      openIds,
      presses,
      press: (key: string) => {
        const fn = presses.get(key);
        if (!fn) throw new Error(`fakeEngine: no Button with key ${key}`);
        fn();
      },
      notice: () => {},
      invalidate: (event: string) => {
        invalidates[event] = (invalidates[event] ?? 0) + 1;
        if (event === 'ui.render') for (const fn of engine.ui.renderListeners) fn();
      },
      renderListeners: [] as (() => void)[],
      resolve: () => elements,
      log: (text: string) => {
        engine.ui.logs.push(text);
      },
      ask: () => Promise.reject(new Error('fakeEngine: ui.ask is not scripted')),
      toast: (text: string) => {
        engine.ui.toasts.push(text);
      },
      status: (text: string | undefined) => {
        engine.ui.statuses.push(text);
      },
      open: async (pane: PaneOpenArgs) => {
        engine.ui.opens.push({ ...pane });
        openIds.add(pane.id);
      },
      close: async (pane: PaneCloseArgs) => {
        engine.ui.closes.push({ ...pane });
        openIds.delete(pane.id);
      },
    },
    session: {
      scripted,
      messages: async () => [],
      cwd: async () => scripted.cwd,
      model: async () => scripted.model,
      turns: async () => scripted.turns,
      id: async () => scripted.id,
      repo: async () => null,
      surfaces: async () => ['terminal'] as const,
      surface: async () => 'terminal' as const,
      usage: async () => JSON.parse(JSON.stringify(scripted.usage)) as SessionUsage,
    },
    command: {
      registered,
      list: async () => [],
      register: async (command: CommandSpec) => {
        registered.push(command);
        return undefined as never;
      },
    },
    fs: {
      files,
      read: async (path: string) => {
        const text = files.get(path);
        if (text === undefined) throw new Error(`ENOENT: ${path}`);
        return text;
      },
      write: async (path: string, text: string) => {
        files.set(path, text);
      },
      list: async (path = '') =>
        [...files.keys()].filter((p) => p.startsWith(path)).map((p) => ({ name: p, kind: 'file' }) as never),
      exists: async (path: string) => files.has(path),
      stat: async (path: string) => {
        const text = files.get(path);
        if (text === undefined) throw new Error(`ENOENT: ${path}`);
        return { size: text.length, kind: 'file' } as never;
      },
      ancestors: async () => [],
    },
    store: {
      map: storeMap,
      get: async (key: string) => storeMap.get(key),
      set: async (key: string, value: unknown) => {
        storeMap.set(key, JSON.parse(JSON.stringify(value)));
      },
      delete: async (key: string) => {
        storeMap.delete(key);
      },
      keys: async () => [...storeMap.keys()],
    },
    clock: {
      now: () => now,
      sleep: (ms: number) => new Promise<void>((resolve) => arm(ms, resolve, false)),
      after: (ms: number, fn: () => void) => arm(ms, fn, false),
      every: (ms: number, fn: () => void) => arm(ms, fn, true),
      tick,
      pending: () => timers.filter((t) => !t.cancelled).length,
    },
    process: {
      calls,
      script,
      run: async (argv: readonly string[]) => {
        calls.push([...argv]);
        const key = argv.join(' ');
        const answer = script[key];
        if (answer === undefined) throw new Error(`fakeEngine: no script for process.run: ${key}`);
        if (answer instanceof Error) throw answer;
        if (typeof answer === 'function') return answer();
        return answer;
      },
    },
    prompt: {
      fills: [] as string[],
      filled: true,
      fill: async ({ text }: { text: string }) => {
        engine.prompt.fills.push(text);
        return { isFilled: engine.prompt.filled };
      },
      submit: async () => {
        throw new Error('fakeEngine: prompt.submit is never called by cctop');
      },
    },
    env: {
      values: env,
      get: async (name: string) => env[name],
      set: async (name: string, value: string | undefined) => {
        if (value === undefined) delete env[name];
        else env[name] = value;
      },
    },
  };
  return engine as unknown as FakeEngine;
}

// One `on(...)` call as the module made it.
export type Registration = {
  event: string;
  matcher: Record<string, unknown> | undefined;
  handler: AnyHandler;
  catchHandler?: AnyHandler;
};

type AnyHandler = ($: EngineInterface, e: unknown, next: unknown) => unknown;

// Whether a pattern (`tool.call`, `tool.*`, `*`, `!tool.call`) selects an event.
function selects(pattern: string, event: string): boolean {
  if (pattern === '*') return true;
  if (pattern.startsWith('!')) return !selects(pattern.slice(1), event);
  if (pattern.endsWith('.*')) return event.startsWith(pattern.slice(0, -1));
  return pattern === event;
}

function matchOne(m: unknown, v: unknown): boolean {
  if (m instanceof RegExp) return typeof v === 'string' && m.test(v);
  if (Array.isArray(m)) return m.some((one) => matchOne(one, v));
  if (m !== null && typeof m === 'object') return matches(m as Record<string, unknown>, v);
  return m === v;
}

function matches(matcher: Record<string, unknown> | undefined, e: unknown): boolean {
  if (matcher === undefined) return true;
  if (e === null || typeof e !== 'object') return false;
  const obj = e as Record<string, unknown>;
  return Object.entries(matcher).every(([k, m]) => matchOne(m, obj[k]));
}

function unsupported(name: string): () => never {
  return () => {
    throw new Error(`fakeOn: next.${name} is not supported by the harness`);
  };
}

// The `next` a dispatched hook receives; `inner` runs the rest of the chain.
function makeNext(inner: (e: unknown) => Promise<unknown>, caught?: { error: HookFailure; called: boolean }) {
  return Object.assign(inner, {
    to: unsupported('to'),
    signal: new AbortController().signal,
    is: (pattern: string, e: unknown) => {
      void e;
      return pattern !== '';
    },
    ...(caught ?? {}),
  });
}

// The surface `fakeOn` stands in for: after every `ui.render` invalidate it
// asks each open pane for its tree at `columns`, the way the engine's dock
// (or inline band) does — unless `hidden`, which is the engine with the
// /diff panel holding the dock: the pane stays open, nothing draws it.
export type FakeSurface = {
  /** The whole screen's width (`e.viewport.columns`). */
  columns: number;
  rows?: number;
  placement?: 'dock' | 'inline';
  /** The pane body's width; defaults to `columns`. */
  bodyColumns?: number;
  hidden?: boolean;
};

export function fakeOn($: FakeEngine = fakeEngine(), opts: { surface?: FakeSurface } = {}) {
  const registrations: Registration[] = [];
  const surface = opts.surface;
  if (surface !== undefined) {
    $.ui.renderListeners.push(() => {
      if (surface.hidden) return;
      // The engine draws on its next frame, after the invalidating hook ran.
      setTimeout(() => {
        for (const id of $.ui.openIds) {
          void dispatch('ui.render', paneRender(id, surface.columns, surface)).catch(() => undefined);
        }
      }, 0);
    });
  }
  const on = ((event: string, a: unknown, b?: unknown) => {
    const reg: Registration =
      typeof a === 'function'
        ? { event, matcher: undefined, handler: a as AnyHandler }
        : { event, matcher: a as Record<string, unknown>, handler: b as AnyHandler };
    registrations.push(reg);
    return {
      catch: (handler: AnyHandler) => {
        reg.catchHandler = handler;
      },
    };
  }) as unknown as On;

  // Runs the registered hooks that select `event` and match `e` as a chain,
  // outermost first; the innermost `next(e)` ends in `core(e)`. A hook that
  // throws is replayed through its `.catch` handler, if it set one.
  async function dispatch<R = unknown>(
    event: string,
    e: unknown,
    core: (e: never) => unknown = () => undefined,
  ): Promise<R> {
    const chain = registrations.filter((r) => selects(r.event, event) && matches(r.matcher, e));
    const run = async (i: number, input: unknown): Promise<unknown> => {
      if (i >= chain.length) return core(input as never);
      const reg = chain[i];
      let called = false;
      let settled: Promise<unknown> | undefined;
      const inner = (e2: unknown) => {
        called = true;
        settled = run(i + 1, e2);
        return settled;
      };
      try {
        return await reg.handler($, input, makeNext(inner));
      } catch (err) {
        if (!reg.catchHandler) throw err;
        const error: HookFailure = {
          kind: 'throw',
          message: err instanceof Error ? err.message : String(err),
          budget: 1000,
        };
        // Replay-safe: a `next(e)` the hook already made is answered again.
        const replay = (e2: unknown) => settled ?? inner(e2);
        return await reg.catchHandler($, input, makeNext(replay, { error, called }));
      }
    };
    return (await run(0, e)) as R;
  }

  return { on, dispatch, registrations, $ };
}

// A `ui.render` event for the `Pane` component, as the terminal surface sends
// it for pane `id` at `columns` wide.
export function paneRender(
  id: string,
  columns: number,
  extra: {
    rows?: number;
    placement?: 'dock' | 'inline';
    isFocused?: boolean;
    title?: string;
    /** Cells across the pane body; defaults to `columns` (the whole screen). */
    bodyColumns?: number;
  } = {},
): RenderInput<'Pane', 'terminal'> {
  const rows = extra.rows ?? 40;
  return {
    surface: 'terminal',
    component: 'Pane',
    requestId: id,
    viewport: { columns, rows },
    props: {
      title: extra.title ?? id,
      isFocused: extra.isFocused ?? false,
      bodyColumns: extra.bodyColumns ?? columns,
      placement: extra.placement ?? 'dock',
      scroll: { offset: 0, bodyRows: rows },
    },
  };
}
