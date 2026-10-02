/// What a document, a preset or a command can refuse.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Rhizome(#[from] rhizome_core::Error),
    #[error("couldn't read or write {path}: {message}")]
    Io { path: String, message: String },
    #[error("the document has no file yet; save it as something first")]
    NoPath,
    #[error("{0}")]
    Preset(String),
    #[error("no command `{0}`")]
    UnknownCommand(String),
    #[error("`{0}` isn't available right now")]
    Disabled(String),
    #[error("command payload: {0}")]
    Payload(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
