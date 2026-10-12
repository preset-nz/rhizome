/**
 * The document as rows, by id with a path index (rhizome decision 48): a view, then updates.
 * Read-only: every edit is a command through the channel, and the commit that comes back is
 * what changes the mirror. Create it outside React, `start()` it once, and read it through the
 * hooks.
 */
import type { PomBackend, Unlisten } from './backend.ts';
import type { NodeId, Row, Schema, Status, Update, View } from './wire.ts';

export interface MirrorOptions {
  /** A re-read that failed after the mirror started. The mirror keeps what it had. */
  onError?: (error: unknown) => void;
}

type Event = { kind: 'update'; update: Update } | { kind: 'status'; status: Status };

/** What `Mirror.apply` did with an update. */
export type Applied = 'applied' | 'stale' | 'gap';

/** The rows and the rules for taking updates: no backend, no events. `Mirror` drives it. */
export class Rows {
  private byId = new Map<NodeId, Row>();
  private paths = new Map<string, NodeId>();
  schema: Schema = { categories: [], types: [] };
  seq = 0;
  generation = 0;

  get size(): number {
    return this.byId.size;
  }

  ids(): IterableIterator<NodeId> {
    return this.byId.keys();
  }

  get(id: NodeId): Row | undefined {
    return this.byId.get(id);
  }

  byPath(path: string): Row | undefined {
    const id = this.paths.get(path);
    return id === undefined ? undefined : this.byId.get(id);
  }

  load(view: View): void {
    this.byId.clear();
    this.paths.clear();
    for (const r of view.rows) this.put(r);
    this.schema = view.schema;
    this.seq = view.seq;
    this.generation = view.generation;
  }

  /**
   * An update applies when it is next in line. `stale` is one from an older generation, or at
   * or before the seq the mirror has. `gap` is anything else: a seq that skips, or a
   * generation the mirror hasn't loaded yet. Either way the caller re-reads the view.
   */
  apply(u: Update): Applied {
    if (u.generation < this.generation) return 'stale';
    if (u.generation > this.generation) return 'gap';
    if (u.seq <= this.seq) return 'stale';
    if (u.seq !== this.seq + 1) return 'gap';
    this.seq = u.seq;
    for (const id of u.removed) this.drop(id);
    for (const r of u.rows) this.put(r);
    return 'applied';
  }

  private put(r: Row): void {
    const old = this.byId.get(r.id);
    if (old && old.path !== r.path && this.paths.get(old.path) === r.id) {
      this.paths.delete(old.path);
    }
    this.byId.set(r.id, r);
    this.paths.set(r.path, r.id);
  }

  private drop(id: NodeId): void {
    const old = this.byId.get(id);
    if (old && this.paths.get(old.path) === id) this.paths.delete(old.path);
    this.byId.delete(id);
  }
}

export class Mirror {
  private readonly rows = new Rows();
  private readonly listeners = new Set<() => void>();
  private current: Status | null = null;
  private loaded = false;
  private reading = false;
  private again = false;
  private buffer: Event[] = [];
  private unlisten: Unlisten[] = [];
  private started = false;
  private stopped = false;

  constructor(
    private readonly backend: PomBackend,
    private readonly options: MirrorOptions = {},
  ) {}

  /** `true` once the first view has loaded. */
  get ready(): boolean {
    return this.loaded;
  }

  get status(): Status | null {
    return this.current;
  }

  get schema(): Schema {
    return this.rows.schema;
  }

  get seq(): number {
    return this.rows.seq;
  }

  get generation(): number {
    return this.rows.generation;
  }

  get size(): number {
    return this.rows.size;
  }

  /** The row for `id`. A row is immutable; a changed node is a new object. */
  node(id: NodeId): Row | undefined {
    return this.rows.get(id);
  }

  nodeAt(path: string): Row | undefined {
    return this.rows.byPath(path);
  }

  ids(): IterableIterator<NodeId> {
    return this.rows.ids();
  }

  /** Calls `f` after the mirror changes. Returns a function that stops listening. */
  subscribe = (f: () => void): (() => void) => {
    this.listeners.add(f);
    return () => {
      this.listeners.delete(f);
    };
  };

  /**
   * Connects, listens, then reads the view, in that order, so nothing a commit sends in
   * between is lost: updates and statuses that arrive before the view resolves wait, and are
   * applied after it (the ones it already covers are dropped as stale). Answers what
   * `connect` did: a path macOS asked the app to open before the page was listening.
   * Calling it again is a no-op.
   */
  async start(): Promise<string | null> {
    if (this.started || this.stopped) return null;
    this.started = true;
    const held = await this.backend.connect();
    this.reading = true; // from here on, events wait for the view
    const u = await this.backend.onUpdate((update) => this.receive({ kind: 'update', update }));
    const s = await this.backend.onStatus((status) => this.receive({ kind: 'status', status }));
    this.unlisten = [u, s];
    if (this.stopped) {
      this.stop();
      return held;
    }
    await this.reload(true);
    // connect sends the status as an event; ask as well, for one that went before we listened
    if (this.current === null && !this.stopped) {
      const status = await this.backend.status();
      if (this.current === null) this.setStatus(status);
    }
    return held;
  }

  /** Stops listening. The rows stay readable. */
  stop(): void {
    this.stopped = true;
    for (const f of this.unlisten.splice(0)) f();
    this.buffer = [];
  }

  private receive(e: Event): void {
    if (this.stopped) return;
    if (this.reading) this.buffer.push(e);
    else this.process(e);
  }

  private process(e: Event): void {
    if (e.kind === 'status') {
      this.setStatus(e.status);
      return;
    }
    const result = this.rows.apply(e.update);
    if (result === 'applied') this.notify();
    else if (result === 'gap') this.refresh();
  }

  private setStatus(s: Status): void {
    this.current = s;
    this.notify();
    if (this.loaded && s.generation > this.rows.generation) this.refresh();
  }

  private refresh(): void {
    this.reload().catch((error) => this.options.onError?.(error));
  }

  /**
   * Reads the view and takes it, then applies what arrived while it was on its way. A
   * re-read asked for meanwhile (another gap, a newer generation) runs again after.
   */
  private async reload(initial = false): Promise<void> {
    if (this.reading && !initial) {
      this.again = true;
      return;
    }
    this.reading = true;
    try {
      do {
        this.again = false;
        const view = await this.backend.view();
        if (this.stopped) return;
        // only ever forward: a view older than what is held would undo an update
        if (!this.loaded || view.generation > this.rows.generation || view.seq >= this.rows.seq) {
          this.rows.load(view);
          this.loaded = true;
          this.notify();
        }
        const waiting = this.buffer.splice(0);
        for (const e of waiting) this.process(e);
      } while (this.again && !this.stopped);
    } finally {
      this.reading = false;
      this.again = false;
    }
  }

  private notify(): void {
    for (const f of [...this.listeners]) f();
  }
}

/** A mirror for `backend`. Call `start()` on it once. */
export function createMirror(backend: PomBackend, options?: MirrorOptions): Mirror {
  return new Mirror(backend, options);
}
