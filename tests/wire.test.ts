/**
 * The Rust side pins these shapes in golden files; this reads the same files, so a field added,
 * renamed or retagged in Rust fails here until `src-ts/wire.ts` follows. Each key list is tied
 * to its interface at compile time (`Keys`): an interface key missing from the list is a type
 * error, and a golden key missing from the list is a test failure.
 */
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';
import { createMirror } from '../src-ts/mirror.ts';
import type {
  BindingRow,
  Change,
  CommandState,
  Commit,
  Ran,
  RefRow,
  Row,
  Schema,
  Status,
  TypeSchema,
  Update,
  ValueSchema,
  View,
} from '../src-ts/wire.ts';
import { FakeBackend, row } from './fake.ts';

const golden = (crate: string, name: string): unknown =>
  JSON.parse(
    readFileSync(
      fileURLToPath(new URL(`../crates/${crate}/tests/golden/${name}`, import.meta.url)),
      'utf8',
    ),
  );

/** Lists exactly the keys of `T`: a missing one is a compile error. */
const keys =
  <T>() =>
  <const K extends readonly (keyof T)[]>(
    k: K & ([Exclude<keyof T, K[number]>] extends [never] ? unknown : never),
  ): readonly string[] =>
    Array.from<unknown>(k).map(String);

const ROW = keys<Row>()([
  'id',
  'path',
  'name',
  'type',
  'role',
  'parent',
  'values',
  'set',
  'refs',
  'bindings',
  'orders',
  'members',
]);
const ROW_REQUIRED = ['id', 'path', 'name', 'type', 'role', 'parent'];
const REF_ROW = keys<RefRow>()(['file', 'node']);
const BINDING = keys<BindingRow>()(['on', 'source', 'values']);
const VALUE_SCHEMA = keys<ValueSchema>()([
  'key',
  'kind',
  'default',
  'range',
  'choices',
  'len',
  'shape',
  'soft_max',
  'label',
  'description',
  'unit',
]);
const TYPE_SCHEMA = keys<TypeSchema>()([
  'name',
  'label',
  'summary',
  'group',
  'categories',
  'values',
  'refs',
  'slots',
  'bindable',
]);
const SCHEMA = keys<Schema>()(['categories', 'types']);
const VIEW = keys<View>()(['seq', 'generation', 'schema', 'rows']);
const UPDATE = keys<Update>()(['seq', 'label', 'changes', 'rows', 'removed', 'generation']);
const STATUS = keys<Status>()([
  'title',
  'path',
  'unsaved',
  'undo',
  'redo',
  'gesture',
  'generation',
]);
const COMMIT = keys<Commit>()(['seq', 'label', 'changes']);
const COMMAND = keys<CommandState>()(['id', 'label', 'enabled']);

type Tag = Change['change'];
const CHANGE: Record<Tag, readonly string[]> = {
  added: ['path', 'id', 'change', 'type'],
  removed: ['path', 'id', 'change', 'type'],
  moved: ['path', 'id', 'change', 'from'],
  value: ['path', 'id', 'change', 'key', 'from', 'to'],
  ref: ['path', 'id', 'change', 'key', 'from', 'to'],
  bound: ['path', 'id', 'change', 'on', 'source', 'from', 'to'],
  reordered: ['path', 'id', 'change', 'order', 'from', 'to'],
  membership: ['path', 'id', 'change', 'added', 'removed'],
};

/** Every key of `o` is allowed, and every required one is there. */
function shape(o: unknown, allowed: readonly string[], required: readonly string[], at: string) {
  expect(o, at).toBeTypeOf('object');
  const have = Object.keys(o as object);
  expect(
    have.filter((k) => !allowed.includes(k)),
    `${at}: keys Rust sends that wire.ts doesn't know`,
  ).toEqual([]);
  expect(
    required.filter((k) => !have.includes(k)),
    `${at}: required keys missing`,
  ).toEqual([]);
}

const ROLES = ['root', 'category', 'group', 'node', 'opaque'];
const KINDS = [
  'bool',
  'int',
  'float',
  'text',
  'choice',
  'vec2',
  'vec3',
  'colour',
  'floats',
  'shaped',
];

function checkRow(r: Row, at: string) {
  shape(r, ROW, ROW_REQUIRED, at);
  expect(ROLES, at).toContain(r.role);
  for (const [k, ref] of Object.entries(r.refs ?? {})) shape(ref, REF_ROW, [], `${at}.refs.${k}`);
  for (const b of r.bindings ?? []) {
    shape(b, BINDING, ['on', 'source'], `${at}.bindings`);
    expect(['slot', 'value']).toContain(Object.keys(b.on)[0]);
  }
  // an empty map or list is left out by Rust, never sent
  for (const k of ['values', 'refs', 'orders'] as const) {
    if (k in r) expect(Object.keys(r[k] ?? {}).length, `${at}.${k} sent empty`).toBeGreaterThan(0);
  }
  for (const k of ['set', 'bindings', 'members'] as const) {
    if (k in r) expect((r[k] ?? []).length, `${at}.${k} sent empty`).toBeGreaterThan(0);
  }
}

function checkChange(c: Change, at: string) {
  const allowed = CHANGE[c.change];
  expect(allowed, `${at}: unknown change "${c.change}"`).toBeDefined();
  shape(c, allowed ?? [], allowed ?? [], at);
}

function checkSchema(s: Schema, at: string) {
  shape(s, SCHEMA, SCHEMA as string[], at);
  for (const c of s.categories) {
    shape(c, ['name', 'origin'], ['name', 'origin'], `${at}.category`);
    expect(['loaded', 'calculated']).toContain(c.origin);
  }
  for (const t of s.types) {
    shape(t, TYPE_SCHEMA, ['name'], `${at}.type ${t.name}`);
    for (const v of [...(t.values ?? []), ...(t.bindable ?? [])]) {
      shape(v, VALUE_SCHEMA, ['key', 'kind', 'default'], `${at}.${t.name}.${v.key}`);
      expect(KINDS).toContain(v.kind);
    }
  }
}

function checkView(v: View, at: string) {
  shape(v, VIEW, VIEW as string[], at);
  checkSchema(v.schema, `${at}.schema`);
  for (const r of v.rows) checkRow(r, `${at}.${r.path}`);
}

function checkUpdate(u: Update, at: string) {
  shape(u, UPDATE, UPDATE as string[], at);
  for (const c of u.changes) checkChange(c, `${at}.change`);
  for (const r of u.rows) checkRow(r, `${at}.${r.path}`);
}

function checkStatus(s: Status, at: string) {
  shape(s, STATUS, STATUS as string[], at);
  expect(typeof s.unsaved).toBe('boolean');
  expect(typeof s.gesture).toBe('boolean');
  expect(typeof s.generation).toBe('number');
  for (const k of ['path', 'undo', 'redo'] as const) {
    expect(s[k] === null || typeof s[k] === 'string', `${at}.${k}`).toBe(true);
  }
}

function checkRan(r: Ran, at: string) {
  if (r === 'nothing') return;
  if ('committed' in r) {
    shape(r.committed, COMMIT, COMMIT as string[], at);
    for (const c of r.committed.changes) checkChange(c, at);
    return;
  }
  expect(Object.keys(r), at).toEqual(['text']);
  expect(typeof r.text).toBe('string');
}

describe('rhizome-core goldens', () => {
  const commit = golden('rhizome-core', 'commit.json') as Commit;
  const viewJson = golden('rhizome-core', 'view.json') as {
    schema: Schema;
    rows: Row[];
    patch: { rows: Row[]; removed: string[] };
  };

  it('commit.json: every kind of change is one the types know', () => {
    shape(commit, COMMIT, COMMIT as string[], 'commit');
    const seen = new Set(commit.changes.map((c) => c.change));
    for (const c of commit.changes) checkChange(c, `commit ${c.change}`);
    expect([...seen].sort()).toEqual(Object.keys(CHANGE).sort());
  });

  it('schema-words.json: the words a type and a value may carry are wire.ts keys', () => {
    const words = golden('rhizome-core', 'schema-words.json') as Schema;
    checkSchema(words, 'schema-words');
    const widget = words.types.find((t) => t.name === 'widget');
    expect(widget?.label).toBe('Widget');
    expect(widget?.summary).toBeTypeOf('string');
    expect(widget?.group).toBe('parts');
    const size = widget?.values?.find((v) => v.key === 'size');
    expect(size?.description).toBeTypeOf('string');
    expect(size?.unit).toBe('cm');
    expect(widget?.bindable?.[0]?.label).toBe('Amount');
  });

  it('view.json: schema, rows and patch have the shapes of the types', () => {
    checkSchema(viewJson.schema, 'schema');
    for (const r of viewJson.rows) checkRow(r, r.path);
    for (const r of viewJson.patch.rows) checkRow(r, `patch ${r.path}`);
    expect(viewJson.patch.removed.every((id) => typeof id === 'string')).toBe(true);
  });

  it('a mirror taken from the rows and moved on by the patch ends as the golden says', async () => {
    // the tree before the commit: `new` not yet added, `old` still there, `ocean` still `sea`
    const before: Row[] = viewJson.rows
      .filter((r) => r.name !== 'new')
      .map((r) => (r.name === 'ocean' ? { ...r, name: 'sea', path: '/images/sea' } : r));
    before.push(row('00000000000000000000000008', '/images/old', { type: 'image' }));
    const backend = new FakeBackend();
    backend.doc = { seq: commit.seq - 1, generation: 0, schema: viewJson.schema, rows: before };
    const mirror = createMirror(backend);
    await mirror.start();
    expect(mirror.nodeAt('/images/sea')?.name).toBe('sea');
    expect(mirror.nodeAt('/images/old')).toBeDefined();

    backend.emitUpdate({
      seq: commit.seq,
      label: commit.label,
      changes: commit.changes,
      rows: viewJson.patch.rows,
      removed: viewJson.patch.removed,
      generation: 0,
    });

    expect(mirror.seq).toBe(commit.seq);
    expect(mirror.nodeAt('/images/sea')).toBeUndefined();
    expect(mirror.nodeAt('/images/old')).toBeUndefined();
    for (const r of viewJson.rows) {
      if (r.role !== 'root') expect(mirror.node(r.id), r.path).toEqual(r);
      expect(mirror.nodeAt(r.path)?.id).toBe(r.id);
    }
    expect(mirror.size).toBe(viewJson.rows.length);
    expect(mirror.node('00000000000000000000000002')?.orders?.draw).toEqual([
      '00000000000000000000000005',
      '00000000000000000000000004',
    ]);
  });
});

describe('rhizome-pom goldens', () => {
  it('view.json is a View', () => {
    checkView(golden('rhizome-pom', 'view.json') as View, 'view');
  });

  it('update.json holds Updates, one with removed ids, one with fresh rows', () => {
    const g = golden('rhizome-pom', 'update.json') as { set: Update; delete: Update };
    checkUpdate(g.set, 'set');
    checkUpdate(g.delete, 'delete');
    expect(g.set.rows.length).toBeGreaterThan(0);
    expect(g.delete.removed.length).toBeGreaterThan(0);
  });

  it('status.json holds Statuses, a gesture and a saved path among them', () => {
    const g = golden('rhizome-pom', 'status.json') as Record<
      'untitled' | 'gesture' | 'saved',
      Status
    >;
    for (const [k, s] of Object.entries(g)) checkStatus(s, k);
    expect(g.gesture.gesture).toBe(true);
    expect(g.saved.path).toBeTypeOf('string');
    expect(g.untitled.path).toBeNull();
  });

  it('ran.json holds every Ran and the command list', () => {
    const g = golden('rhizome-pom', 'ran.json') as {
      nothing: Ran;
      committed: Ran;
      text: Ran;
      commands: CommandState[];
    };
    for (const k of ['nothing', 'committed', 'text'] as const) checkRan(g[k], k);
    expect(g.nothing).toBe('nothing');
    expect(g.committed).toHaveProperty('committed');
    expect(g.text).toHaveProperty('text');
    for (const c of g.commands) {
      shape(c, COMMAND, COMMAND as string[], `command ${c.id}`);
      expect(typeof c.enabled).toBe('boolean');
    }
  });

  it('a mirror started on the View takes the Updates that follow', async () => {
    const v = golden('rhizome-pom', 'view.json') as View;
    const g = golden('rhizome-pom', 'update.json') as { set: Update; delete: Update };
    const backend = new FakeBackend();
    // the golden View is taken after both threads exist, before the set
    backend.doc = v;
    const mirror = createMirror(backend);
    await mirror.start();
    expect(mirror.seq).toBe(v.seq);
    expect(g.set.seq).toBe(v.seq + 1);
    backend.emitUpdate(g.set);
    expect(mirror.nodeAt('/threads/warp')?.values?.['thread.tension']).toBe(0.8);
    backend.emitUpdate(g.delete);
    expect(mirror.nodeAt('/threads/weft')).toBeUndefined();
    expect(mirror.seq).toBe(g.delete.seq);
  });
});
