use crate::value::ValueKind;

/// Everything a write, a load or a registry build can refuse.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("no node at {0}")]
    NotFound(String),
    #[error("not a valid path: {0:?}")]
    InvalidPath(String),
    #[error("not a valid name: {0:?}")]
    InvalidName(String),
    #[error("not a valid node id: {0:?}")]
    InvalidId(String),
    #[error("{0} already exists")]
    NameTaken(String),
    #[error("node type `{0}` is not declared")]
    UnknownType(String),
    #[error("`{type_name}` can't live in category `{category}`")]
    NotAllowed { type_name: String, category: String },
    #[error("{0}")]
    Structural(String),
    #[error("{0} has a type this app doesn't know, so it can't be changed")]
    Opaque(String),
    #[error("{path} has no key `{key}`")]
    UnknownKey { path: String, key: String },
    #[error("{path} has no slot `{slot}`")]
    UnknownSlot { path: String, slot: String },
    #[error("{path}: `{key}` wants {expected:?}, got {got}")]
    WrongKind {
        path: String,
        key: String,
        expected: ValueKind,
        got: String,
    },
    #[error("{path}: `{key}` = {value} is outside {min}..={max}")]
    OutOfRange {
        path: String,
        key: String,
        value: String,
        min: f64,
        max: f64,
    },
    #[error("{path}: `{key}` must be a finite number")]
    NotFinite { path: String, key: String },
    #[error("{path}: `{key}` has no choice {value:?}")]
    NotAChoice {
        path: String,
        key: String,
        value: String,
    },
    #[error("{0} can't be a binding source")]
    NotBindable(String),
    #[error("{0} is not a group")]
    NotAGroup(String),
    #[error("{child} is not a child of {owner}")]
    NotAChild { owner: String, child: String },
    #[error("{0} is listed twice")]
    Duplicate(String),
    #[error("can't move {0} inside itself")]
    Cycle(String),
    #[error("can't move {from} under {to}: different category")]
    CrossesCategory { from: String, to: String },
    #[error("{path}: {message}")]
    Check { path: String, message: String },
    #[error("a gesture is already open")]
    GestureOpen,
    #[error("no such gesture is open")]
    NoGesture,
    #[error("not a rhizome file: {0}")]
    Format(String),
    #[error("rhizome file version {0} is newer than this build reads")]
    Version(u64),
    #[error("registry: {0}")]
    Registry(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
