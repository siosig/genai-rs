//! Ports of `google/genai/tests/afc/test_invoke_function_from_dict_args.py`.
//!
//! Python's `invoke_function_from_dict_args(args, function)` reads the
//! function's annotations at run time and converts dict arguments to pydantic
//! models. A Rust tool declares its argument type up front, and
//! `FunctionTool::call` deserializes the JSON arguments into it, so the
//! equivalent of "convert args to the annotated types" is a `serde` struct
//! (pydantic model -> struct, `Union[..]` -> untagged enum). A mismatch is
//! `FunctionCallError::UnknownArgument` (upstream
//! `UnknownFunctionCallArgumentError`); a failing callable is
//! `FunctionCallError::Invocation` (upstream `FunctionInvocationError`).

use std::collections::HashMap;

use gemini_genai::{Error, errors::FunctionCallError, function_tool};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SimpleModel {
    key1_simple: i64,
    key2_simple: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SimpleModel1 {
    key1_simple: i64,
    key2_simple: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
struct SimpleModel2 {
    key3_simple: String,
    key4_simple: f64,
}

fn assert_unknown_argument(result: &Result<Value, Error>, case: &str) {
    assert!(
        matches!(
            result,
            Err(Error::FunctionCall(
                FunctionCallError::UnknownArgument { .. }
            ))
        ),
        "{case}: expected UnknownArgument, got {result:?}"
    );
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_builtin_primitive_types
#[tokio::test]
async fn test_builtin_primitive_types() {
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: i64,
        y: f64,
        z: String,
        w: bool,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_primitive",
        "Primitive arguments.",
        |args: Args| async move {
            Ok(json!({
                "x": args.x + 1,
                "y": args.y + 1.0,
                "z": format!("{}1.0", args.z),
                "w": !args.w,
            }))
        },
    );

    let actual = tool
        .call(json!({"x": 1, "y": 1.0, "z": "1.0", "w": true}))
        .await
        .expect("the call succeeds");

    assert_eq!(actual, json!({"x": 2, "y": 2.0, "z": "1.01.0", "w": false}));
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_builtin_compound_types
#[tokio::test]
async fn test_builtin_compound_types() {
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: Vec<i64>,
        y: HashMap<String, f64>,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_compound",
        "Compound arguments.",
        |mut args: Args| async move {
            args.x.push(3);
            args.y.insert("key3".to_owned(), 3.0);
            Ok(json!({"new_x": args.x, "new_y": args.y}))
        },
    );

    let actual = tool
        .call(json!({"x": [1, 2], "y": {"key1": 1.0, "key2": 2.0}}))
        .await
        .expect("the call succeeds");

    assert_eq!(
        actual,
        json!({
            "new_x": [1, 2, 3],
            "new_y": {"key1": 1.0, "key2": 2.0, "key3": 3.0},
        })
    );
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_nested_pydantic_model
#[tokio::test]
async fn test_nested_pydantic_model() {
    #[derive(Clone, Serialize, Deserialize, JsonSchema)]
    #[expect(
        clippy::struct_field_names,
        reason = "the field names mirror upstream's `ComplexModel`"
    )]
    struct ComplexModel {
        key1_complex: SimpleModel,
        key2_complex: Vec<SimpleModel>,
        key3_complex: HashMap<String, SimpleModel>,
    }
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: ComplexModel,
    }
    fn bump(model: &mut SimpleModel) {
        model.key1_simple += 1;
        model.key2_simple += 1.0;
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_nested_model",
        "Nested model arguments.",
        |args: Args| async move {
            let mut y = args.x;
            bump(&mut y.key1_complex);
            y.key2_complex.iter_mut().for_each(bump);
            y.key3_complex.values_mut().for_each(bump);
            Ok(y)
        },
    );

    let actual = tool
        .call(json!({"x": {
            "key1_complex": {"key1_simple": 1, "key2_simple": 1.0},
            "key2_complex": [
                {"key1_simple": 2, "key2_simple": 2.0},
                {"key1_simple": 3, "key2_simple": 3.0},
            ],
            "key3_complex": {
                "key1_simple": {"key1_simple": 4, "key2_simple": 4.0},
                "key2_simple": {"key1_simple": 5, "key2_simple": 5.0},
            },
        }}))
        .await
        .expect("the call succeeds");

    assert_eq!(
        actual,
        json!({
            "key1_complex": {"key1_simple": 2, "key2_simple": 2.0},
            "key2_complex": [
                {"key1_simple": 3, "key2_simple": 3.0},
                {"key1_simple": 4, "key2_simple": 4.0},
            ],
            "key3_complex": {
                "key1_simple": {"key1_simple": 5, "key2_simple": 5.0},
                "key2_simple": {"key1_simple": 6, "key2_simple": 6.0},
            },
        })
    );
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_pydantic_model_in_list_union_type
#[tokio::test]
async fn test_pydantic_model_in_list_union_type() {
    #[derive(Serialize, Deserialize, JsonSchema)]
    #[serde(untagged)]
    enum IntOrModel {
        Int(i64),
        Model(SimpleModel),
    }
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: Vec<IntOrModel>,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_list_union",
        "List of a union.",
        |args: Args| async move {
            Ok(args
                .x
                .into_iter()
                .map(|item| match item {
                    IntOrModel::Int(value) => IntOrModel::Int(value + 1),
                    IntOrModel::Model(mut model) => {
                        model.key1_simple += 1;
                        model.key2_simple += 1.0;
                        IntOrModel::Model(model)
                    }
                })
                .collect::<Vec<_>>())
        },
    );

    let actual = tool
        .call(json!({"x": [1, {"key1_simple": 1, "key2_simple": 1.0}]}))
        .await
        .expect("the call succeeds");

    assert_eq!(actual, json!([2, {"key1_simple": 2, "key2_simple": 2.0}]));
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_unknown_pydantic_model_argument
#[tokio::test]
async fn test_unknown_pydantic_model_argument() {
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: SimpleModel,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_unknown_model",
        "One model.",
        |args: Args| async move { Ok(args.x) },
    );

    let result = tool
        .call(json!({"x": {"key3_simple": 1, "key2_simple": 1.0}}))
        .await;

    assert_unknown_argument(&result, "a model missing a required field");
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_unknown_pydantic_model_argument_with_union_type
#[tokio::test]
async fn test_unknown_pydantic_model_argument_with_union_type() {
    #[derive(Serialize, Deserialize, JsonSchema)]
    #[serde(untagged)]
    enum Either {
        First(SimpleModel1),
        Second(SimpleModel2),
    }
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: Either,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_unknown_union",
        "A union of two models.",
        |args: Args| async move { Ok(args.x) },
    );

    let result = tool
        .call(json!({"x": {"key5_simple": 1, "key4_simple": 1.0}}))
        .await;

    assert_unknown_argument(&result, "a value matching neither model");
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_unknown_pydantic_model_argument_with_union_type_and_builtin_type
#[tokio::test]
async fn test_unknown_pydantic_model_argument_with_union_type_and_builtin_type() {
    #[derive(Serialize, Deserialize, JsonSchema)]
    #[serde(untagged)]
    enum ModelOrInt {
        Model(SimpleModel1),
        Int(i64),
    }
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: ModelOrInt,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_unknown_union_builtin",
        "A model or an int.",
        |args: Args| async move { Ok(args.x) },
    );

    let result = tool
        .call(json!({"x": {"key5_simple": 1, "key4_simple": 1.0}}))
        .await;

    assert_unknown_argument(&result, "a value matching neither the model nor int");
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_incompatible_value_and_annotation
#[tokio::test]
async fn test_incompatible_value_and_annotation() {
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: i64,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_incompatible",
        "Adds one.",
        |args: Args| async move { Ok(args.x + 1) },
    );

    for (case, x) in [
        ("a dict", json!({"k": "v"})),
        ("a string", json!("a")),
        ("a list", json!([])),
        ("a float", json!(1.0)),
        ("an empty dict", json!({})),
    ] {
        assert_unknown_argument(&tool.call(json!({ "x": x })).await, case);
    }
}

// upstream-test: afc/test_invoke_function_from_dict_args.py::test_function_invocation_error
#[tokio::test]
async fn test_function_invocation_error() {
    #[derive(Deserialize, JsonSchema)]
    struct Args {
        x: i64,
    }
    let tool = function_tool::<Args, _, _, _>(
        "afc_invoke_failing",
        "Divides by zero.",
        |args: Args| async move {
            args.x
                .checked_div(0)
                .ok_or_else(|| Error::Validation("division by zero".to_owned()))
        },
    );

    let result = tool.call(json!({"x": 1})).await;

    match result {
        Err(Error::FunctionCall(FunctionCallError::Invocation { function, message })) => {
            assert_eq!(function, "afc_invoke_failing");
            assert!(message.contains("division by zero"), "{message}");
        }
        other => panic!("expected FunctionCallError::Invocation, got {other:?}"),
    }
}
