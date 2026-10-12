/**
 * The ordering on top of a `PomBackend`. Every call goes through one promise chain, so commands
 * reach Rust in the order they were made and a gesture's `end` finds its token.
 */
import type { PomBackend } from './backend.ts';
import type { CommandState, Ran, Status, View } from './wire.ts';

export interface ChannelOptions {
  /**
   * Start with the gate shut: calls queue until `release()`. For an app that has to be ready
   * before its first command may run. The default is open.
   */
  held?: boolean;
  /** Resolves after the next paint; a drag's next command waits for it. */
  nextFrame?: () => Promise<void>;
}

const defaultNextFrame = (): Promise<void> =>
  new Promise((resolve) => {
    if (typeof requestAnimationFrame === 'function') requestAnimationFrame(() => resolve());
    else setTimeout(resolve, 0);
  });

interface Waiting {
  id: string;
  payload: unknown;
  coalesce?: string;
  done: Promise<Ran>;
}

export interface Channel {
  /** Calls made and not yet answered; a drag that replaced a waiting move doesn't count. */
  readonly pending: number;
  /** Lets the queued calls through. A no-op on a gate that is already open. */
  release(): void;
  /** Called whenever the queue empties. Returns a function that stops listening. */
  onIdle(f: () => void): () => void;
  run(id: string, payload?: unknown, coalesce?: string): Promise<Ran>;
  /**
   * A command for a scrub or a transform only: latest wins. While one is on its way, a newer
   * move with the same key replaces the one waiting instead of queueing behind it, and the
   * next is sent only after a frame, once the last update could be applied and painted.
   * Without this a slow redraw builds a backlog and the result trails the pointer.
   *
   * **Never use it for input that accumulates**, such as a paint stroke: the moves it drops
   * are gone. A stroke batches its own cells and calls `run` once per frame.
   */
  drag(key: string, id: string, payload: unknown, coalesce?: string): Promise<Ran>;
  /** Opens a gesture; its token is what `end` and `cancel` take. */
  begin(label: string): Promise<number>;
  /** Takes the token, or its promise, so it is queued behind everything the gesture ran. */
  end(token: number | Promise<number>): Promise<void>;
  cancel(token: number | Promise<number>): Promise<void>;
  view(): Promise<View>;
  status(): Promise<Status>;
  commands(payload?: unknown): Promise<CommandState[]>;
}

export function createChannel(backend: PomBackend, options: ChannelOptions = {}): Channel {
  const nextFrame = options.nextFrame ?? defaultNextFrame;
  let release: () => void = () => undefined;
  let chain: Promise<unknown> = options.held
    ? new Promise<void>((resolve) => {
        release = resolve;
      })
    : Promise.resolve();
  let pending = 0;
  const idle = new Set<() => void>();

  /** Runs `f` after every call made before it, and after the gate opens. */
  function queued<T>(f: () => Promise<T>): Promise<T> {
    pending += 1;
    const next = chain.then(f);
    chain = next.catch(() => undefined);
    const settle = () => {
      pending -= 1;
      if (pending === 0) for (const f of [...idle]) f();
    };
    next.then(settle, settle);
    return next;
  }

  const waiting = new Map<string, Waiting>();

  return {
    get pending() {
      return pending;
    },
    release: () => release(),
    onIdle: (f) => {
      idle.add(f);
      return () => {
        idle.delete(f);
      };
    },
    run: (id, payload = {}, coalesce) => queued(() => backend.run(id, payload, coalesce)),
    drag(key, id, payload, coalesce) {
      const w = waiting.get(key);
      if (w) {
        w.id = id;
        w.payload = payload;
        w.coalesce = coalesce;
        return w.done;
      }
      const entry = { id, payload, coalesce } as Waiting;
      waiting.set(key, entry);
      entry.done = queued(async () => {
        waiting.delete(key);
        try {
          return await backend.run(entry.id, entry.payload, entry.coalesce);
        } finally {
          await nextFrame();
        }
      });
      return entry.done;
    },
    begin: (label) => queued(() => backend.begin(label)),
    end: (token) =>
      queued(async () => {
        const t = await Promise.resolve(token).catch(() => null);
        if (t !== null) await backend.end(t);
      }),
    cancel: (token) =>
      queued(async () => {
        const t = await Promise.resolve(token).catch(() => null);
        if (t !== null) await backend.cancel(t);
      }),
    view: () => queued(() => backend.view()),
    status: () => queued(() => backend.status()),
    commands: (payload) => queued(() => backend.commands(payload)),
  };
}
