//! Hand-written extensions to the generated types: ergonomic constructors
//! and response accessors that mirror Python properties/classmethods, plus
//! the handful of small support types the generator's field-type overrides
//! reference (see `tools/codegen/gen_types.py` `FIELD_TYPE_OVERRIDES`).

use serde::{Deserialize, Serialize};

use super::generated::{
    CodeExecutionResult, ExecutableCode, FunctionCall, GenerateContentResponse, JSONSchemaType,
    Part,
};

/// `JSONSchema.type`: either a single [`JSONSchemaType`] or a list of them
/// (JSON Schema allows both forms for a `"type"` keyword). Mirrors the
/// Python SDK's `Union[JSONSchemaType, list[JSONSchemaType]]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonSchemaTypeOrList {
    /// A single JSON Schema type.
    Single(JSONSchemaType),
    /// Multiple allowed JSON Schema types.
    Multiple(Vec<JSONSchemaType>),
}

impl From<JSONSchemaType> for JsonSchemaTypeOrList {
    fn from(value: JSONSchemaType) -> Self {
        Self::Single(value)
    }
}

impl From<Vec<JSONSchemaType>> for JsonSchemaTypeOrList {
    fn from(value: Vec<JSONSchemaType>) -> Self {
        Self::Multiple(value)
    }
}

impl GenerateContentResponse {
    fn first_candidate_parts(&self) -> Option<&[Part]> {
        self.candidates
            .as_deref()
            .and_then(|candidates| candidates.first())
            .and_then(|candidate| candidate.content.as_ref())
            .and_then(|content| content.parts.as_deref())
    }

    /// The concatenated text of every non-thought text part in the first
    /// candidate, or `None` if there is no text. Mirrors Python's
    /// `GenerateContentResponse.text` property.
    #[must_use]
    pub fn text(&self) -> Option<String> {
        let parts = self.first_candidate_parts()?;
        let mut out = String::new();
        let mut found = false;
        for part in parts {
            if part.thought == Some(true) {
                continue;
            }
            if let Some(text) = &part.text {
                out.push_str(text);
                found = true;
            }
        }
        found.then_some(out)
    }

    /// Every part of the first candidate, if any. Mirrors Python's
    /// `GenerateContentResponse.parts` property.
    #[must_use]
    pub fn parts(&self) -> Option<&[Part]> {
        self.first_candidate_parts()
    }

    /// Every function call requested by the first candidate. Mirrors
    /// Python's `GenerateContentResponse.function_calls` property.
    #[must_use]
    pub fn function_calls(&self) -> Vec<&FunctionCall> {
        self.first_candidate_parts()
            .into_iter()
            .flatten()
            .filter_map(|part| part.function_call.as_ref())
            .collect()
    }

    /// The first candidate's executable-code part, if any. Mirrors
    /// Python's `GenerateContentResponse.executable_code` property.
    #[must_use]
    pub fn executable_code(&self) -> Option<&ExecutableCode> {
        self.first_candidate_parts()?
            .iter()
            .find_map(|part| part.executable_code.as_ref())
    }

    /// The first candidate's code-execution-result part, if any. Mirrors
    /// Python's `GenerateContentResponse.code_execution_result` property.
    #[must_use]
    pub fn code_execution_result(&self) -> Option<&CodeExecutionResult> {
        self.first_candidate_parts()?
            .iter()
            .find_map(|part| part.code_execution_result.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::super::generated::{Candidate, Content};
    use super::*;

    fn response_with_parts(parts: Vec<Part>) -> GenerateContentResponse {
        GenerateContentResponse {
            candidates: Some(vec![Candidate {
                content: Some(Content {
                    parts: Some(parts),
                    role: Some("model".to_owned()),
                }),
                ..Default::default()
            }]),
            ..Default::default()
        }
    }

    #[test]
    fn text_concatenates_non_thought_text_parts() {
        let response = response_with_parts(vec![
            Part::from_text("Hello, "),
            Part {
                text: Some("(thinking)".to_owned()),
                thought: Some(true),
                ..Default::default()
            },
            Part::from_text("world!"),
        ]);
        assert_eq!(response.text().as_deref(), Some("Hello, world!"));
    }

    #[test]
    fn text_is_none_without_candidates() {
        assert_eq!(GenerateContentResponse::default().text(), None);
    }

    #[test]
    fn function_calls_collects_every_function_call_part() {
        let response = response_with_parts(vec![
            Part::from_function_call("a", std::collections::HashMap::new()),
            Part::from_text("not a call"),
            Part::from_function_call("b", std::collections::HashMap::new()),
        ]);
        let calls = response.function_calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].name.as_deref(), Some("a"));
        assert_eq!(calls[1].name.as_deref(), Some("b"));
    }
}
