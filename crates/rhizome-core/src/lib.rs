//! rhizome-core: the node API the preset.nz desktop apps share.
//!
//! A tree of nodes addressed by path from `/`. Each app declares its node types in a
//! [`Registry`]; the core stores and validates nodes of those types and never knows what they
//! mean. Reads go through borrowed [`Node`] views. Writes happen only inside an [`Edit`],
//! and every edit is one undo step and returns a [`Commit`] with what changed.
//!
//! The design is `design/node-api.md` and `design/rhizome-api.md` in the guidance repo.

mod diff;
mod dto;
mod edit;
mod error;
mod file;
mod id;
mod op;
mod path;
mod registry;
mod state;
mod tree;
mod value;
mod view;

pub use diff::{Change, ChangeKind, Changeset};
pub use dto::{
    BindingRow, CategorySchema, Patch, RefRow, Role, Row, Schema, TypeSchema, ValueSchema, row,
};
pub use edit::{At, Edit};
pub use error::{Error, Result};
pub use file::{FORMAT_VERSION, Fragment, Issue, LoadReport, PasteReport};
pub use id::{IdSource, NodeId};
pub use op::{Applied, Op, OpRef};
pub use path::{Path, valid_name};
pub use registry::{
    CATEGORY, Check, GROUP, NodeType, Origin, ROOT, Registry, RegistryBuilder, Rule, ValueSpec,
    Violation,
};
pub use state::{On, Ref};
pub use tree::{Commit, GestureId, HISTORY, Snapshot, Tree};
pub use value::{Choice, Colour, Key, KeyName, Value, ValueKind, ValueType};
pub use view::{Binding, Node};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_is_send() {
        fn send<T: Send>() {}
        send::<Tree>();
        send::<std::sync::Arc<Registry>>();
    }
}
