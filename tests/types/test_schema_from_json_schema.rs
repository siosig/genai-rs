//! Ported from upstream `tests/types/test_schema_from_json_schema.py` (and
//! `test_convert_json_schema_with_cycle` of `test_types.py`):
//! [`Schema::from_json_schema`].
//!
//! Upstream builds `types.JSONSchema` objects and also calls with
//! `api_option='VERTEX_AI'`; this crate takes the JSON Schema as a
//! `serde_json::Value` and only targets the Gemini Developer API, so each test
//! runs the Developer API conversion once. Where upstream builds the same
//! `JSONSchema` twice (enum member vs. plain string) the JSON form is identical.

use std::collections::HashMap;

use gemini_genai::types::{Schema, Type};
use serde_json::json;

fn typed(schema_type: Type) -> Schema {
    Schema {
        r#type: Some(schema_type),
        ..Schema::default()
    }
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "builds the Option-typed Schema field directly"
)]
fn properties<const N: usize>(entries: [(&str, Schema); N]) -> Option<HashMap<String, Schema>> {
    Some(
        entries
            .into_iter()
            .map(|(key, schema)| (key.to_owned(), schema))
            .collect(),
    )
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "builds the Option-typed Schema field directly"
)]
fn strings(items: &[&str]) -> Option<Vec<String>> {
    Some(items.iter().map(|item| (*item).to_owned()).collect())
}

// upstream-test: types/test_schema_from_json_schema.py::test_empty_json_schema_conversion
#[test]
fn test_empty_json_schema_conversion() {
    let schema = Schema::from_json_schema(&json!({})).expect("converts");

    assert_eq!(schema, Schema::default());
}

// upstream-test: types/test_schema_from_json_schema.py::test_not_null_type_conversion
#[test]
fn test_not_null_type_conversion() {
    let cases = [
        ("string", Type::String),
        ("number", Type::Number),
        ("integer", Type::Integer),
        ("boolean", Type::Boolean),
        ("array", Type::Array),
        ("object", Type::Object),
    ];
    for (json_schema_type, expected) in cases {
        let schema =
            Schema::from_json_schema(&json!({ "type": json_schema_type })).expect("converts");

        assert_eq!(schema, typed(expected), "type {json_schema_type}");
    }
}

// upstream-test: types/test_schema_from_json_schema.py::test_nullable_conversion
#[test]
fn test_nullable_conversion() {
    let schema =
        Schema::from_json_schema(&json!({ "type": ["string", "null"] })).expect("converts");

    assert_eq!(
        schema,
        Schema {
            nullable: Some(true),
            ..typed(Type::String)
        }
    );
}

// upstream-test: types/test_schema_from_json_schema.py::test_nullable_in_union_like_type_conversion
#[test]
fn test_nullable_in_union_like_type_conversion() {
    let schema = Schema::from_json_schema(&json!({
        "type": ["string", "null", "object", "number", "array", "boolean", "integer"],
    }))
    .expect("converts");

    let expected = Schema {
        nullable: Some(true),
        any_of: Some(vec![
            typed(Type::String),
            typed(Type::Object),
            typed(Type::Number),
            typed(Type::Array),
            typed(Type::Boolean),
            typed(Type::Integer),
        ]),
        ..Schema::default()
    };
    assert_eq!(schema, expected);
}

// upstream-test: types/test_schema_from_json_schema.py::test_union_like_type_conversion_suite1
#[test]
fn test_union_like_type_conversion_suite1() {
    let schema = Schema::from_json_schema(&json!({
        "type": ["string", "object", "null"],
        "description": "description",
        "default": "default",
        "max_length": 10,
        "min_length": 5,
        "enum": ["value1", "value2"],
        "format": "format",
        "pattern": "pattern",
        "title": "title",
        "min_properties": 1,
        "max_properties": 2,
        "required": ["field1", "field2"],
        "properties": {
            "field1": {"type": "string"},
            "field2": {"type": "integer"},
        },
    }))
    .expect("converts");

    let expected = Schema {
        nullable: Some(true),
        any_of: Some(vec![
            Schema {
                description: Some("description".to_owned()),
                max_length: Some(10),
                min_length: Some(5),
                r#enum: strings(&["value1", "value2"]),
                format: Some("format".to_owned()),
                pattern: Some("pattern".to_owned()),
                title: Some("title".to_owned()),
                ..typed(Type::String)
            },
            Schema {
                properties: properties([
                    ("field1", typed(Type::String)),
                    ("field2", typed(Type::Integer)),
                ]),
                required: strings(&["field1", "field2"]),
                min_properties: Some(1),
                max_properties: Some(2),
                title: Some("title".to_owned()),
                description: Some("description".to_owned()),
                ..typed(Type::Object)
            },
        ]),
        ..Schema::default()
    };
    assert_eq!(schema, expected);
}

// upstream-test: types/test_schema_from_json_schema.py::test_union_like_type_conversion_suite2
#[test]
fn test_union_like_type_conversion_suite2() {
    let schema = Schema::from_json_schema(&json!({
        "type": ["integer", "array"],
        "description": "description",
        "items": {"type": "integer", "maximum": 2, "minimum": 1},
        "min_items": 1,
        "max_items": 2,
        "title": "title",
        "enum": ["1", "2"],
        "maximum": 2,
        "minimum": 1,
    }))
    .expect("converts");

    let expected = Schema {
        any_of: Some(vec![
            Schema {
                description: Some("description".to_owned()),
                maximum: Some(2.0),
                minimum: Some(1.0),
                r#enum: strings(&["1", "2"]),
                title: Some("title".to_owned()),
                ..typed(Type::Integer)
            },
            Schema {
                items: Some(Box::new(Schema {
                    maximum: Some(2.0),
                    minimum: Some(1.0),
                    ..typed(Type::Integer)
                })),
                min_items: Some(1),
                max_items: Some(2),
                title: Some("title".to_owned()),
                description: Some("description".to_owned()),
                ..typed(Type::Array)
            },
        ]),
        ..Schema::default()
    };
    assert_eq!(schema, expected);
}

// upstream-test: types/test_schema_from_json_schema.py::test_array_type_conversion
#[test]
fn test_array_type_conversion() {
    let schema = Schema::from_json_schema(&json!({
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "field1": {"type": "string"},
                "field2": {"type": "integer"},
            },
            "required": ["field1", "field2"],
            "min_properties": 1,
            "max_properties": 2,
            "title": "title",
            "description": "description",
        },
    }))
    .expect("converts");

    let expected = Schema {
        items: Some(Box::new(Schema {
            properties: properties([
                ("field1", typed(Type::String)),
                ("field2", typed(Type::Integer)),
            ]),
            required: strings(&["field1", "field2"]),
            min_properties: Some(1),
            max_properties: Some(2),
            title: Some("title".to_owned()),
            description: Some("description".to_owned()),
            ..typed(Type::Object)
        })),
        ..typed(Type::Array)
    };
    assert_eq!(schema, expected);
}

// upstream-test: types/test_schema_from_json_schema.py::test_complex_object_type_conversion
#[test]
fn test_complex_object_type_conversion() {
    let schema = Schema::from_json_schema(&json!({
        "type": "object",
        "properties": {
            "field1": {
                "type": ["string", "array", "null"],
                "description": "description1",
                "max_length": 20,
                "min_length": 15,
                "enum": ["value1", "value2"],
                "format": "format",
                "pattern": "pattern",
                "title": "title1",
                "items": {"type": "integer", "maximum": 2, "minimum": 1},
                "min_items": 1,
                "max_items": 2,
            },
            "field2": {"type": "integer"},
        },
        "required": ["field1", "field2"],
        "min_properties": 1,
        "max_properties": 2,
        "title": "title",
        "description": "description",
    }))
    .expect("converts");

    let field1 = Schema {
        nullable: Some(true),
        any_of: Some(vec![
            Schema {
                description: Some("description1".to_owned()),
                max_length: Some(20),
                min_length: Some(15),
                r#enum: strings(&["value1", "value2"]),
                format: Some("format".to_owned()),
                pattern: Some("pattern".to_owned()),
                title: Some("title1".to_owned()),
                ..typed(Type::String)
            },
            Schema {
                items: Some(Box::new(Schema {
                    maximum: Some(2.0),
                    minimum: Some(1.0),
                    ..typed(Type::Integer)
                })),
                min_items: Some(1),
                max_items: Some(2),
                title: Some("title1".to_owned()),
                description: Some("description1".to_owned()),
                ..typed(Type::Array)
            },
        ]),
        ..Schema::default()
    };
    let expected = Schema {
        properties: properties([("field1", field1), ("field2", typed(Type::Integer))]),
        required: strings(&["field1", "field2"]),
        min_properties: Some(1),
        max_properties: Some(2),
        title: Some("title".to_owned()),
        description: Some("description".to_owned()),
        ..typed(Type::Object)
    };
    assert_eq!(schema, expected);
}

// upstream-test: types/test_types.py::test_convert_json_schema_with_cycle
#[test]
fn test_convert_json_schema_with_cycle() {
    let schema = Schema::from_json_schema(&json!({
        "type": "object",
        "properties": {"foo": {"$ref": "#/$defs/Foo"}},
        "$defs": {
            "Foo": {
                "type": "object",
                "properties": {"foo": {"$ref": "#/$defs/Foo"}},
            },
        },
    }))
    .expect("converts");

    assert_eq!(schema.r#type, Some(Type::Object));
    let properties = schema.properties.expect("properties");
    let foo = &properties["foo"];
    assert_eq!(foo.r#type, Some(Type::Object));
    assert_eq!(
        foo.properties.as_ref().expect("nested properties")["foo"],
        Schema::default()
    );
}
