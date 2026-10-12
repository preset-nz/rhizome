import { describe, expect, it } from 'vitest';
import { createChannel } from '../src-ts/channel.ts';
import { FakeBackend, flush, gate } from './fake.ts';

const frames = () => {
  const waits: (() => void)[] = [];
  return {
    nextFrame: () => new Promise<void>((resolve) => waits.push(resolve)),
    waits,
    step: async () => {
      waits.shift()?.();
      await flush();
    },
  };
};

describe('channel', () => {
  it('sends calls in the order they were made, one at a time', async () => {
    const backend = new FakeBackend();
    const g = gate();
    backend.runGate = g.promise;
    const ch = createChannel(backend);
    const a = ch.run('a');
    const b = ch.run('b');
    const c = ch.commands();
    await flush();
    expect(backend.calls).toEqual(['run a']);
    g.open();
    await Promise.all([a, b, c]);
    expect(backend.calls).toEqual(['run a', 'ran a', 'run b', 'ran b', 'commands']);
  });

  it('starts open, and release is a harmless no-op', async () => {
    const backend = new FakeBackend();
    const ch = createChannel(backend);
    await ch.run('a');
    ch.release();
    await ch.run('b');
    expect(backend.ran.map((r) => r.id)).toEqual(['a', 'b']);
  });

  it('holds calls until release when asked to', async () => {
    const backend = new FakeBackend();
    const ch = createChannel(backend, { held: true });
    const a = ch.run('a');
    const b = ch.run('b');
    await flush();
    expect(backend.calls).toEqual([]);
    expect(ch.pending).toBe(2);
    ch.release();
    await Promise.all([a, b]);
    expect(backend.ran.map((r) => r.id)).toEqual(['a', 'b']);
    expect(ch.pending).toBe(0);
  });

  it('defaults the payload to an empty object and passes coalesce on', async () => {
    const backend = new FakeBackend();
    const ch = createChannel(backend);
    await ch.run('a', undefined, 'knob');
    expect(backend.ran[0]).toEqual({ id: 'a', payload: {}, coalesce: 'knob' });
  });

  it('keeps going after a call fails', async () => {
    const backend = new FakeBackend();
    backend.run = async () => {
      throw new Error('refused');
    };
    const ch = createChannel(backend);
    await expect(ch.run('a')).rejects.toThrow('refused');
    expect((await ch.status()).title).toBe('Untitled');
  });

  it('tells when the queue empties', async () => {
    const backend = new FakeBackend();
    const ch = createChannel(backend);
    let idle = 0;
    const off = ch.onIdle(() => {
      idle += 1;
    });
    await Promise.all([ch.run('a'), ch.run('b')]);
    await flush();
    expect(idle).toBe(1);
    off();
    await ch.run('c');
    await flush();
    expect(idle).toBe(1);
  });

  describe('gestures', () => {
    it('end finds the token begin answered, behind everything the gesture ran', async () => {
      const backend = new FakeBackend();
      const ch = createChannel(backend);
      const token = ch.begin('Paint');
      void ch.run('a');
      void ch.run('b');
      await ch.end(token);
      expect(backend.calls).toEqual(['begin Paint', 'run a', 'ran a', 'run b', 'ran b', 'end 1']);
    });

    it('cancel takes the token too, and a number works as well as a promise', async () => {
      const backend = new FakeBackend();
      const ch = createChannel(backend);
      await ch.cancel(ch.begin('Drag'));
      await ch.end(await ch.begin('Again'));
      expect(backend.calls).toEqual(['begin Drag', 'cancel 1', 'begin Again', 'end 2']);
    });

    it('sends no end when begin failed', async () => {
      const backend = new FakeBackend();
      backend.begin = async () => {
        throw new Error('no');
      };
      const ch = createChannel(backend);
      const token = ch.begin('x');
      token.catch(() => undefined);
      await ch.end(token);
      await ch.cancel(token);
      expect(backend.calls).toEqual([]);
    });
  });

  describe('drag', () => {
    it('is latest-wins: a newer move replaces the one waiting', async () => {
      const backend = new FakeBackend();
      const g = gate();
      backend.runGate = g.promise;
      const f = frames();
      const ch = createChannel(backend, { nextFrame: f.nextFrame });
      const first = ch.drag('k', 'move', { x: 1 });
      await flush();
      expect(backend.ran).toHaveLength(1); // on its way
      const second = ch.drag('k', 'move', { x: 2 });
      const third = ch.drag('k', 'move', { x: 3 });
      expect(second).toBe(third);
      g.open();
      await flush();
      await f.step(); // the frame after the first
      await flush();
      await f.step();
      await Promise.all([first, third]);
      expect(backend.ran.map((r) => r.payload)).toEqual([{ x: 1 }, { x: 3 }]);
    });

    it('waits a frame before the next command', async () => {
      const backend = new FakeBackend();
      const f = frames();
      const ch = createChannel(backend, { nextFrame: f.nextFrame });
      const d = ch.drag('k', 'move', 1);
      const after = ch.run('later');
      await flush();
      expect(backend.calls).toEqual(['run move', 'ran move']);
      await f.step();
      await Promise.all([d, after]);
      expect(backend.calls).toEqual(['run move', 'ran move', 'run later', 'ran later']);
    });

    it('keeps different keys apart', async () => {
      const backend = new FakeBackend();
      const f = frames();
      const ch = createChannel(backend, { nextFrame: f.nextFrame });
      const a = ch.drag('a', 'move', 1);
      const b = ch.drag('b', 'move', 2);
      await flush();
      await f.step();
      await f.step();
      await Promise.all([a, b]);
      expect(backend.ran.map((r) => r.payload)).toEqual([1, 2]);
    });

    it('sends the latest coalesce key with the latest payload', async () => {
      const backend = new FakeBackend();
      const g = gate();
      backend.runGate = g.promise;
      const ch = createChannel(backend, { nextFrame: async () => undefined });
      const blocker = ch.run('first');
      const a = ch.drag('k', 'move', 1, 'one');
      void ch.drag('k', 'move', 2, 'two');
      g.open();
      await Promise.all([blocker, a]);
      expect(backend.ran.at(-1)).toEqual({ id: 'move', payload: 2, coalesce: 'two' });
    });
  });
});
