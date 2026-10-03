//! The automatic function calling (AFC) request loops and their helpers.
//!
//! Mirrors Python's `_extra_utils.py` helpers and the AFC `while` loops
//! inlined in `Models.generate_content` / `Models.generate_content_stream`
//! (`google/genai/models.py`). Callables are registered by
//! [`crate::automatic_function_calling_util`].
//!
//! The helpers are `pub` inside this crate-private module only so that
//! `crate::__test_support` can re-export them to the integration tests; they
//! are not part of the crate's public API.

#![expect(
    clippy::implicit_hasher,
    reason = "the function maps are always the default-hasher `HashMap` built by `get_function_map`; generalizing over `BuildHasher` would only add a type parameter to every caller"
)]

use std::{collections::HashMap, sync::Arc};

use futures_util::StreamExt;
use serde_json::{Map, Number, Value};

use crate::{
    api_client::headers::library_label,
    automatic_function_calling_util::{FunctionTool, registered_tool, registered_tools_for},
    errors::{Error, FunctionCallError, Result},
    models::{GenerateContentStream, Models},
    types::{
        Content, Contents, FunctionCall, GenerateContentConfig, GenerateContentResponse,
        GenerateImagesConfig, HttpOptions, Part, Tool,
    },
};

/// The default `maximum_remote_calls`, matching Python's
/// `_DEFAULT_MAX_REMOTE_CALLS_AFC`.
const DEFAULT_MAX_REMOTE_CALLS_AFC: i64 = 10;

/// The `usage` label `get_usage_header` attaches to AFC requests.
const USAGE_AFC: &str = "afc";

/// Largest magnitude below which every integral `f64` is exactly an `i64`.
const I64_EXACT_F64_LIMIT: f64 = 9.0e18;

/// The tool indexes in `config.tools` that make AFC impossible: a tool that
/// declares a function no callable is registered for (a hand-written
/// `FunctionDeclaration`), or one that lists MCP servers. Mirrors Python's
/// `_extra_utils.find_afc_incompatible_tool_indexes`.
///
/// In Python a callable in `tools` is not a `types.Tool`, so only declarations
/// written by hand count. A [`Tool`] built by [`Tool::from_function`] is also a
/// `Tool` with a declaration here, so a declaration counts as hand-written
/// only when no callable is registered under its name. As in Python, a tool
/// that has both a bare declaration and MCP servers is listed twice.
///
/// `is_agent_platform` mirrors Python's flag: MCP servers are only
/// incompatible on the Gemini Developer API.
#[must_use]
pub fn find_afc_incompatible_tool_indexes(
    config: Option<&GenerateContentConfig>,
    is_agent_platform: bool,
) -> Vec<usize> {
    let tools = config.and_then(|c| c.tools.as_deref()).unwrap_or_default();
    let mut indexes = Vec::new();
    for (index, tool) in tools.iter().enumerate() {
        if has_bare_function_declaration(tool) {
            indexes.push(index);
        }
        if !is_agent_platform && tool.mcp_servers.as_ref().is_some_and(|s| !s.is_empty()) {
            indexes.push(index);
        }
    }
    indexes
}

/// Whether `tool` declares a function that no registered callable backs.
fn has_bare_function_declaration(tool: &Tool) -> bool {
    tool.function_declarations
        .iter()
        .flatten()
        .any(|declaration| {
            declaration
                .name
                .as_deref()
                .is_none_or(|name| registered_tool(name).is_none())
        })
}

/// Logs a warning when only some of the tools are incompatible with AFC (when
/// every tool is, the caller evidently meant manual function calling). Mirrors
/// Python's `_extra_utils.log_afc_incompatible_tools_warning`.
pub fn log_afc_incompatible_tools_warning(
    config: Option<&GenerateContentConfig>,
    incompatible_tools_indexes: &[usize],
) {
    if incompatible_tools_indexes.is_empty() {
        return;
    }
    let original_tools_length = config.and_then(|c| c.tools.as_ref()).map_or(0, Vec::len);
    if incompatible_tools_indexes.len() != original_tools_length {
        let indices = incompatible_tools_indexes
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        tracing::warn!(
            "Tools at indices [{indices}] are not compatible with automatic function calling \
             (AFC). AFC is disabled. If AFC is intended, please include callable tools built \
             with `Tool::from_function` in the tool list, and do not include function \
             declarations without a callable or MCP servers in the tool list."
        );
    }
}

/// The callable behind every function `config.tools` declares, keyed by
/// function name. Mirrors Python's `_extra_utils.get_function_map`.
///
/// Python also rejects coroutine functions on its synchronous methods and
/// merges MCP session adapters; here every callable is async and MCP tools
/// are ordinary registered callables, so neither check exists.
#[must_use]
pub fn get_function_map(
    config: Option<&GenerateContentConfig>,
) -> HashMap<String, Arc<dyn FunctionTool>> {
    registered_tools_for(config)
}

/// Converts float values with no decimal part to integers, recursing through
/// arrays and objects. Mirrors Python's
/// `_extra_utils.convert_number_values_for_function_call_args`.
#[must_use]
pub fn convert_number_values_for_function_call_args(args: &Value) -> Value {
    match args {
        Value::Number(number) => Value::Number(integral_float_as_int(number)),
        Value::Object(map) => Value::Object(convert_number_values_for_dict_function_call_args(map)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(convert_number_values_for_function_call_args)
                .collect(),
        ),
        other => other.clone(),
    }
}

/// [`convert_number_values_for_function_call_args`] over the entries of a
/// JSON object. Mirrors Python's
/// `_extra_utils.convert_number_values_for_dict_function_call_args`.
#[must_use]
pub fn convert_number_values_for_dict_function_call_args(
    args: &Map<String, Value>,
) -> Map<String, Value> {
    args.iter()
        .map(|(key, value)| {
            (
                key.clone(),
                convert_number_values_for_function_call_args(value),
            )
        })
        .collect()
}

/// `number` as an integer if it is a float with no decimal part (and fits
/// an `i64`), otherwise unchanged.
fn integral_float_as_int(number: &Number) -> Number {
    let Some(float) = number.as_f64().filter(|_| number.is_f64()) else {
        return number.clone();
    };
    if float.fract() == 0.0 && float.abs() < I64_EXACT_F64_LIMIT {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the value is integral and bounded by I64_EXACT_F64_LIMIT, so the cast is exact"
        )]
        let integer = float as i64;
        Number::from(integer)
    } else {
        number.clone()
    }
}

/// Turns one model-requested function call into a `FunctionResponse`
/// [`Part`], by looking up and invoking the matching registered tool. Float
/// arguments with no decimal part are first converted to integers (see
/// [`convert_number_values_for_function_call_args`]), as Python does before
/// invoking.
///
/// # Errors
/// [`Error::FunctionCall`]: [`FunctionCallError::UnsupportedFunction`] if
/// `call.name` has no registered tool in `callables`, or
/// [`FunctionCallError::UnknownArgument`] if the tool's [`FunctionTool::call`]
/// reports the model's arguments don't match its declared type. Both abort
/// the AFC loop (mirroring a Rust static-typing violation, unlike Python's
/// dynamic-typing tolerance, which reports it to the model). Any other error
/// from the tool becomes an `error` field on the returned `FunctionResponse`
/// instead, and does not abort the loop.
pub async fn invoke_function_from_dict_args(
    call: &FunctionCall,
    callables: &HashMap<String, Arc<dyn FunctionTool>>,
) -> Result<Part> {
    let name = call.name.clone().unwrap_or_default();
    let Some(tool) = callables.get(&name) else {
        return Err(Error::FunctionCall(FunctionCallError::UnsupportedFunction(
            name,
        )));
    };
    let args = call.args.clone().unwrap_or_default().into_iter().collect();
    let args = Value::Object(convert_number_values_for_dict_function_call_args(&args));
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

/// The `FunctionResponse` parts answering every `functionCall` part in
/// `parts`, in order. A call without a name is skipped.
///
/// # Errors
/// See [`invoke_function_from_dict_args`].
pub async fn function_response_parts(
    parts: &[Part],
    callables: &HashMap<String, Arc<dyn FunctionTool>>,
) -> Result<Vec<Part>> {
    let mut response_parts = Vec::new();
    for call in parts
        .iter()
        .filter_map(|part| part.function_call.as_ref())
        .filter(|call| call.name.is_some())
    {
        response_parts.push(invoke_function_from_dict_args(call, callables).await?);
    }
    Ok(response_parts)
}

/// The `FunctionResponse` parts answering the function calls in the first
/// candidate of `response`. Mirrors Python's
/// `_extra_utils.get_function_response_parts` (and its `_async` twin: every
/// callable here is async).
///
/// Unlike Python, a call whose `args` are absent is still invoked, with no
/// arguments, so that a zero-argument function is answered.
///
/// # Errors
/// See [`invoke_function_from_dict_args`].
pub async fn get_function_response_parts(
    response: &GenerateContentResponse,
    function_map: &HashMap<String, Arc<dyn FunctionTool>>,
) -> Result<Vec<Part>> {
    let parts = response
        .candidates
        .as_ref()
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.content.as_ref())
        .and_then(|content| content.parts.as_deref())
        .unwrap_or_default();
    function_response_parts(parts, function_map).await
}

/// Whether AFC is disabled for this request. Mirrors Python's
/// `_extra_utils.should_disable_afc`.
#[must_use]
pub fn should_disable_afc(config: Option<&GenerateContentConfig>) -> bool {
    let Some(afc) = config.and_then(|c| c.automatic_function_calling.as_ref()) else {
        return false;
    };
    if let Some(max) = afc.maximum_remote_calls.filter(|max| *max <= 0) {
        tracing::warn!(
            "max_remote_calls in automatic_function_calling_config {max} is less than or equal \
             to 0. Disabling automatic function calling. Please set max_remote_calls to a \
             positive integer."
        );
        return true;
    }
    afc.disable.unwrap_or(false)
}

/// The maximum number of remote (AFC) calls for this request. Mirrors
/// Python's `_extra_utils.get_max_remote_calls_afc`.
///
/// # Errors
/// [`Error::Validation`] if AFC is disabled for this request (see
/// [`should_disable_afc`]): there is then no budget to read.
pub fn get_max_remote_calls_afc(config: Option<&GenerateContentConfig>) -> Result<i64> {
    let Some(config) = config else {
        return Ok(DEFAULT_MAX_REMOTE_CALLS_AFC);
    };
    if should_disable_afc(Some(config)) {
        return Err(Error::Validation(
            "automatic function calling is not enabled, but SDK is trying to get max remote \
             calls."
                .to_owned(),
        ));
    }
    Ok(config
        .automatic_function_calling
        .as_ref()
        .and_then(|afc| afc.maximum_remote_calls)
        .unwrap_or(DEFAULT_MAX_REMOTE_CALLS_AFC))
}

/// Rejects configs that cannot work with AFC: streaming function-call
/// arguments while AFC is not disabled. Mirrors Python's
/// `_extra_utils.raise_error_for_afc_incompatible_config`.
///
/// # Errors
/// [`Error::Validation`] if
/// `tool_config.function_calling_config.stream_function_call_arguments` is
/// set and `automatic_function_calling.disable` is not `true`.
pub fn raise_error_for_afc_incompatible_config(
    config: Option<&GenerateContentConfig>,
) -> Result<()> {
    let Some(config) = config else {
        return Ok(());
    };
    let Some(function_calling_config) = config
        .tool_config
        .as_ref()
        .and_then(|tool_config| tool_config.function_calling_config.as_ref())
    else {
        return Ok(());
    };
    let disable_afc = config
        .automatic_function_calling
        .as_ref()
        .and_then(|afc| afc.disable)
        .unwrap_or(false);
    if function_calling_config
        .stream_function_call_arguments
        .unwrap_or(false)
        && !disable_afc
    {
        return Err(Error::Validation(
            "Running in streaming mode with stream_function_call_arguments enabled, this \
             feature is not compatible with automatic function calling (AFC). Please set \
             config.automatic_function_calling.disable to True to disable AFC or leave \
             config.tool_config.function_calling_config.stream_function_call_arguments to be \
             empty or set to False to disable streaming function call arguments."
                .to_owned(),
        ));
    }
    Ok(())
}

/// Whether the AFC turns are reported on the response as
/// `automatic_function_calling_history`. Mirrors Python's
/// `_extra_utils.should_append_afc_history`.
#[must_use]
pub fn should_append_afc_history(config: Option<&GenerateContentConfig>) -> bool {
    !config
        .and_then(|c| c.automatic_function_calling.as_ref())
        .and_then(|afc| afc.ignore_call_history)
        .unwrap_or(false)
}

/// Appends the first candidate's content of `chunk` to `contents` and returns
/// them. Mirrors Python's `_extra_utils.append_chunk_contents`, which the
/// streaming AFC loop uses to carry each round's output into the next
/// request.
#[must_use]
pub fn append_chunk_contents(
    mut contents: Vec<Content>,
    chunk: &GenerateContentResponse,
) -> Vec<Content> {
    contents.extend(first_candidate_content(chunk));
    contents
}

/// A config with `http_options`, so that [`get_usage_header`] can label it.
pub trait HasHttpOptions {
    /// The config's per-request HTTP options.
    fn http_options_mut(&mut self) -> &mut Option<HttpOptions>;
}

impl HasHttpOptions for GenerateContentConfig {
    fn http_options_mut(&mut self) -> &mut Option<HttpOptions> {
        &mut self.http_options
    }
}

impl HasHttpOptions for GenerateImagesConfig {
    fn http_options_mut(&mut self) -> &mut Option<HttpOptions> {
        &mut self.http_options
    }
}

/// `config` (or a default one) with this SDK's `usage` marker added to its
/// `user-agent` and `x-goog-api-client` headers, e.g. `gemini-genai/0.2.2+afc`.
/// Mirrors Python's `_extra_utils.get_usage_header`.
///
/// A header that already carries the marker is left alone, so calling this
/// repeatedly does not duplicate it. A header that carries the library label
/// without a marker gets it inserted there; any other value gets the labelled
/// marker appended.
#[must_use]
pub fn get_usage_header<C: HasHttpOptions + Default>(config: Option<C>, usage: &str) -> C {
    let library = library_label();
    let usage_header = format!("{library}+{usage}");
    let mut config = config.unwrap_or_default();
    let http_options = config
        .http_options_mut()
        .get_or_insert_with(HttpOptions::default);
    let headers = http_options.headers.get_or_insert_with(HashMap::new);
    for header_key in ["user-agent", "x-goog-api-client"] {
        match headers.get_mut(header_key) {
            Some(existing) => {
                if !existing.contains(&format!("+{usage}")) && !existing.contains(&usage_header) {
                    if existing.contains(&library) {
                        *existing = existing.replace(&library, &usage_header);
                    } else {
                        existing.push(' ');
                        existing.push_str(&usage_header);
                    }
                }
            }
            None => {
                headers.insert(header_key.to_owned(), usage_header.clone());
            }
        }
    }
    config
}

/// The first candidate's `content`, if any.
fn first_candidate_content(response: &GenerateContentResponse) -> Option<Content> {
    response
        .candidates
        .as_ref()
        .and_then(|candidates| candidates.first())
        .and_then(|candidate| candidate.content.clone())
}

/// Why a request must not run the AFC loop, or the loop's inputs if it must.
enum AfcPlan {
    /// Issue exactly one plain request.
    Skip,
    /// Run the loop with these callables and this request budget.
    Run {
        function_map: HashMap<String, Arc<dyn FunctionTool>>,
        remaining: i64,
    },
}

/// Decides whether `config` runs the AFC loop, in the order Python checks:
/// disabled, then incompatible tools, then (streaming only) incompatible
/// config.
fn plan_afc(config: Option<&GenerateContentConfig>, streaming: bool) -> Result<AfcPlan> {
    if should_disable_afc(config) {
        return Ok(AfcPlan::Skip);
    }
    let incompatible = find_afc_incompatible_tool_indexes(config, false);
    if !incompatible.is_empty() {
        log_afc_incompatible_tools_warning(config, &incompatible);
        return Ok(AfcPlan::Skip);
    }
    if streaming {
        raise_error_for_afc_incompatible_config(config)?;
    }
    let function_map = get_function_map(config);
    if function_map.is_empty() {
        return Ok(AfcPlan::Skip);
    }
    let remaining = get_max_remote_calls_afc(config)?;
    Ok(AfcPlan::Run {
        function_map,
        remaining,
    })
}

/// Drives [`Models::generate_content`]. If `config.tools` declares at
/// least one function with a callable registered via
/// [`Tool::from_function`] (directly, or via `crate::mcp_utils::mcp_tools`),
/// and AFC is neither disabled nor blocked by an incompatible tool, this runs
/// the automatic-function-calling loop:
///
/// ```text
/// remaining = maximum_remote_calls (default 10); history = []
/// loop:
///   resp = request()          // carries a `+afc` usage header
///   remaining -= 1
///   if remaining == 0 -> break   // no request left to send a result with:
///                                // the function is not called at all
///   parts = run the functions resp called; if none -> break
///   history = history or contents; history += [resp.content, user{parts}]
/// resp.automatic_function_calling_history = history   // unless ignore_call_history
/// ```
///
/// Otherwise (no registered tools declared, `automatic_function_calling.disable`,
/// `maximum_remote_calls <= 0`, or a tool AFC cannot drive) this issues exactly
/// one request and returns its response unmodified.
///
/// # Errors
/// See [`Models::generate_content`].
pub(crate) async fn generate_content(
    models: &Models,
    model: &str,
    contents: Contents,
    config: Option<GenerateContentConfig>,
) -> Result<GenerateContentResponse> {
    let AfcPlan::Run {
        function_map,
        mut remaining,
    } = plan_afc(config.as_ref(), false)?
    else {
        return models.generate_content_once(model, contents, config).await;
    };

    let config_to_call = get_usage_header(config.clone(), USAGE_AFC);
    let mut contents: Vec<Content> = contents.into();
    let mut history: Vec<Content> = Vec::new();
    let mut response;

    loop {
        response = models
            .generate_content_once(model, contents.clone(), Some(config_to_call.clone()))
            .await?;
        remaining -= 1;
        if remaining <= 0 {
            break;
        }

        let response_parts = get_function_response_parts(&response, &function_map).await?;
        let (Some(call_content), false) = (
            first_candidate_content(&response),
            response_parts.is_empty(),
        ) else {
            break;
        };
        let response_content = Content {
            role: Some("user".to_owned()),
            parts: Some(response_parts),
        };
        if history.is_empty() {
            history.extend(contents.iter().cloned());
        }
        contents.push(call_content.clone());
        contents.push(response_content.clone());
        history.push(call_content);
        history.push(response_content);
    }

    if should_append_afc_history(config.as_ref()) {
        response.automatic_function_calling_history = Some(history);
    }
    Ok(response)
}

/// Drives [`Models::generate_content_stream`]. Runs the same loop as
/// [`generate_content`] across streaming requests (see Python's
/// `Models.generate_content_stream`): every chunk of every round is yielded;
/// once a round is drained, the functions it called are answered and the next
/// round's request carries the round's output and the answers. From the second
/// round on, each chunk carries the history so far. A round that spends the
/// budget does not run its functions.
///
/// The first request is issued before this returns, so a failure to start it
/// is the call's `Err`; every later failure is the stream's last item.
///
/// # Errors
/// See [`Models::generate_content_stream`].
pub(crate) async fn generate_content_stream(
    models: &Models,
    model: &str,
    contents: Contents,
    config: Option<GenerateContentConfig>,
) -> Result<GenerateContentStream> {
    let AfcPlan::Run {
        function_map,
        remaining,
    } = plan_afc(config.as_ref(), true)?
    else {
        return models
            .generate_content_stream_once(model, contents, config)
            .await;
    };

    let append_history = should_append_afc_history(config.as_ref());
    let config_to_call = get_usage_header(config, USAGE_AFC);
    let mut loop_contents: Vec<Content> = contents.into();
    let mut round = models
        .generate_content_stream_once(model, loop_contents.clone(), Some(config_to_call.clone()))
        .await?;
    let models = models.clone();
    let model = model.to_owned();

    let stream = async_stream::stream! {
        let mut remaining = remaining - 1;
        let mut history: Vec<Content> = Vec::new();
        loop {
            let is_last_remote_call_afc = remaining <= 0;
            let mut model_output: Vec<Content> = Vec::new();
            let mut response_parts: Vec<Part> = Vec::new();

            while let Some(item) = round.next().await {
                let mut chunk = match item {
                    Ok(chunk) => chunk,
                    Err(error) => {
                        yield Err(error);
                        return;
                    }
                };
                if append_history && !history.is_empty() {
                    chunk.automatic_function_calling_history = Some(history.clone());
                }
                if !is_last_remote_call_afc {
                    match get_function_response_parts(&chunk, &function_map).await {
                        Ok(parts) => response_parts.extend(parts),
                        Err(error) => {
                            yield Err(error);
                            return;
                        }
                    }
                }
                model_output = append_chunk_contents(model_output, &chunk);
                yield Ok(chunk);
            }

            if is_last_remote_call_afc || response_parts.is_empty() {
                break;
            }

            let response_content = Content {
                role: Some("user".to_owned()),
                parts: Some(response_parts),
            };
            if history.is_empty() {
                history.extend(loop_contents.iter().cloned());
            }
            loop_contents.extend(model_output.iter().cloned());
            loop_contents.push(response_content.clone());
            history.extend(model_output);
            history.push(response_content);

            round = match models
                .generate_content_stream_once(
                    &model,
                    loop_contents.clone(),
                    Some(config_to_call.clone()),
                )
                .await
            {
                Ok(next) => next,
                Err(error) => {
                    yield Err(error);
                    return;
                }
            };
            remaining -= 1;
        }
    };
    Ok(GenerateContentStream::new(stream))
}

#[cfg(test)]
mod conformance_tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use serde_json::{Map, Value, json};
    use tracing::{
        Event, Level, Metadata,
        span::{Attributes, Id, Record},
    };

    use super::*;

    /// Counts WARN events; the crate has no `tracing-subscriber` dev-dependency.
    struct WarnCounter(Arc<AtomicUsize>);

    impl tracing::Subscriber for WarnCounter {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &Attributes<'_>) -> Id {
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, event: &Event<'_>) {
            if *event.metadata().level() == Level::WARN {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    fn warnings_logged(config: Option<&GenerateContentConfig>, indexes: &[usize]) -> usize {
        let count = Arc::new(AtomicUsize::new(0));
        tracing::subscriber::with_default(WarnCounter(Arc::clone(&count)), || {
            log_afc_incompatible_tools_warning(config, indexes);
        });
        count.load(Ordering::SeqCst)
    }

    fn config_with_tools(count: usize) -> GenerateContentConfig {
        GenerateContentConfig {
            tools: Some(vec![Tool::default(); count]),
            ..Default::default()
        }
    }

    #[test]
    fn log_afc_incompatible_tools_warning_is_silent_without_incompatible_tools() {
        assert_eq!(warnings_logged(Some(&config_with_tools(2)), &[]), 0);
    }

    #[test]
    fn log_afc_incompatible_tools_warning_warns_when_only_some_tools_are_incompatible() {
        assert_eq!(warnings_logged(Some(&config_with_tools(3)), &[1]), 1);
    }

    #[test]
    fn log_afc_incompatible_tools_warning_is_silent_when_every_tool_is_incompatible() {
        assert_eq!(warnings_logged(Some(&config_with_tools(2)), &[0, 1]), 0);
    }

    #[test]
    fn log_afc_incompatible_tools_warning_treats_missing_config_as_zero_tools() {
        assert_eq!(warnings_logged(None, &[0]), 1);
    }

    fn object(value: &Value) -> Map<String, Value> {
        value.as_object().cloned().unwrap_or_default()
    }

    #[test]
    fn convert_number_values_for_dict_function_call_args_turns_integral_floats_into_ints() {
        let converted =
            convert_number_values_for_dict_function_call_args(&object(&json!({"n": 2.0})));
        assert_eq!(converted["n"], json!(2));
        assert!(converted["n"].is_i64());
    }

    #[test]
    fn convert_number_values_for_dict_function_call_args_keeps_fractional_floats() {
        let converted =
            convert_number_values_for_dict_function_call_args(&object(&json!({"n": 2.5})));
        assert_eq!(converted["n"], json!(2.5));
    }

    #[test]
    fn convert_number_values_for_dict_function_call_args_recurses_into_nested_values() {
        let converted = convert_number_values_for_dict_function_call_args(&object(
            &json!({"a": [1.0, {"b": 3.0}], "s": "x", "z": null, "t": true}),
        ));
        assert_eq!(
            Value::Object(converted),
            json!({"a": [1, {"b": 3}], "s": "x", "z": null, "t": true})
        );
    }

    #[test]
    fn convert_number_values_for_dict_function_call_args_of_empty_map_is_empty() {
        assert!(convert_number_values_for_dict_function_call_args(&Map::new()).is_empty());
    }

    fn text_content(role: &str, text: &str) -> Content {
        Content {
            role: Some(role.to_owned()),
            parts: Some(vec![Part {
                text: Some(text.to_owned()),
                ..Default::default()
            }]),
        }
    }

    fn response_with(content: Option<Content>) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![crate::types::Candidate {
                content,
                ..Default::default()
            }]),
            ..Default::default()
        }
    }

    #[test]
    fn append_chunk_contents_appends_the_first_candidate_content() {
        let first = text_content("user", "hi");
        let reply = text_content("model", "hello");
        let appended =
            append_chunk_contents(vec![first.clone()], &response_with(Some(reply.clone())));
        assert_eq!(appended, vec![first, reply]);
    }

    #[test]
    fn append_chunk_contents_uses_only_the_first_of_several_candidates() {
        let mut chunk = response_with(Some(text_content("model", "one")));
        if let Some(candidates) = chunk.candidates.as_mut() {
            candidates.push(crate::types::Candidate {
                content: Some(text_content("model", "two")),
                ..Default::default()
            });
        }
        let appended = append_chunk_contents(Vec::new(), &chunk);
        assert_eq!(appended, vec![text_content("model", "one")]);
    }

    #[test]
    fn append_chunk_contents_leaves_contents_unchanged_when_chunk_has_no_candidates() {
        let before = vec![text_content("user", "hi")];
        let appended = append_chunk_contents(before.clone(), &GenerateContentResponse::default());
        assert_eq!(appended, before);
    }

    #[test]
    fn append_chunk_contents_leaves_contents_unchanged_when_candidate_has_no_content() {
        let before = vec![text_content("user", "hi")];
        let appended = append_chunk_contents(before.clone(), &response_with(None));
        assert_eq!(appended, before);
    }
}
