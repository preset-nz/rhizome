use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::RangeInclusive;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::diff::Changeset;
use crate::error::{Error, Result};
use crate::path::valid_name;
use crate::shape::Shape;
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
    /// How many floats a `Floats` value holds.
    pub len: Option<usize>,
    /// What a `Shaped` value must look like.
    pub shape: Option<Shape>,
    /// The range's max is where a control's travel ends, not a limit: a typed value may go
    /// past it (Blender's soft max). The min still holds.
    pub soft_max: bool,
    /// What a person calls the value, such as `Drive`. Words only: the core never reads it.
    pub label: Option<String>,
    /// One sentence on what the value does, for documentation and hover text.
    pub description: Option<String>,
    /// A short unit shown after the number, such as `Hz`, `ms` or `%`.
    pub unit: Option<String>,
    /// Display names for an `Int` value's steps, in order from the range's minimum. Words
    /// only: the core never reads them.
    pub names: Option<Vec<String>>,
    /// Display labels for a `Choice` value's choices, by choice (`polaroid_600` shown as
    /// `Instant`). A choice without one shows as itself. Words only: the stored value and
    /// the check stay on the choice.
    pub choice_labels: BTreeMap<String, String>,
}

/// Why a value doesn't fit its spec.
#[derive(Debug, PartialEq)]
pub(crate) enum Problem {
    WrongKind,
    NotFinite,
    OutOfRange {
        min: f64,
        max: f64,
    },
    NotAChoice,
    /// A `Floats` of the wrong length, or a `Shaped` that doesn't fit: why.
    Misfit(String),
}

impl ValueSpec {
    fn new(key: impl KeyName, kind: ValueKind, default: Value) -> Self {
        ValueSpec {
            key: key.key_name().to_string(),
            kind,
            default,
            range: None,
            choices: Vec::new(),
            len: None,
            shape: None,
            soft_max: false,
            label: None,
            description: None,
            unit: None,
            names: None,
            choice_labels: BTreeMap::new(),
        }
    }

    /// Names the value for people. Words only; nothing reads it but documentation and a UI.
    pub fn label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    /// One sentence on what the value does.
    pub fn description(mut self, description: &str) -> Self {
        self.description = Some(description.to_string());
        self
    }

    /// Both words at once: the label and the sentence.
    pub fn described(self, label: &str, description: &str) -> Self {
        self.label(label).description(description)
    }

    /// A short unit shown after the number, such as `Hz`.
    pub fn unit(mut self, unit: &str) -> Self {
        self.unit = Some(unit.to_string());
        self
    }

    /// Display names for an int value's steps, in order from the range's minimum.
    pub fn names(mut self, names: &[&str]) -> Self {
        self.names = Some(names.iter().map(|n| n.to_string()).collect());
        self
    }

    /// Display labels for some of a choice value's choices, as (choice, label) pairs.
    ///
    /// # Panics
    /// On a label for a choice the value doesn't have: a mistake in the declaration.
    pub fn choice_labels(mut self, labels: &[(&str, &str)]) -> Self {
        for (choice, label) in labels {
            assert!(
                self.choices.iter().any(|c| c == choice),
                "`{}`: `choice_labels` names {choice:?}, which is not a choice",
                self.key
            );
            self.choice_labels
                .insert(choice.to_string(), label.to_string());
        }
        self
    }

    /// Makes the max soft: past it is allowed, the range stays for a control to span.
    pub fn soft_max(mut self) -> Self {
        self.soft_max = true;
        self
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

    /// A float with no natural range, such as a position or a rotation.
    pub fn float_unbounded(key: impl KeyName, default: f64) -> Self {
        Self::new(key, ValueKind::Float, Value::Float(default))
    }

    /// Exactly `default.len()` floats, such as a 4×4 matrix.
    pub fn floats(key: impl KeyName, default: &[f64]) -> Self {
        let mut s = Self::new(key, ValueKind::Floats, Value::Floats(default.to_vec()));
        s.len = Some(default.len());
        s
    }

    /// A structured value of `shape`, defaulting to the shape's empty value.
    pub fn shaped(key: impl KeyName, shape: Shape) -> Self {
        let mut s = Self::new(key, ValueKind::Shaped, Value::Shaped(shape.empty()));
        s.shape = Some(shape);
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
            && nums
                .iter()
                .any(|x| *x < min || (*x > max && !self.soft_max))
        {
            return Some(Problem::OutOfRange { min, max });
        }
        if let Value::Choice(c) = v
            && !self.choices.contains(c)
        {
            return Some(Problem::NotAChoice);
        }
        if let (Value::Floats(f), Some(n)) = (v, self.len)
            && f.len() != n
        {
            return Some(Problem::Misfit(format!(
                "should hold {n} numbers, not {}",
                f.len()
            )));
        }
        if let (Value::Shaped(j), Some(shape)) = (v, &self.shape)
            && let Err(why) = shape.check(j)
        {
            return Some(Problem::Misfit(why));
        }
        None
    }

    /// Pulls a numeric value into range. Used by load, never by a write.
    pub(crate) fn clamp(&self, v: &Value) -> Value {
        let Some((min, max)) = self.bounds() else {
            return v.clone();
        };
        let max = if self.soft_max { f64::INFINITY } else { max };
        let c = |x: f64| x.clamp(min, max);
        match v {
            Value::Int(i) => Value::Int((*i as f64).clamp(min, max) as i64),
            Value::Float(x) => Value::Float(c(*x)),
            Value::Vec2(a) => Value::Vec2(a.map(c)),
            Value::Vec3(a) => Value::Vec3(a.map(c)),
            Value::Colour(a) => Value::Colour(a.map(c)),
            Value::Floats(a) => Value::Floats(a.iter().copied().map(c).collect()),
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
    pub(crate) label: Option<String>,
    pub(crate) summary: Option<String>,
    pub(crate) group: Option<String>,
    pub(crate) category: Option<String>,
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
            label: None,
            summary: None,
            group: None,
            category: None,
        }
    }

    /// What a person calls the type, such as `Echo`. Words only: the core never reads it.
    /// Read back with [`label_text`](Self::label_text).
    pub fn label(mut self, label: &str) -> Self {
        self.label = Some(label.to_string());
        self
    }

    /// One sentence on what a node of this type is. Read back with
    /// [`summary_text`](Self::summary_text).
    pub fn summary(mut self, summary: &str) -> Self {
        self.summary = Some(summary.to_string());
        self
    }

    /// A free tag the app sorts its types by, such as `effect`. Unrelated to group nodes. Read
    /// back with [`group_tag`](Self::group_tag).
    pub fn group(mut self, group: &str) -> Self {
        self.group = Some(group.to_string());
        self
    }

    /// What the node is to someone browsing, such as `colour`, `abstract` or `reverb`. Sits
    /// beside [`group`](Self::group), which is the app's lane; a picker or an index sorts by
    /// this. Free text, so an app keeps its short list in one place. Read back with
    /// [`category_tag`](Self::category_tag). Unrelated to the registry's categories, which
    /// say where a node may live.
    pub fn category(mut self, category: &str) -> Self {
        self.category = Some(category.to_string());
        self
    }

    /// Gives the most recently added value its label and one-sentence description:
    /// `.float("drive", 1.0..=10.0, 1.0).described("Drive", "Pushes the signal harder.")`.
    ///
    /// # Panics
    /// Before any value has been added: a mistake in the declaration, not in data.
    pub fn described(self, label: &str, description: &str) -> Self {
        self.last_value(|s| s.described(label, description), "described")
    }

    /// Gives the most recently added value a unit: `.float("delay", 0.0..=2000.0, 250.0).unit("ms")`.
    ///
    /// # Panics
    /// Before any value has been added.
    pub fn unit(self, unit: &str) -> Self {
        self.last_value(|s| s.unit(unit), "unit")
    }

    /// Gives the most recently added int value display names for its steps, in order from
    /// the range's minimum: `.int("mode", 0..=2, 0).names(&["Off", "Half", "Full"])`.
    ///
    /// # Panics
    /// Before any value has been added.
    pub fn names(self, names: &[&str]) -> Self {
        self.last_value(|s| s.names(names), "names")
    }

    /// Gives the most recently added choice value display labels for some of its choices:
    /// `.choice("stock", &["sx70", "plain"], "plain").choice_labels(&[("sx70", "Instant")])`.
    ///
    /// # Panics
    /// Before any value has been added, or on a label for a choice the value doesn't have.
    pub fn choice_labels(self, labels: &[(&str, &str)]) -> Self {
        self.last_value(|s| s.choice_labels(labels), "choice_labels")
    }

    fn last_value(mut self, f: impl FnOnce(ValueSpec) -> ValueSpec, what: &str) -> Self {
        let Some(last) = self.values.pop() else {
            panic!("`{}`: `{what}` follows a value", self.name);
        };
        self.values.push(f(last));
        self
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

    pub fn float_unbounded(self, key: impl KeyName, default: f64) -> Self {
        self.value(ValueSpec::float_unbounded(key, default))
    }

    pub fn floats(self, key: impl KeyName, default: &[f64]) -> Self {
        self.value(ValueSpec::floats(key, default))
    }

    pub fn shaped(self, key: impl KeyName, shape: Shape) -> Self {
        self.value(ValueSpec::shaped(key, shape))
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

    /// The label given with [`label`](Self::label), if any.
    pub fn label_text(&self) -> Option<&str> {
        self.label.as_deref()
    }

    /// The sentence given with [`summary`](Self::summary), if any.
    pub fn summary_text(&self) -> Option<&str> {
        self.summary.as_deref()
    }

    /// The tag given with [`group`](Self::group), if any.
    pub fn group_tag(&self) -> Option<&str> {
        self.group.as_deref()
    }

    /// The category given with [`category`](Self::category), if any.
    pub fn category_tag(&self) -> Option<&str> {
        self.category.as_deref()
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
