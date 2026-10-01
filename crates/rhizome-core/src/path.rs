use std::cmp::Ordering;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::Error;

/// An address in the tree: `/images/sky/blur`. The root is `/`.
///
/// Paths order segment by segment, so a parent always sorts before its children.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Path(String);

/// A node name: ASCII letters, digits, `_`, `-` and `.`, and not `.` or `..`.
pub fn valid_name(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

impl Path {
    pub fn root() -> Path {
        Path("/".into())
    }

    pub fn parse(s: &str) -> Result<Path, Error> {
        if s == "/" {
            return Ok(Path::root());
        }
        let bad = || Error::InvalidPath(s.to_string());
        let rest = s.strip_prefix('/').ok_or_else(bad)?;
        if !rest.split('/').all(valid_name) {
            return Err(bad());
        }
        Ok(Path(s.to_string()))
    }

    pub fn is_root(&self) -> bool {
        self.0 == "/"
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|s| !s.is_empty())
    }

    pub fn depth(&self) -> usize {
        self.segments().count()
    }

    /// The last segment; empty for the root.
    pub fn name(&self) -> &str {
        self.0.rsplit('/').next().unwrap_or("")
    }

    pub fn parent(&self) -> Option<Path> {
        if self.is_root() {
            return None;
        }
        match self.0.rfind('/') {
            Some(0) => Some(Path::root()),
            Some(i) => Some(Path(self.0[..i].to_string())),
            None => None,
        }
    }

    /// Appends a segment. The caller has checked the name.
    pub(crate) fn join(&self, name: &str) -> Path {
        if self.is_root() {
            Path(format!("/{name}"))
        } else {
            Path(format!("{}/{name}", self.0))
        }
    }

    /// True when `self` is `ancestor` or lies under it.
    pub fn is_within(&self, ancestor: &Path) -> bool {
        ancestor.is_root()
            || self == ancestor
            || self
                .0
                .strip_prefix(&ancestor.0)
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl Ord for Path {
    fn cmp(&self, other: &Self) -> Ordering {
        self.segments().cmp(other.segments())
    }
}

impl PartialOrd for Path {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Path({})", self.0)
    }
}

impl Serialize for Path {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Path {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Path::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_refuses() {
        assert!(Path::parse("/").unwrap().is_root());
        assert_eq!(Path::parse("/a/b.c").unwrap().name(), "b.c");
        for bad in ["", "a", "/a/", "//", "/a//b", "/a/..", "/a b"] {
            assert!(Path::parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parent_sorts_before_children() {
        let a = Path::parse("/a").unwrap();
        let ab = Path::parse("/a/b").unwrap();
        let a_b = Path::parse("/a-b").unwrap();
        assert!(a < ab && ab < a_b);
        assert_eq!(ab.parent(), Some(a.clone()));
        assert_eq!(a.parent(), Some(Path::root()));
        assert!(ab.is_within(&a) && !a_b.is_within(&a));
    }
}
