//! Automatic function calling (AFC): registering Rust callables (and, via
//! `crate::mcp`, MCP tools) as model-invocable functions, and the request
//! loop that invokes them without further user intervention.
//!
//! Mirrors Python's `_automatic_function_calling_util.py` /
//! `_extra_utils.py` helpers and the AFC `while` loop inlined in
//! `AsyncModels.generate_content` (`google/genai/models.py`).

use std::{
    collections::HashMap,
    future::Future,
    marker::PhantomData,
    sync::{Arc, Mutex, MutexGuard, OnceLock},
};

use futures_util::future::BoxFuture;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    error::{Error, FunctionCallError, Result},
    models::Models,
    types::{
        AutomaticFunctionCallingConfig, Content, Contents, FunctionCall, FunctionDeclaration,
        GenerateContentConfig, GenerateContentResponse, Part, Tool,
    },
};

/// The default `maximum_remote_calls`, matching Python's
/// `_DEFAULT_MAX_REMOTE_CALLS_AFC`.
const DEFAULT_MAX_REMOTE_CALLS: i64 = 10;

/// A Rust callable the model can invoke automatically. Mirrors Python's
/// bare-callable `Tool` entries (a plain function, or coroutine function,
/// passed directly in `config.tools`).
///
/// Build one with [`function_tool`], or by hand for cases `function_tool`
/// doesn't cover (e.g. [`crate::mcp`]'s bridge to an MCP server), then
/// register it with [`Tool::from_function`] to include it in
/// `GenerateContentConfig.tools`.
pub trait FunctionTool: Send + Sync {
    /// The declaration sent to the model: name, description, and JSON
    /// Schema of the expected arguments.
    fn declaration(&self) -> FunctionDeclaration;

    /// Invokes the tool with the model-supplied arguments and returns the
    /// JSON result to send back to the model as a `FunctionResponse`.
    ///
    /// # Errors
    /// [`Error::FunctionCall`] (or any other crate error) if the call
    /// fails; the AFC loop reports this back to the model as an `error`
    /// field on the `FunctionResponse` and continues, except for
    /// [`FunctionCallError::UnknownArgument`], which aborts the loop (see
    /// [`crate::models::Models::generate_content`]).
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>>;
}

/// Process-wide registry mapping a function name to the callable most
/// recently registered under it via [`Tool::from_function`].
///
/// # Why a registry, and why keyed by name
///
/// `Tool` is a generated, plain-data struct (`Clone + Serialize +
/// Deserialize`, sent verbatim on the wire) with no room for a
/// non-serializable `Arc<dyn FunctionTool>` field, and
/// `Models::generate_content`'s signature -- fixed by the public API
/// contract to mirror Python's -- has no side channel for passing a
/// registry alongside `GenerateContentConfig`. The only thing that
/// survives from [`Tool::from_function`]'s caller all the way to the AFC
/// loop inside `generate_content` is `config.tools: Vec<Tool>` itself, so
/// the callable must be recoverable from that alone. A `Tool`'s declared
/// function name is the one piece of data guaranteed to be both present
/// there and echoed back by the model in `FunctionCall.name` -- exactly
/// what the AFC loop needs to look the callable back up -- so this
/// registry is keyed by that name.
///
/// # Caveat
///
/// Because this registry is process-wide, registering two different
/// callables under the same function name (e.g. concurrently, from two
/// unrelated `generate_content` calls racing each other) means the most
/// recent registration wins for *both* -- unlike Python, where each call's
/// `function_map` is rebuilt fresh from that call's own `config.tools` and
/// so never leaks across calls. Give each distinct callable a unique
/// name to avoid this; a typical application registers each tool once, at
/// startup, which sidesteps the issue entirely.
fn registry() -> &'static Mutex<HashMap<String, Arc<dyn FunctionTool>>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<dyn FunctionTool>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

#[expect(
    clippy::unwrap_used,
    reason = "the registry mutex is only ever held for the duration of a HashMap insert/get, which cannot panic; poisoning would mean an unrelated panic already occurred while holding this lock, which already leaves the process in an unrecoverable state, so propagating that panic here is correct"
)]
fn registry_lock() -> MutexGuard<'static, HashMap<String, Arc<dyn FunctionTool>>> {
    registry().lock().unwrap()
}

impl Tool {
    /// Builds a [`Tool`] declaring `tool`'s function to the model, and
    /// registers `tool` in the process-wide registry (see this module's
    /// docs) so the AFC loop can find and invoke it when the model calls it
    /// by name.
    #[must_use]
    pub fn from_function(tool: Arc<dyn FunctionTool>) -> Tool {
        let declaration = tool.declaration();
        let name = declaration.name.clone().unwrap_or_default();
        registry_lock().insert(name, tool);
        Tool {
            function_declarations: Some(vec![declaration]),
            ..Default::default()
        }
    }
}

/// Builds a [`FunctionTool`] from a plain async Rust function, generating
/// its JSON Schema argument declaration from `A`'s [`schemars::JsonSchema`]
/// implementation.
///
/// The generated declaration sets
/// `FunctionDeclaration.parameters_json_schema` (raw JSON Schema) rather
/// than `parameters` (this crate's OpenAPI-shaped `Schema`), mirroring
/// Python's own `parse_function_declaration_json_schema` path -- `A`'s
/// derived schema is already a complete JSON Schema object, so no
/// `OpenAPI`-`Schema` reconstruction is needed.
///
/// Note on wire casing: this field reaches the wire as
/// `parameters_json_schema` (`snake_case`), not `parametersJsonSchema`.
/// That is *not* a porting defect -- Python behaves identically, because
/// its `_Tool_to_mldev` passes `function_declarations` through as
/// `[item for item in ...]` (leaving pydantic model instances in a list),
/// and `_common.convert_to_dict` only camelizes one dict level deep, so
/// it never reaches fields of models nested inside a list. Verified
/// empirically against google-genai 2.19.0. The server accepts it: proto3
/// JSON mapping requires parsers to accept the original proto field name
/// as well as its `lowerCamelCase` form.
#[must_use]
pub fn function_tool<A, R, F, Fut>(name: &str, description: &str, f: F) -> Arc<dyn FunctionTool>
where
    A: schemars::JsonSchema + DeserializeOwned + Send + 'static,
    R: Serialize + Send + 'static,
    F: Fn(A) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    struct Callable<A, R, F, Fut> {
        name: String,
        description: String,
        f: F,
        // `A`/`R`/`Fut` only ever appear behind `F`'s call signature, never
        // as a stored value; a bare `fn` pointer marker keeps the struct
        // `Send + Sync` regardless of `A`/`R`/`Fut`'s own auto-trait
        // status, while still constraining them so the impl below is
        // well-formed (each type parameter must appear in `Self`).
        _marker: PhantomData<fn(A) -> (R, Fut)>,
    }

    impl<A, R, F, Fut> FunctionTool for Callable<A, R, F, Fut>
    where
        A: schemars::JsonSchema + DeserializeOwned + Send + 'static,
        R: Serialize + Send + 'static,
        F: Fn(A) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
    {
        fn declaration(&self) -> FunctionDeclaration {
            let schema: Value = schemars::schema_for!(A).into();
            FunctionDeclaration {
                name: Some(self.name.clone()),
                description: Some(self.description.clone()),
                parameters_json_schema: Some(schema),
                ..Default::default()
            }
        }

        fn call(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
            Box::pin(async move {
                let parsed: A = serde_json::from_value(args).map_err(|error| {
                    Error::FunctionCall(FunctionCallError::UnknownArgument {
                        function: self.name.clone(),
                        message: error.to_string(),
                    })
                })?;
                let result = (self.f)(parsed).await?;
                Ok(serde_json::to_value(result)?)
            })
        }
    }

    Arc::new(Callable::<A, R, F, Fut> {
        name: name.to_owned(),
        description: description.to_owned(),
        f,
        _marker: PhantomData,
    })
}

/// Every function declared by `tools` that has a callable registered for
/// it, keyed by function name.
fn registered_tools_for(
    config: Option<&GenerateContentConfig>,
) -> HashMap<String, Arc<dyn FunctionTool>> {
    let Some(tools) = config.and_then(|c| c.tools.as_ref()) else {
        return HashMap::new();
    };
    let registry = registry_lock();
    let mut found = HashMap::new();
    for tool in tools {
        let Some(declarations) = tool.function_declarations.as_ref() else {
            continue;
        };
        for declaration in declarations {
            let Some(name) = declaration.name.as_ref() else {
                continue;
            };
            if let Some(callable) = registry.get(name) {
                found.insert(name.clone(), Arc::clone(callable));
            }
        }
    }
    found
}

/// Whether AFC is disabled for this request. Mirrors Python's
/// `_extra_utils.should_disable_afc`.
fn should_disable(afc: Option<&AutomaticFunctionCallingConfig>) -> bool {
    let Some(afc) = afc else {
        return false;
    };
    if afc.maximum_remote_calls.is_some_and(|max| max <= 0) {
        return true;
    }
    afc.disable.unwrap_or(false)
}

/// The maximum number of remote (AFC) calls for this request. Mirrors
/// Python's `_extra_utils.get_max_remote_calls_afc`.
fn max_remote_calls(afc: Option<&AutomaticFunctionCallingConfig>) -> i64 {
    afc.and_then(|a| a.maximum_remote_calls)
        .unwrap_or(DEFAULT_MAX_REMOTE_CALLS)
}

/// Turns one model-requested function call into a `FunctionResponse`
/// [`Part`], by looking up and invoking the matching registered tool.
///
/// # Errors
/// [`Error::FunctionCall`]: [`FunctionCallError::UnsupportedFunction`] if
/// `call.name` has no registered tool in `callables`, or
/// [`FunctionCallError::UnknownArgument`] if the tool's [`FunctionTool::call`]
/// reports the model's arguments don't match its declared type. Both abort
/// the AFC loop (mirroring a Rust static-typing violation, unlike Python's
/// dynamic-typing tolerance). Any other error from the tool becomes an
/// `error` field on the returned `FunctionResponse` instead, and does not
/// abort the loop (mirrored in [`generate_content`]'s caller, since this
/// function can't itself distinguish "abort" from "continue" in its return
/// type).
async fn invoke(
    call: &FunctionCall,
    callables: &HashMap<String, Arc<dyn FunctionTool>>,
) -> Result<Part> {
    let name = call.name.clone().unwrap_or_default();
    let Some(tool) = callables.get(&name) else {
        return Err(Error::FunctionCall(FunctionCallError::UnsupportedFunction(
            name,
        )));
    };
    let args = Value::Object(call.args.clone().unwrap_or_default().into_iter().collect());
    match tool.call(args).await {
        Ok(result) => {
            let mut response = HashMap::with_capacity(1);
            response.insert("result".to_owned(), result);
            Ok(Part::from_function_response(name, response))
        }
        Err(error @ Error::FunctionCall(FunctionCallError::UnknownArgument { .. })) => Err(error),
        Err(error) => {
            let mut response = HashMap::with_capacity(1);
            response.insert("error".to_owned(), Value::String(error.to_string()));
            Ok(Part::from_function_response(name, response))
        }
    }
}

/// Drives [`Models::generate_content`]. If `config.tools` declares at
/// least one function with a callable registered via
/// [`Tool::from_function`] (directly, or via `crate::mcp::mcp_tools`), and
/// AFC isn't disabled, this runs the automatic-function-calling loop:
///
/// ```text
/// remaining = maximum_remote_calls (default 10); history = []
/// loop:
///   resp = request()
///   calls = resp.function_calls(); if none -> break
///   for call in calls: result = tool.call(args) -> FunctionResponse part (error field on failure)
///   history += [resp.content, Content{role: user, parts: function_responses}]
///   remaining -= 1; if remaining == 0 -> break
/// resp.automatic_function_calling_history = history
/// ```
///
/// Otherwise (no registered tools declared, `automatic_function_calling.disable`,
/// or `maximum_remote_calls <= 0`) this issues exactly one request and
/// returns its response unmodified, exactly like `Models::generate_content`
/// did before AFC existed.
///
/// # Errors
/// See [`Models::generate_content`].
pub(crate) async fn generate_content(
    models: &Models,
    model: &str,
    contents: Contents,
    config: Option<GenerateContentConfig>,
) -> Result<GenerateContentResponse> {
    let callables = registered_tools_for(config.as_ref());
    if callables.is_empty() {
        return models.generate_content_once(model, contents, config).await;
    }

    let afc_config = config
        .as_ref()
        .and_then(|c| c.automatic_function_calling.as_ref());
    if should_disable(afc_config) {
        return models.generate_content_once(model, contents, config).await;
    }

    let mut remaining = max_remote_calls(afc_config);
    let ignore_call_history = afc_config
        .and_then(|a| a.ignore_call_history)
        .unwrap_or(false);

    let mut contents: Vec<Content> = contents.into();
    let mut history: Vec<Content> = Vec::new();
    let mut response;

    loop {
        response = models
            .generate_content_once(model, contents.clone(), config.clone())
            .await?;
        remaining -= 1;

        let calls: Vec<FunctionCall> = response.function_calls().into_iter().cloned().collect();
        if calls.is_empty() {
            break;
        }

        let mut response_parts = Vec::with_capacity(calls.len());
        for call in &calls {
            response_parts.push(invoke(call, &callables).await?);
        }

        let Some(call_content) = response
            .candidates
            .as_ref()
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.content.clone())
        else {
            break;
        };
        let response_content = Content {
            role: Some("user".to_owned()),
            parts: Some(response_parts),
        };
        contents.push(call_content.clone());
        contents.push(response_content.clone());
        history.push(call_content);
        history.push(response_content);

        if remaining == 0 {
            break;
        }
    }

    if !ignore_call_history {
        response.automatic_function_calling_history = Some(history);
    }
    Ok(response)
}
