/**
 * The one module that talks to `rhizome-pom-tauri`: its `pom_*` commands and its
 * `pom://commit` and `pom://status` events. Commands only (rhizome decision 46): there is no
 * way to send a raw edit. An app's own events and commands stay in the app.
 *
 * `PomBackend` is the interface; `tauriBackend` implements it over Tauri, and tests pass a fake.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { CommandState, Ran, Status, Update, View } from './wire.ts';

export type Unlisten = () => void;

export interface PomBackend {
  /**
   * The front end is listening. Ends any gesture a reloaded page left open, sends the status,
   * and answers a path macOS asked the app to open before the page was listening, if any. A file
   * opened that way wins over a restored one.
   */
  connect(): Promise<string | null>;
  view(): Promise<View>;
  status(): Promise<Status>;
  /** Every command with its label and whether it can run with `payload`. */
  commands(payload?: unknown): Promise<CommandState[]>;
  /**
   * Runs a command by id. With `coalesce`, consecutive runs with that key are one undo step.
   */
  run(id: string, payload?: unknown, coalesce?: string): Promise<Ran>;
  /** Opens a gesture: commands run until `end` are one undo step. Answers its token. */
  begin(label: string): Promise<number>;
  end(token: number): Promise<void>;
  /** Ends a gesture and takes back everything done in it. */
  cancel(token: number): Promise<void>;
  /** Every commit, in order: undo, redo and cancel included. */
  onUpdate(f: (u: Update) => void): Promise<Unlisten>;
  /** The status, whenever it changed. */
  onStatus(f: (s: Status) => void): Promise<Unlisten>;
}

/** The event names `rhizome-pom-tauri` emits. */
export const COMMIT_EVENT = 'pom://commit';
export const STATUS_EVENT = 'pom://status';

/** Tauri camelCases the argument names of a Rust command; these are all one word. */
export const tauriBackend: PomBackend = {
  connect: () => invoke<string | null>('pom_connect'),
  view: () => invoke<View>('pom_view'),
  status: () => invoke<Status>('pom_status'),
  commands: (payload) => invoke<CommandState[]>('pom_commands', { payload }),
  run: (id, payload = {}, coalesce) => invoke<Ran>('pom_run', { id, payload, coalesce }),
  begin: (label) => invoke<number>('pom_begin', { label }),
  end: (token) => invoke<void>('pom_end', { token }),
  cancel: (token) => invoke<void>('pom_cancel', { token }),
  onUpdate: (f) => listen<Update>(COMMIT_EVENT, (e) => f(e.payload)),
  onStatus: (f) => listen<Status>(STATUS_EVENT, (e) => f(e.payload)),
};
