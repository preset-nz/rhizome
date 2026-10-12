/**
 * React hooks over a `Mirror`, through `useSyncExternalStore`. The mirror is made outside
 * React and handed in with `PomProvider`, so StrictMode's double mount costs nothing. Rows are
 * immutable and a changed node is a new object, so a hook returns the same value until its own
 * node changes and the component re-renders only then.
 */
import {
  createContext,
  createElement,
  type ReactNode,
  useContext,
  useSyncExternalStore,
} from 'react';
import type { Mirror } from './mirror.ts';
import type { NodeId, Row, Status } from './wire.ts';

const MirrorContext = createContext<Mirror | null>(null);

export function PomProvider(props: { mirror: Mirror; children?: ReactNode }): ReactNode {
  return createElement(MirrorContext.Provider, { value: props.mirror }, props.children);
}

/** The mirror from the nearest `PomProvider`. */
export function useMirror(): Mirror {
  const mirror = useContext(MirrorContext);
  if (!mirror) throw new Error('usePom hooks need a <PomProvider mirror={…}> above them');
  return mirror;
}

/** The node with this id, or `undefined` while it isn't there (or before the first view). */
export function useNode(id: NodeId | null | undefined): Row | undefined {
  const mirror = useMirror();
  return useSyncExternalStore(
    mirror.subscribe,
    () => (id == null ? undefined : mirror.node(id)),
    () => undefined,
  );
}

/** The node at this path, e.g. `/images/sky`. It follows a rename or a move. */
export function useNodeAt(path: string | null | undefined): Row | undefined {
  const mirror = useMirror();
  return useSyncExternalStore(
    mirror.subscribe,
    () => (path == null ? undefined : mirror.nodeAt(path)),
    () => undefined,
  );
}

const NONE: readonly NodeId[] = Object.freeze([]);

/**
 * The ids in the node's named order, such as `draw`: the row's own `orders[order]` list, so it
 * is the same array until the node changes. Empty when the node or the order isn't there.
 */
export function useChildren(id: NodeId | null | undefined, order: string): readonly NodeId[] {
  const mirror = useMirror();
  return useSyncExternalStore(
    mirror.subscribe,
    () => (id == null ? undefined : mirror.node(id)?.orders?.[order]) ?? NONE,
    () => NONE,
  );
}

/** The document's status (title, unsaved, undo and redo labels, gesture), or `null` before it. */
export function useStatus(): Status | null {
  const mirror = useMirror();
  return useSyncExternalStore(
    mirror.subscribe,
    () => mirror.status,
    () => null,
  );
}
