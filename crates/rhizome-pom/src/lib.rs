//! rhizome-pom: POM, the Preset Object Model.
//!
//! The base every app's object model is built on, between `rhizome-core` (mechanics) and
//! the app (business logic). It knows no app. An app implements [`ObjectModel`] with only
//! its parts (kinds and their policy, preset aggregates, commands, a projection) and gets
//! the rest from [`Document`]: files, history passed through, re-projection after every
//! change, presets, policy enforced at commit, and built-in commands.
//!
//! The design is `design/pom.md` in the guidance repo.

mod command;
mod document;
mod error;
mod host;
mod model;
mod presets;

pub use command::{Command, Commands, Outcome, payload};
pub use document::{Document, FileStore, MemoryStore, Store};
pub use error::{Error, Result};
pub use host::{CommandState, Event, Host, Issue, Pom, Ran, Status, host};
pub use model::{KindRef, Kinds, ObjectModel, Pin, Policy};
pub use presets::{
    Aggregate, BindingState, KindPresetsRef, NodeValues, NodeValuesState, PRESET, PRESETS,
    PresetRef, Report,
};
