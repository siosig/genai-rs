//! Port of `transformers/test_schema.py`.
//!
//! The upstream tests build their schemas with `pydantic` models and
//! `model_json_schema()`; where the behavior under test is `process_schema`
//! / `handle_null_fields` itself, the port feeds the JSON Schema pydantic
//! would have emitted, as a literal. The tests that only exercise the
//! pydantic-to-schema step are excluded. The Vertex halves of the
//! parameterized tests do not apply (Developer API only).

use gemini_genai::__test_support::transformers as t;
use serde_json::{Value, json};

fn process(schema: &mut Value, order_properties: bool) -> gemini_genai::Result<()> {
    t::process_schema(schema, None, order_properties)
}

fn country_info_json_schema() -> Value {
    json!({
        "properties": {
            "name": {"title": "Name", "type": "string"},
            "population": {"title": "Population", "type": "integer"},
            "capital": {"title": "Capital", "type": "string"},
            "continent": {"title": "Continent", "type": "string"},
            "gdp": {"title": "Gdp", "type": "integer"},
            "official_language": {"title": "Official Language", "type": "string"},
            "total_area_sq_mi": {"title": "Total Area Sq Mi", "type": "integer"},
        },
        "required": [
            "name", "population", "capital", "continent", "gdp",
            "official_language", "total_area_sq_mi",
        ],
        "title": "CountryInfo",
        "type": "object",
    })
}

// upstream-test: transformers/test_schema.py::test_t_schema_for_null_fields
#[test]
fn test_t_schema_for_null_fields() {
    let mut schema = json!({
        "properties": {
            "name": {"title": "Name", "type": "string"},
            "population": {
                "anyOf": [{"type": "integer"}, {"type": "null"}],
                "default": null,
                "title": "Population",
            },
        },
        "required": ["name"],
        "title": "CountryInfoWithNullFields",
        "type": "object",
    });
    process(&mut schema, true).unwrap();
    assert_eq!(schema["properties"]["population"]["nullable"], json!(true));
    assert_eq!(schema["properties"]["population"]["type"], json!("integer"));
    assert!(schema["properties"]["population"].get("anyOf").is_none());
}

// upstream-test: transformers/test_schema.py::test_schema_with_no_null_fields_is_unchanged
#[test]
fn test_schema_with_no_null_fields_is_unchanged() {
    let properties = json!({
        "name": {"title": "Name", "type": "string"},
        "total_area_sq_mi": {
            "anyOf": [{"type": "integer"}, {"type": "float"}],
            "default": "null",
            "title": "Total Area Sq Mi",
        },
    });
    for (name, schema) in properties.as_object().unwrap() {
        let before = schema.clone();
        let mut schema = schema.clone();
        t::handle_null_fields(&mut schema);
        assert_eq!(before, schema, "{name}");
    }
}

// upstream-test: transformers/test_schema.py::test_schema_with_default_value
#[test]
fn test_schema_with_default_value() {
    let mut schema = json!({
        "properties": {
            "name": {"title": "Name", "type": "string"},
            "population": {"default": 0, "title": "Population", "type": "integer"},
        },
        "required": ["name"],
        "title": "CountryInfoWithDefaultValue",
        "type": "object",
    });
    process(&mut schema, true).unwrap();
    assert_eq!(
        schema,
        json!({
            "properties": {
                "name": {"title": "Name", "type": "string"},
                "population": {"default": 0, "title": "Population", "type": "integer"},
            },
            "required": ["name"],
            "title": "CountryInfoWithDefaultValue",
            "type": "object",
            "property_ordering": ["name", "population"],
        })
    );
}

// upstream-test: transformers/test_schema.py::test_schema_with_any_of
#[test]
fn test_schema_with_any_of() {
    let mut schema = json!({
        "properties": {
            "name": {"title": "Name", "type": "string"},
            "restaurants_per_capita": {
                "anyOf": [{"type": "integer"}, {"type": "number"}],
                "title": "Restaurants Per Capita",
            },
        },
        "required": ["name", "restaurants_per_capita"],
        "title": "CountryInfoWithAnyOf",
        "type": "object",
    });
    let mut expected = schema.clone();
    expected["property_ordering"] = json!(["name", "restaurants_per_capita"]);
    process(&mut schema, true).unwrap();
    assert_eq!(schema, expected);
}

// upstream-test: transformers/test_schema.py::test_complex_dict_schema_with_anyof_is_unchanged
#[test]
fn test_complex_dict_schema_with_anyof_is_unchanged() {
    let apple_or_orange = |name: &str, other: &str| {
        json!({
            "title": name,
            "description": format!("Describes an {name}"),
            "type": "OBJECT",
            "properties": {
                "type": {"type": "STRING", "description": format!("Always '{name}'")},
                other: {"type": "STRING", "description": format!("The {other}")},
            },
            "propertyOrdering": ["type", other],
            "required": ["type", other],
        })
    };
    let mut schema = json!({
        "type": "OBJECT",
        "title": "Fruit Basket",
        "description": "A structured representation of a fruit basket",
        "required": ["fruit"],
        "properties": {
            "fruit": {
                "type": "ARRAY",
                "description": "An ordered list of the fruit in the basket",
                "items": {
                    "description": "A piece of fruit",
                    "anyOf": [apple_or_orange("apple", "color"), apple_or_orange("orange", "size")],
                },
            }
        },
    });
    let before = schema.clone();
    process(&mut schema, true).unwrap();
    assert_eq!(before, schema);
}

// upstream-test: transformers/test_schema.py::test_process_schema_converts_const_to_enum
#[test]
fn test_process_schema_converts_const_to_enum() {
    let mut schema = json!({"type": "STRING", "const": "FOO"});
    process(&mut schema, true).unwrap();
    assert_eq!(schema, json!({"type": "STRING", "enum": ["FOO"]}));
}

// upstream-test: transformers/test_schema.py::test_process_schema_forbids_non_string_const
#[test]
fn test_process_schema_forbids_non_string_const() {
    let mut schema = json!({"type": "INTEGER", "const": 123});
    let err = process(&mut schema, true).unwrap_err();
    assert!(
        err.to_string().contains("Literal values must be strings"),
        "unexpected error: {err}"
    );
}

/// Runs `schema` through `process_schema` with each `order_properties`
/// setting and compares with the matching expectation.
fn assert_ordering(schema: &Value, without: &Value, with: &Value) {
    for order_properties in [false, true] {
        let mut actual = schema.clone();
        assert!(
            process(&mut actual, order_properties).is_ok(),
            "order_properties={order_properties}"
        );
        let expected = if order_properties { with } else { without };
        assert_eq!(&actual, expected, "order_properties={order_properties}");
    }
}

fn foo_bar(extra: Option<Value>) -> Value {
    let mut schema = json!({
        "type": "OBJECT",
        "properties": {"foo": {"type": "STRING"}, "bar": {"type": "STRING"}},
    });
    if let Some(ordering) = extra {
        schema["property_ordering"] = ordering;
    }
    schema
}

// upstream-test: transformers/test_schema.py::test_process_schema_order_properties_propagates_into_defs
#[test]
fn test_process_schema_order_properties_propagates_into_defs() {
    let schema = json!({"$ref": "#/$defs/Foo", "$defs": {"Foo": foo_bar(None)}});
    assert_ordering(
        &schema,
        &foo_bar(None),
        &foo_bar(Some(json!(["foo", "bar"]))),
    );
}

// upstream-test: transformers/test_schema.py::test_process_schema_order_properties_propagates_into_items
#[test]
fn test_process_schema_order_properties_propagates_into_items() {
    let schema = json!({"type": "ARRAY", "items": foo_bar(None)});
    assert_ordering(
        &schema,
        &schema,
        &json!({"type": "ARRAY", "items": foo_bar(Some(json!(["foo", "bar"])))}),
    );
}

// upstream-test: transformers/test_schema.py::test_process_schema_order_properties_propagates_into_prefix_items
#[test]
fn test_process_schema_order_properties_propagates_into_prefix_items() {
    let schema = json!({"type": "ARRAY", "prefixItems": [foo_bar(None)]});
    assert_ordering(
        &schema,
        &schema,
        &json!({"type": "ARRAY", "prefixItems": [foo_bar(Some(json!(["foo", "bar"])))]}),
    );
}

// upstream-test: transformers/test_schema.py::test_process_schema_order_properties_propagates_into_properties
#[test]
fn test_process_schema_order_properties_propagates_into_properties() {
    let schema = json!({
        "type": "OBJECT",
        "properties": {"xyz": foo_bar(None), "abc": {"type": "STRING"}},
    });
    assert_ordering(
        &schema,
        &schema,
        &json!({
            "type": "OBJECT",
            "properties": {
                "xyz": foo_bar(Some(json!(["foo", "bar"]))),
                "abc": {"type": "STRING"},
            },
            "property_ordering": ["xyz", "abc"],
        }),
    );
}

// upstream-test: transformers/test_schema.py::test_process_schema_order_properties_propagates_into_additional_properties
#[test]
fn test_process_schema_order_properties_propagates_into_additional_properties() {
    // Developer API branch of the upstream test: a non-empty
    // `additionalProperties` is rejected, whatever `order_properties` is.
    let schema = json!({"type": "OBJECT", "additionalProperties": foo_bar(None)});
    for order_properties in [false, true] {
        let err = process(&mut schema.clone(), order_properties).unwrap_err();
        assert!(
            err.to_string().contains(
                "additionalProperties is only supported in Gemini Enterprise Agent Platform mode"
            ),
            "order_properties={order_properties}: {err}"
        );
    }
}

// upstream-test: transformers/test_schema.py::test_process_schema_order_properties_propagates_into_any_of
#[test]
fn test_process_schema_order_properties_propagates_into_any_of() {
    let schema = json!({"anyOf": [foo_bar(None), {"type": "STRING"}]});
    assert_ordering(
        &schema,
        &schema,
        &json!({"anyOf": [foo_bar(Some(json!(["foo", "bar"]))), {"type": "STRING"}]}),
    );
}

// upstream-test: transformers/test_schema.py::test_process_schema_with_cycle
#[test]
fn test_process_schema_with_cycle() {
    let mut schema = json!({
        "type": "OBJECT",
        "properties": {"recursive": {"$ref": "#/$defs/RecursiveObject"}},
        "$defs": {
            "RecursiveObject": {
                "type": "OBJECT",
                "properties": {"self": {"$ref": "#/$defs/RecursiveObject"}},
            }
        },
    });
    process(&mut schema, true).unwrap();
    assert_eq!(
        schema,
        json!({
            "type": "OBJECT",
            "properties": {
                "recursive": {"type": "OBJECT", "properties": {"self": {}}},
            },
        })
    );
}

// upstream-test: transformers/test_schema.py::test_t_schema_does_not_change_property_ordering_if_set
#[test]
fn test_t_schema_does_not_change_property_ordering_if_set() {
    let mut schema = country_info_json_schema();
    schema["property_ordering"] = json!(["code", "symbol", "name"]);
    process(&mut schema, true).unwrap();
    assert_eq!(
        schema["propertyOrdering"],
        json!(["code", "symbol", "name"])
    );
    assert!(schema.get("property_ordering").is_none());
}

// upstream-test: transformers/test_schema.py::test_t_schema_sets_property_ordering_for_json_schema
#[test]
fn test_t_schema_sets_property_ordering_for_json_schema() {
    let mut schema = country_info_json_schema();
    process(&mut schema, true).unwrap();
    assert_eq!(
        schema["property_ordering"],
        json!([
            "name",
            "population",
            "capital",
            "continent",
            "gdp",
            "official_language",
            "total_area_sq_mi",
        ])
    );
}
