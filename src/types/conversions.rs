//! Ergonomic conversions into [`Content`]/[`Part`], mirroring Python's
//! `ContentUnion`/`PartUnion`/`ContentListUnion` acceptance and the
//! `Part.from_*` classmethods. Where Python coerces dynamically (a bare
//! `str`, a `PIL.Image`, a dict) at call time, this crate does the
//! equivalent coercion at compile time via these `From` impls (see
//! `research.md` R-04).

use std::collections::HashMap;

use serde_json::Value;

use super::generated::{Blob, Content, FileData, FunctionCall, FunctionResponse, Part};

const ROLE_USER: &str = "user";
const ROLE_MODEL: &str = "model";

impl Part {
    /// Builds a text [`Part`]. Mirrors Python's `Part.from_text`.
    #[must_use]
    pub fn from_text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..Default::default()
        }
    }

    /// Builds a [`Part`] from inline bytes and their MIME type. Mirrors
    /// Python's `Part.from_bytes`.
    #[must_use]
    pub fn from_bytes(data: impl Into<Vec<u8>>, mime_type: impl Into<String>) -> Self {
        Self {
            inline_data: Some(Blob {
                data: Some(data.into()),
                mime_type: Some(mime_type.into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Builds a [`Part`] referencing an uploaded file (or any URI-addressable
    /// resource) by URI and MIME type. Mirrors Python's `Part.from_uri`.
    #[must_use]
    pub fn from_uri(uri: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            file_data: Some(FileData {
                file_uri: Some(uri.into()),
                mime_type: Some(mime_type.into()),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Builds a [`Part`] carrying a model-predicted function call. Mirrors
    /// Python's `Part.from_function_call`.
    #[must_use]
    pub fn from_function_call(name: impl Into<String>, args: HashMap<String, Value>) -> Self {
        Self {
            function_call: Some(FunctionCall {
                name: Some(name.into()),
                args: Some(args),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// Builds a [`Part`] carrying a function's result, to be sent back to
    /// the model. Mirrors Python's `Part.from_function_response`.
    #[must_use]
    pub fn from_function_response(
        name: impl Into<String>,
        response: HashMap<String, Value>,
    ) -> Self {
        Self {
            function_response: Some(FunctionResponse {
                name: Some(name.into()),
                response: Some(response),
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    fn is_model_turn(&self) -> bool {
        self.function_call.is_some()
    }

    /// Reads `path`'s bytes and builds an inline-data [`Part`], inferring
    /// the MIME type from the file extension via `mime_guess` (falling
    /// back to `application/octet-stream` if the extension is unknown or
    /// absent). No direct Python equivalent -- there, callers read the
    /// file themselves and pass the resulting `bytes` to
    /// `Part.from_bytes`; this is a convenience wrapper folding that file
    /// read into [`Part::from_bytes`] for the common case.
    ///
    /// # Errors
    /// Returns the underlying [`std::io::Error`] if `path` cannot be read.
    pub fn from_file_bytes(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        let data = std::fs::read(path)?;
        let mime_type = mime_guess::from_path(path).first_or_octet_stream();
        Ok(Self::from_bytes(data, mime_type.essence_str()))
    }
}

impl From<&str> for Part {
    fn from(text: &str) -> Self {
        Self::from_text(text)
    }
}

impl From<String> for Part {
    fn from(text: String) -> Self {
        Self::from_text(text)
    }
}

fn content_with_role(parts: Vec<Part>) -> Content {
    let role = if parts.iter().any(Part::is_model_turn) {
        ROLE_MODEL
    } else {
        ROLE_USER
    };
    Content {
        parts: Some(parts),
        role: Some(role.to_owned()),
    }
}

impl From<&str> for Content {
    fn from(text: &str) -> Self {
        content_with_role(vec![Part::from_text(text)])
    }
}

impl From<String> for Content {
    fn from(text: String) -> Self {
        content_with_role(vec![Part::from_text(text)])
    }
}

impl From<Part> for Content {
    fn from(part: Part) -> Self {
        content_with_role(vec![part])
    }
}

impl From<Vec<Part>> for Content {
    fn from(parts: Vec<Part>) -> Self {
        content_with_role(parts)
    }
}

/// Ergonomic input for any API accepting `contents`, mirroring Python's
/// `ContentListUnion`: a bare string, a single [`Part`]/[`Content`], or a
/// list of either, all normalize to `Vec<Content>`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contents(pub Vec<Content>);

impl From<Contents> for Vec<Content> {
    fn from(contents: Contents) -> Self {
        contents.0
    }
}

impl From<&str> for Contents {
    fn from(text: &str) -> Self {
        Self(vec![Content::from(text)])
    }
}

impl From<String> for Contents {
    fn from(text: String) -> Self {
        Self(vec![Content::from(text)])
    }
}

impl From<Content> for Contents {
    fn from(content: Content) -> Self {
        Self(vec![content])
    }
}

impl From<Part> for Contents {
    fn from(part: Part) -> Self {
        Self(vec![Content::from(part)])
    }
}

impl From<Vec<Content>> for Contents {
    fn from(contents: Vec<Content>) -> Self {
        Self(contents)
    }
}

impl From<Vec<Part>> for Contents {
    fn from(parts: Vec<Part>) -> Self {
        Self(vec![Content::from(parts)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_becomes_a_single_user_content_with_one_text_part() {
        let contents: Contents = "hello".into();
        assert_eq!(contents.0.len(), 1);
        assert_eq!(contents.0[0].role.as_deref(), Some("user"));
        assert_eq!(
            contents.0[0].parts.as_ref().unwrap()[0].text.as_deref(),
            Some("hello")
        );
    }

    #[test]
    fn part_from_bytes_sets_inline_data() {
        let part = Part::from_bytes(vec![1, 2, 3], "image/png");
        let blob = part.inline_data.unwrap();
        assert_eq!(blob.data, Some(vec![1, 2, 3]));
        assert_eq!(blob.mime_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn part_from_file_bytes_reads_the_file_and_infers_mime_type_from_extension() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("genai-rs-test-{}.png", std::process::id()));
        std::fs::write(&path, [1, 2, 3, 4]).unwrap();

        let part = Part::from_file_bytes(&path).unwrap();
        std::fs::remove_file(&path).unwrap();

        let blob = part.inline_data.unwrap();
        assert_eq!(blob.data, Some(vec![1, 2, 3, 4]));
        assert_eq!(blob.mime_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn part_from_file_bytes_errors_for_a_missing_file() {
        let result = Part::from_file_bytes("/no/such/file/genai-rs-does-not-exist");
        assert!(result.is_err());
    }

    #[test]
    fn content_role_is_model_when_it_contains_a_function_call() {
        let part = Part::from_function_call("get_weather", HashMap::new());
        let content: Content = part.into();
        assert_eq!(content.role.as_deref(), Some("model"));
    }

    #[test]
    fn vec_of_parts_becomes_one_content() {
        let contents: Contents = vec![Part::from_text("a"), Part::from_text("b")].into();
        assert_eq!(contents.0.len(), 1);
        assert_eq!(contents.0[0].parts.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn vec_of_contents_passes_through() {
        let contents: Contents = vec![Content::from("a"), Content::from("b")].into();
        assert_eq!(contents.0.len(), 2);
    }
}
