use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Error;

/// A node's identity: globally unique, never reused, stable across rename and move.
///
/// Written as a 26-character ULID. Paths are addresses; ids are identity.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(u128);

impl NodeId {
    pub const fn from_u128(n: u128) -> Self {
        NodeId(n)
    }

    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", ulid::Ulid(self.0))
    }
}

impl fmt::Debug for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "NodeId({self})")
    }
}

impl FromStr for NodeId {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        ulid::Ulid::from_string(s)
            .map(|u| NodeId(u.0))
            .map_err(|_| Error::InvalidId(s.to_string()))
    }
}

impl Serialize for NodeId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Where new ids come from. `Ulid` in apps; `Sequential` in tests, so golden files are stable.
#[derive(Clone, Debug)]
pub enum IdSource {
    Ulid,
    Sequential { next: u128 },
}

impl IdSource {
    pub fn sequential() -> Self {
        IdSource::Sequential { next: 1 }
    }

    pub(crate) fn next(&mut self) -> NodeId {
        match self {
            IdSource::Ulid => NodeId(ulid::Ulid::new().0),
            IdSource::Sequential { next } => {
                let id = *next;
                *next += 1;
                NodeId(id)
            }
        }
    }

    /// After a load, a sequential source must never hand out an id the file already uses.
    pub(crate) fn skip_past(&mut self, id: NodeId) {
        if let IdSource::Sequential { next } = self
            && id.0 >= *next
        {
            *next = id.0 + 1;
        }
    }
}
