import { describe, expect, it } from 'vitest';
import { createMirror, type Mirror } from '../src-ts/mirror.ts';
import { FakeBackend, flush, gate, row, status, update, view } from './fake.ts';

const A = row('a', '/things/a');
const B = row('b', '/things/b');

async function started(rows = [A, B], seq = 3) {
  const backend = new FakeBackend();
  backend.doc = view(rows, { seq });
  backend.current = status();
  const mirror = createMirror(backend);
  await mirror.start();
  return { backend, mirror };
}

const changes = (mirror: Mirror) => {
  let n = 0;
  mirror.subscribe(() => {
    n += 1;
  });
  return () => n;
};

describe('mirror start', () => {
  it('connects, subscribes to updates and status, then reads the view', async () => {
    const backend = new FakeBackend();
    backend.doc = view([A], { seq: 1 });
    const mirror = createMirror(backend);
    expect(mirror.ready).toBe(false);
    await mirror.start();
    expect(backend.calls.slice(0, 4)).toEqual(['connect', 'onUpdate', 'onStatus', 'view']);
    expect(mirror.ready).toBe(true);
    expect(mirror.node('a')).toBe(A);
    expect(mirror.seq).toBe(1);
  });

  it('answers the path connect was holding', async () => {
    const backend = new FakeBackend();
    backend.held = '/docs/one.loom';
    expect(await createMirror(backend).start()).toBe('/docs/one.loom');
  });

  it('takes the status from the event connect sends, else asks', async () => {
    const backend = new FakeBackend();
    const mirror = createMirror(backend);
    await mirror.start();
    expect(backend.calls).toContain('status');
    expect(mirror.status).toEqual(status());

    const sent = new FakeBackend();
    const m2 = createMirror(sent);
    sent.onConnect = () => undefined;
    const done = m2.start();
    await flush();
    sent.emitStatus(status({ title: 'From the event' }));
    await done;
    expect(m2.status?.title).toBe('From the event');
  });

  it('applies an update that arrives before the view resolves', async () => {
    const backend = new FakeBackend();
    backend.doc = view([A], { seq: 3 });
    const g = gate();
    backend.viewGate = g.promise;
    const mirror = createMirror(backend);
    const done = mirror.start();
    await flush();
    // seq 4 is after the view; seq 3 is already in it
    backend.emitUpdate(update(3, { rows: [row('a', '/things/a', { name: 'older' })] }));
    backend.emitUpdate(update(4, { rows: [row('a', '/things/a', { name: 'newer' })] }));
    expect(mirror.ready).toBe(false);
    g.open();
    await done;
    expect(mirror.node('a')?.name).toBe('newer');
    expect(mirror.seq).toBe(4);
  });

  it('is not misled by an update the view already covers', async () => {
    const backend = new FakeBackend();
    backend.doc = view([row('a', '/things/a', { name: 'covered' })], { seq: 5 });
    const g = gate();
    backend.viewGate = g.promise;
    const mirror = createMirror(backend);
    const done = mirror.start();
    await flush();
    backend.emitUpdate(update(5, { rows: [row('a', '/things/a', { name: 'stale copy' })] }));
    g.open();
    await done;
    expect(mirror.node('a')?.name).toBe('covered');
  });

  it('re-reads when a buffered update leaves a gap after the view', async () => {
    const backend = new FakeBackend();
    backend.doc = view([A], { seq: 1 });
    const g = gate();
    backend.viewGate = g.promise;
    const mirror = createMirror(backend);
    const done = mirror.start();
    await flush();
    backend.doc = view([A, B], { seq: 4 });
    backend.emitUpdate(update(4, { rows: [B] }));
    g.open();
    await done;
    expect(mirror.node('b')).toBe(B);
    expect(mirror.seq).toBe(4);
  });

  it('stops listening on stop, and a second start is a no-op', async () => {
    const { backend, mirror } = await started();
    expect(await mirror.start()).toBeNull();
    expect(backend.calls.filter((c) => c === 'connect')).toHaveLength(1);
    mirror.stop();
    expect(backend.listening).toBe(0);
    backend.emitUpdate(update(4, { rows: [row('c', '/things/c')] }));
    expect(mirror.node('c')).toBeUndefined();
  });
});

describe('mirror updates', () => {
  it('applies the next update and notifies', async () => {
    const { backend, mirror } = await started();
    const count = changes(mirror);
    const fresh = row('a', '/things/a', { name: 'changed' });
    backend.emitUpdate(update(4, { rows: [fresh] }));
    expect(mirror.node('a')).toBe(fresh);
    expect(mirror.node('b')).toBe(B); // untouched rows keep their identity
    expect(mirror.seq).toBe(4);
    expect(count()).toBe(1);
  });

  it('ignores a stale update: at or before its seq, or an older generation', async () => {
    const { backend, mirror } = await started();
    const count = changes(mirror);
    backend.emitUpdate(update(3, { rows: [row('a', '/things/a', { name: 'x' })] }));
    backend.emitUpdate(update(2, { rows: [row('a', '/things/a', { name: 'y' })] }));
    expect(mirror.node('a')).toBe(A);
    expect(count()).toBe(0);
    expect(backend.viewCalls).toBe(1);
  });

  it('re-reads the view on a seq gap', async () => {
    const { backend, mirror } = await started();
    backend.doc = view([A, B, row('c', '/things/c')], { seq: 6 });
    backend.emitUpdate(update(6, { rows: [row('c', '/things/c')] }));
    await flush();
    expect(backend.viewCalls).toBe(2);
    expect(mirror.seq).toBe(6);
    expect(mirror.node('c')).toBeDefined();
  });

  it('reloads the view when the status moves to a new generation', async () => {
    const { backend, mirror } = await started();
    const other = row('z', '/other/z');
    backend.doc = view([other], { seq: 0, generation: 1 });
    backend.emitStatus(status({ generation: 1, title: 'Opened' }));
    await flush();
    expect(mirror.generation).toBe(1);
    expect(mirror.node('a')).toBeUndefined();
    expect(mirror.node('z')).toBe(other);
    expect(mirror.nodeAt('/things/a')).toBeUndefined();
    expect(mirror.status?.title).toBe('Opened');
    // a late update of the old generation is stale
    backend.emitUpdate(update(4, { generation: 0, rows: [A] }));
    expect(mirror.node('a')).toBeUndefined();
  });

  it('re-reads on an update from a generation it has not loaded', async () => {
    const { backend, mirror } = await started();
    backend.doc = view([row('z', '/other/z')], { seq: 1, generation: 2 });
    backend.emitUpdate(update(1, { generation: 2, rows: [row('z', '/other/z')] }));
    await flush();
    expect(mirror.generation).toBe(2);
    expect(mirror.node('z')).toBeDefined();
  });

  it('holds updates that arrive during a re-read, and applies them after', async () => {
    const { backend, mirror } = await started();
    const g = gate();
    backend.viewGate = g.promise;
    backend.doc = view([A, B, row('c', '/things/c')], { seq: 6 });
    backend.emitUpdate(update(6, { rows: [row('c', '/things/c')] })); // gap
    await flush();
    backend.emitUpdate(update(7, { rows: [row('d', '/things/d')] })); // during the read
    g.open();
    await flush();
    expect(mirror.seq).toBe(7);
    expect(mirror.node('d')).toBeDefined();
  });

  it('keeps what it had when a re-read fails, and says so', async () => {
    const backend = new FakeBackend();
    backend.doc = view([A], { seq: 1 });
    const errors: unknown[] = [];
    const mirror = createMirror(backend, { onError: (e) => errors.push(e) });
    await mirror.start();
    backend.view = async () => {
      throw new Error('gone');
    };
    backend.emitUpdate(update(5));
    await flush();
    expect(errors).toHaveLength(1);
    expect(mirror.node('a')).toBe(A);
    // and it can still take the next one in line
    backend.emitUpdate(update(2, { rows: [row('a', '/things/a', { name: 'ok' })] }));
    expect(mirror.node('a')?.name).toBe('ok');
  });
});

describe('mirror paths', () => {
  it('drops removed rows from the id map and the path index', async () => {
    const { backend, mirror } = await started();
    backend.emitUpdate(update(4, { removed: ['a'] }));
    expect(mirror.node('a')).toBeUndefined();
    expect(mirror.nodeAt('/things/a')).toBeUndefined();
    expect(mirror.nodeAt('/things/b')).toBe(B);
  });

  it('follows a rename', async () => {
    const { backend, mirror } = await started();
    const renamed = row('a', '/things/alpha', { name: 'alpha' });
    backend.emitUpdate(update(4, { rows: [renamed] }));
    expect(mirror.nodeAt('/things/a')).toBeUndefined();
    expect(mirror.nodeAt('/things/alpha')).toBe(renamed);
  });

  it('follows a move, descendants included', async () => {
    const parent = row('p', '/things/p');
    const child = row('c', '/things/p/c', { parent: 'p' });
    const { backend, mirror } = await started([parent, child]);
    backend.emitUpdate(
      update(4, {
        rows: [row('p', '/moved/p'), row('c', '/moved/p/c', { parent: 'p' })],
      }),
    );
    expect(mirror.nodeAt('/things/p')).toBeUndefined();
    expect(mirror.nodeAt('/things/p/c')).toBeUndefined();
    expect(mirror.nodeAt('/moved/p/c')?.id).toBe('c');
  });

  it('keeps both entries when two rows swap paths', async () => {
    const { backend, mirror } = await started();
    backend.emitUpdate(update(4, { rows: [row('a', '/things/b'), row('b', '/things/a')] }));
    expect(mirror.nodeAt('/things/a')?.id).toBe('b');
    expect(mirror.nodeAt('/things/b')?.id).toBe('a');
  });

  it('does not lose a path a new row took over when the old one is removed', async () => {
    const { backend, mirror } = await started();
    backend.emitUpdate(update(4, { removed: ['a'], rows: [row('a2', '/things/a')] }));
    expect(mirror.nodeAt('/things/a')?.id).toBe('a2');
  });
});
