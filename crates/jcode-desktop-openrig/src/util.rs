//! Lenient serde helpers. The daemon's read models grow by addition, so a field of an
//! unexpected shape reads as absent instead of failing the whole view.
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::Value;
use std::collections::BTreeMap;

/// A served scalar (string, number or bool), shown the way the TUI's `String(v)` shows it.
/// Null, empty and structured values read as absent.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Text(Option<String>);

impl<'de> Deserialize<'de> for Text {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(match Value::deserialize(d)? {
            Value::String(s) if !s.is_empty() => Some(s),
            Value::Number(n) => Some(match n.as_f64() {
                Some(f) if n.is_f64() => js_number(f),
                _ => n.to_string(),
            }),
            Value::Bool(b) => Some(b.to_string()),
            _ => None,
        }))
    }
}
impl Text {
    pub fn get(&self) -> Option<&str> {
        self.0.as_deref()
    }
    pub fn or(&self, fallback: &str) -> String {
        self.0.clone().unwrap_or_else(|| fallback.to_owned())
    }
    /// The TUI's `str(v)`: the value, else "unknown".
    pub fn unknown(&self) -> String {
        self.or("unknown")
    }
    pub fn is(&self, expected: &str) -> bool {
        self.get() == Some(expected)
    }
    pub fn is_set(&self) -> bool {
        self.0.is_some()
    }
    pub fn into_option(self) -> Option<String> {
        self.0
    }
}

/// JavaScript's `String(number)` for the values the daemon serves.
pub fn js_number(f: f64) -> String {
    if f.is_finite() && f == f.trunc() && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

/// An optional member that reads as absent when missing, null or of another shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Lax<T>(pub Option<T>);
impl<T> Default for Lax<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<'de, T: DeserializeOwned> Deserialize<'de> for Lax<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(
            Value::deserialize(d)
                .ok()
                .and_then(|v| serde_json::from_value(v).ok()),
        ))
    }
}
impl<T> Lax<T> {
    pub fn get(&self) -> Option<&T> {
        self.0.as_ref()
    }
}
impl Lax<f64> {
    pub fn num(&self) -> Option<f64> {
        self.0.filter(|n| n.is_finite())
    }
}
impl Lax<bool> {
    pub fn truthy(&self) -> bool {
        self.0 == Some(true)
    }
}

/// A list that drops members that do not parse. `is_array` tells a served empty list from a missing one.
#[derive(Clone, Debug, PartialEq)]
pub struct Items<T>(pub Option<Vec<T>>);
impl<T> Default for Items<T> {
    fn default() -> Self {
        Self(None)
    }
}
impl<'de, T: DeserializeOwned> Deserialize<'de> for Items<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(match Value::deserialize(d)? {
            Value::Array(items) => Some(
                items
                    .into_iter()
                    .filter_map(|v| serde_json::from_value(v).ok())
                    .collect(),
            ),
            _ => None,
        }))
    }
}
impl<T> Items<T> {
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.as_deref().unwrap_or(&[]).iter()
    }
    pub fn as_slice(&self) -> &[T] {
        self.0.as_deref().unwrap_or(&[])
    }
    pub fn is_array(&self) -> bool {
        self.0.is_some()
    }
    pub fn len(&self) -> usize {
        self.as_slice().len()
    }
    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
}
impl<'a, T> IntoIterator for &'a Items<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// A JSON object whose values are parsed leniently, in key order.
#[derive(Clone, Debug, PartialEq)]
pub struct Table<T>(pub BTreeMap<String, T>);
impl<T> Default for Table<T> {
    fn default() -> Self {
        Self(BTreeMap::new())
    }
}
impl<'de, T: DeserializeOwned> Deserialize<'de> for Table<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Self(match Value::deserialize(d)? {
            Value::Object(map) => map
                .into_iter()
                .filter_map(|(k, v)| Some((k, serde_json::from_value(v).ok()?)))
                .collect(),
            _ => BTreeMap::new(),
        }))
    }
}

pub fn join(parts: impl IntoIterator<Item = String>, sep: &str) -> String {
    parts.into_iter().collect::<Vec<_>>().join(sep)
}

/// Clip `value` at `max` characters with an ellipsis. For one-line row labels.
pub fn clip(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        value.to_owned()
    } else {
        format!(
            "{}…",
            value
                .chars()
                .take(max.saturating_sub(1))
                .collect::<String>()
        )
    }
}

/// An RFC 3339 instant, or nothing when the value is absent or malformed.
pub(crate) fn timestamp(value: Option<&str>) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(value?)
        .ok()
        .map(Into::into)
}
