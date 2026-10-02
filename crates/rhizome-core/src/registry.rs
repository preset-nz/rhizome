use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::RangeInclusive;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::diff::Changeset;
use crate::error::{Error, Result};
use crate::path::valid_name;
use crate::value::{KeyName, Value, ValueKind};
use crate::view::Node;

/// The type names the core owns. An app can't declare them.
pub const ROOT: &str = "root";
pub const CATEGORY: &str = "category";
pub const GROUP: &str = "group";

/// Whether a category's nodes are owned by a file or built from a recipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Loaded,
    Calculated,
}

/// One value key in a node type's schema. Declaration order is layout order.
#[derive(Clone, Debug, PartialEq)]
pub struct ValueSpec {
    pub key: String,
    pub kind: ValueKind,
    pub default: Value,
    /// Inclusive range for `Int` and `Float`; `Colour` components are always 0 to 1.
    pub range: Option<(f64, f64)>,
    pub choices: Vec<String>,
}

/// Why a value doesn't fit its spec.
#[derive(Debug, PartialEq)]
pub(crate) enum Problem {
    WrongKind,
    NotFinite,
    OutOfRange { min: f64, max: f64 },
    NotAChoice,
}

impl ValueSpec {
    fn new(key: impl KeyName, kind: ValueKind, default: Value) -> Self {
        ValueSpec {
            key: key.key_name().to_string(),
            kind,
            default,
            range: None,
            choices: Vec::new(),
        }
    }

    pub fn bool(key: impl KeyName, default: bool) -> Self {
        Self::new(key, ValueKind::Bool, Value::Bool(default))
    }

    pub fn int(key: impl KeyName, range: RangeInclusive<i64>, default: i64) -> Self {
        let mut s = Self::new(key, ValueKind::Int, Value::Int(default));
        s.range = Some((*range.start() as f64, *range.end() as f64));
        s
    }

    pub fn float(key: impl KeyName, range: RangeInclusive<f64>, default: f64) -> Self {
        let mut s = Self::new(key, ValueKind::Float, Value::Float(default));
        s.range = Some((*range.start(), *range.end()));
        s
    }

    pub fn text(key: impl KeyName, default: &str) -> Self {
        Self::new(key, ValueKind::Text, Value::Text(default.into()))
    }

    pub fn choice(key: impl KeyName, choices: &[&str], default: &str) -> Self {
        let mut s = Self::new(key, ValueKind::Choice, Value::Choice(default.into()));
        s.choices = choices.iter().map(|c| c.to_string()).collect();
        s
    }

    pub fn vec2(key: impl KeyName, default: [f64; 2]) -> Self {
        Self::new(key, ValueKind::Vec2, Value::Vec2(default))
    }

    pub fn vec3(key: impl KeyName, default: [f64; 3]) -> Self {
        Self::new(key, ValueKind::Vec3, Value::Vec3(default))
    }

    pub fn colour(key: impl KeyName, default: [f64; 4]) -> Self {
        Self::new(key, ValueKind::Colour, Value::Colour(default))
    }

    fn bounds(&self) -> Option<(f64, f64)> {
        match self.kind {
            ValueKind::Colour => Some((0.0, 1.0)),
            _ => self.range,
        }
    }

    pub(crate) fn problem(&self, v: &Value) -> Option<Problem> {
        if v.kind() != self.kind {
            return Some(Problem::WrongKind);
        }
        if v.floats().iter().any(|x| !x.is_finite()) {
            return Some(Problem::NotFinite);
        }
        let nums: Vec<f64> = match v {
            Value::Int(i) => vec![*i as f64],
            other => other.floats().to_vec(),
        };
        if let Some((min, max)) = self.bounds()
            && nums.iter().any(|x| *x < min || *x > max)
        {
            return Some(Problem::OutOfRange { min, max });
        }
        if let Value::Choice(c) = v
            && !self.choices.contains(c)
        {
            return Some(Problem::NotAChoice);
        }
        None
    }

    /// Pulls a numeric value into range. Used by load, never by a write.
    pub(crate) fn clamp(&self, v: &Value) -> Value {
        let Some((min, max)) = self.bounds() else {
            return v.clone();
        };
        let c = |x: f64| x.clamp(min, max);
        match v {
            Value::Int(i) => Value::Int((*i as f64).clamp(min, max) as i64),
            Value::Float(x) => Value::Float(c(*x)),
            Value::Vec2(a) => Value::Vec2(a.map(c)),
            Value::Vec3(a) => Value::Vec3(a.map(c)),
            Value::Colour(a) => Value::Colour(a.map(c)),
            other => other.clone(),
        }
    }
}

/// A rule the object model attaches to a node type, run on every node of that type at commit.
pub type Check = Arc<dyn Fn(&Node<'_>) -> Result<(), String> + Send + Sync>;

/// Why a tree rule refused an edit, and where.
#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub path: String,
    pub message: String,
}

/// A rule over the whole tree, run at every commit with the root and what the edit changed.
/// For what a per-node `check` can't say: "at most one per parent", "can't be removed".
pub type Rule = Arc<dyn Fn(&Node<'_>, &Changeset) -> Result<(), Violation> + Send + Sync>;

/// A node type, declared by an app's object model. The core stores and validates nodes of it
/// and never knows what it means.
#[derive(Clone)]
pub struct NodeType {
    pub(crate) name: String,
    pub(crate) values: Vec<ValueSpec>,
    pub(crate) refs: Vec<String>,
    pub(crate) slots: Vec<String>,
    pub(crate) bindable: Option<Vec<ValueSpec>>,
    pub(crate) categories: Vec<String>,
    pub(crate) check: Option<Check>,
}

impl fmt::Debug for NodeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NodeType")
            .field("name", &self.name)
            .finish()
    }
}

impl NodeType {
    pub fn new(name: &str) -> Self {
        NodeType {
            name: name.to_string(),
            values: Vec::new(),
            refs: Vec::new(),
            slots: Vec::new(),
            bindable: None,
            categories: Vec::new(),
            check: None,
        }
    }

    pub fn value(mut self, spec: ValueSpec) -> Self {
        self.values.push(spec);
        self
    }

    pub fn bool(self, key: impl KeyName, default: bool) -> Self {
        self.value(ValueSpec::bool(key, default))
    }

    pub fn int(self, key: impl KeyName, range: RangeInclusive<i64>, default: i64) -> Self {
        self.value(ValueSpec::int(key, range, default))
    }

    pub fn float(self, key: impl KeyName, range: RangeInclusive<f64>, default: f64) -> Self {
        self.value(ValueSpec::float(key, range, default))
    }

    pub fn text(self, key: impl KeyName, default: &str) -> Self {
        self.value(ValueSpec::text(key, default))
    }

    pub fn choice(self, key: impl KeyName, choices: &[&str], default: &str) -> Self {
        self.value(ValueSpec::choice(key, choices, default))
    }

    pub fn vec2(self, key: impl KeyName, default: [f64; 2]) -> Self {
        self.value(ValueSpec::vec2(key, default))
    }

    pub fn vec3(self, key: impl KeyName, default: [f64; 3]) -> Self {
        self.value(ValueSpec::vec3(key, default))
    }

    pub fn colour(self, key: impl KeyName, default: [f64; 4]) -> Self {
        self.value(ValueSpec::colour(key, default))
    }

    /// A key that holds a [`Ref`](crate::Ref): to a file, or to a node.
    pub fn reference(mut self, key: impl KeyName) -> Self {
        self.refs.push(key.key_name().to_string());
        self
    }

    /// A named slot another node can be bound into, such as `mask`. One source per slot.
    pub fn slot(mut self, name: &str) -> Self {
        self.slots.push(name.to_string());
        self
    }

    /// Nodes of this type can be a binding source. Each binding carries these values.
    pub fn bindable(mut self, values: impl IntoIterator<Item = ValueSpec>) -> Self {
        self.bindable = Some(values.into_iter().collect());
        self
    }

    /// Restricts the type to these categories. Without it, any category.
    pub fn in_categories(mut self, names: &[&str]) -> Self {
        self.categories = names.iter().map(|n| n.to_string()).collect();
        self
    }

    pub fn check(
        mut self,
        f: impl Fn(&Node<'_>) -> Result<(), String> + Send + Sync + 'static,
    ) -> Self {
        self.check = Some(Arc::new(f));
        self
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn values(&self) -> &[ValueSpec] {
        &self.values
    }

    pub fn spec(&self, key: &str) -> Option<&ValueSpec> {
        self.values.iter().find(|s| s.key == key)
    }

    pub fn refs(&self) -> &[String] {
        &self.refs
    }

    pub fn slots(&self) -> &[String] {
        &self.slots
    }

    pub fn binding_values(&self) -> Option<&[ValueSpec]> {
        self.bindable.as_deref()
    }

    pub fn categories(&self) -> &[String] {
        &self.categories
    }
}

/// The node types and categories one app declares. Frozen once built.
pub struct Registry {
    categories: Vec<(String, Origin)>,
    types: BTreeMap<String, NodeType>,
    pub(crate) rules: Vec<Rule>,
}

#[derive(Default)]
pub struct RegistryBuilder {
    categories: Vec<(String, Origin)>,
    types: Vec<NodeType>,
    rules: Vec<Rule>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("categories", &self.categories)
            .field("types", &self.types.keys().collect::<Vec<_>>())
            .field("rules", &self.rules.len())
            .finish()
    }
}

impl Registry {
    pub fn builder() -> RegistryBuilder {
        RegistryBuilder::default()
    }

    pub fn node_type(&self, name: &str) -> Option<&NodeType> {
        self.types.get(name)
    }

    pub fn category(&self, name: &str) -> Option<Origin> {
        self.categories
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, o)| *o)
    }

    pub fn categories(&self) -> impl Iterator<Item = (&str, Origin)> {
        self.categories.iter().map(|(n, o)| (n.as_str(), *o))
    }

    pub fn node_types(&self) -> impl Iterator<Item = &NodeType> {
        self.types.values()
    }
}

impl RegistryBuilder {
    pub fn category(&mut self, name: &str, origin: Origin) -> &mut Self {
        self.categories.push((name.to_string(), origin));
        self
    }

    pub fn node(&mut self, t: NodeType) -> &mut Self {
        self.types.push(t);
        self
    }

    /// Adds a rule over the whole tree, run at every commit.
    pub fn rule(
        &mut self,
        f: impl Fn(&Node<'_>, &Changeset) -> Result<(), Violation> + Send + Sync + 'static,
    ) -> &mut Self {
        self.rules.push(Arc::new(f));
        self
    }

    pub fn build(&mut self) -> Result<Arc<Registry>> {
        let err = |m: String| Err(Error::Registry(m));
        let mut cats = BTreeSet::new();
        for (name, _) in &self.categories {
            if !valid_name(name) || !cats.insert(name.as_str()) {
                return err(format!("category `{name}` is invalid or declared twice"));
            }
        }
        let mut types = BTreeMap::new();
        for t in &self.types {
            if !valid_name(&t.name) || [ROOT, CATEGORY, GROUP].contains(&t.name.as_str()) {
                return err(format!("`{}` is not a usable type name", t.name));
            }
            let mut keys = BTreeSet::new();
            let all = t.values.iter().map(|s| &s.key).chain(&t.refs);
            for k in all {
                if k.is_empty() || !keys.insert(k.as_str()) {
                    return err(format!(
                        "`{}`: key `{k}` is empty or declared twice",
                        t.name
                    ));
                }
            }
            let mut slots = BTreeSet::new();
            if !t.slots.iter().all(|s| !s.is_empty() && slots.insert(s)) {
                return err(format!("`{}`: a slot is empty or declared twice", t.name));
            }
            let specs = t.values.iter().chain(t.bindable.iter().flatten());
            for s in specs {
                if s.problem(&s.default).is_some() {
                    return err(format!(
                        "`{}`: default of `{}` doesn't fit its spec",
                        t.name, s.key
                    ));
                }
            }
            if let Some(c) = t.categories.iter().find(|c| !cats.contains(c.as_str())) {
                return err(format!("`{}`: category `{c}` is not declared", t.name));
            }
            if types.insert(t.name.clone(), t.clone()).is_some() {
                return err(format!("type `{}` declared twice", t.name));
            }
        }
        Ok(Arc::new(Registry {
            categories: self.categories.clone(),
            types,
            rules: self.rules.clone(),
        }))
    }
}
