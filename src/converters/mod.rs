//! Shared helpers used by the generated `mldev` request/response converters
//! (`generated/`): `getv`/`setv` path accessors and small utilities.
//! Faithfully ports Python's `_common.get_value_by_path` /
//! `_common.set_value_by_path`, including the `foo[]` (array map) and
//! `foo[0]` (first element) path-segment conventions.

pub mod generated;

use serde_json::{Map, Value};

use crate::error::{Error, Result};

fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_none_or(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// Reads a value out of `data` by a path of keys, where a key ending in
/// `[]` maps over an array (returning a new array of per-element results)
/// and a key ending in `[0]` drills into the array's first element. A
/// single `["_self"]` path returns `data` itself. Missing/falsy
/// intermediates return `None`, matching Python's `get_value_by_path`.
pub(crate) fn getv(data: Option<&Value>, keys: &[&str]) -> Option<Value> {
    if keys == ["_self"] {
        return data.cloned();
    }
    let mut current = data.cloned();
    for (i, key) in keys.iter().enumerate() {
        if !is_truthy(current.as_ref()) {
            return None;
        }
        // Safe: `is_truthy` returned true, so `current` is `Some`.
        let cur = current.take().unwrap_or(Value::Null);
        if let Some(key_name) = key.strip_suffix("[]") {
            let arr = cur.get(key_name)?.as_array()?;
            let rest = &keys[i + 1..];
            let results = arr
                .iter()
                .map(|item| getv(Some(item), rest).unwrap_or(Value::Null))
                .collect();
            return Some(Value::Array(results));
        }
        if let Some(key_name) = key.strip_suffix("[0]") {
            let first = cur.get(key_name)?.as_array()?.first()?;
            return getv(Some(first), &keys[i + 1..]);
        }
        current = cur.get(*key).cloned();
        current.as_ref()?;
    }
    current
}

fn ensure_object(data: &mut Value) {
    if !data.is_object() {
        *data = Value::Object(Map::new());
    }
}

#[expect(
    clippy::expect_used,
    reason = "documented invariant: ensure_object() is called immediately before each expect(), so the value is always an Object at that point"
)]
fn setv_leaf(data: &mut Value, key: &str, value: Value) -> Result<()> {
    ensure_object(data);
    let obj = data
        .as_object_mut()
        .expect("ensure_object just made this an Object");

    if key == "_self" {
        if let Value::Object(new_fields) = value {
            obj.extend(new_fields);
            return Ok(());
        }
        return Err(Error::Validation(
            "setv: `_self` leaf assignment requires an object value".to_owned(),
        ));
    }

    match obj.get(key) {
        Some(existing) if !existing.is_null() => {
            if matches!(&value, Value::Null) || !is_truthy(Some(&value)) {
                // Don't overwrite an existing non-empty value with an empty one.
                Ok(())
            } else if existing == &value {
                Ok(())
            } else if existing.is_object() && value.is_object() {
                let Value::Object(new_fields) = value else {
                    unreachable!()
                };
                obj.get_mut(key)
                    .and_then(Value::as_object_mut)
                    .expect("checked is_object above")
                    .extend(new_fields);
                Ok(())
            } else {
                Err(Error::Validation(format!(
                    "setv: cannot overwrite existing key `{key}` (existing: {existing}, new: {value})"
                )))
            }
        }
        _ => {
            obj.insert(key.to_owned(), value);
            Ok(())
        }
    }
}

/// Writes `value` into `data` at the given path, creating intermediate
/// objects/arrays as needed. A `None` `value` is a no-op (matches Python's
/// `if value is None: return`), as is a `None` `data` (matches setting
/// into an absent `parent_object`). See [`getv`] for the `[]`/`[0]`
/// path-segment conventions; when an existing non-empty leaf value is
/// present, objects are merged rather than overwritten (used for
/// tuning-dataset fields).
///
/// # Errors
/// Returns [`Error::Validation`] if an `[]` path segment is given a
/// non-array value, or if an existing non-object leaf would be silently
/// overwritten by an incompatible value.
#[expect(
    clippy::expect_used,
    reason = "documented invariant: ensure_object()/insert() are called immediately before each expect(), so the value is always present at that point"
)]
pub(crate) fn setv(data: Option<&mut Value>, keys: &[&str], value: Value) -> Result<()> {
    if value.is_null() {
        return Ok(());
    }
    let Some(data) = data else { return Ok(()) };
    let Some((first, rest)) = keys.split_first() else {
        return Ok(());
    };
    if rest.is_empty() {
        return setv_leaf(data, first, value);
    }

    if let Some(key_name) = first.strip_suffix("[]") {
        ensure_object(data);
        let obj = data.as_object_mut().expect("ensured object");
        if !obj.contains_key(key_name) {
            let Value::Array(items) = &value else {
                return Err(Error::Validation(format!(
                    "setv: value must be a list given array path `{first}`"
                )));
            };
            obj.insert(
                key_name.to_owned(),
                Value::Array(vec![Value::Object(Map::new()); items.len()]),
            );
        }
        let array = obj
            .get_mut(key_name)
            .and_then(Value::as_array_mut)
            .expect("inserted or pre-existing array");
        match value {
            Value::Array(values) => {
                for (item, item_value) in array.iter_mut().zip(values) {
                    setv(Some(item), rest, item_value)?;
                }
            }
            other => {
                for item in array.iter_mut() {
                    setv(Some(item), rest, other.clone())?;
                }
            }
        }
        return Ok(());
    }

    if let Some(key_name) = first.strip_suffix("[0]") {
        ensure_object(data);
        let obj = data.as_object_mut().expect("ensured object");
        if !obj.contains_key(key_name) {
            obj.insert(
                key_name.to_owned(),
                Value::Array(vec![Value::Object(Map::new())]),
            );
        }
        let first_item = obj
            .get_mut(key_name)
            .and_then(Value::as_array_mut)
            .and_then(|a| a.first_mut())
            .expect("inserted or pre-existing first element");
        return setv(Some(first_item), rest, value);
    }

    ensure_object(data);
    let obj = data.as_object_mut().expect("ensured object");
    let entry = obj
        .entry((*first).to_owned())
        .or_insert_with(|| Value::Object(Map::new()));
    setv(Some(entry), rest, value)
}

/// Narrows a generated `_to_mldev` converter's `Value` return into a `&mut
/// Map`. Every such converter unconditionally returns `Value::Object(...)`
/// (see `tools/codegen/gen_converters.py`'s `to_object` template) -- a
/// true crate-internal invariant, not a caller mistake -- so this is the
/// single, documented place that invariant is asserted, rather than
/// repeating `.as_object_mut().expect(...)` at every call site.
#[expect(
    clippy::expect_used,
    reason = "documented invariant: every generated `_to_mldev` converter returns Value::Object"
)]
pub(crate) fn as_object_mut(value: &mut Value) -> &mut Map<String, Value> {
    value
        .as_object_mut()
        .expect("converters always return an Object")
}

/// Builds the [`Error::UnsupportedByBackend`] a generated converter raises
/// when a Vertex-AI-only field is set on a Gemini Developer API client.
pub(crate) fn vertex_only_error(field: &'static str) -> Error {
    Error::UnsupportedByBackend {
        field,
        backend: crate::error::Backend::VertexAi,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::{getv, setv};

    #[test]
    fn getv_reads_a_nested_path() {
        let data = json!({"a": {"b": "v"}});
        assert_eq!(getv(Some(&data), &["a", "b"]), Some(json!("v")));
    }

    #[test]
    fn getv_maps_over_an_array_path_segment() {
        let data = json!({"a": {"b": [{"c": "v1"}, {"c": "v2"}]}});
        assert_eq!(
            getv(Some(&data), &["a", "b[]", "c"]),
            Some(json!(["v1", "v2"]))
        );
    }

    #[test]
    fn getv_drills_into_first_element_path_segment() {
        let data = json!({"a": [{"b": "v"}]});
        assert_eq!(getv(Some(&data), &["a[0]", "b"]), Some(json!("v")));
    }

    #[test]
    fn getv_self_returns_the_whole_value() {
        let data = json!({"a": 1});
        assert_eq!(getv(Some(&data), &["_self"]), Some(data));
    }

    #[test]
    fn getv_missing_path_returns_none() {
        let data = json!({"a": {}});
        assert_eq!(getv(Some(&data), &["a", "b"]), None);
    }

    #[test]
    fn setv_creates_nested_objects() {
        let mut data = Value::Object(Map::new());
        setv(Some(&mut data), &["a", "b"], json!("v")).unwrap();
        assert_eq!(data, json!({"a": {"b": "v"}}));
    }

    #[test]
    fn setv_none_value_is_a_noop() {
        let mut data = json!({"a": 1});
        setv(Some(&mut data), &["b"], Value::Null).unwrap();
        assert_eq!(data, json!({"a": 1}));
    }

    #[test]
    fn setv_none_target_is_a_noop() {
        // Must not panic: mirrors Python's `set_value_by_path(None, ...)`.
        setv(None, &["a"], json!("v")).unwrap();
    }

    #[test]
    fn setv_array_path_distributes_list_values() {
        let mut data = Value::Object(Map::new());
        setv(Some(&mut data), &["a", "b[]", "c"], json!(["v1", "v2"])).unwrap();
        assert_eq!(data, json!({"a": {"b": [{"c": "v1"}, {"c": "v2"}]}}));
    }

    #[test]
    fn setv_array_path_broadcasts_a_scalar_to_existing_items() {
        let mut data = json!({"a": {"b": [{"c": "v1"}, {}]}});
        setv(Some(&mut data), &["a", "b[]", "d"], json!("shared")).unwrap();
        assert_eq!(
            data,
            json!({"a": {"b": [{"c": "v1", "d": "shared"}, {"d": "shared"}]}})
        );
    }

    #[test]
    fn setv_array_path_errors_when_value_is_not_a_list_and_key_is_new() {
        let mut data = Value::Object(Map::new());
        let err = setv(Some(&mut data), &["a", "b[]", "c"], json!("not-a-list")).unwrap_err();
        assert!(err.to_string().contains("must be a list"));
    }

    #[test]
    fn setv_merges_dict_into_existing_dict_leaf() {
        let mut data = json!({"a": {"x": 1}});
        setv(Some(&mut data), &["a"], json!({"y": 2})).unwrap();
        assert_eq!(data, json!({"a": {"x": 1, "y": 2}}));
    }

    #[test]
    fn setv_ignores_new_empty_value_over_existing_nonempty() {
        let mut data = json!({"a": "kept"});
        setv(Some(&mut data), &["a"], json!("")).unwrap();
        assert_eq!(data, json!({"a": "kept"}));
    }

    #[test]
    fn setv_self_leaf_merges_into_the_current_object() {
        let mut data = json!({"existing": true});
        setv(Some(&mut data), &["_self"], json!({"added": 1})).unwrap();
        assert_eq!(data, json!({"existing": true, "added": 1}));
    }

    #[test]
    fn setv_errors_on_incompatible_overwrite() {
        let mut data = json!({"a": "existing"});
        let err = setv(Some(&mut data), &["a"], json!("different")).unwrap_err();
        assert!(err.to_string().contains("cannot overwrite"));
    }
}
