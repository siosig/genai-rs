//! Ported from upstream `tests/types/test_schema_json_schema.py`:
//! [`Schema::json_schema`].
//!
//! Upstream returns a `types.JSONSchema` and asserts on its non-None fields;
//! this crate returns the JSON Schema as a `serde_json::Value` in JSON Schema
//! keyword spelling (`anyOf`, `minItems`, ...), so each test compares the
//! whole value (which also pins the set of fields that are present).

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

// upstream-test: types/test_schema_json_schema.py::test_empty_schema_conversion
#[test]
fn test_empty_schema_conversion() {
    assert_eq!(Schema::default().json_schema(), json!({}));
}

// upstream-test: types/test_schema_json_schema.py::test_not_null_type_conversion
#[test]
fn test_not_null_type_conversion() {
    let cases = [
        (Type::Object, "object"),
        (Type::Array, "array"),
        (Type::String, "string"),
        (Type::Number, "number"),
        (Type::Boolean, "boolean"),
        (Type::Integer, "integer"),
    ];
    for (schema_type, expected) in cases {
        assert_eq!(
            typed(schema_type).json_schema(),
            json!({ "type": expected }),
            "type {expected}"
        );
    }
}

// upstream-test: types/test_schema_json_schema.py::test_unspecified_type_conversion
#[test]
fn test_unspecified_type_conversion() {
    assert_eq!(typed(Type::TypeUnspecified).json_schema(), json!({}));
}

// upstream-test: types/test_schema_json_schema.py::test_nullable_conversion
#[test]
fn test_nullable_conversion() {
    let schema = Schema {
        nullable: Some(true),
        ..typed(Type::String)
    };

    let json_schema = schema.json_schema();

    // Upstream compares as sets; the order (`null` first) follows its
    // field-declaration order.
    let types = json_schema["type"].as_array().expect("type is a list");
    assert_eq!(types.len(), 2);
    assert!(types.contains(&json!("null")) && types.contains(&json!("string")));
    assert_eq!(json_schema.as_object().expect("object").len(), 1);
}

// upstream-test: types/test_schema_json_schema.py::test_property_conversion
#[test]
fn test_property_conversion() {
    let schema = Schema {
        properties: properties([("key1", typed(Type::String)), ("key2", typed(Type::Number))]),
        ..typed(Type::Object)
    };

    assert_eq!(
        schema.json_schema(),
        json!({
            "type": "object",
            "properties": {"key1": {"type": "string"}, "key2": {"type": "number"}},
        })
    );
}

// upstream-test: types/test_schema_json_schema.py::test_complex_property_conversion
#[test]
fn test_complex_property_conversion() {
    let schema = Schema {
        properties: properties([
            (
                "key1",
                Schema {
                    properties: properties([
                        ("key2", typed(Type::String)),
                        ("key3", typed(Type::Number)),
                    ]),
                    ..typed(Type::Object)
                },
            ),
            (
                "key2",
                Schema {
                    items: Some(Box::new(typed(Type::String))),
                    ..typed(Type::Array)
                },
            ),
        ]),
        ..typed(Type::Object)
    };

    assert_eq!(
        schema.json_schema(),
        json!({
            "type": "object",
            "properties": {
                "key1": {
                    "type": "object",
                    "properties": {"key2": {"type": "string"}, "key3": {"type": "number"}},
                },
                "key2": {"type": "array", "items": {"type": "string"}},
            },
        })
    );
}

// upstream-test: types/test_schema_json_schema.py::test_items_conversion
#[test]
fn test_items_conversion() {
    let schema = Schema {
        items: Some(Box::new(typed(Type::String))),
        ..typed(Type::Array)
    };

    assert_eq!(
        schema.json_schema(),
        json!({"type": "array", "items": {"type": "string"}})
    );
}

// upstream-test: types/test_schema_json_schema.py::test_complex_items_conversion
#[test]
fn test_complex_items_conversion() {
    let schema = Schema {
        items: Some(Box::new(Schema {
            properties: properties([("key1", typed(Type::String)), ("key2", typed(Type::Number))]),
            ..typed(Type::Object)
        })),
        ..typed(Type::Array)
    };

    assert_eq!(
        schema.json_schema(),
        json!({
            "type": "array",
            "items": {
                "type": "object",
                "properties": {"key1": {"type": "string"}, "key2": {"type": "number"}},
            },
        })
    );
}

// upstream-test: types/test_schema_json_schema.py::test_any_of_conversion
#[test]
fn test_any_of_conversion() {
    let schema = Schema {
        any_of: Some(vec![typed(Type::String), typed(Type::Number)]),
        ..typed(Type::Object)
    };

    assert_eq!(
        schema.json_schema(),
        json!({"type": "object", "anyOf": [{"type": "string"}, {"type": "number"}]})
    );
}

/// The first `test_complex_any_of_conversion` of upstream's file. Python
/// redefines the name further down, so pytest never runs this body; it is
/// kept (under another name, without a marker) because it is still a valid
/// check of the conversion.
#[test]
fn test_complex_any_of_conversion_shadowed_upstream() {
    let schema = Schema {
        any_of: Some(vec![
            Schema {
                properties: properties([
                    ("key1", typed(Type::String)),
                    ("key2", typed(Type::Number)),
                ]),
                ..typed(Type::Object)
            },
            Schema {
                items: Some(Box::new(typed(Type::String))),
                ..typed(Type::Array)
            },
        ]),
        ..typed(Type::Object)
    };

    assert_eq!(
        schema.json_schema(),
        json!({
            "type": "object",
            "anyOf": [
                {
                    "type": "object",
                    "properties": {"key1": {"type": "string"}, "key2": {"type": "number"}},
                },
                {"type": "array", "items": {"type": "string"}},
            ],
        })
    );
}

// upstream-test: types/test_schema_json_schema.py::test_example_conversion
#[test]
fn test_example_conversion() {
    let schema = Schema {
        example: Some(json!("this is an example")),
        ..Schema::default()
    };

    assert_eq!(schema.json_schema(), json!({}));
}

// upstream-test: types/test_schema_json_schema.py::test_property_ordering_conversion
#[test]
fn test_property_ordering_conversion() {
    let schema = Schema {
        property_ordering: strings(&["a", "b"]),
        ..Schema::default()
    };

    assert_eq!(schema.json_schema(), json!({}));
}

// upstream-test: types/test_schema_json_schema.py::test_direct_conversion
#[test]
fn test_direct_conversion() {
    let schema = Schema {
        pattern: Some("^[a-z]+$".to_owned()),
        default: Some(json!(1)),
        max_length: Some(10),
        title: Some("title".to_owned()),
        min_length: Some(2),
        min_properties: Some(3),
        max_properties: Some(7),
        description: Some("description".to_owned()),
        r#enum: strings(&["enum1", "enum2"]),
        format: Some("email".to_owned()),
        max_items: Some(199),
        maximum: Some(300.0),
        min_items: Some(6),
        minimum: Some(40.0),
        required: strings(&["required1", "required2"]),
        ..Schema::default()
    };

    assert_eq!(
        schema.json_schema(),
        json!({
            "pattern": "^[a-z]+$",
            "default": 1,
            "maxLength": 10,
            "title": "title",
            "minLength": 2,
            "minProperties": 3,
            "maxProperties": 7,
            "description": "description",
            "enum": ["enum1", "enum2"],
            "format": "email",
            "maxItems": 199,
            "maximum": 300.0,
            "minItems": 6,
            "minimum": 40.0,
            "required": ["required1", "required2"],
        })
    );
}

fn fruit(name: &str, description: &str, variety_description: &str) -> Schema {
    Schema {
        title: Some(name.to_owned()),
        description: Some(description.to_owned()),
        properties: properties([
            (
                "type",
                Schema {
                    description: Some(format!("Always \"{}\"", name.to_lowercase())),
                    ..typed(Type::String)
                },
            ),
            (
                "variety",
                Schema {
                    description: Some(variety_description.to_owned()),
                    ..typed(Type::String)
                },
            ),
        ]),
        property_ordering: strings(&["type", "variety"]),
        required: strings(&["type", "variety"]),
        ..typed(Type::Object)
    }
}

fn fruit_json(name: &str, description: &str, variety_description: &str) -> serde_json::Value {
    json!({
        "title": name,
        "description": description,
        "type": "object",
        "properties": {
            "type": {"type": "string", "description": format!("Always \"{}\"", name.to_lowercase())},
            "variety": {"type": "string", "description": variety_description},
        },
        "required": ["type", "variety"],
    })
}

// upstream-test: types/test_schema_json_schema.py::test_complex_any_of_conversion
#[test]
fn test_complex_any_of_conversion() {
    let apple = (
        "Apple",
        "Describes an apple",
        "The variety of apple (e.g., \"Granny Smith\")",
    );
    let orange = (
        "Orange",
        "Describes an orange",
        "The variety of orange (e.g.,\"Navel orange\")",
    );
    let schema = Schema {
        title: Some("Fruit Basket".to_owned()),
        description: Some("A structured representation of a fruit basket".to_owned()),
        properties: properties([(
            "fruit",
            Schema {
                description: Some("An ordered list of the fruit in the basket".to_owned()),
                items: Some(Box::new(Schema {
                    any_of: Some(vec![
                        fruit(apple.0, apple.1, apple.2),
                        fruit(orange.0, orange.1, orange.2),
                    ]),
                    ..Schema::default()
                })),
                ..typed(Type::Array)
            },
        )]),
        required: strings(&["fruit"]),
        ..typed(Type::Object)
    };

    assert_eq!(
        schema.json_schema(),
        json!({
            "type": "object",
            "title": "Fruit Basket",
            "description": "A structured representation of a fruit basket",
            "properties": {
                "fruit": {
                    "type": "array",
                    "description": "An ordered list of the fruit in the basket",
                    "items": {
                        "anyOf": [
                            fruit_json(apple.0, apple.1, apple.2),
                            fruit_json(orange.0, orange.1, orange.2),
                        ],
                    },
                },
            },
            "required": ["fruit"],
        })
    );
}
