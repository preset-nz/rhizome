import type { PomBackend, Unlisten } from '../src-ts/backend.ts';
import type { CommandState, Ran, Row, Status, Update, View } from '../src-ts/wire.ts';

export const row = (id: string, path: string, extra: Partial<Row> = {}): Row => ({
  id,
  path,
  name: path.split('/').pop() ?? '',
  type: 'node',
  role: 'node',
  parent: null,
  ...extra,
});

export const status = (extra: Partial<Status> = {}): Status => ({
  title: 'Untitled',
  path: null,
  unsaved: false,
  undo: null,
  redo: null,
  gesture: false,
  generation: 0,
  ...extra,
});

export const view = (rows: Row[], extra: Partial<View> = {}): View => ({
  seq: 0,
  generation: 0,
  schema: { categories: [], types: [] },
  rows,
  ...extra,
});

export const update = (seq: number, extra: Partial<Update> = {}): Update => ({
  seq,
  label: `step ${seq}`,
  changes: [],
  rows: [],
  removed: [],
  generation: 0,
  ...extra,
});

/** A backend that records its calls in order and lets a test emit events and hold answers. */
export class FakeBackend implements PomBackend {
  calls: string[] = [];
  doc: View = view([]);
  current: Status = status();
  held: string | null = null;
  tokens = 0;
  /** When set, `run` waits for it, so a test can hold a command in flight. */
  runGate: Promise<void> | null = null;
  /** When set, `view` waits for it. */
  viewGate: Promise<void> | null = null;
  /** How many times `view` was asked. */
  viewCalls = 0;
  private updates = new Set<(u: Update) => void>();
  private statuses = new Set<(s: Status) => void>();
  /** Runs inside `connect`, to emit what Rust sends during it. */
  onConnect: () => void = () => undefined;
  ran: { id: string; payload: unknown; coalesce?: string }[] = [];

  async connect() {
    this.calls.push('connect');
    this.onConnect();
    return this.held;
  }
  async view() {
    this.calls.push('view');
    this.viewCalls += 1;
    const snapshot = this.doc;
    if (this.viewGate) await this.viewGate;
    return snapshot;
  }
  async status() {
    this.calls.push('status');
    return this.current;
  }
  async commands(): Promise<CommandState[]> {
    this.calls.push('commands');
    return [{ id: 'file.save', label: 'Save', enabled: true }];
  }
  async run(id: string, payload?: unknown, coalesce?: string): Promise<Ran> {
    this.calls.push(`run ${id}`);
    this.ran.push({ id, payload, coalesce });
    if (this.runGate) await this.runGate;
    this.calls.push(`ran ${id}`);
    return 'nothing';
  }
  async begin(label: string) {
    this.calls.push(`begin ${label}`);
    this.tokens += 1;
    return this.tokens;
  }
  async end(token: number) {
    this.calls.push(`end ${token}`);
  }
  async cancel(token: number) {
    this.calls.push(`cancel ${token}`);
  }
  async onUpdate(f: (u: Update) => void): Promise<Unlisten> {
    this.calls.push('onUpdate');
    this.updates.add(f);
    return () => {
      this.updates.delete(f);
    };
  }
  async onStatus(f: (s: Status) => void): Promise<Unlisten> {
    this.calls.push('onStatus');
    this.statuses.add(f);
    return () => {
      this.statuses.delete(f);
    };
  }

  get listening(): number {
    return this.updates.size + this.statuses.size;
  }
  emitUpdate(u: Update) {
    for (const f of [...this.updates]) f(u);
  }
  emitStatus(s: Status) {
    for (const f of [...this.statuses]) f(s);
  }
}

/** A promise and the function that resolves it. */
export function gate(): { promise: Promise<void>; open: () => void } {
  let open: () => void = () => undefined;
  const promise = new Promise<void>((resolve) => {
    open = resolve;
  });
  return { promise, open };
}

/** Lets pending promise callbacks run. */
export const flush = async (n = 8) => {
  for (let i = 0; i < n; i++) await Promise.resolve();
};
