// @vitest-environment jsdom
import { act, cleanup, render } from '@testing-library/react';
import { StrictMode } from 'react';
import { afterEach, describe, expect, it } from 'vitest';
import { createMirror } from '../src-ts/mirror.ts';
import { PomProvider, useChildren, useNode, useNodeAt, useStatus } from '../src-ts/react.ts';
import { FakeBackend, row, status, update, view } from './fake.ts';

afterEach(cleanup);

const A = row('a', '/layers/a', { orders: { draw: ['b', 'c'] } });
const B = row('b', '/layers/b');
const C = row('c', '/layers/c');

async function setup() {
  const backend = new FakeBackend();
  backend.doc = view([A, B, C], { seq: 1 });
  backend.current = status({ title: 'One' });
  const mirror = createMirror(backend);
  await mirror.start();
  const push = (u: ReturnType<typeof update>) => act(() => backend.emitUpdate(u));
  return { backend, mirror, push };
}

function probe<T>(use: () => T) {
  const seen: T[] = [];
  const Probe = () => {
    seen.push(use());
    return null;
  };
  return { Probe, seen };
}

describe('hooks', () => {
  it('useNode returns the row and re-renders when that row changes, not otherwise', async () => {
    const { mirror, push } = await setup();
    const { Probe, seen } = probe(() => useNode('b'));
    render(
      <PomProvider mirror={mirror}>
        <Probe />
      </PomProvider>,
    );
    expect(seen).toEqual([B]);
    await push(update(2, { rows: [row('c', '/layers/c', { name: 'other' })] }));
    expect(seen).toHaveLength(1);
    const changed = row('b', '/layers/b', { name: 'changed' });
    await push(update(3, { rows: [changed] }));
    expect(seen).toHaveLength(2);
    expect(seen[1]).toBe(changed);
  });

  it('useNode is undefined for no id, an unknown id and a removed node', async () => {
    const { mirror, push } = await setup();
    const none = probe(() => useNode(null));
    const unknown = probe(() => useNode('nope'));
    const gone = probe(() => useNode('c'));
    render(
      <PomProvider mirror={mirror}>
        <none.Probe />
        <unknown.Probe />
        <gone.Probe />
      </PomProvider>,
    );
    expect(none.seen).toEqual([undefined]);
    expect(unknown.seen).toEqual([undefined]);
    await push(update(2, { removed: ['c'] }));
    expect(gone.seen.at(-1)).toBeUndefined();
  });

  it('useNodeAt follows a rename', async () => {
    const { mirror, push } = await setup();
    const { Probe, seen } = probe(() => useNodeAt('/layers/b'));
    render(
      <PomProvider mirror={mirror}>
        <Probe />
      </PomProvider>,
    );
    expect(seen.at(-1)).toBe(B);
    await push(update(2, { rows: [row('b', '/layers/bee')] }));
    expect(seen.at(-1)).toBeUndefined();
  });

  it('useChildren reads the row orders, and is the same array until the row changes', async () => {
    const { mirror, push } = await setup();
    const { Probe, seen } = probe(() => useChildren('a', 'draw'));
    const missing = probe(() => useChildren('a', 'nope'));
    const noRow = probe(() => useChildren('zzz', 'draw'));
    render(
      <PomProvider mirror={mirror}>
        <Probe />
        <missing.Probe />
        <noRow.Probe />
      </PomProvider>,
    );
    expect(seen[0]).toEqual(['b', 'c']);
    expect(seen[0]).toBe(A.orders?.draw);
    expect(missing.seen[0]).toEqual([]);
    expect(missing.seen[0]).toBe(noRow.seen[0]); // one shared empty list
    await push(update(2, { rows: [row('b', '/layers/b', { name: 'x' })] }));
    expect(seen).toHaveLength(1);
    const reordered = row('a', '/layers/a', { orders: { draw: ['c', 'b'] } });
    await push(update(3, { rows: [reordered] }));
    expect(seen.at(-1)).toEqual(['c', 'b']);
  });

  it('useStatus returns the status, re-rendering only when a new one arrives', async () => {
    const { backend, mirror, push } = await setup();
    const { Probe, seen } = probe(() => useStatus());
    render(
      <PomProvider mirror={mirror}>
        <Probe />
      </PomProvider>,
    );
    expect(seen.at(-1)?.title).toBe('One');
    await push(update(2, { rows: [row('b', '/layers/b', { name: 'x' })] }));
    expect(seen).toHaveLength(1);
    await act(() => backend.emitStatus(status({ title: 'Two', unsaved: true })));
    expect(seen.at(-1)?.title).toBe('Two');
    expect(seen).toHaveLength(2);
  });

  it('is harmless under StrictMode: no extra start, no leaked listeners', async () => {
    const { backend, mirror, push } = await setup();
    const { Probe, seen } = probe(() => useNode('b'));
    const view_ = render(
      <StrictMode>
        <PomProvider mirror={mirror}>
          <Probe />
        </PomProvider>
      </StrictMode>,
    );
    expect(backend.calls.filter((c) => c === 'connect')).toHaveLength(1);
    const before = seen.length;
    await push(update(2, { rows: [row('b', '/layers/b', { name: 'x' })] }));
    expect(seen.length).toBe(before + 2); // one render, doubled by StrictMode
    view_.unmount();
    expect(seen.at(-1)?.name).toBe('x');
  });

  it('throws a clear error outside a provider', () => {
    const { Probe } = probe(() => useNode('a'));
    const quiet = console.error;
    console.error = () => undefined;
    try {
      expect(() => render(<Probe />)).toThrow(/PomProvider/);
    } finally {
      console.error = quiet;
    }
  });
});
