use std::fmt;
use std::marker::PhantomData;

use serde_json::Value as Json;

/// A stored value. A `Choice` holds the chosen value itself, never an index.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i64),
    Float(f64),
    Text(String),
    Choice(String),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    /// Red, green, blue, alpha, each 0 to 1.
    Colour([f64; 4]),
    /// A fixed number of floats, such as a 4×4 matrix.
    Floats(Vec<f64>),
    /// Plain JSON of a declared [`Shape`](crate::Shape) (decision 51).
    Shaped(Json),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueKind {
    Bool,
    Int,
    Float,
    Text,
    Choice,
    Vec2,
    Vec3,
    Colour,
    Floats,
    Shaped,
}

impl Value {
    pub fn kind(&self) -> ValueKind {
        match self {
            Value::Bool(_) => ValueKind::Bool,
            Value::Int(_) => ValueKind::Int,
            Value::Float(_) => ValueKind::Float,
            Value::Text(_) => ValueKind::Text,
            Value::Choice(_) => ValueKind::Choice,
            Value::Vec2(_) => ValueKind::Vec2,
            Value::Vec3(_) => ValueKind::Vec3,
            Value::Colour(_) => ValueKind::Colour,
            Value::Floats(_) => ValueKind::Floats,
            Value::Shaped(_) => ValueKind::Shaped,
        }
    }

    pub(crate) fn floats(&self) -> &[f64] {
        match self {
            Value::Float(f) => std::slice::from_ref(f),
            Value::Vec2(v) => v,
            Value::Vec3(v) => v,
            Value::Colour(v) => v,
            Value::Floats(v) => v,
            _ => &[],
        }
    }

    /// The value as plain JSON, as the file format writes it.
    pub fn to_json(&self) -> Json {
        let num = |f: f64| Json::Number(serde_json::Number::from_f64(f).expect("finite"));
        let nums = |v: &[f64]| Json::Array(v.iter().copied().map(num).collect());
        match self {
            Value::Bool(b) => Json::Bool(*b),
            Value::Int(i) => Json::from(*i),
            Value::Float(f) => num(*f),
            Value::Text(s) | Value::Choice(s) => Json::String(s.clone()),
            Value::Vec2(v) => nums(v),
            Value::Vec3(v) => nums(v),
            Value::Colour(v) => nums(v),
            Value::Floats(v) => nums(v),
            Value::Shaped(j) => j.clone(),
        }
    }

    /// Reads JSON as a value of `kind`. `None` when it doesn't fit.
    pub fn from_json(kind: ValueKind, j: &Json) -> Option<Value> {
        fn arr<const N: usize>(j: &Json) -> Option<[f64; N]> {
            let a = j.as_array()?;
            if a.len() != N {
                return None;
            }
            let mut out = [0.0; N];
            for (o, v) in out.iter_mut().zip(a) {
                *o = v.as_f64()?;
            }
            Some(out)
        }
        Some(match kind {
            ValueKind::Bool => Value::Bool(j.as_bool()?),
            ValueKind::Int => Value::Int(j.as_i64()?),
            ValueKind::Float => Value::Float(j.as_f64()?),
            ValueKind::Text => Value::Text(j.as_str()?.to_string()),
            ValueKind::Choice => Value::Choice(j.as_str()?.to_string()),
            ValueKind::Vec2 => Value::Vec2(arr(j)?),
            ValueKind::Vec3 => Value::Vec3(arr(j)?),
            ValueKind::Colour => Value::Colour(arr(j)?),
            ValueKind::Floats => Value::Floats(
                j.as_array()?
                    .iter()
                    .map(|x| x.as_f64())
                    .collect::<Option<Vec<f64>>>()?,
            ),
            ValueKind::Shaped => Value::Shaped(j.clone()),
        })
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let list = |f: &mut fmt::Formatter<'_>, v: &[f64]| {
            let parts: Vec<String> = v.iter().map(|x| format!("{x:?}")).collect();
            write!(f, "({})", parts.join(", "))
        };
        match self {
            Value::Bool(b) => write!(f, "{b}"),
            Value::Int(i) => write!(f, "{i}"),
            Value::Float(x) => write!(f, "{x:?}"),
            Value::Text(s) => write!(f, "{s:?}"),
            Value::Choice(s) => write!(f, "{s}"),
            Value::Vec2(v) => list(f, v),
            Value::Vec3(v) => list(f, v),
            Value::Colour(v) => {
                f.write_str("rgba")?;
                list(f, v)
            }
            Value::Floats(v) => list(f, v),
            Value::Shaped(j) => write!(f, "{j}"),
        }
    }
}

/// The typed value of a `Choice` key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice(pub String);

/// The typed value of a `Colour` key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Colour(pub [f64; 4]);

/// A Rust type that a value key can hold.
pub trait ValueType: Sized {
    const KIND: ValueKind;
    fn into_value(self) -> Value;
    fn from_value(v: &Value) -> Option<Self>;
}

macro_rules! value_type {
    ($t:ty, $kind:ident, $v:ident => $into:expr, $pat:pat => $from:expr) => {
        impl ValueType for $t {
            const KIND: ValueKind = ValueKind::$kind;
            fn into_value(self) -> Value {
                let $v = self;
                $into
            }
            fn from_value(v: &Value) -> Option<Self> {
                match v {
                    $pat => Some($from),
                    _ => None,
                }
            }
        }
    };
}

value_type!(bool, Bool, v => Value::Bool(v), Value::Bool(b) => *b);
value_type!(i64, Int, v => Value::Int(v), Value::Int(i) => *i);
value_type!(f64, Float, v => Value::Float(v), Value::Float(x) => *x);
value_type!(String, Text, v => Value::Text(v), Value::Text(s) => s.clone());
value_type!(Choice, Choice, v => Value::Choice(v.0), Value::Choice(s) => Choice(s.clone()));
value_type!([f64; 2], Vec2, v => Value::Vec2(v), Value::Vec2(a) => *a);
value_type!([f64; 3], Vec3, v => Value::Vec3(v), Value::Vec3(a) => *a);
value_type!(Colour, Colour, v => Value::Colour(v.0), Value::Colour(a) => Colour(*a));
value_type!(Vec<f64>, Floats, v => Value::Floats(v), Value::Floats(a) => a.clone());
value_type!(Json, Shaped, v => Value::Shaped(v), Value::Shaped(j) => j.clone());

/// A value key with its type: `const RADIUS: Key<f64> = Key::new("blur.radius");`.
///
/// Object-model code uses these and gets a compile error for a wrong type. Generic code
/// (UI, CLI, IPC) uses the string and a [`Value`], and gets a runtime error instead.
pub struct Key<T> {
    name: &'static str,
    _t: PhantomData<fn() -> T>,
}

impl<T> Key<T> {
    pub const fn new(name: &'static str) -> Self {
        Key {
            name,
            _t: PhantomData,
        }
    }

    pub const fn name(&self) -> &'static str {
        self.name
    }
}

impl<T> Clone for Key<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Key<T> {}

impl<T> fmt::Debug for Key<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Key({})", self.name)
    }
}

/// Anything that names a value key: a `&str` or a typed [`Key`].
pub trait KeyName {
    fn key_name(&self) -> &str;
}

impl KeyName for &str {
    fn key_name(&self) -> &str {
        self
    }
}

impl KeyName for String {
    fn key_name(&self) -> &str {
        self
    }
}

impl<T> KeyName for Key<T> {
    fn key_name(&self) -> &str {
        self.name
    }
}

/// A value serialises as [`Value::to_json`]: plain JSON, read back by its schema's kind.
impl serde::Serialize for Value {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.to_json().serialize(s)
    }
}
