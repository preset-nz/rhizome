//! The declared shape of a structured value (decision 51): lists, records and optionals of
//! scalar kinds. A shaped value is plain JSON, checked against its shape on every write and
//! load, and diffed as one value. Vector paths are the case it exists for:
//!
//! ```
//! # use rhizome_core::Shape;
//! let paths = Shape::list(Shape::record([
//!     ("anchors", Shape::list(Shape::record([
//!         ("point", Shape::Vec2),
//!         ("handle_in", Shape::optional(Shape::Vec2)),
//!         ("handle_out", Shape::optional(Shape::Vec2)),
//!     ]))),
//!     ("closed", Shape::Bool),
//! ]));
//! assert!(paths.check(&serde_json::json!([{"anchors": [{"point": [0, 0]}], "closed": true}])).is_ok());
//! ```

use serde::Serialize;
use serde_json::Value as Json;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Bool,
    Int,
    Float,
    Text,
    /// One of these strings.
    Choice(Vec<String>),
    Vec2,
    Vec3,
    /// Red, green, blue, alpha, each 0 to 1.
    Colour,
    List(Box<Shape>),
    /// Named fields in declared order. A field whose shape is optional may be absent.
    Record(Vec<(String, Shape)>),
    /// The shape, or absent, or `null`.
    Optional(Box<Shape>),
}

impl Shape {
    pub fn list(item: Shape) -> Shape {
        Shape::List(Box::new(item))
    }

    pub fn record<'k>(fields: impl IntoIterator<Item = (&'k str, Shape)>) -> Shape {
        Shape::Record(
            fields
                .into_iter()
                .map(|(k, s)| (k.to_string(), s))
                .collect(),
        )
    }

    pub fn optional(inner: Shape) -> Shape {
        Shape::Optional(Box::new(inner))
    }

    pub fn choice(choices: &[&str]) -> Shape {
        Shape::Choice(choices.iter().map(|c| c.to_string()).collect())
    }

    /// The simplest value of the shape: an empty list, a record of its fields' empties, an
    /// absent optional, zero, false, the empty text, the first choice.
    pub fn empty(&self) -> Json {
        match self {
            Shape::Bool => Json::Bool(false),
            Shape::Int => Json::from(0),
            Shape::Float => Json::from(0.0),
            Shape::Text => Json::String(String::new()),
            Shape::Choice(c) => Json::String(c.first().cloned().unwrap_or_default()),
            Shape::Vec2 => serde_json::json!([0.0, 0.0]),
            Shape::Vec3 => serde_json::json!([0.0, 0.0, 0.0]),
            Shape::Colour => serde_json::json!([0.0, 0.0, 0.0, 1.0]),
            Shape::List(_) => Json::Array(vec![]),
            Shape::Record(fields) => Json::Object(
                fields
                    .iter()
                    .filter(|(_, s)| !matches!(s, Shape::Optional(_)))
                    .map(|(k, s)| (k.clone(), s.empty()))
                    .collect(),
            ),
            Shape::Optional(_) => Json::Null,
        }
    }

    /// Whether `v` has this shape; if not, where it first doesn't, as a path like
    /// `[2].anchors[0].point`.
    pub fn check(&self, v: &Json) -> Result<(), String> {
        self.check_at(v, "")
    }

    fn check_at(&self, v: &Json, at: &str) -> Result<(), String> {
        let here = |what: &str| {
            let at = if at.is_empty() { "the value" } else { at };
            Err(format!("{at} should be {what}"))
        };
        let nums = |n: usize, unit: bool| -> bool {
            v.as_array().is_some_and(|a| {
                a.len() == n
                    && a.iter().all(|x| {
                        x.as_f64()
                            .is_some_and(|f| f.is_finite() && (!unit || (0.0..=1.0).contains(&f)))
                    })
            })
        };
        match self {
            Shape::Bool if v.is_boolean() => Ok(()),
            Shape::Bool => here("true or false"),
            Shape::Int if v.is_i64() => Ok(()),
            Shape::Int => here("a whole number"),
            Shape::Float if v.as_f64().is_some_and(f64::is_finite) => Ok(()),
            Shape::Float => here("a number"),
            Shape::Text if v.is_string() => Ok(()),
            Shape::Text => here("text"),
            Shape::Choice(c) if v.as_str().is_some_and(|s| c.iter().any(|x| x == s)) => Ok(()),
            Shape::Choice(c) => here(&format!("one of {}", c.join(", "))),
            Shape::Vec2 if nums(2, false) => Ok(()),
            Shape::Vec2 => here("two numbers"),
            Shape::Vec3 if nums(3, false) => Ok(()),
            Shape::Vec3 => here("three numbers"),
            Shape::Colour if nums(4, true) => Ok(()),
            Shape::Colour => here("four numbers from 0 to 1"),
            Shape::Optional(_) if v.is_null() => Ok(()),
            Shape::Optional(inner) => inner.check_at(v, at),
            Shape::List(item) => {
                let Some(a) = v.as_array() else {
                    return here("a list");
                };
                for (i, x) in a.iter().enumerate() {
                    item.check_at(x, &format!("{at}[{i}]"))?;
                }
                Ok(())
            }
            Shape::Record(fields) => {
                let Some(o) = v.as_object() else {
                    return here("a record");
                };
                for k in o.keys() {
                    if !fields.iter().any(|(f, _)| f == k) {
                        return Err(format!("{at}.{k} isn't a field"));
                    }
                }
                for (k, s) in fields {
                    let field_at = format!("{at}.{k}");
                    match o.get(k) {
                        Some(x) => s.check_at(x, &field_at)?,
                        None if matches!(s, Shape::Optional(_)) => {}
                        None => return Err(format!("{field_at} is missing")),
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn paths() -> Shape {
        Shape::list(Shape::record([
            (
                "anchors",
                Shape::list(Shape::record([
                    ("point", Shape::Vec2),
                    ("handle_in", Shape::optional(Shape::Vec2)),
                    ("handle_out", Shape::optional(Shape::Vec2)),
                ])),
            ),
            ("closed", Shape::Bool),
        ]))
    }

    #[test]
    fn a_vector_path_fits() {
        let v = json!([
            {"anchors": [{"point": [0, 0], "handle_out": [5, 0]}, {"point": [10, 0], "handle_in": null}], "closed": false},
            {"anchors": [], "closed": true}
        ]);
        assert_eq!(paths().check(&v), Ok(()));
        assert_eq!(paths().check(&paths().empty()), Ok(()));
    }

    #[test]
    fn a_misfit_says_where() {
        let check = |v| paths().check(&v).unwrap_err();
        assert_eq!(
            check(json!([{"anchors": [{"point": [0]}], "closed": true}])),
            "[0].anchors[0].point should be two numbers"
        );
        assert_eq!(check(json!([{"anchors": []}])), "[0].closed is missing");
        assert_eq!(
            check(json!([{"anchors": [], "closed": true, "smooth": 1}])),
            "[0].smooth isn't a field"
        );
        assert_eq!(check(json!({})), "the value should be a list");
        assert!(Shape::Colour.check(&json!([1, 1, 1, 2])).is_err());
        assert!(Shape::choice(&["a"]).check(&json!("b")).is_err());
    }
}
