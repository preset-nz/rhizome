/**
 * The wire types: what `rhizome-pom-tauri` carries, written from the Rust structs. Each says
 * which one it follows. Rust skips empty maps and lists and unset options when it serialises,
 * so those fields are optional here. A test reads the goldens in the crates' `tests/golden/` folders so
 * a drift between the two fails the build.
 */

/** A node's id: 26 characters of Crockford base32 (rhizome-core `NodeId`, as a string). */
export type NodeId = string;

/** Plain JSON, as a node's values travel (rhizome-core `Value::to_json`). */
export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

/** What a node is to the tree (rhizome-core `Role`, snake_case). */
export type Role = 'root' | 'category' | 'group' | 'node' | 'opaque';

/** What a binding attaches to on its target (rhizome-core `On`, snake_case). */
export type On = { slot: string } | { value: string };

/** A reference by id as a row carries it (rhizome-core `RefRow`). */
export interface RefRow {
  file?: string;
  node?: NodeId;
}

/** A reference as a change carries it (rhizome-core `Ref`): a row's, plus the path written to disk. */
export interface Ref extends RefRow {
  path?: string;
}

/** A binding onto a node, its values resolved (rhizome-core `BindingRow`). */
export interface BindingRow {
  on: On;
  source: NodeId;
  values?: Record<string, Json>;
}

/** One node as plain data (rhizome-core `Row`; `type_name` is `type` on the wire). */
export interface Row {
  id: NodeId;
  path: string;
  name: string;
  type: string;
  role: Role;
  parent: NodeId | null;
  /** Every value in the schema, resolved. */
  values?: Record<string, Json>;
  /** The keys actually stored; the rest are defaults. */
  set?: string[];
  refs?: Record<string, RefRow>;
  bindings?: BindingRow[];
  /** Named child orders, such as a draw order. */
  orders?: Record<string, NodeId[]>;
  members?: NodeId[];
}

/** Whether a category's nodes are owned by a file or built from a recipe (rhizome-core `Origin`). */
export type Origin = 'loaded' | 'calculated';

/** rhizome-core `ValueKind`, snake_case. */
export type ValueKind =
  | 'bool'
  | 'int'
  | 'float'
  | 'text'
  | 'choice'
  | 'vec2'
  | 'vec3'
  | 'colour'
  | 'floats'
  | 'shaped';

/** What a `shaped` value must look like (rhizome-core `Shape`, externally tagged, snake_case). */
export type Shape =
  | 'bool'
  | 'int'
  | 'float'
  | 'text'
  | 'vec2'
  | 'vec3'
  | 'colour'
  | { choice: string[] }
  | { list: Shape }
  | { record: [string, Shape][] }
  | { optional: Shape };

/** rhizome-core `ValueSchema`. */
export interface ValueSchema {
  key: string;
  kind: ValueKind;
  default: Json;
  range?: [number, number];
  choices?: string[];
  len?: number;
  shape?: Shape;
  /** The range's max is a control's end, not a limit. Absent when false. */
  soft_max?: boolean;
  /** What a person calls the value. Absent when the app gave none. */
  label?: string;
  /** One sentence on what the value does. */
  description?: string;
  /** A short unit shown after the number, such as `Hz`. */
  unit?: string;
  /** Display names for an int value's steps, in order from the range's minimum. */
  names?: string[];
  /** Display labels for some of the choices, by choice. Absent when none. */
  choice_labels?: Record<string, string>;
}

/** rhizome-core `CategorySchema`. */
export interface CategorySchema {
  name: string;
  origin: Origin;
}

/** rhizome-core `TypeSchema`. */
export interface TypeSchema {
  name: string;
  /** What a person calls the type. Absent when the app gave none. */
  label?: string;
  /** One sentence on what a node of this type is. */
  summary?: string;
  /** The app's free tag for sorting its types, such as `effect`. */
  group?: string;
  /** What the node is to someone browsing, such as `colour`; beside `group`. Absent when the app gave none. */
  category?: string;
  categories?: string[];
  values?: ValueSchema[];
  refs?: string[];
  slots?: string[];
  bindable?: ValueSchema[];
}

/** The registry as plain data (rhizome-core `Schema`). */
export interface Schema {
  categories: CategorySchema[];
  types: TypeSchema[];
}

/** One entry of a commit, tagged by `change` (rhizome-core `Change` with its flattened `ChangeKind`). */
export type Change = { path: string; id: NodeId } & (
  | { change: 'added'; type: string }
  | { change: 'removed'; type: string }
  | { change: 'moved'; from: string }
  | { change: 'value'; key: string; from: Json | null; to: Json | null }
  | { change: 'ref'; key: string; from: Ref | null; to: Ref | null }
  | {
      change: 'bound';
      on: On;
      source: NodeId;
      from: Record<string, Json> | null;
      to: Record<string, Json> | null;
    }
  | { change: 'reordered'; order: string; from: NodeId[]; to: NodeId[] }
  | { change: 'membership'; added: NodeId[]; removed: NodeId[] }
);

/** A commit (rhizome-core `Commit`; a `Changeset` is its entries as a list). */
export interface Commit {
  seq: number;
  label: string;
  changes: Change[];
}

/** The whole document, for a mirror to start from (rhizome-pom `View`). */
export interface View {
  seq: number;
  generation: number;
  schema: Schema;
  rows: Row[];
}

/** A commit as a mirror takes it, on `pom://commit` (rhizome-pom `Update`). */
export interface Update {
  seq: number;
  label: string;
  changes: Change[];
  /** The fresh rows of every node the commit touched. */
  rows: Row[];
  removed: NodeId[];
  /** The tree this belongs to. */
  generation: number;
}

/** The document as a window shows it, on `pom://status` (rhizome-pom `Status`). */
export interface Status {
  title: string;
  path: string | null;
  unsaved: boolean;
  /** What Undo and Redo would do, when they can. */
  undo: string | null;
  redo: string | null;
  gesture: boolean;
  /** Only grows; moves whenever the whole tree is replaced. */
  generation: number;
}

/** What `pom_run` answers (rhizome-pom `Ran`, snake_case, externally tagged). */
export type Ran = 'nothing' | { committed: Commit } | { text: string };

/** A command as a menu shows it (rhizome-pom `CommandState`). */
export interface CommandState {
  id: string;
  label: string;
  enabled: boolean;
}
