use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::edit::Edit;
use crate::error::{Error, Result};
use crate::file::{Fragment, Issue};
use crate::id::NodeId;
use crate::state::{On, Ref};
use crate::value::Value;

/// One write verb as data. IPC, CLIs, tests and scripts all speak `Op`.
///
/// Every place that names a node takes a path (`/images/sky`) or an id. Values are plain
/// JSON, read against the node's schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Op {
    Add {
        parent: String,
        #[serde(rename = "type")]
        type_name: String,
        name: String,
    },
    AddUnique {
        parent: String,
        #[serde(rename = "type")]
        type_name: String,
        base: String,
    },
    Remove {
        at: String,
    },
    Rename {
        at: String,
        name: String,
    },
    MoveTo {
        at: String,
        parent: String,
    },
    Copy {
        at: String,
        parent: String,
    },
    Paste {
        parent: String,
        fragment: String,
    },
    Set {
        at: String,
        key: String,
        value: Json,
    },
    Reset {
        at: String,
        key: String,
    },
    SetRef {
        at: String,
        key: String,
        #[serde(rename = "ref")]
        reference: Ref,
    },
    ClearRef {
        at: String,
        key: String,
    },
    Bind {
        target: String,
        on: On,
        source: String,
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        values: BTreeMap<String, Json>,
    },
    Unbind {
        target: String,
        on: On,
        source: String,
    },
    Join {
        group: String,
        members: Vec<String>,
    },
    Leave {
        group: String,
        members: Vec<String>,
    },
    SetOrder {
        owner: String,
        name: String,
        ids: Vec<String>,
    },
    AppendToOrder {
        owner: String,
        name: String,
        id: String,
    },
}

/// What an applied `Op` made, when it made something.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Applied {
    pub node: Option<NodeId>,
    pub issues: Vec<Issue>,
}

fn kind_error(path: String, key: &str, expected: crate::ValueKind, got: &Json) -> Error {
    Error::WrongKind {
        path,
        key: key.into(),
        expected,
        got: got.to_string(),
    }
}

impl Edit<'_> {
    /// Runs one `Op`, exactly as its verb would.
    pub fn apply(&mut self, op: &Op) -> Result<Applied> {
        let made = |id: NodeId| Applied {
            node: Some(id),
            issues: Vec::new(),
        };
        let done = Applied::default();
        Ok(match op {
            Op::Add {
                parent,
                type_name,
                name,
            } => made(self.add(parent, type_name, name)?),
            Op::AddUnique {
                parent,
                type_name,
                base,
            } => made(self.add_unique(parent, type_name, base)?),
            Op::Remove { at } => {
                self.remove(at)?;
                done
            }
            Op::Rename { at, name } => {
                self.rename(at, name)?;
                done
            }
            Op::MoveTo { at, parent } => {
                self.move_to(at, parent)?;
                done
            }
            Op::Copy { at, parent } => made(self.copy(at, parent)?),
            Op::Paste { parent, fragment } => {
                let report = self.paste(parent, &Fragment::from_text(fragment)?)?;
                Applied {
                    node: report.nodes.first().copied(),
                    issues: report.issues,
                }
            }
            Op::Set { at, key, value } => {
                let node = self.at(at).ok_or_else(|| Error::NotFound(at.clone()))?;
                let path = node.path().to_string();
                if node.is_opaque() {
                    return Err(Error::Opaque(path));
                }
                let spec = node.node_type().and_then(|t| t.spec(key)).ok_or_else(|| {
                    Error::UnknownKey {
                        path: path.clone(),
                        key: key.clone(),
                    }
                })?;
                let v = Value::from_json(spec.kind, value)
                    .ok_or_else(|| kind_error(path, key, spec.kind, value))?;
                self.set_value(at, key, v)?;
                done
            }
            Op::Reset { at, key } => {
                self.reset(at, key.as_str())?;
                done
            }
            Op::SetRef { at, key, reference } => {
                self.set_ref(at, key.as_str(), reference.clone())?;
                done
            }
            Op::ClearRef { at, key } => {
                self.clear_ref(at, key.as_str())?;
                done
            }
            Op::Bind {
                target,
                on,
                source,
                values,
            } => {
                let src = self
                    .at(source)
                    .ok_or_else(|| Error::NotFound(source.clone()))?;
                let path = src.path().to_string();
                if src.is_opaque() {
                    return Err(Error::Opaque(path));
                }
                let specs = src
                    .node_type()
                    .and_then(|t| t.binding_values())
                    .ok_or_else(|| Error::NotBindable(path.clone()))?;
                let mut vals = Vec::new();
                for (k, j) in values {
                    let spec =
                        specs
                            .iter()
                            .find(|s| &s.key == k)
                            .ok_or_else(|| Error::UnknownKey {
                                path: path.clone(),
                                key: k.clone(),
                            })?;
                    let v = Value::from_json(spec.kind, j)
                        .ok_or_else(|| kind_error(path.clone(), k, spec.kind, j))?;
                    vals.push((k.clone(), v));
                }
                self.bind(target, on.clone(), source, vals)?;
                done
            }
            Op::Unbind { target, on, source } => {
                self.unbind(target, on.clone(), source)?;
                done
            }
            Op::Join { group, members } => {
                self.join(group, members)?;
                done
            }
            Op::Leave { group, members } => {
                self.leave(group, members)?;
                done
            }
            Op::SetOrder { owner, name, ids } => {
                self.set_order(owner, name, ids)?;
                done
            }
            Op::AppendToOrder { owner, name, id } => {
                self.append_to_order(owner, name, id)?;
                done
            }
        })
    }
}
