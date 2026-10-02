//! Themes: a shared choice that nodes follow by reference and look up through their
//! ancestors, such as a palette chosen for a campaign and overridden for one map.
//!
//! A theme is never written onto a node. A node follows one; `resolve` walks up from any
//! node to the nearest follower, else the theme's fallback. The choice is a reference, so
//! it saves, undoes and diffs like any other. Built-in themes are code; user themes come
//! when an app needs them.

use std::collections::BTreeMap;
use std::marker::PhantomData;

use rhizome_core::{Edit, NodeId, Ref, Tree};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value as Json;

use crate::error::{Error, Result};

/// The reference key a following kind gets for theme `kind`.
pub fn theme_key(kind: &str) -> String {
    format!("theme.{kind}")
}

fn theme_file(kind: &str, name: &str) -> String {
    format!("theme:{kind}/{name}")
}

struct Entry {
    catalogue: Vec<(String, Json)>,
    fallback: Option<String>,
    followers: Vec<String>,
}

/// The app's themes, as [`ObjectModel::themes`](crate::ObjectModel::themes) declares them.
#[derive(Default)]
pub struct Themes {
    kinds: BTreeMap<String, Entry>,
}

/// One theme being declared.
pub struct ThemeRef<'a, S: Serialize> {
    entry: &'a mut Entry,
    _s: PhantomData<S>,
}

impl<S: Serialize> ThemeRef<'_, S> {
    /// The built-in themes, in menu order.
    pub fn catalogue<'n>(self, entries: impl IntoIterator<Item = (&'n str, S)>) -> Self {
        for (name, state) in entries {
            let json = serde_json::to_value(state).expect("theme state serialises");
            self.entry.catalogue.push((name.to_string(), json));
        }
        self
    }

    /// What a node resolves to when neither it nor an ancestor follows anything.
    pub fn fallback(self, name: &str) -> Self {
        self.entry.fallback = Some(name.to_string());
        self
    }

    /// Kinds that can follow this theme. Their descendants resolve through them.
    pub fn followed_by(self, kinds: &[&str]) -> Self {
        self.entry
            .followers
            .extend(kinds.iter().map(|k| k.to_string()));
        self
    }
}

/// A resolved theme: which node chose it (`None` for the fallback), its name, its state.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedTheme {
    pub follower: Option<NodeId>,
    pub name: String,
    pub state: Json,
}

impl ResolvedTheme {
    /// The state as the app's own type.
    pub fn state<S: DeserializeOwned>(&self) -> Result<S> {
        serde_json::from_value(self.state.clone()).map_err(|e| Error::Preset(e.to_string()))
    }
}

impl Themes {
    pub fn theme<S: Serialize>(&mut self, id: &str) -> ThemeRef<'_, S> {
        let entry = self.kinds.entry(id.to_string()).or_insert_with(|| Entry {
            catalogue: Vec::new(),
            fallback: None,
            followers: Vec::new(),
        });
        ThemeRef {
            entry,
            _s: PhantomData,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        for (id, e) in &self.kinds {
            if !rhizome_core::valid_name(id) {
                return Err(Error::Model(format!(
                    "theme id `{id}` must be a valid node name"
                )));
            }
            if let Some(f) = &e.fallback
                && !e.catalogue.iter().any(|(n, _)| n == f)
            {
                return Err(Error::Model(format!(
                    "theme `{id}`: fallback `{f}` isn't in its catalogue"
                )));
            }
        }
        Ok(())
    }

    pub(crate) fn followers(&self) -> Vec<(String, Vec<String>)> {
        self.kinds
            .iter()
            .map(|(id, e)| (id.clone(), e.followers.clone()))
            .collect()
    }

    fn entry(&self, kind: &str) -> Result<&Entry> {
        self.kinds
            .get(kind)
            .ok_or_else(|| Error::UnknownTheme(kind.into()))
    }

    /// The built-in themes of `kind`, in menu order.
    pub fn names(&self, kind: &str) -> Result<Vec<String>> {
        Ok(self
            .entry(kind)?
            .catalogue
            .iter()
            .map(|(n, _)| n.clone())
            .collect())
    }

    /// The node's own choice, else the nearest ancestor's, else the fallback.
    pub fn resolve(&self, tree: &Tree, kind: &str, node: NodeId) -> Result<Option<ResolvedTheme>> {
        let e = self.entry(kind)?;
        let key = theme_key(kind);
        let prefix = format!("theme:{kind}/");
        let mut at = tree.get(node);
        if at.is_none() {
            return Err(Error::Rhizome(rhizome_core::Error::NotFound(
                node.to_string(),
            )));
        }
        while let Some(n) = at {
            let chosen = n
                .reference(&key)
                .and_then(|r| r.file.as_deref())
                .and_then(|f| f.strip_prefix(&prefix))
                .and_then(|name| e.catalogue.iter().find(|(c, _)| c == name));
            if let Some((name, state)) = chosen {
                return Ok(Some(ResolvedTheme {
                    follower: Some(n.id()),
                    name: name.clone(),
                    state: state.clone(),
                }));
            }
            at = n.parent();
        }
        Ok(e.fallback.as_ref().map(|name| ResolvedTheme {
            follower: None,
            name: name.clone(),
            state: e
                .catalogue
                .iter()
                .find(|(c, _)| c == name)
                .map(|(_, s)| s.clone())
                .expect("validated"),
        }))
    }

    /// Makes `node` follow a theme, or stop following with `None`.
    pub(crate) fn follow(
        &self,
        tx: &mut Edit<'_>,
        kind: &str,
        node: NodeId,
        name: Option<&str>,
    ) -> rhizome_core::Result<()> {
        let e = self
            .entry(kind)
            .map_err(|e| rhizome_core::Error::Structural(e.to_string()))?;
        let key = theme_key(kind);
        let Some(name) = name else {
            return tx.clear_ref(node, key);
        };
        if !e.catalogue.iter().any(|(n, _)| n == name) {
            return Err(rhizome_core::Error::Structural(format!(
                "no {kind} theme “{name}”"
            )));
        }
        tx.set_ref(node, key, Ref::file(theme_file(kind, name)))
    }
}
